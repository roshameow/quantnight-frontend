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
    #[serde(skip_serializing_if = "Option::is_none")]
    priority_campaign: Option<PriorityRuntime>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    priority_campaign: Option<PriorityRuntime>,
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
    registration: Document,
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
        registration: task.clone(),
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

// Priority metadata is streamed independently and cached as priority_campaign
// alongside numeric events. Never substitute summary.reserved for actual states.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct PrioritySourceSummary {
    source_task: String,
    source_remote: bool,
    root_count: usize,
    stage_distribution: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
struct PriorityRuntime {
    collection: String,
    task_id: String,
    is_remote: bool,
    updated_at: u64,
    auth_profile: String,
    // Compatibility campaign fields stay on the wire; per_root UI never uses
    // current_round as a task-global progress indicator.
    campaign_id: Option<String>,
    pool_id: Option<String>,
    mode: Option<String>,
    stage_distribution: Option<std::collections::BTreeMap<String, usize>>,
    root_count: Option<usize>,
    waiting_feedback: Option<usize>,
    waiting_slots: Option<usize>,
    completed: Option<usize>,
    blocked: Option<usize>,
    source_tasks: Vec<String>,
    source_summaries: Option<Vec<PrioritySourceSummary>>,
    stage: Option<String>,
    status: Option<String>,
    reason: Option<String>,
    current_round: Option<usize>,
    max_rounds: Option<usize>,
    max_per_round: Option<usize>,
    reserved: Option<usize>,
    unresolved: Option<usize>,
    data_source: String,
    stats_confirmed: bool,
    counts: Option<ProgressCounts>,
    stale: bool,
    last_error: Option<String>,
}

fn priority_config(task: &Document) -> Option<&Document> {
    task.get_document("mission_config").ok().or_else(|| task.get_document("startConfig").ok()).or_else(|| task.get_document("config").ok())
}

fn round_enabled(task: &Document) -> bool {
    (task.get_str("taskType").ok().or_else(|| task.get_str("task_type").ok()) == Some("priority"))
        && priority_config(task).and_then(|cfg| cfg.get_document("rounds").ok())
            .and_then(|rounds| rounds.get_bool("enabled").ok()) == Some(true)
}

fn priority_runtime(task: &ResolvedTask, name: &str, updated_at: u64) -> PriorityRuntime {
    PriorityRuntime {
        collection: name.into(), task_id: task.task_id.clone(), is_remote: task.is_remote, updated_at,
        auth_profile: task.registration.get_str("auth_profile").ok().filter(|s| !s.is_empty()).unwrap_or("user1").into(),
        pool_id: None, mode: None, stage_distribution: None, root_count: None,
        waiting_feedback: None, waiting_slots: None, completed: None, blocked: None,
        source_tasks: Vec::new(), source_summaries: None,
        campaign_id: None, stage: None, status: None, reason: None, current_round: None,
        max_rounds: priority_config(&task.registration).and_then(|cfg| cfg.get_document("rounds").ok())
            .and_then(|rounds| count_field(rounds, "max_rounds").ok()),
        max_per_round: priority_config(&task.registration).and_then(|cfg| cfg.get_document("rounds").ok())
            .and_then(|rounds| count_field(rounds, "max_simulations_per_round").ok()),
        reserved: None, unresolved: None, data_source: "unconfirmed".into(),
        stats_confirmed: false, counts: None, stale: true, last_error: None,
    }
}

fn priority_summary_filter(name: &str, auth: &str) -> Document {
    doc! { "_id": format!("{auth}:{name}") }
}

fn priority_ledger_filter(name: &str, auth: &str, campaign: &str) -> Document {
    doc! { "task": name, "auth_profile": auth, "campaign_id": campaign }
}

fn apply_summary(runtime: &mut PriorityRuntime, summary: Option<&Document>) -> Result<()> {
    let Some(summary) = summary else { return Ok(()); };
    runtime.campaign_id = Some(summary.get_str("campaign_id").context("Priority 登记记录缺少 campaign_id")?
        .to_owned());
    if runtime.campaign_id.as_deref() == Some("") { return Err(anyhow!("Priority 登记记录的 campaign_id 为空")); }
    for (name, target) in [("stage", &mut runtime.stage), ("status", &mut runtime.status), ("reason", &mut runtime.reason)] {
        *target = summary.get_str(name).ok().map(str::to_owned);
    }
    for (name, target) in [("current_round", &mut runtime.current_round), ("max_rounds", &mut runtime.max_rounds), ("max_per_round", &mut runtime.max_per_round), ("reserved", &mut runtime.reserved)] {
        if summary.contains_key(name) { *target = Some(count_field(summary, name)?); }
    }
    Ok(())
}

// Only a successful absent legacy summary establishes control-only pause.
// Existing campaigns (including stale summaries) must still resolve exact IDs;
// a lookup failure must never be mistaken for absence.
fn apply_legacy_monitor_summary(task: &Document, summary: Result<Option<Document>>,
    runtime: &mut PriorityRuntime) -> Result<bool> {
    let summary = summary?;
    if round_enabled(task) || runtime.pool_id.is_some() || runtime.mode.as_deref() == Some("per_root") {
        return Err(anyhow!("当前任务采用逐root模式，需要对应的root池进度记录"));
    }
    let control_only = summary.is_none()
        && task.get_str("taskType").ok().or_else(|| task.get_str("task_type").ok()) == Some("priority")
        && task.get_str("status").ok() == Some("paused");
    apply_summary(runtime, summary.as_ref())?;
    runtime.mode = Some("legacy".into());
    if control_only {
        runtime.stage = Some("paused".into());
        runtime.status = Some("paused".into());
        runtime.reason = Some("paused_legacy_without_ledger".into());
        runtime.data_source = "control_only".into();
        runtime.stats_confirmed = false;
        runtime.counts = None;
        runtime.unresolved = None;
        runtime.stale = true;
        runtime.last_error = None;
    }
    Ok(control_only)
}

fn root_stage_filter(name: &str, auth: &str, pool: &str) -> Document {
    doc! { "priority_task": name, "auth_profile": auth, "pool_id": pool, "retired": { "$ne": true } }
}

// All roots, including R0, belong to the logical source target, not the
// selected Priority host. These groups never define the exact-item denominator.
fn root_source_summaries(summary: &Document, name: &str, root_count: usize,
    distribution: &std::collections::BTreeMap<String, usize>) -> Result<Option<Vec<PrioritySourceSummary>>> {
    let rows = match summary.get("source_summaries") {
        None | Some(Bson::Null) => return Ok(None),
        Some(Bson::Array(rows)) => rows,
        _ => return Err(anyhow!("root池来源汇总格式错误：source_summaries需要数组")),
    };
    let mut groups = Vec::new();
    let mut bindings = std::collections::HashSet::new();
    let mut total = 0_usize;
    let mut stages = [0_usize; 4];
    for row in rows {
        let row = row.as_document().context("root池来源条目格式错误：需要对象")?;
        let source = row.get_str("source_task").context("root池来源条目缺少 source_task")?;
        if source.is_empty() || source == name || source.starts_with("system.") || source.contains(['$', '\0']) {
            return Err(anyhow!("root池来源任务名称校验失败：source_task格式或任务归属有误"));
        }
        let remote = row.get_bool("source_remote").context("root池来源条目缺少 source_remote")?;
        if !bindings.insert((source.to_owned(), remote)) { return Err(anyhow!("root池来源汇总包含重复的任务和运行主机")); }
        let count = count_field(row, "root_count")?;
        let stage_counts = row.get_document("stage_distribution")?;
        if stage_counts.len() != 4 { return Err(anyhow!("root池来源阶段统计需要包含0、1、2、3四个阶段")); }
        let mut stage_distribution = std::collections::BTreeMap::new();
        let mut sum = 0_usize;
        for (index, stage) in ["0", "1", "2", "3"].iter().enumerate() {
            let value = count_field(stage_counts, stage)?;
            sum = sum.checked_add(value).context("root池来源阶段数量求和超出数值范围")?;
            stages[index] = stages[index].checked_add(value).context("root池各来源阶段总数超出数值范围")?;
            stage_distribution.insert((*stage).to_owned(), value);
        }
        if sum != count { return Err(anyhow!("root池来源的阶段数量之和与root总数存在差异")); }
        total = total.checked_add(count).context("root池各来源root数量求和超出数值范围")?;
        groups.push(PrioritySourceSummary { source_task: source.into(), source_remote: remote,
            root_count: count, stage_distribution });
    }
    if total != root_count { return Err(anyhow!("各来源root数量之和与root池总数存在差异")); }
    for (index, stage) in ["0", "1", "2", "3"].iter().enumerate() {
        if stages[index] != distribution.get(*stage).copied().unwrap_or(0) {
            return Err(anyhow!("各来源阶段统计之和与root池阶段统计存在差异"));
        }
    }
    Ok(Some(groups))
}

