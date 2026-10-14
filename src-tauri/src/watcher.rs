use std::{
    collections::HashMap,
    future::Future,
    sync::{atomic::{AtomicU64, Ordering}, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use bson::{doc, Bson, Document};
use futures_util::{stream, StreamExt, TryStreamExt};
use mongodb::{
    change_stream::event::ChangeStreamEvent,
    options::{ChangeStreamOptions, FindOneOptions, FindOptions, FullDocumentType},
    Client,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::{
    sync::{mpsc, Notify},
    time::{interval, sleep, timeout, MissedTickBehavior},
};

use crate::{config::AppConfig, mongo_manager::MongoClients};

static TASKS_WATCH_STARTED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static PROGRESS_REFRESH: Notify = Notify::const_new();
static LAST_PROGRESS_VERSION: AtomicU64 = AtomicU64::new(0);
const PROGRESS_TIMEOUT: Duration = Duration::from_secs(15);
const METADATA_TIMEOUT: Duration = Duration::from_secs(5);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const COALESCE_INTERVAL: Duration = Duration::from_millis(500);
const MAX_CONCURRENT_STATS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressSource {
    LocalMission,
    RemoteMission,
    LocalData,
    RemoteData,
}

impl ProgressSource {
    fn is_remote(self) -> bool {
        matches!(self, Self::RemoteMission | Self::RemoteData)
    }

    fn is_mission(self) -> bool {
        matches!(self, Self::LocalMission | Self::RemoteMission)
    }
}

#[derive(Debug, Clone)]
struct ProgressRequest {
    collection: String,
    source: ProgressSource,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
struct ProgressCounts {
    success: usize,
    error: usize,
    total: usize,
    priority_success: usize,
    priority_error: usize,
    priority_total: usize,
}

#[derive(Debug, Clone, Serialize)]
struct TaskProgress {
    collection: String,
    task_id: String,
    #[serde(flatten)]
    counts: ProgressCounts,
    is_remote: bool,
    // Query start time, not completion time: an older slow query cannot overwrite a newer query.
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
struct TaskProgressError {
    collection: String,
    task_id: Option<String>,
    is_remote: Option<bool>,
    error: String,
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
struct TaskProgressRefreshError {
    scope: &'static str,
    error: String,
    updated_at: u64,
}

#[derive(Debug)]
struct ResolvedTask {
    task_id: String,
    is_remote: bool,
}

fn emit_discovery_error(app: &AppHandle, scope: &'static str, error: &anyhow::Error, updated_at: u64) {
    let _ = app.emit("task-progress-refresh-error", TaskProgressRefreshError {
        scope, error: format!("{error:#}"), updated_at,
    });
}

fn timestamp_ms() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    // Distinct ordering even for same-millisecond queries or a backwards clock adjustment.
    let previous = LAST_PROGRESS_VERSION.fetch_update(Ordering::SeqCst, Ordering::SeqCst,
        |last| Some(now.max(last + 1))).unwrap();
    now.max(previous + 1)
}

async fn refresh_loop<F: Future<Output = ()>>(
    notification: &Notify, period: Duration, mut refresh: impl FnMut() -> F,
) {
    let mut tick = interval(period);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick.tick().await; // ready's notification supplies the initial refresh
    loop {
        tokio::select! {
            _ = notification.notified() => {},
            _ = tick.tick() => {},
        }
        // One refresh in flight; Notify merges repeated requests into a single permit.
        refresh().await;
    }
}

/// Start only one set of workers; every ready call requests a fresh background snapshot.
#[tauri::command]
pub async fn frontend_ready(
    app: AppHandle,
    mongo_clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> std::result::Result<(), String> {
    if TASKS_WATCH_STARTED.set(()).is_ok() {
        let clients = mongo_clients.inner().clone();
        let cfg = config.inner().clone();
        let watch_app = app.clone();
        let watch_clients = clients.clone();
        let watch_config = cfg.clone();
        tauri::async_runtime::spawn(async move {
            watch_simulation_db_changes(watch_app, watch_clients, watch_config).await;
        });
        tauri::async_runtime::spawn(async move {
            refresh_loop(&PROGRESS_REFRESH, REFRESH_INTERVAL, ||
                emit_all_collections_once(app.clone(), clients.clone(), cfg.clone())).await;
        });
    }
    PROGRESS_REFRESH.notify_one();
    Ok(())
}

/// Fast command: database work runs in the coalesced background worker, not in task actions.
#[tauri::command]
pub async fn refresh_task_progress() -> std::result::Result<(), String> {
    PROGRESS_REFRESH.notify_one();
    Ok(())
}

async fn with_deadline<T>(duration: Duration, future: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(duration, future).await.map_err(|_| anyhow!("查询超时（{}s）", duration.as_secs()))?
}

/// Local registration wins, including deactivation. A missing local record retains event origin.
fn select_source(task: Option<&Document>, source: ProgressSource) -> Option<bool> {
    match task {
        Some(task) if task.get_str("status").ok() == Some("deactive") => None,
        Some(task) => Some(task.get_bool("isRemote").unwrap_or(false)),
        None if source.is_remote() => Some(true),
        None => None,
    }
}

fn active_task_filter(name: &str) -> Document {
    doc! { "name": name, "status": { "$ne": "deactive" } }
}

fn resolved_task(task: &Document, is_remote: bool) -> Result<ResolvedTask> {
    Ok(ResolvedTask {
        task_id: task.get_object_id("_id").context("任务缺少 ObjectId")?.to_hex(),
        is_remote,
    })
}

async fn resolve_source(clients: &MongoClients, name: &str, config: &AppConfig, source: ProgressSource) -> Result<Option<ResolvedTask>> {
    with_deadline(METADATA_TIMEOUT, async {
        let tasks = clients.local.database(&config.mongodb.databases.mission).collection::<Document>("tasks");
        // Soft-deleted old records must not hide an active replacement with the same name.
        let options = FindOneOptions::builder().sort(doc! { "_id": -1 }).build();
        if let Some(task) = tasks.find_one(active_task_filter(name), options.clone()).await? {
            return Ok(Some(resolved_task(&task, task.get_bool("isRemote").unwrap_or(false))?));
        }
        // A locally deleted name masks a synced remote copy, just as the task list does.
        if !source.is_remote() || tasks.find_one(doc! { "name": name }, None).await?.is_some() {
            return Ok(None);
        }
        let remote_tasks = clients.remote.database(&config.mongodb.databases.mission).collection::<Document>("tasks");
        match remote_tasks.find_one(active_task_filter(name), options).await? {
            Some(task) => Ok(Some(resolved_task(&task, true)?)),
            None => Ok(None),
        }
    }).await.context("查询任务运行方式失败")
    // Errors propagate; never silently reroute or attach statistics to an unknown task identity.
}

fn count_field(document: &Document, name: &str) -> Result<usize> {
    let value = match document.get(name) {
        Some(Bson::Int32(n)) => i64::from(*n),
        Some(Bson::Int64(n)) => *n,
        _ => return Err(anyhow!("统计字段 {name} 不是整数")),
    };
    usize::try_from(value).map_err(|_| anyhow!("统计字段 {name} 超出范围"))
}

fn parse_counts(document: Option<Document>) -> Result<ProgressCounts> {
    match document {
        // Only a successfully exhausted, empty cursor represents zero progress.
        None => Ok(ProgressCounts::default()),
        Some(doc) => Ok(ProgressCounts {
            success: count_field(&doc, "success")?,
            error: count_field(&doc, "error")?,
            total: count_field(&doc, "total")?,
            priority_success: count_field(&doc, "priority_success")?,
            priority_error: count_field(&doc, "priority_error")?,
            priority_total: count_field(&doc, "priority_total")?,
        }),
    }
}

fn stats_pipeline() -> Vec<Document> {
    vec![doc! { "$group": {
        "_id": null,
        "total": { "$sum": 1 },
        "success": { "$sum": { "$cond": [ { "$eq": ["$status", "success"] }, 1, 0 ] } },
        "error": { "$sum": { "$cond": [ { "$in": ["$status", ["fail", "error"]] }, 1, 0 ] } },
        "priority_total": { "$sum": { "$cond": [ { "$gt": ["$priority", 0] }, 1, 0 ] } },
        "priority_success": { "$sum": { "$cond": [
            { "$and": [ { "$eq": ["$status", "success"] }, { "$gt": ["$priority", 0] } ] }, 1, 0
        ] } },
        "priority_error": { "$sum": { "$cond": [
            { "$and": [ { "$in": ["$status", ["fail", "error"]] }, { "$gt": ["$priority", 0] } ] }, 1, 0
        ] } },
    }}]
}

async fn read_counts(mut cursor: impl futures_util::TryStream<Ok = Document, Error = mongodb::error::Error> + Unpin) -> Result<ProgressCounts> {
    let document = cursor.try_next().await.context("读取统计游标失败")?;
    parse_counts(document)
}

async fn query_counts(client: &Client, name: &str, config: &AppConfig) -> Result<ProgressCounts> {
    with_deadline(PROGRESS_TIMEOUT, async {
        let collection = client.database(&config.mongodb.databases.simulation).collection::<Document>(name);
        let cursor = collection.aggregate(stats_pipeline(), None).await.context("Mongo aggregate failed")?;
        read_counts(cursor).await
    }).await
}

async fn emit_progress(app: &AppHandle, clients: &MongoClients, config: &AppConfig, request: ProgressRequest) {
    let updated_at = timestamp_ms();
    let mut is_remote = None;
    let mut task_id = None;
    let result: Result<()> = async {
        let Some(task) = resolve_source(clients, &request.collection, config, request.source).await? else {
            return Ok(()); // Deleted tasks and unregistered data collections are not cards.
        };
        is_remote = Some(task.is_remote);
        task_id = Some(task.task_id.clone());
        let client = if task.is_remote { &clients.remote } else { &clients.local };
        let counts = query_counts(client, &request.collection, config).await?;
        app.emit("task-progress-update", TaskProgress {
            collection: request.collection.clone(), task_id: task.task_id, counts,
            is_remote: task.is_remote, updated_at,
        })?;
        Ok(())
    }.await;
    if let Err(error) = result {
        let message = format!("{error:#}");
        eprintln!("⚠️ 任务进度刷新失败 [{}]: {}（保留上次统计）", request.collection, message);
        let _ = app.emit("task-progress-error", TaskProgressError {
            collection: request.collection, task_id, is_remote, error: message, updated_at,
        });
    }
}

async fn emit_requests(app: &AppHandle, clients: &MongoClients, config: &AppConfig, requests: Vec<ProgressRequest>) {
    stream::iter(requests).map(|request| emit_progress(app, clients, config, request))
        .buffer_unordered(MAX_CONCURRENT_STATS).collect::<Vec<_>>().await;
}

async fn registered_tasks(client: &Client, config: &AppConfig) -> Result<Vec<Document>> {
    with_deadline(METADATA_TIMEOUT, async {
        let tasks = client.database(&config.mongodb.databases.mission).collection::<Document>("tasks");
        let options = FindOptions::builder().projection(doc! { "name": 1, "isRemote": 1, "status": 1 }).build();
        let cursor = tasks.find(None, options).await?;
        Ok(cursor.try_collect().await?)
    }).await
}

/// Periodic and explicit snapshots cover both locally registered remote jobs and remote-only jobs.
async fn emit_all_collections_once(app: AppHandle, clients: Arc<MongoClients>, config: AppConfig) {
    let local_discovery_version = timestamp_ms();
    let local_tasks = match registered_tasks(&clients.local, &config).await {
        Ok(tasks) => tasks,
        Err(error) => {
            eprintln!("⚠️ 无法加载本地任务进度列表: {error:#}");
            emit_discovery_error(&app, "all", &error, local_discovery_version);
            return; // Cannot determine local precedence; do not guess the source.
        }
    };
    let mut seen = std::collections::HashSet::new();
    let mut local_requests = Vec::new();
    let mut active_names = std::collections::HashSet::new();
    for task in local_tasks {
        if let Ok(name) = task.get_str("name") {
            seen.insert(name.to_owned());
            if select_source(Some(&task), ProgressSource::LocalMission).is_some() && active_names.insert(name.to_owned()) {
                local_requests.push(ProgressRequest { collection: name.to_owned(), source: ProgressSource::LocalMission });
            }
        }
    }
    // Discover and emit remote-only jobs independently; slow local jobs cannot hold them back.
    let remote_discovery_version = timestamp_ms();
    let refresh_remote = async {
        if config.mongodb.local_uri == config.mongodb.remote_uri { return; }
        match registered_tasks(&clients.remote, &config).await {
            Ok(tasks) => {
                let mut requests = Vec::new();
                for task in tasks {
                    if task.get_str("status").ok() == Some("deactive") { continue; }
                    if let Ok(name) = task.get_str("name") {
                        if seen.insert(name.to_owned()) {
                            requests.push(ProgressRequest { collection: name.to_owned(), source: ProgressSource::RemoteMission });
                        }
                    }
                }
                emit_requests(&app, &clients, &config, requests).await;
            }
            Err(error) => {
                eprintln!("⚠️ 远程任务列表暂时不可用: {error:#}（后台将重试）");
                emit_discovery_error(&app, "remote", &error, remote_discovery_version);
            }
        }
    };
    tokio::join!(emit_requests(&app, &clients, &config, local_requests), refresh_remote);
}

fn extract_request(change: &ChangeStreamEvent<Document>, source: ProgressSource) -> Option<ProgressRequest> {
    let collection = if source.is_mission() {
        change.full_document.as_ref()?.get_str("name").ok()?.to_owned()
    } else {
        change.ns.as_ref()?.coll.clone()?
    };
    if collection.starts_with("system.") { return None; }
    Some(ProgressRequest { collection, source })
}

/// Retain dirty state until the next flush; never drop a collection's final change.
fn mark_dirty(pending: &mut HashMap<String, ProgressSource>, request: ProgressRequest) {
    pending.entry(request.collection).and_modify(|source| {
        if request.source.is_remote() { *source = request.source; }
    }).or_insert(request.source);
}

async fn watch_source(client: Arc<Client>, config: AppConfig, source: ProgressSource, tx: mpsc::Sender<ProgressRequest>) {
    let mut retry_delay = Duration::from_secs(5);
    loop {
        let db_name = if source.is_mission() { &config.mongodb.databases.mission } else { &config.mongodb.databases.simulation };
        let db = client.database(db_name);
        let options = ChangeStreamOptions::builder()
            .full_document(Some(FullDocumentType::UpdateLookup))
            .max_await_time(Some(Duration::from_secs(1))).build();
        let open = async {
            if source.is_mission() { db.collection::<Document>("tasks").watch([], options).await }
            else { db.watch([], options).await }
        };
        match timeout(METADATA_TIMEOUT, open).await {
            Ok(Ok(mut changes)) => {
                retry_delay = Duration::from_secs(5);
                println!("进度 watcher 已连接: {source:?}");
                while let Some(change) = changes.next().await {
                    match change {
                        Ok(change) => if let Some(request) = extract_request(&change, source) {
                            if tx.send(request).await.is_err() { return; }
                        },
                        Err(error) => {
                            eprintln!("⚠️ 进度 watcher 中断 [{source:?}]: {error}（将重连，定时刷新仍有效）");
                            break;
                        }
                    }
                }
            }
            Ok(Err(error)) => eprintln!("⚠️ 无法建立进度 watcher [{source:?}]: {error}（定时刷新兜底）"),
            Err(_) => eprintln!("⚠️ 建立进度 watcher 超时 [{source:?}]（定时刷新兜底）"),
        }
        if tx.is_closed() { return; }
        sleep(retry_delay).await;
        retry_delay = (retry_delay * 2).min(Duration::from_secs(60));
        // Snapshot refresh covers any changes missed while reconnecting.
        PROGRESS_REFRESH.notify_one();
    }
}

/// Keep collecting changes while bounded statistics are in flight. Repeated changes to an
/// in-flight collection remain dirty and receive a trailing refresh after it completes.
async fn dispatch_requests<F: Future<Output = ()>>(
    mut rx: mpsc::Receiver<ProgressRequest>, limit: usize, period: Duration,
    mut emit: impl FnMut(ProgressRequest) -> F,
) {
    let mut pending = HashMap::new();
    let mut in_flight = std::collections::HashSet::new();
    let mut active = futures_util::stream::FuturesUnordered::new();
    let mut flush = interval(period);
    flush.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut closed = false;
    loop {
        tokio::select! {
            request = rx.recv(), if !closed => match request {
                Some(request) => mark_dirty(&mut pending, request),
                None => closed = true,
            },
            completed = active.next(), if !active.is_empty() => {
                if let Some(collection) = completed { in_flight.remove(&collection); }
            },
            _ = flush.tick() => {
                while let Ok(request) = rx.try_recv() { mark_dirty(&mut pending, request); }
                let names: Vec<String> = pending.keys().filter(|name| !in_flight.contains(*name))
                    .take(limit.saturating_sub(active.len())).cloned().collect();
                for collection in names {
                    let source = pending.remove(&collection).unwrap();
                    in_flight.insert(collection.clone());
                    let future = emit(ProgressRequest { collection: collection.clone(), source });
                    active.push(async move { future.await; collection });
                }
            }
        }
        if closed && pending.is_empty() && active.is_empty() { break; }
    }
}

async fn watch_simulation_db_changes(app: AppHandle, clients: Arc<MongoClients>, config: AppConfig) {
    let (tx, rx) = mpsc::channel::<ProgressRequest>(1024);
    for source in [ProgressSource::LocalMission, ProgressSource::LocalData, ProgressSource::RemoteMission, ProgressSource::RemoteData] {
        if source.is_remote() && config.mongodb.local_uri == config.mongodb.remote_uri { continue; }
        let client = if source.is_remote() { clients.remote.clone() } else { clients.local.clone() };
        tokio::spawn(watch_source(client, config.clone(), source, tx.clone()));
    }
    drop(tx);
    dispatch_requests(rx, MAX_CONCURRENT_STATS, COALESCE_INTERVAL, |request| {
        let app = app.clone();
        let clients = clients.clone();
        let config = config.clone();
        async move { emit_progress(&app, &clients, &config, request).await; }
    }).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_versions_are_strictly_increasing() {
        let first = timestamp_ms();
        assert!(timestamp_ms() > first);
    }

    #[tokio::test]
    async fn refresh_loop_coalesces_requests_without_overlap() {
        let notification = Arc::new(Notify::new());
        let gate = Arc::new(Notify::new());
        let (tx, mut rx) = mpsc::channel(8);
        let worker_notification = notification.clone();
        let worker_gate = gate.clone();
        let worker = tokio::spawn(async move {
            refresh_loop(&worker_notification, Duration::from_secs(3600), || {
                let tx = tx.clone();
                let gate = worker_gate.clone();
                async move { tx.send(()).await.unwrap(); gate.notified().await; }
            }).await;
        });
        notification.notify_one();
        timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        for _ in 0..100 { notification.notify_one(); }
        assert!(timeout(Duration::from_millis(10), rx.recv()).await.is_err());
        gate.notify_one();
        timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        gate.notify_one();
        assert!(timeout(Duration::from_millis(10), rx.recv()).await.is_err());
        worker.abort();
    }

    #[tokio::test]
    async fn periodic_refresh_runs_without_any_change_stream_events() {
        let notification = Notify::new();
        let (tx, mut rx) = mpsc::channel(8);
        let worker = tokio::spawn(async move {
            refresh_loop(&notification, Duration::from_millis(5), || {
                let tx = tx.clone();
                async move { tx.send(()).await.unwrap(); }
            }).await;
        });
        for _ in 0..2 { timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap(); }
        worker.abort();
    }

    #[test]
    fn active_lookup_excludes_old_soft_deleted_tasks() {
        let filter = active_task_filter("same-name");
        assert_eq!(filter.get_str("name").unwrap(), "same-name");
        assert_eq!(filter.get_document("status").unwrap().get_str("$ne").unwrap(), "deactive");
        let id = bson::oid::ObjectId::new();
        let route = resolved_task(&doc! { "_id": id }, true).unwrap();
        assert_eq!(route.task_id, id.to_hex());
    }

    #[tokio::test]
    async fn slow_statistics_do_not_block_event_collection_or_trailing_refresh() {
        let (tx, rx) = mpsc::channel(8);
        let (started, mut observed) = mpsc::channel(8);
        let gate = Arc::new(Notify::new());
        let worker_gate = gate.clone();
        let worker = tokio::spawn(async move {
            dispatch_requests(rx, 2, Duration::from_millis(1), |request| {
                let started = started.clone();
                let gate = worker_gate.clone();
                async move {
                    started.send(request.collection.clone()).await.unwrap();
                    if request.collection == "slow" { gate.notified().await; }
                }
            }).await;
        });
        let request = |name: &str| ProgressRequest { collection: name.into(), source: ProgressSource::RemoteData };
        tx.send(request("slow")).await.unwrap();
        assert_eq!(timeout(Duration::from_secs(1), observed.recv()).await.unwrap().unwrap(), "slow");
        tx.send(request("slow")).await.unwrap(); // final update while previous query is blocked
        tx.send(request("healthy")).await.unwrap();
        assert_eq!(timeout(Duration::from_secs(1), observed.recv()).await.unwrap().unwrap(), "healthy");
        assert!(timeout(Duration::from_millis(10), observed.recv()).await.is_err());
        gate.notify_one();
        assert_eq!(timeout(Duration::from_secs(1), observed.recv()).await.unwrap().unwrap(), "slow");
        gate.notify_one();
        drop(tx);
        timeout(Duration::from_secs(1), worker).await.unwrap().unwrap();
    }

    #[test]
    fn local_registration_wins_over_event_origin() {
        assert_eq!(select_source(Some(&doc! { "isRemote": true }), ProgressSource::LocalData), Some(true));
        assert_eq!(select_source(Some(&doc! { "isRemote": false }), ProgressSource::RemoteData), Some(false));
        assert_eq!(select_source(Some(&doc! { "status": "deactive", "isRemote": true }), ProgressSource::RemoteData), None);
    }

    #[test]
    fn remote_only_events_do_not_fall_back_to_local() {
        assert_eq!(select_source(None, ProgressSource::RemoteData), Some(true));
        assert_eq!(select_source(None, ProgressSource::RemoteMission), Some(true));
        assert_eq!(select_source(None, ProgressSource::LocalData), None);
    }

    #[test]
    fn successful_empty_cursor_is_zero_but_invalid_counts_are_errors() {
        assert_eq!(parse_counts(None).unwrap(), ProgressCounts::default());
        assert!(parse_counts(Some(doc! { "total": 1 })).is_err());
        assert!(count_field(&doc! { "total": -1 }, "total").is_err());
    }

    #[test]
    fn serialized_progress_matches_the_frontend_contract() {
        let payload = TaskProgress {
            collection: "task".into(), task_id: "identity".into(),
            counts: ProgressCounts::default(), is_remote: true, updated_at: 123,
        };
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["task_id"], "identity");
        assert_eq!(json["is_remote"], true);
        assert_eq!(json["updated_at"], 123);
        for name in ["total", "success", "error", "priority_total", "priority_success", "priority_error"] {
            assert_eq!(json[name], 0);
        }
        assert!(json.get("counts").is_none()); // Counters are flattened, not nested.
    }

    #[test]
    fn accepts_int32_and_int64_counts() {
        let counts = parse_counts(Some(doc! {
            "total": 7196_i64, "success": 2136_i32, "error": 46_i64,
            "priority_total": 11_i32, "priority_success": 11_i64, "priority_error": 0_i32,
        })).unwrap();
        assert_eq!(counts.total, 7196);
        assert_eq!(counts.success - counts.priority_success, 2125);
        assert_eq!(counts.error, 46);
    }

    #[test]
    fn coalescing_retains_final_change_and_remote_origin() {
        let mut pending = HashMap::new();
        for source in [ProgressSource::RemoteData, ProgressSource::LocalData, ProgressSource::RemoteData] {
            mark_dirty(&mut pending, ProgressRequest { collection: "task".into(), source });
        }
        assert_eq!(pending.len(), 1);
        let requests: Vec<_> = pending.drain().collect();
        assert_eq!(requests[0].1, ProgressSource::RemoteData);
        mark_dirty(&mut pending, ProgressRequest { collection: "task".into(), source: ProgressSource::RemoteData });
        assert_eq!(pending.len(), 1); // Last event after a flush must schedule another flush.
    }

    #[tokio::test]
    async fn cursor_error_does_not_become_an_empty_collection() {
        let cursor = stream::iter(vec![Err(mongodb::error::Error::custom("read failed"))]);
        assert!(read_counts(cursor).await.is_err());
        let empty = stream::iter(Vec::<std::result::Result<Document, mongodb::error::Error>>::new());
        assert_eq!(read_counts(empty).await.unwrap(), ProgressCounts::default());
    }

    #[tokio::test]
    #[ignore = "read-only integration check requiring the user's configured MongoDBs"]
    async fn configured_remote_task_stats() {
        let config = crate::config::load_config(std::path::Path::new("config.toml")).unwrap();
        let clients = MongoClients::new(&config).await.unwrap();
        let task = resolve_source(&clients, "asi_d1_selected4_ideas", &config, ProgressSource::LocalMission).await.unwrap().unwrap();
        assert!(task.is_remote);
        assert!(!task.task_id.is_empty());
        let stats = query_counts(&clients.remote, "asi_d1_selected4_ideas", &config).await.unwrap();
        println!("Read-only remote stats: {stats:?}");
        assert!(stats.total > 0);
        assert!(stats.success > 0);
        assert!(stats.total >= stats.success + stats.error);
        assert!(stats.priority_success <= stats.success);
        let hello = clients.remote.database("admin").run_command(doc! { "hello": 1 }, None).await.unwrap();
        assert_eq!(hello.get_str("setName").unwrap(), "rs0");
    }

    #[tokio::test]
    async fn failures_and_timeouts_do_not_become_zero_statistics() {
        let failure: Result<ProgressCounts> = with_deadline(Duration::from_secs(1), async { Err(anyhow!("cursor failed")) }).await;
        assert!(failure.is_err());
        let timeout_result: Result<ProgressCounts> = with_deadline(Duration::from_millis(1), async {
            std::future::pending::<Result<ProgressCounts>>().await
        }).await;
        assert!(timeout_result.is_err());
    }
}