fn apply_root_summary(runtime: &mut PriorityRuntime, summary: &Document) -> Result<()> {
    // A failed refresh must not expose old or partially validated source groups.
    runtime.source_summaries = None;
    if summary.get_str("_id").ok() != Some(&format!("{}:{}", runtime.auth_profile, runtime.collection))
        || summary.get_str("mode").ok() != Some("per_root") {
        return Err(anyhow!("Priority root池身份或模式校验失败"));
    }
    let pool = summary.get_str("pool_id").context("root池登记记录缺少 pool_id")?;
    if pool.is_empty() { return Err(anyhow!("root池登记记录的 pool_id 为空")); }
    let distribution = summary.get_document("stage_distribution")?
        .iter().map(|(key, _)| {
            if !["0", "1", "2", "3"].contains(&key.as_str()) { return Err(anyhow!("root池阶段统计字段有误：有效阶段为0、1、2、3")); }
            Ok((key.clone(), count_field(summary.get_document("stage_distribution")?, key)?))
        }).collect::<Result<_>>()?;
    let root_count = count_field(summary, "root_count")?;
    let source_summaries = root_source_summaries(summary, &runtime.collection, root_count, &distribution)?;
    runtime.pool_id = Some(pool.into());
    runtime.mode = Some("per_root".into());
    runtime.stage_distribution = Some(distribution);
    runtime.root_count = Some(root_count);
    for (key, target) in [("waiting_feedback", &mut runtime.waiting_feedback),
        ("waiting_slots", &mut runtime.waiting_slots), ("completed", &mut runtime.completed),
        ("blocked", &mut runtime.blocked)] {
        if summary.contains_key(key) { *target = Some(count_field(summary, key)?); }
    }
    runtime.status = summary.get_str("status").ok().map(str::to_owned);
    runtime.reason = summary.get_str("reason").ok().map(str::to_owned);
    if let Ok(names) = summary.get_array("source_tasks") {
        runtime.source_tasks = names.iter().map(|name| name.as_str().filter(|s| !s.is_empty())
            .map(str::to_owned).ok_or_else(|| anyhow!("root池来源任务列表格式错误：需要有效任务名"))).collect::<Result<_>>()?;
    }
    // No task-level round or denominator maximum; registry counts are metadata.
    runtime.current_round = None;
    runtime.reserved = None;
    runtime.source_summaries = source_summaries;
    Ok(())
}

#[derive(Debug)]
struct RootLedgerSelection {
    by_source: std::collections::BTreeMap<(bool, String), Vec<bson::oid::ObjectId>>,
    total: usize,
}

// Root origin is immutable across stages. A same ID on a different execution
// DB is a distinct entry; hashes never substitute for exact entry IDs.
fn root_ledger_selection(rows: &[Document], name: &str, auth: &str, pool: &str) -> Result<RootLedgerSelection> {
    let mut unique = std::collections::BTreeMap::new();
    let mut roots = std::collections::BTreeMap::new();
    let mut stages = std::collections::BTreeMap::new();
    for row in rows {
        for (key, value) in [("priority_task", name), ("auth_profile", auth), ("pool_id", pool)] {
            if row.get_str(key).ok() != Some(value) { return Err(anyhow!("root阶段账本归属校验失败：{key}与当前任务记录存在差异")); }
        }
        let root = row.get_str("root_alpha_id").context("root阶段账本缺少 root_alpha_id")?;
        if root.is_empty() { return Err(anyhow!("root的alpha ID为空")); }
        let source = row.get_str("source_task").context("root阶段账本缺少 source_task")?;
        if source.is_empty() || source == name || source.starts_with("system.") || source.contains(['$', '\0']) {
            return Err(anyhow!("root池来源任务名称校验失败：source_task格式或任务归属有误"));
        }
        let remote = row.get_bool("source_remote").context("root阶段账本缺少 source_remote")?;
        let binding = (remote, source.to_owned());
        if let Some(previous) = roots.insert(root.to_owned(), binding.clone()) {
            if previous != binding { return Err(anyhow!("同一root对应了多个来源任务或运行主机")); }
        }
        let number = count_field(row, "number")?;
        if !(1..=3).contains(&number) { return Err(anyhow!("root阶段编号需要为1、2或3")); }
        let entries = row.get_array("entries").context("root阶段账本缺少候选条目明细")?;
        if entries.len() > 60 { return Err(anyhow!("root单阶段候选条目超过60条")); }
        // Duplicate stage records must be byte-for-byte identical, not extend a
        // reserved stage. There is deliberately no pool-wide 60/180 check.
        if let Some(previous) = stages.insert((root.to_owned(), number), entries.clone()) {
            if previous != *entries { return Err(anyhow!("同一root阶段的候选登记内容存在差异")); }
        }
        for entry in entries {
            let entry = entry.as_document().context("root候选条目格式错误：需要对象")?;
            if entry.get_str("collection").ok() != Some(source) {
                return Err(anyhow!("root候选的集合与登记来源任务存在差异"));
            }
            let id = match entry.get("item_id") {
                Some(Bson::ObjectId(id)) => *id,
                Some(Bson::String(id)) => bson::oid::ObjectId::parse_str(id).context("root候选的 item_id 格式错误")?,
                _ => return Err(anyhow!("root候选的 item_id 需要ObjectId或十六进制ID")),
            };
            let hash = entry.get_str("payload_hash").context("root候选条目缺少 payload_hash")?;
            if hash.is_empty() { return Err(anyhow!("root候选的 payload_hash 为空")); }
            if let Some(previous) = unique.insert((remote, source.to_owned(), id), hash.to_owned()) {
                if previous != hash { return Err(anyhow!("同一root候选ID对应了多个表达式与参数摘要")); }
            }
        }
    }
    let total = unique.len();
    let mut by_source = std::collections::BTreeMap::<_, Vec<_>>::new();
    for ((remote, source, id), _) in unique { by_source.entry((remote, source)).or_default().push(id); }
    Ok(RootLedgerSelection { by_source, total })
}

// CP/ledger host and immutable source execution host are separate selections.
fn select_host<'a, T>(local: &'a T, remote: &'a T, is_remote: bool) -> &'a T {
    if is_remote { remote } else { local }
}

fn missing_root_pool(runtime: &mut PriorityRuntime, registration: &Document) -> Result<ProgressCounts> {
    runtime.mode = Some("per_root".into());
    runtime.reason = Some("root_pool_not_registered".into());
    let host = if runtime.is_remote { "远程" } else { "本地" };
    // Describe the recorded stage; numeric progress still requires the root pool.
    let awaiting_start = registration.get_str("status").ok() == Some("pending")
        && !["startedAt", "launchAttemptId", "launchAttemptAt", "launchHistory",
             "launchAttempts", "launchReceipt", "startReceipt", "consumerPid", "simulatorRuntime"]
            .iter().any(|field| registration.contains_key(*field));
    if awaiting_start {
        Err(anyhow!("{host} Priority 任务已登记，等待启动"))
    } else {
        Err(anyhow!("{host} Priority 任务已登记，等待进度记录（root池）"))
    }
}

async fn query_root_counts(clients: &MongoClients, client: &Client, name: &str, config: &AppConfig,
    runtime: &mut PriorityRuntime) -> Result<ProgressCounts> {
    let rows: Vec<Document> = client.database(&config.mongodb.databases.mission)
        .collection::<Document>("priority_root_stages")
        .find(root_stage_filter(name, &runtime.auth_profile, runtime.pool_id.as_deref().unwrap()), None)
        .await?.try_collect().await?;
    let selection = root_ledger_selection(&rows, name, &runtime.auth_profile, runtime.pool_id.as_deref().unwrap())?;
    let mut observed = ProgressCounts::default();
    for ((remote, source), ids) in selection.by_source {
        let execution = select_host(&clients.local, &clients.remote, remote);
        let counts = read_counts(execution.database(&config.mongodb.databases.simulation)
            .collection::<Document>(&source).aggregate(priority_stats_pipeline(Some(&ids)), None).await?).await?;
        observed.total += counts.total;
        observed.success += counts.success;
        observed.error += counts.error;
        if !runtime.source_tasks.contains(&source) { runtime.source_tasks.push(source); }
    }
    runtime.source_tasks.sort();
    runtime.source_tasks.dedup();
    let (counts, unresolved) = reserved_counts(selection.total, observed)?;
    runtime.unresolved = Some(unresolved);
    runtime.data_source = "root_ledger".into();
    Ok(counts)
}

#[derive(Debug)]
struct LedgerSelection {
    by_collection: std::collections::BTreeMap<String, Vec<bson::oid::ObjectId>>,
    total: usize,
    current_round: usize,
}

// Pure BSON parser. Exact IDs, not payload hashes or priority flags, define the
// denominator. Same ID in different collections is distinct; reused IDs aren't.
fn ledger_selection(rows: &[Document], name: &str, auth: &str, campaign: &str) -> Result<LedgerSelection> {
    let mut unique = std::collections::BTreeMap::new();
    let mut current_round = 0;
    for row in rows {
        for (key, value) in [("task", name), ("auth_profile", auth), ("campaign_id", campaign)] {
            if row.get_str(key).ok() != Some(value) { return Err(anyhow!("Priority阶段账本归属校验失败：{key}与当前任务记录存在差异")); }
        }
        let number = count_field(row, "number")?;
        if number == 0 { return Err(anyhow!("Priority账本轮次编号需要为正整数")); }
        current_round = current_round.max(number);
        for entry in row.get_array("entries").context("Priority阶段账本缺少候选条目 entries")? {
            let entry = entry.as_document().context("Priority候选条目格式错误：需要对象")?;
            let collection = entry.get_str("collection").context("Priority候选条目缺少 collection")?;
            if collection.is_empty() || collection.starts_with("system.") || collection.contains(['$', '\0']) {
                return Err(anyhow!("Priority候选的集合名称格式错误"));
            }
            let id = match entry.get("item_id") {
                Some(Bson::ObjectId(id)) => *id,
                Some(Bson::String(id)) => bson::oid::ObjectId::parse_str(id).context("Priority候选的 item_id 格式错误")?,
                _ => return Err(anyhow!("Priority候选的 item_id 需要ObjectId或十六进制ID")),
            };
            let hash = entry.get_str("payload_hash").context("Priority候选条目缺少 payload_hash")?;
            if hash.is_empty() { return Err(anyhow!("Priority候选的 payload_hash 为空")); }
            let key = (collection.to_owned(), id);
            if let Some(previous) = unique.insert(key, hash.to_owned()) {
                if previous != hash { return Err(anyhow!("同一Priority候选ID对应了多个表达式与参数摘要")); }
            }
        }
    }
    let total = unique.len();
    let mut by_collection = std::collections::BTreeMap::<_, Vec<_>>::new();
    for ((collection, id), _) in unique { by_collection.entry(collection).or_default().push(id); }
    Ok(LedgerSelection { by_collection, total, current_round })
}

const PRIORITY_FAILURES: [&str; 8] = ["fail", "failed", "error", "permanent_error", "timeout_error", "cancelled", "canceled", "rejected"];

fn priority_stats_pipeline(ids: Option<&[bson::oid::ObjectId]>) -> Vec<Document> {
    let mut pipeline = Vec::new();
    if let Some(ids) = ids { pipeline.push(doc! { "$match": { "_id": { "$in": ids.to_vec() } } }); }
    // Monitoring loss is UNKNOWN even if legacy code stored a terminal-looking status.
    let unknown = doc! { "$or": [
        { "$eq": ["$terminalStatus", "UNKNOWN"] },
        { "$eq": ["$acceptance", "UNKNOWN"] },
        { "$eq": ["$errorClass", "campaign_outcome_unknown"] },
        { "$eq": ["$errorClass", "campaign_rate_limited"] },
    ] };
    pipeline.push(doc! { "$set": { "_priority_state": { "$cond": [unknown, "unknown",
        { "$toLower": { "$convert": { "input": "$status", "to": "string", "onError": "", "onNull": "" } } }
    ] } } });
    let mut group = stats_pipeline().remove(0);
    let fields = group.get_document_mut("$group").unwrap();
    let success = doc! { "$sum": { "$cond": [{ "$eq": ["$_priority_state", "success"] }, 1, 0] } };
    let error = doc! { "$sum": { "$cond": [{ "$in": ["$_priority_state", PRIORITY_FAILURES.to_vec()] }, 1, 0] } };
    fields.insert("success", success.clone());
    fields.insert("error", error.clone());
    fields.insert("priority_success", success);
    fields.insert("priority_error", error);
    fields.insert("priority_total", doc! { "$sum": 1 });
    pipeline.push(group);
    pipeline
}

fn reserved_counts(total: usize, observed: ProgressCounts) -> Result<(ProgressCounts, usize)> {
    if observed.total > total || observed.success + observed.error > observed.total {
        return Err(anyhow!("Priority实际状态计数超出账本登记数量"));
    }
    let unresolved = total - observed.success - observed.error;
    Ok((ProgressCounts { total, priority_total: total, priority_success: observed.success,
        priority_error: observed.error, ..observed }, unresolved))
}

fn confirmed_priority_fallback(counts: ProgressCounts) -> Result<(ProgressCounts, usize)> {
    if counts.total == 0 {
        return Err(anyhow!("Priority阶段账本缺失，当前任务集合为空，待取得进度记录"));
    }
    reserved_counts(counts.total, counts)
}

async fn query_priority_counts(client: &Client, name: &str, config: &AppConfig, runtime: &mut PriorityRuntime) -> Result<ProgressCounts> {
    let simulation = client.database(&config.mongodb.databases.simulation);
    let rows: Vec<Document> = if let Some(campaign) = runtime.campaign_id.as_deref() {
        client.database(&config.mongodb.databases.mission).collection::<Document>("priority_campaign_rounds")
            .find(priority_ledger_filter(name, &runtime.auth_profile, campaign), None).await?.try_collect().await?
    } else { Vec::new() };
    if rows.is_empty() {
        // Transitional fallback only to the task's own collection. Never adopt all
        // priority_collection items, nor pretend a missing old ledger is empty work.
        if runtime.reserved.unwrap_or(0) > 0 {
            return Err(anyhow!("Priority已有登记候选，阶段账本缺少对应的候选ID明细"));
        }
        let counts = read_counts(simulation.collection::<Document>(name)
            .aggregate(priority_stats_pipeline(None), None).await?).await?;
        let (counts, unresolved) = confirmed_priority_fallback(counts)?;
        runtime.unresolved = Some(unresolved);
        runtime.data_source = "own_task_fallback".into();
        return Ok(counts);
    }
    let selection = ledger_selection(&rows, name, &runtime.auth_profile, runtime.campaign_id.as_deref().unwrap())?;
    if selection.total == 0 { return Err(anyhow!("Priority阶段账本的候选ID列表为空，待取得进度记录")); }
    runtime.current_round = Some(runtime.current_round.unwrap_or(0).max(selection.current_round));
    let mut observed = ProgressCounts::default();
    for (collection, ids) in selection.by_collection {
        let counts = read_counts(simulation.collection::<Document>(&collection)
            .aggregate(priority_stats_pipeline(Some(&ids)), None).await?).await?;
        observed.total += counts.total;
        observed.success += counts.success;
        observed.error += counts.error;
    }
    let (counts, unresolved) = reserved_counts(selection.total, observed)?;
    runtime.unresolved = Some(unresolved);
    runtime.data_source = "ledger".into();
    Ok(counts)
}

fn count_field(document: &Document, name: &str) -> Result<usize> {
    let value = match document.get(name) {
        Some(Bson::Int32(n)) => i64::from(*n),
        Some(Bson::Int64(n)) => *n,
        _ => return Err(anyhow!("统计字段 {name} 格式错误：需要整数")),
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
        let cursor = collection.aggregate(stats_pipeline(), None).await.context("MongoDB任务统计查询失败")?;
        read_counts(cursor).await
    }).await
}

async fn emit_progress(app: &AppHandle, clients: &MongoClients, config: &AppConfig, request: ProgressRequest) {
    let updated_at = timestamp_ms();
    let mut is_remote = None;
    let mut task_id = None;
    let mut priority_campaign = None;
    let result: Result<()> = async {
        let Some(task) = resolve_source(clients, &request.collection, config, request.source).await? else {
            return Ok(()); // Deleted tasks and unregistered data collections are not cards.
        };
        is_remote = Some(task.is_remote);
        task_id = Some(task.task_id.clone());
        let client = select_host(&clients.local, &clients.remote, task.is_remote);
        let counts = if task.registration.get_str("taskType").ok().or_else(|| task.registration.get_str("task_type").ok()) == Some("priority") {
            let mut runtime = priority_runtime(&task, &request.collection, updated_at);
            let result = with_deadline(PROGRESS_TIMEOUT, async {
                // Root registry/stages live on the selected Priority run host.
                // Feedback below still follows each root's immutable source_remote.
                let mission = client.database(&config.mongodb.databases.mission);
                let filter = priority_summary_filter(&request.collection, &runtime.auth_profile);
                let root_pool = mission.collection::<Document>("priority_root_pools")
                    .find_one(filter.clone(), None).await?;
                if let Some(summary) = root_pool.as_ref() {
                    apply_root_summary(&mut runtime, summary)?;
                    let _ = app.emit("task-priority-runtime-update", &runtime);
                    query_root_counts(clients, client, &request.collection, config, &mut runtime).await.map(Some)
                } else if round_enabled(&task.registration) {
                    missing_root_pool(&mut runtime, &task.registration).map(Some)
                } else {
                    // Read-only legacy diagnosis. No migration, requeue, reset,
                    // standalone consumer or RuntimeJob redispatch from watcher.
                    let legacy = client.database(&config.mongodb.databases.mission).collection::<Document>("priority_campaign_tasks")
                        .find_one(filter, None).await.context("查询旧版Priority登记记录失败");
                    if apply_legacy_monitor_summary(&task.registration, legacy, &mut runtime)? {
                        return Ok(None); // No numeric/zero-count event or refresh-error log.
                    }
                    let _ = app.emit("task-priority-runtime-update", &runtime);
                    query_priority_counts(client, &request.collection, config, &mut runtime).await.map(Some)
                }
            }).await;
            match result {
                Ok(None) => {
                    let _ = app.emit("task-priority-runtime-update", &runtime);
                    return Ok(());
                }
                Ok(Some(counts)) => {
                    runtime.stats_confirmed = true;
                    runtime.counts = Some(counts.clone());
                    runtime.stale = !["ledger", "root_ledger"].contains(&runtime.data_source.as_str());
                    if runtime.stale { runtime.last_error = Some("阶段账本缺失；当前统计范围为本任务集合".into()); }
                    let _ = app.emit("task-priority-runtime-update", &runtime);
                    priority_campaign = Some(runtime);
                    counts
                }
                Err(error) => {
                    runtime.stale = true;
                    runtime.last_error = Some(format!("{error:#}"));
                    let _ = app.emit("task-priority-runtime-update", &runtime);
                    priority_campaign = Some(runtime);
                    return Err(error);
                }
            }
        } else {
            query_counts(client, &request.collection, config).await?
        };
        app.emit("task-progress-update", TaskProgress {
            collection: request.collection.clone(), task_id: task.task_id, counts,
            is_remote: task.is_remote, updated_at, priority_campaign: priority_campaign.clone(),
        })?;
        Ok(())
    }.await;
    if let Err(error) = result {
        let message = format!("{error:#}");
        eprintln!("⚠️ 任务进度读取提示 [{}]: {}", request.collection, message);
        let _ = app.emit("task-progress-error", TaskProgressError {
            collection: request.collection, task_id, is_remote, error: message, updated_at, priority_campaign,
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
            eprintln!("⚠️ 本地任务进度列表读取失败: {error:#}");
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
                eprintln!("⚠️ 远程任务列表读取失败: {error:#}（下一轮刷新重试）");
                emit_discovery_error(&app, "remote", &error, remote_discovery_version);
            }
        }
    };
    tokio::join!(emit_requests(&app, &clients, &config, local_requests), refresh_remote);
}

fn extract_request(change: &ChangeStreamEvent<Document>, source: ProgressSource) -> Option<ProgressRequest> {
    if source.is_mission() && change.ns.as_ref().and_then(|ns| ns.coll.as_deref()) != Some("tasks") {
        if matches!(change.ns.as_ref().and_then(|ns| ns.coll.as_deref()),
            Some("priority_root_pools" | "priority_root_stages" | "priority_campaign_tasks" | "priority_campaign_rounds")) {
            PROGRESS_REFRESH.notify_one();
        }
        return None;
    }
    let collection = if source.is_mission() {
        change.full_document.as_ref()?.get_str("name").ok()?.to_owned()
    } else {
        change.ns.as_ref()?.coll.clone()?
    };
    if !source.is_mission() { PROGRESS_REFRESH.notify_one(); }
    if collection.starts_with("system.") { return None; }
    Some(ProgressRequest { collection, source })
}

/// Retain dirty state until the next flush; never drop a collection's final change.
fn mark_dirty(pending: &mut HashMap<String, ProgressSource>, request: ProgressRequest) {
    pending.entry(request.collection).and_modify(|source| {
        if request.source.is_remote() { *source = request.source; }
    }).or_insert(request.source);
}

async fn watch_source(app: AppHandle, client: Arc<Client>, config: AppConfig, source: ProgressSource, tx: mpsc::Sender<ProgressRequest>) {
    let mut retry_delay = Duration::from_secs(5);
    loop {
        let db_name = if source.is_mission() { &config.mongodb.databases.mission } else { &config.mongodb.databases.simulation };
        let db = client.database(db_name);
        let options = ChangeStreamOptions::builder()
            .full_document(Some(FullDocumentType::UpdateLookup))
            .max_await_time(Some(Duration::from_secs(1))).build();
        let open = async {
            db.watch([], options).await
        };
        match timeout(METADATA_TIMEOUT, open).await {
            Ok(Ok(mut changes)) => {
                retry_delay = Duration::from_secs(5);
                println!("进度 watcher 已连接: {source:?}");
                while let Some(change) = changes.next().await {
                    match change {
                        Ok(change) => {
                            if source.is_mission() && change.ns.as_ref().and_then(|ns| ns.coll.as_deref()) == Some("tasks") {
                                // CLI/background generation updates control status too.
                                // Notify the UI to re-read its authoritative task list.
                                let _ = app.emit("task-registry-changed", serde_json::json!({"remote": source.is_remote()}));
                            }
                            if let Some(request) = extract_request(&change, source) {
                                if tx.send(request).await.is_err() { return; }
                            }
                        },
                        Err(error) => {
                            eprintln!("⚠️ 进度 watcher 中断 [{source:?}]: {error}（将重连，定时刷新仍有效）");
                            break;
                        }
                    }
                }
            }
            Ok(Err(error)) => eprintln!("⚠️ 进度事件订阅建立失败 [{source:?}]: {error}（定时刷新继续运行）"),
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
        tokio::spawn(watch_source(app.clone(), client, config.clone(), source, tx.clone()));
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
    fn priority_task() -> Document {
        doc! { "_id": bson::oid::ObjectId::new(), "taskType": "priority", "auth_profile": "user1",
            "mission_config": { "rounds": { "enabled": true, "max_rounds": 3 } } }
    }

    fn ledger_row(collection: &str, ids: &[bson::oid::ObjectId], number: i32) -> Document {
        let entries: Vec<_> = ids.iter().map(|id| Bson::Document(doc! {
            "collection": collection, "item_id": id.to_hex(), "payload_hash": format!("hash-{id}")
        })).collect();
        doc! { "task": "pipeline__priority", "auth_profile": "user1", "campaign_id": "campaign", "number": number, "entries": entries }
    }

    #[test]
    fn priority_round_detection_and_summary_are_pure_metadata() {
        let mut task = priority_task();
        assert!(round_enabled(&task));
        task.get_document_mut("mission_config").unwrap().get_document_mut("rounds").unwrap().insert("enabled", false);
        assert!(!round_enabled(&task));
        task.remove("mission_config");
        task.insert("config", doc! { "rounds": { "enabled": true } });
        assert!(round_enabled(&task));
        task.insert("taskType", "regular");
        assert!(!round_enabled(&task));
        let route = resolved_task(&priority_task(), true).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        apply_summary(&mut runtime, Some(&doc! { "campaign_id": "campaign", "stage": "feedback",
            "status": "blocked", "reason": "feedback_incomplete_or_wait_timeout", "current_round": 1,
            "max_rounds": 3, "max_per_round": 60, "reserved": 60 })).unwrap();
        assert_eq!(runtime.status.as_deref(), Some("blocked"));
        assert_eq!(runtime.reserved, Some(60));
        assert_eq!(runtime.max_rounds, Some(3));
        assert_eq!(runtime.max_per_round, Some(60));
        assert!(!runtime.stats_confirmed);
        assert!(runtime.counts.is_none()); // summary cannot manufacture real statistics
        let (pending_counts, unresolved) = reserved_counts(60, ProgressCounts::default()).unwrap();
        runtime.counts = Some(pending_counts.clone());
        runtime.unresolved = Some(unresolved);
        runtime.stats_confirmed = true;
        let payload = TaskProgress { collection: runtime.collection.clone(), task_id: runtime.task_id.clone(),
            counts: pending_counts, is_remote: true, updated_at: 100, priority_campaign: Some(runtime.clone()) };
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["total"], 60);
        assert_eq!(json["error"], 0);
        assert_eq!(json["priority_campaign"]["auth_profile"], "user1");
        assert_eq!(json["priority_campaign"]["max_rounds"], 3);
        assert_eq!(json["priority_campaign"]["max_per_round"], 60);
        assert_eq!(json["priority_campaign"]["status"], "blocked");
        assert_eq!(json["priority_campaign"]["counts"]["total"], 60);
        assert_eq!(json["priority_campaign"]["counts"]["error"], 0);
        assert_eq!(priority_summary_filter("pipeline__priority", "user1"), doc! { "_id": "user1:pipeline__priority" });
        assert_eq!(priority_ledger_filter("pipeline__priority", "user1", "campaign"),
            doc! { "task": "pipeline__priority", "auth_profile": "user1", "campaign_id": "campaign" });
        assert!(apply_summary(&mut runtime, Some(&doc! { "reserved": 60 })).is_err());
    }

    #[test]
    fn ledger_preserves_old_sixty_ids_and_deduplicates_across_rounds() {
        let ids: Vec<_> = (0..60).map(|_| bson::oid::ObjectId::new()).collect();
        let old = ledger_row("priority_collection", &ids, 1);
        let repeat = ledger_row("priority_collection", &ids[0..10], 2);
        let new_id = bson::oid::ObjectId::new();
        let new = ledger_row("pipeline__priority", &[new_id], 2);
        let selection = ledger_selection(&[old, repeat, new], "pipeline__priority", "user1", "campaign").unwrap();
        assert_eq!(selection.total, 61);
        assert_eq!(selection.current_round, 2);
        assert_eq!(selection.by_collection["priority_collection"].len(), 60);
        assert_eq!(selection.by_collection["pipeline__priority"], vec![new_id]);
        let same_id_elsewhere = ledger_row("pipeline__priority", &[ids[0]], 2);
        let selection = ledger_selection(&[ledger_row("priority_collection", &ids, 1), same_id_elsewhere],
            "pipeline__priority", "user1", "campaign").unwrap();
        assert_eq!(selection.total, 61);
    }

    #[test]
    fn ledger_rejects_unscoped_old_malformed_and_conflicting_entries() {
        let id = bson::oid::ObjectId::new();
        let good = ledger_row("priority_collection", &[id], 1);
        for key in ["task", "auth_profile", "campaign_id", "entries", "number"] {
            let mut invalid = good.clone(); invalid.remove(key);
            assert!(ledger_selection(&[invalid], "pipeline__priority", "user1", "campaign").is_err(), "{key}");
        }
        let mut invalid = good.clone();
        invalid.get_array_mut("entries").unwrap()[0].as_document_mut().unwrap().insert("item_id", "bad");
        assert!(ledger_selection(&[invalid], "pipeline__priority", "user1", "campaign").is_err());
        let mut conflict = good.clone();
        conflict.get_array_mut("entries").unwrap()[0].as_document_mut().unwrap().insert("payload_hash", "different");
        assert!(ledger_selection(&[good.clone(), conflict], "pipeline__priority", "user1", "campaign").is_err());
        let mut native = good.clone();
        native.get_array_mut("entries").unwrap()[0].as_document_mut().unwrap().insert("item_id", id);
        assert_eq!(ledger_selection(&[good, native], "pipeline__priority", "user1", "campaign").unwrap().total, 1);
    }

    fn root_row(root: &str, source: &str, remote: bool, number: i32, ids: &[bson::oid::ObjectId]) -> Document {
        let mut row = ledger_row(source, ids, number);
        row.remove("task"); row.remove("campaign_id");
        row.insert("priority_task", "pipeline__priority");
        row.insert("pool_id", "pool-a");
        row.insert("root_alpha_id", root);
        row.insert("source_task", source);
        row.insert("source_remote", remote);
        row
    }

    #[test]
    fn priority_cp_host_selection_does_not_rebind_mixed_source_feedback() {
        let local = "local";
        let remote = "remote";
        for cp_remote in [false, true] {
            let mut registration = priority_task();
            registration.insert("isRemote", cp_remote);
            let route = resolved_task(&registration, cp_remote).unwrap();
            assert!(std::ptr::eq(select_host(&local, &remote, route.is_remote),
                if cp_remote { &remote } else { &local }));
            let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
            let ids = [bson::oid::ObjectId::new(), bson::oid::ObjectId::new()];
            let rows = vec![
                root_row("root-local", "source-local", false, 1, &ids[..1]),
                root_row("root-remote", "source-remote", true, 1, &ids[1..]),
            ];
            let selection = root_ledger_selection(&rows, "pipeline__priority", "user1", "pool-a").unwrap();
            assert_eq!(selection.total, 2);
            for ((source_remote, source), source_ids) in selection.by_source {
                assert_eq!(*select_host(&local, &remote, source_remote),
                    if source == "source-remote" { "remote" } else { "local" });
                assert_eq!(source_ids.len(), 1);
            }
            // Missing selected-host root registry is authoritative absence, not
            // permission to consult an old local/remote global campaign.
            assert!(missing_root_pool(&mut runtime, &registration).is_err());
            assert_eq!(runtime.is_remote, cp_remote);
            assert_eq!(runtime.reason.as_deref(), Some("root_pool_not_registered"));
            assert_eq!(runtime.mode.as_deref(), Some("per_root"));
            assert!(runtime.campaign_id.is_none());
            assert!(runtime.counts.is_none());
            assert!(!runtime.stats_confirmed);
        }
    }

    #[test]
    fn missing_root_pool_explains_selected_host_and_recorded_stage() {
        for remote in [false, true] {
            let host = if remote { "远程" } else { "本地" };
            for status in ["pending", "running", "paused"] {
                let mut task = priority_task();
                task.insert("isRemote", remote);
                task.insert("status", status);
                let mut runtime = priority_runtime(&resolved_task(&task, remote).unwrap(), "pipeline__priority", 100);
                let message = missing_root_pool(&mut runtime, &task).unwrap_err().to_string();
                let expected = if status == "pending" {
                    format!("{host} Priority 任务已登记，等待启动")
                } else {
                    format!("{host} Priority 任务已登记，等待进度记录（root池）")
                };
                assert_eq!(message, expected);
                assert_eq!(runtime.reason.as_deref(), Some("root_pool_not_registered"));
                assert!(runtime.counts.is_none());
                assert!(!runtime.stats_confirmed);
            }
            let mut task = priority_task();
            task.insert("isRemote", remote);
            task.insert("status", "pending");
            task.insert("startedAt", "2026-10-23T00:00:00Z");
            let mut runtime = priority_runtime(&resolved_task(&task, remote).unwrap(), "pipeline__priority", 100);
            assert_eq!(missing_root_pool(&mut runtime, &task).unwrap_err().to_string(),
                format!("{host} Priority 任务已登记，等待进度记录（root池）"));
        }
    }

    #[test]
    fn root_pool_summary_is_distribution_metadata_not_global_round_or_denominator() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        let summary = doc! { "_id": "user1:pipeline__priority", "mode": "per_root", "pool_id": "pool-a",
            "stage_distribution": { "0": 1, "1": 5, "2": 3, "3": 2 }, "root_count": 11,
            "waiting_feedback": 5, "waiting_slots": 3, "completed": 2, "blocked": 0,
            "source_tasks": ["source-a", "source-b"], "status": "active", "reserved": 600, "current_round": 1 };
        apply_root_summary(&mut runtime, &summary).unwrap();
        assert_eq!(runtime.root_count, Some(11));
        assert_eq!(runtime.stage_distribution.as_ref().unwrap()["0"], 1);
        assert_eq!(runtime.stage_distribution.as_ref().unwrap()["2"], 3);
        assert_eq!(runtime.waiting_slots, Some(3));
        assert_eq!(runtime.current_round, None);
        assert_eq!(runtime.reserved, None);
        assert!(runtime.counts.is_none());
        assert_eq!(root_stage_filter("pipeline__priority", "user1", "pool-a"),
            doc! { "priority_task": "pipeline__priority", "auth_profile": "user1", "pool_id": "pool-a", "retired": { "$ne": true } });
        for (key, value) in [("_id", "other:task"), ("mode", "legacy"), ("pool_id", "")] {
            let mut bad = summary.clone(); bad.insert(key, value);
            assert!(apply_root_summary(&mut runtime, &bad).is_err());
        }
    }

    fn source_summary(source: &str, remote: bool, stages: [i64; 4]) -> Document {
        doc! { "source_task": source, "source_remote": remote, "root_count": stages.iter().sum::<i64>(),
            "stage_distribution": { "0": stages[0], "1": stages[1], "2": stages[2], "3": stages[3] } }
    }

    fn grouped_root_summary() -> Document {
        doc! { "_id": "user1:pipeline__priority", "mode": "per_root", "pool_id": "pool-a",
            "root_count": 7, "stage_distribution": { "0": 3, "1": 2, "2": 1, "3": 1 },
            "source_tasks": ["source-a", "source-b"], "source_summaries": [
                source_summary("source-a", false, [1, 2, 0, 1]),
                source_summary("source-b", true, [2, 0, 1, 0]) ] }
    }

    #[test]
    fn source_summaries_include_r0_without_changing_exact_item_counts_and_serialize() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        assert!(runtime.source_summaries.is_none());
        let counts = ProgressCounts { total: 3, success: 1, error: 1, ..Default::default() };
        runtime.counts = Some(counts.clone());
        runtime.stats_confirmed = true;
        apply_root_summary(&mut runtime, &grouped_root_summary()).unwrap();
        let groups = runtime.source_summaries.as_ref().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].source_task, "source-a");
        assert!(!groups[0].source_remote);
        assert_eq!(groups[0].root_count, 4);
        assert_eq!(groups[0].stage_distribution["0"], 1);
        assert_eq!(groups[1].source_task, "source-b");
        assert!(groups[1].source_remote);
        assert_eq!(groups[1].root_count, 3);
        assert_eq!(groups[1].stage_distribution["0"], 2);
        assert_eq!(runtime.counts.as_ref(), Some(&counts));
        assert!(runtime.stats_confirmed);
        assert_eq!(runtime.current_round, None);
        assert_eq!(runtime.reserved, None);
        let payload = TaskProgress { collection: runtime.collection.clone(), task_id: runtime.task_id.clone(),
            counts, is_remote: false, updated_at: 100, priority_campaign: Some(runtime) };
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["total"], 3);
        assert_eq!(json["priority_campaign"]["root_count"], 7);
        assert_eq!(json["priority_campaign"]["counts"]["total"], 3);
        assert_eq!(json["priority_campaign"]["source_summaries"], serde_json::json!([
            { "source_task": "source-a", "source_remote": false, "root_count": 4,
                "stage_distribution": { "0": 1, "1": 2, "2": 0, "3": 1 } },
            { "source_task": "source-b", "source_remote": true, "root_count": 3,
                "stage_distribution": { "0": 2, "1": 0, "2": 1, "3": 0 } }
        ]));
    }

    #[test]
    fn same_source_name_logical_targets_stay_separate_when_priority_host_switches() {
        let mut summary = grouped_root_summary();
        summary.get_array_mut("source_summaries").unwrap()[1].as_document_mut().unwrap()
            .insert("source_task", "source-a");
        let mut previous = None;
        for cp_remote in [false, true] {
            let route = resolved_task(&priority_task(), cp_remote).unwrap();
            let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
            apply_root_summary(&mut runtime, &summary).unwrap();
            let groups = runtime.source_summaries.as_ref().unwrap();
            assert_eq!(runtime.is_remote, cp_remote);
            assert_eq!(groups[0].source_task, groups[1].source_task);
            assert!(!groups[0].source_remote);
            assert!(groups[1].source_remote);
            assert_eq!(groups[0].root_count, 4);
            assert_eq!(groups[1].root_count, 3);
            if let Some(previous) = previous { assert_eq!(runtime.source_summaries, previous); }
            previous = Some(runtime.source_summaries);
        }
    }

    #[test]
    fn old_missing_or_null_source_summaries_remain_optional() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        for value in [None, Some(Bson::Null)] {
            apply_root_summary(&mut runtime, &grouped_root_summary()).unwrap();
            let mut old = grouped_root_summary();
            old.remove("source_summaries");
            if let Some(value) = value { old.insert("source_summaries", value); }
            apply_root_summary(&mut runtime, &old).unwrap();
            assert!(runtime.source_summaries.is_none());
            assert!(serde_json::to_value(&runtime).unwrap()["source_summaries"].is_null());
            assert!(runtime.counts.is_none());
        }
        let mut summary = grouped_root_summary();
        summary.insert("root_count", 0);
        summary.insert("stage_distribution", doc! {});
        summary.insert("source_summaries", Vec::<Bson>::new());
        apply_root_summary(&mut runtime, &summary).unwrap();
        assert_eq!(runtime.source_summaries, Some(Vec::new()));
        // Global zero stages may be omitted by older producers; group stages may not.
        summary.insert("root_count", 1);
        summary.insert("stage_distribution", doc! { "0": 1 });
        summary.insert("source_summaries", vec![source_summary("source-a", false, [1, 0, 0, 0])]);
        apply_root_summary(&mut runtime, &summary).unwrap();
        assert_eq!(runtime.source_summaries.as_ref().unwrap()[0].stage_distribution["3"], 0);
    }

    #[test]
    fn malformed_source_summaries_are_rejected_before_publishing_any_groups() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        let good = grouped_root_summary();
        let group = source_summary("source-a", false, [1, 2, 0, 1]);
        let mut invalid = Vec::new();
        for value in [Bson::String("bad".into()), Bson::Document(doc! {}), Bson::Boolean(false)] {
            let mut bad = good.clone(); bad.insert("source_summaries", value); invalid.push(bad);
        }
        let mut bad = good.clone(); bad.insert("source_summaries", vec![Bson::Null]); invalid.push(bad);
        let mut bad = good.clone(); bad.get_array_mut("source_summaries").unwrap().push(Bson::Document(group.clone())); invalid.push(bad);
        for key in ["source_task", "source_remote", "root_count", "stage_distribution"] {
            let mut bad_group = group.clone(); bad_group.remove(key);
            let mut bad = good.clone(); bad.get_array_mut("source_summaries").unwrap()[0] = Bson::Document(bad_group); invalid.push(bad);
        }
        for source in ["", "pipeline__priority", "system.roots", "bad$name", "bad\0name"] {
            let mut bad = good.clone();
            bad.get_array_mut("source_summaries").unwrap()[0].as_document_mut().unwrap().insert("source_task", source);
            invalid.push(bad);
        }
        for (key, values) in [
            ("source_task", vec![Bson::Null, Bson::Int32(1)]),
            ("source_remote", vec![Bson::Null, Bson::Int32(0), Bson::String("false".into())]),
            ("root_count", vec![Bson::Null, Bson::Int32(-1), Bson::Int64(-1), Bson::Double(4.0), Bson::Boolean(true), Bson::String("4".into())]),
            ("stage_distribution", vec![Bson::Null, Bson::Array(vec![])])
        ] {
            for value in values {
                let mut bad = good.clone();
                bad.get_array_mut("source_summaries").unwrap()[0].as_document_mut().unwrap().insert(key, value);
                invalid.push(bad);
            }
        }
        for key in ["0", "1", "2", "3"] {
            let mut bad_group = group.clone(); bad_group.get_document_mut("stage_distribution").unwrap().remove(key);
            let mut bad = good.clone(); bad.get_array_mut("source_summaries").unwrap()[0] = Bson::Document(bad_group); invalid.push(bad);
            for value in [Bson::Null, Bson::Int32(-1), Bson::Int64(-1), Bson::Double(1.0), Bson::Boolean(true), Bson::String("1".into())] {
                let mut bad = good.clone();
                bad.get_array_mut("source_summaries").unwrap()[0].as_document_mut().unwrap()
                    .get_document_mut("stage_distribution").unwrap().insert(key, value);
                invalid.push(bad);
            }
        }
        for replace in [false, true] {
            let mut bad = good.clone();
            let stages = bad.get_array_mut("source_summaries").unwrap()[0].as_document_mut().unwrap()
                .get_document_mut("stage_distribution").unwrap();
            if replace { stages.remove("3"); }
            stages.insert("4", 1);
            invalid.push(bad);
        }
        // A later metadata failure must also prevent validated groups from leaking.
        let mut bad = good.clone(); bad.insert("waiting_slots", -1); invalid.push(bad);
        let mut bad = good.clone(); bad.insert("source_tasks", vec![Bson::Null]); invalid.push(bad);
        for (index, bad) in invalid.iter().enumerate() {
            apply_root_summary(&mut runtime, &good).unwrap();
            assert!(apply_root_summary(&mut runtime, bad).is_err(), "case {index}: {bad:?}");
            assert!(runtime.source_summaries.is_none(), "case {index}");
            assert!(runtime.counts.is_none());
            assert!(!runtime.stats_confirmed);
        }
    }

    #[test]
    fn inconsistent_source_summary_totals_never_survive_a_stale_refresh() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        let good = grouped_root_summary();
        let mut invalid = Vec::new();
        let mut bad = good.clone(); bad.get_array_mut("source_summaries").unwrap()[1].as_document_mut().unwrap()
            .insert("root_count", 4); invalid.push(bad);
        let mut bad = good.clone(); bad.insert("root_count", 8); invalid.push(bad);
        let mut bad = good.clone(); bad.get_document_mut("stage_distribution").unwrap().insert("0", 4); invalid.push(bad);
        let mut bad = good.clone(); bad.get_document_mut("stage_distribution").unwrap().remove("0"); invalid.push(bad);
        let mut bad = good.clone(); bad.insert("source_summaries", Vec::<Bson>::new()); invalid.push(bad);
        for bad in invalid {
            apply_root_summary(&mut runtime, &good).unwrap();
            let error = apply_root_summary(&mut runtime, &bad).unwrap_err();
            runtime.stale = true;
            runtime.last_error = Some(error.to_string());
            let payload = TaskProgressError { collection: runtime.collection.clone(), task_id: Some(runtime.task_id.clone()),
                is_remote: Some(runtime.is_remote), error: error.to_string(), updated_at: 101, priority_campaign: Some(runtime.clone()) };
            let json = serde_json::to_value(payload).unwrap();
            assert_eq!(json["priority_campaign"]["stale"], true);
            assert!(json["priority_campaign"]["source_summaries"].is_null());
            assert!(json["priority_campaign"]["counts"].is_null());
        }
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn source_summary_checked_sums_reject_overflow() {
        let route = resolved_task(&priority_task(), false).unwrap();
        let mut runtime = priority_runtime(&route, "pipeline__priority", 100);
        let mut summary = grouped_root_summary();
        summary.insert("root_count", i64::MAX);
        summary.insert("stage_distribution", doc! { "0": i64::MAX });
        let mut overflowing = source_summary("source-a", false, [0, 0, 0, 0]);
        overflowing.insert("root_count", i64::MAX);
        overflowing.insert("stage_distribution", doc! { "0": i64::MAX, "1": i64::MAX, "2": 2_i64, "3": 0 });
        summary.insert("source_summaries", vec![overflowing]);
        assert!(apply_root_summary(&mut runtime, &summary).unwrap_err().to_string().contains("来源阶段数量求和超出数值范围"));
        assert!(runtime.source_summaries.is_none());
        for same_stage in [false, true] {
            let groups: Vec<_> = (0..3).map(|index| {
                let mut stages = [0; 4]; stages[if same_stage { 0 } else { index }] = i64::MAX;
                source_summary(&format!("source-{index}"), false, stages)
            }).collect();
            summary.insert("source_summaries", groups);
            let error = apply_root_summary(&mut runtime, &summary).unwrap_err().to_string();
            assert!(error.contains(if same_stage { "各来源阶段总数超出数值范围" } else { "各来源root数量求和超出数值范围" }), "{error}");
            assert!(runtime.source_summaries.is_none());
        }
    }

    #[test]
    fn roots_independent_more_than_180_and_route_by_origin_execution_db() {
        let ids: Vec<_> = (0..60).map(|_| bson::oid::ObjectId::new()).collect();
        let rows = vec![root_row("r1", "source-a", false, 1, &ids),
            root_row("r2", "source-a", true, 2, &ids), root_row("r3", "source-b", false, 3, &ids),
            root_row("r4", "source-b", true, 1, &ids)];
        let selection = root_ledger_selection(&rows, "pipeline__priority", "user1", "pool-a").unwrap();
        assert_eq!(selection.total, 240);
        assert_eq!(selection.by_source.len(), 4);
        let (counts, unresolved) = reserved_counts(selection.total, ProgressCounts::default()).unwrap();
        assert_eq!(counts.total, 240); assert_eq!(unresolved, 240);
        let mut repeat = rows.clone(); repeat.push(rows[0].clone());
        assert_eq!(root_ledger_selection(&repeat, "pipeline__priority", "user1", "pool-a").unwrap().total, 240);
    }

    #[test]
    fn root_stage_immutable_source_ids_hash_scope_and_local_limits() {
        let id = bson::oid::ObjectId::new();
        let good = root_row("r1", "source-a", false, 1, &[id]);
        for key in ["priority_task", "auth_profile", "pool_id", "root_alpha_id", "source_task", "source_remote", "number", "entries"] {
            let mut bad = good.clone(); bad.remove(key);
            assert!(root_ledger_selection(&[bad], "pipeline__priority", "user1", "pool-a").is_err(), "{key}");
        }
        for number in [0, 4] {
            let mut bad = good.clone(); bad.insert("number", number);
            assert!(root_ledger_selection(&[bad], "pipeline__priority", "user1", "pool-a").is_err());
        }
        let too_many = (0..61).map(|_| bson::oid::ObjectId::new()).collect::<Vec<_>>();
        assert!(root_ledger_selection(&[root_row("r1", "source-a", false, 1, &too_many)], "pipeline__priority", "user1", "pool-a").is_err());
        let changed = root_row("r1", "source-b", true, 2, &[id]);
        assert!(root_ledger_selection(&[good.clone(), changed], "pipeline__priority", "user1", "pool-a").is_err());
        let mut wrongroute = good.clone();
        wrongroute.get_array_mut("entries").unwrap()[0].as_document_mut().unwrap().insert("collection", "priority_collection");
        assert!(root_ledger_selection(&[wrongroute], "pipeline__priority", "user1", "pool-a").is_err());
        let mut changed = good.clone();
        changed.get_array_mut("entries").unwrap()[0].as_document_mut().unwrap().insert("payload_hash", "changed");
        assert!(root_ledger_selection(&[good, changed], "pipeline__priority", "user1", "pool-a").is_err());
    }

    // Tiny offline BSON expression evaluator exercises the actual pipeline shape,
    // not a second implementation of terminal-status classification.
    fn eval(expr: &Bson, row: &Document) -> Bson {
        match expr {
            Bson::String(path) if path.starts_with('$') => row.get(&path[1..]).cloned().unwrap_or(Bson::Null),
            Bson::Document(op) => {
                let (name, arg) = op.iter().next().unwrap();
                let args = || arg.as_array().unwrap().iter().map(|e| eval(e, row)).collect::<Vec<_>>();
                match name.as_str() {
                    "$eq" => { let a = args(); Bson::Boolean(a[0] == a[1]) },
                    "$or" => Bson::Boolean(args().iter().any(|v| v.as_bool() == Some(true))),
                    "$in" => { let a = arg.as_array().unwrap(); Bson::Boolean(a[1].as_array().unwrap().contains(&eval(&a[0], row))) },
                    "$cond" => { let a = arg.as_array().unwrap(); eval(&a[if eval(&a[0], row).as_bool() == Some(true) { 1 } else { 2 }], row) },
                    "$toLower" => Bson::String(eval(arg, row).as_str().unwrap_or("").to_lowercase()),
                    "$convert" => { let d = arg.as_document().unwrap(); let v = eval(d.get("input").unwrap(), row);
                        if matches!(v, Bson::String(_)) { v } else { Bson::String("".into()) } },
                    _ => panic!("unsupported expression {name}"),
                }
            }
            _ => expr.clone(),
        }
    }

    fn fake_counts(ids: &[bson::oid::ObjectId], rows: Vec<Document>) -> ProgressCounts {
        let pipeline = priority_stats_pipeline(Some(ids));
        let exact = pipeline[0].get_document("$match").unwrap().get_document("_id").unwrap().get_array("$in").unwrap();
        let expression = pipeline[1].get_document("$set").unwrap().get("_priority_state").unwrap();
        let group = pipeline[2].get_document("$group").unwrap();
        let mut totals = doc! { "total": 0, "success": 0, "error": 0, "priority_total": 0, "priority_success": 0, "priority_error": 0 };
        for mut row in rows.into_iter().filter(|row| exact.contains(row.get("_id").unwrap())) {
            row.insert("_priority_state", eval(expression, &row));
            for key in ["total", "success", "error", "priority_total", "priority_success", "priority_error"] {
                let arg = group.get_document(key).unwrap().get("$sum").unwrap();
                let count = eval(arg, &row).as_i32().unwrap();
                let old = totals.get_i32(key).unwrap(); totals.insert(key, old + count);
            }
        }
        parse_counts(Some(totals)).unwrap()
    }

    fn legacy_paused_task() -> Document {
        let mut task = priority_task();
        task.remove("mission_config");
        task.insert("status", "paused");
        task
    }

    #[test]
    fn paused_legacy_absent_summary_is_control_only_not_confirmed_zero() {
        for remote in [false, true] {
            let task = legacy_paused_task();
            let route = resolved_task(&task, remote).unwrap();
            let mut runtime = priority_runtime(&route, "sign_flip__priority", 100);
            assert!(apply_legacy_monitor_summary(&task, Ok(None), &mut runtime).unwrap());
            let json = serde_json::to_value(&runtime).unwrap();
            assert_eq!(json["status"], "paused");
            assert_eq!(json["stage"], "paused");
            assert_eq!(json["mode"], "legacy");
            assert_eq!(json["reason"], "paused_legacy_without_ledger");
            assert_eq!(json["data_source"], "control_only");
            assert_eq!(json["stats_confirmed"], false);
            assert_eq!(json["stale"], true);
            for key in ["counts", "last_error", "unresolved", "campaign_id", "pool_id"] {
                assert!(json[key].is_null(), "{key}");
            }
            assert_eq!(json["is_remote"], remote);
            // Repeated refreshes remain metadata-only, never a refresh failure.
            assert!(apply_legacy_monitor_summary(&task, Ok(None), &mut runtime).unwrap());
            assert!(runtime.last_error.is_none());
        }
    }

    #[test]
    fn legacy_running_missing_and_paused_existing_campaign_still_observe_counts() {
        let mut task = legacy_paused_task();
        task.insert("status", "running");
        let mut runtime = priority_runtime(&resolved_task(&task, true).unwrap(), "sign_flip__priority", 100);
        assert!(!apply_legacy_monitor_summary(&task, Ok(None), &mut runtime).unwrap());
        assert!(confirmed_priority_fallback(ProgressCounts::default()).is_err());

        task.insert("status", "paused");
        // A stale/blocked campaign is NOT proof that accepted jobs stopped.
        let summary = doc! { "campaign_id": "campaign", "status": "blocked",
            "reason": "feedback_incomplete_or_wait_timeout", "reserved": 1 };
        assert!(!apply_legacy_monitor_summary(&task, Ok(Some(summary)), &mut runtime).unwrap());
        assert_eq!(runtime.campaign_id.as_deref(), Some("campaign"));
        assert_eq!(runtime.status.as_deref(), Some("blocked"));
        let id = bson::oid::ObjectId::new();
        let selection = ledger_selection(&[ledger_row("priority_collection", &[id], 1)],
            "pipeline__priority", "user1", runtime.campaign_id.as_deref().unwrap()).unwrap();
        assert_eq!(selection.by_collection["priority_collection"], vec![id]);
        let (counts, unresolved) = reserved_counts(selection.total, ProgressCounts {
            total: 1, success: 1, ..Default::default()
        }).unwrap();
        assert_eq!(counts.success, 1);
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn paused_legacy_lookup_errors_and_malformed_summary_are_not_control_only() {
        for status in ["paused", "running"] {
            let mut task = legacy_paused_task();
            task.insert("status", status);
            for message in ["network disconnected", "查询超时（15s）"] {
                let mut runtime = priority_runtime(&resolved_task(&task, true).unwrap(), "sign_flip__priority", 100);
                let error = apply_legacy_monitor_summary(&task, Err(anyhow!("{message}")), &mut runtime).unwrap_err();
                assert_eq!(error.to_string(), message);
                assert_ne!(runtime.data_source, "control_only");
            }
            let mut runtime = priority_runtime(&resolved_task(&task, true).unwrap(), "sign_flip__priority", 100);
            assert!(apply_legacy_monitor_summary(&task, Ok(Some(doc! { "reserved": 60 })), &mut runtime).is_err());
            assert_ne!(runtime.data_source, "control_only");
        }
    }

    #[test]
    fn paused_root_mode_never_falls_back_to_legacy_control_only() {
        let mut task = priority_task();
        task.insert("status", "paused");
        let mut runtime = priority_runtime(&resolved_task(&task, false).unwrap(), "pipeline__priority", 100);
        assert!(apply_legacy_monitor_summary(&task, Ok(None), &mut runtime).is_err());
        assert!(missing_root_pool(&mut runtime, &task).is_err());
        assert_eq!(runtime.reason.as_deref(), Some("root_pool_not_registered"));
        assert_ne!(runtime.data_source, "control_only");
        let legacy_task = legacy_paused_task();
        for pool in [None, Some("pool-a".to_owned())] {
            runtime.mode = Some("per_root".into());
            runtime.pool_id = pool;
            assert!(apply_legacy_monitor_summary(&legacy_task, Ok(None), &mut runtime).is_err());
            assert_eq!(runtime.mode.as_deref(), Some("per_root"));
        }
    }

    #[test]
    fn missing_ledger_empty_fallback_is_not_confirmed_zero_progress() {
        assert!(confirmed_priority_fallback(ProgressCounts::default()).is_err());
        let (counts, unresolved) = confirmed_priority_fallback(ProgressCounts {
            total: 3, success: 1, error: 1, ..Default::default()
        }).unwrap();
        assert_eq!(counts.total, 3);
        assert_eq!(unresolved, 1);
    }

    #[test]
    fn actual_bson_pipeline_counts_rejected_400_but_not_unknown_or_cooling_429() {
        let ids: Vec<_> = (0..8).map(|_| bson::oid::ObjectId::new()).collect();
        let rejected = doc! { "_id": ids[0], "status": "rejected",
            "terminalStatus": "NOT_ACCEPTED", "acceptance": "NOT_ACCEPTED", "httpStatus": 400,
            "errorClass": "campaign_validation_rejected", "rejection_evidence_received": true };
        let mut rows = vec![rejected.clone()];
        for (i, key, value) in [(1, "terminalStatus", "UNKNOWN"), (2, "acceptance", "UNKNOWN"),
            (3, "errorClass", "campaign_outcome_unknown")] {
            let mut unknown = rejected.clone();
            unknown.insert("_id", ids[i]);
            unknown.insert(key, value);
            rows.push(unknown);
        }
        for (i, status) in [(4, "quota_deferred"), (5, "failed"), (6, "rejected")] {
            let mut cooling = rejected.clone();
            cooling.insert("_id", ids[i]);
            cooling.insert("status", status);
            cooling.insert("httpStatus", 429);
            cooling.insert("errorClass", "campaign_rate_limited");
            rows.push(cooling);
        }
        // Each uncertainty marker wins even over a terminal-looking rejected/failed status.
        for (i, row) in rows.iter().enumerate() {
            let counts = fake_counts(&ids, vec![row.clone()]);
            assert_eq!(counts.total, 1);
            assert_eq!(counts.success, 0);
            assert_eq!(counts.error, usize::from(i == 0), "row {i}: {row:?}");
            assert_eq!(counts.priority_error, counts.error);
        }
        let mut unowned = rejected;
        unowned.insert("_id", bson::oid::ObjectId::new());
        rows.push(unowned); // exact-ID selection must not adopt unrelated rejections
        let observed = fake_counts(&ids, rows);
        assert_eq!(observed.total, 7);
        assert_eq!(observed.success, 0);
        assert_eq!(observed.error, 1);
        let (counts, unresolved) = reserved_counts(ids.len(), observed).unwrap();
        assert_eq!(counts.total, 8); // one missing exact-ID row remains unresolved
        assert_eq!(counts.priority_total, 8);
        assert_eq!(counts.priority_error, 1);
        assert_eq!(unresolved, 7); // six UNKNOWN/cooling rows plus the missing row
    }

    #[test]
    fn actual_bson_pipeline_keeps_unknown_missing_and_retryable_out_of_terminal_counts() {
        let ids: Vec<_> = (0..60).map(|_| bson::oid::ObjectId::new()).collect();
        let statuses = ["success", "permanent_error", "cancelled", "timeout_error", "error", "fail", "failed",
            "transient_error", "quota_deferred", "pending", "processing", "UNKNOWN", "success", "error", "SUCCESS", "timeout_error", "failed", "failed"];
        let mut rows: Vec<_> = statuses.iter().enumerate().map(|(i, status)| doc! { "_id": ids[i], "status": *status }).collect();
        rows[12].insert("terminalStatus", "UNKNOWN");
        rows[13].insert("errorClass", "campaign_outcome_unknown");
        rows[15].insert("terminalStatus", "UNKNOWN");
        rows[16].insert("acceptance", "UNKNOWN");
        rows[17].insert("errorClass", "campaign_rate_limited");
        rows.push(doc! { "_id": bson::oid::ObjectId::new(), "status": "success" }); // not ours
        let observed = fake_counts(&ids, rows);
        assert_eq!(observed.total, 18);
        assert_eq!(observed.success, 2);
        assert_eq!(observed.error, 6);
        let (counts, unresolved) = reserved_counts(60, observed).unwrap();
        assert_eq!(counts.total, 60); // includes all 44 missing rows, never 0 from empty task-name collection
        assert_eq!(counts.priority_total, 60);
        assert_eq!(unresolved, 52);
        assert_eq!(fake_counts(&ids, vec![]), ProgressCounts::default());
        let (missing, unknown) = reserved_counts(60, ProgressCounts::default()).unwrap();
        assert_eq!(missing.total, 60);
        assert_eq!(unknown, 60);
        assert!(reserved_counts(1, ProgressCounts { total: 2, ..Default::default() }).is_err());
        assert!(reserved_counts(1, ProgressCounts { total: 1, success: 1, error: 1, ..Default::default() }).is_err());
    }



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
            counts: ProgressCounts::default(), is_remote: true, updated_at: 123, priority_campaign: None,
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
