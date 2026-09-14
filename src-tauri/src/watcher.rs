// std
use std::{collections::HashMap, sync::Arc, time::{Duration, Instant}};

// external crates
use bson::{doc, Document};
use futures_util::future::join_all;
use futures_util::stream::StreamExt;
use mongodb::{
    change_stream::{ChangeStream, event::ChangeStreamEvent},
    options::{ChangeStreamOptions, FullDocumentType},
    Client,
};
use serde::Serialize;
use tauri::{AppHandle, Result as TauriResult, State, Emitter};
use tokio::sync::{mpsc::{channel, Sender}, Mutex};
use tokio::time::timeout;

// local crate
use crate::mongo_manager::MongoClients;
use crate::config::AppConfig;



#[derive(Debug, Serialize, Clone)]
struct TaskProgress {
    collection: String,
    success: usize,
    error: usize,
    total: usize,
    priority_success: Option<usize>,
    priority_error: Option<usize>,
    priority_total: Option<usize>,
    is_remote: bool,
}

static TASKS_WATCH_STARTED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// 远程统计/变更流硬超时：隧道慢/挂时快速失败并发零值，避免启动 emit 和进度更新被拖住
const REMOTE_PROGRESS_TIMEOUT: Duration = Duration::from_secs(2);

/// 前端 ready 时调用，启动 watcher
#[tauri::command]
pub async fn frontend_ready(
    app: AppHandle,
    mongo_clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<(), String> {
    // 从 State 中正确提取内部的 Arc<T>
    let mongo_clients_arc = mongo_clients.inner().clone();
    let local = mongo_clients_arc.local.clone();
    let remote = mongo_clients_arc.remote.clone();

    // 启动时异步地 emit 所有集合的初始状态
    let app_clone = app.clone();
    let config_clone = config.inner().clone();
    let mongo_clients_clone_for_spawn = mongo_clients_arc.clone();
    tauri::async_runtime::spawn(async move {
        emit_all_collections_once(app_clone, mongo_clients_clone_for_spawn, config_clone).await;
    });

    // 确保 watcher 只启动一次
    if TASKS_WATCH_STARTED.set(()).is_ok() {
        let app_clone = app.clone();
        let config_clone = config.inner().clone();
        tauri::async_runtime::spawn(async move {
            watch_simulation_db_changes(app_clone, local, remote, config_clone).await;
        });
        println!("🔥 simulation_db 全局 watcher 已启动");
    } else {
        println!("全局 watcher 已启动，跳过重复启动");
    }

    Ok(())
}

/// 从 ChangeStreamEvent 中提取 collection 名称（mission/task 或 simulation_db）
fn extract_collection_name(change: &ChangeStreamEvent<Document>, source: &str) -> Option<String> {
    match source {
        "mission" => change.full_document.as_ref()
            .and_then(|doc| doc.get_str("name").ok())
            .map(|s| s.to_string())
            .or_else(|| {
                println!("⚠️ mission event full_document missing: {:?}", change);
                None
            }),
        "db" => {
            let coll = change.ns.as_ref()?.coll.clone()?;
            if coll.starts_with("system.") {
                None
            } else {
                Some(coll)
            }
        }
        _ => None,
    }
}

/// 根据 collection 名称 emit 任务进度。
/// `is_remote_override`：Some(true/false) 时跳过本地查询直接指定（用于远程独有任务）；None 时自动查询本地任务表。
pub async fn emit_progress_for_collection(
    app: AppHandle,
    client_local: Arc<Client>,
    client_remote: Arc<Client>,
    collection_name: String,
    config: AppConfig,
    is_remote_override: Option<bool>,
) -> TauriResult<()> {

    // ① 确定 isRemote
    let is_remote = match is_remote_override {
        Some(v) => v,
        None => {
            let db_mission = client_local.database(&config.mongodb.databases.mission);
            let tasks_coll = db_mission.collection::<Document>("tasks");

            let filter = doc! {
                "name": &collection_name,
                "status": { "$ne": "deactive" }
            };

            match tasks_coll.find_one(filter, None).await {
                Ok(Some(doc)) => doc.get_bool("isRemote").unwrap_or(false),
                Ok(None) => {
                    println!("⚠️ 未找到任务 [{}]，默认 is_remote=false", collection_name);
                    false
                }
                Err(e) => {
                    eprintln!("❌ 查询 isRemote 失败 [{}]: {}", collection_name, e);
                    false
                }
            }
        }
    };

    // ② 根据 isRemote 选择 DB
    let db = if is_remote {
        client_remote.database(&config.mongodb.databases.simulation)
    } else {
        client_local.database(&config.mongodb.databases.simulation)
    };
    let collection = db.collection::<Document>(&collection_name);

    // ③ 查询统计：使用 aggregate 一次性获取所有数量。
    // 远程统计走 SSH 隧道，可能慢/不可用：加超时保护，失败/超时发零值而不是卡住。
    let pipeline = vec![
        doc! { "$group": {
            "_id": null,
            // 所有任务总数
            "total": { "$sum": 1 },
            "success": { "$sum": { "$cond": [ { "$eq": ["$status", "success"] }, 1, 0 ] } },
            "error": { "$sum": { "$cond": [ { "$in": ["$status", ["fail", "error"]] }, 1, 0 ] } },
            // priority 条件统计
            "priority_total": { "$sum": { "$cond": [ { "$gt": ["$priority", 0] }, 1, 0 ] } },
            "priority_success": { "$sum": { "$cond": [
                { "$and": [
                    { "$eq": ["$status", "success"] },
                    { "$gt": ["$priority", 0] }
                ]}, 1, 0] } },
            "priority_error": { "$sum": { "$cond": [
                { "$and": [
                    { "$in": ["$status", ["fail", "error"]] },
                    { "$gt": ["$priority", 0] }
                ]}, 1, 0] } },
        }}
    ];

    let timeout_dur = if is_remote { REMOTE_PROGRESS_TIMEOUT } else { Duration::from_secs(5) };
    let (total, success, error, priority_total, priority_success, priority_error) =
        match timeout(timeout_dur, async {
            let mut agg_cursor = collection.aggregate(pipeline, None).await.map_err(|e| e.to_string())?;
            if let Some(Ok(doc)) = agg_cursor.next().await {
                Ok::<_, String>((
                    doc.get_i32("total").unwrap_or(0) as usize,
                    doc.get_i32("success").unwrap_or(0) as usize,
                    doc.get_i32("error").unwrap_or(0) as usize,
                    doc.get_i32("priority_total").unwrap_or(0) as usize,
                    doc.get_i32("priority_success").unwrap_or(0) as usize,
                    doc.get_i32("priority_error").unwrap_or(0) as usize,
                ))
            } else {
                Ok((0, 0, 0, 0, 0, 0))
            }
        })
        .await
        {
            Ok(Ok(stats)) => stats,
            Ok(Err(e)) => {
                eprintln!("⚠️ 统计任务进度失败 [{}]: {}", collection_name, e);
                (0, 0, 0, 0, 0, 0)
            }
            Err(_) => {
                eprintln!("⚠️ 统计任务进度超时 [{}]（{}s），发零值", collection_name, timeout_dur.as_secs());
                (0, 0, 0, 0, 0, 0)
            }
        };

    // ④ emit
    let payload = TaskProgress {
        collection: collection_name.clone(),
        success,
        error,
        total,
        is_remote,
        priority_success: Some(priority_success),
        priority_error: Some(priority_error),
        priority_total: Some(priority_total),
    };

    app.emit("task-progress-update", payload)?;

    Ok(())
}

/// 启动时一次性 emit 所有 collection 的任务进度（并行，远程任务受超时保护）
async fn emit_all_collections_once(
    app: AppHandle,
    mongo_clients: Arc<MongoClients>,
    config: AppConfig,
) {
    let db_local = mongo_clients.local.database(&config.mongodb.databases.mission);
    let collection = db_local.collection::<Document>("tasks");

    println!("🔥 初始 emit 任务进度 for tasks 集合");

    let filter = doc! { "status": { "$ne": "deactive" } };
    let mut futures = Vec::new();
    let mut emitted = std::collections::HashSet::new();

    // ① 本地任务：自动查询 isRemote
    if let Ok(mut cursor) = collection.find(filter.clone(), None).await {
        while let Some(Ok(doc)) = cursor.next().await {
            let name = doc.get_str("name").unwrap_or("未知").to_string();
            println!("✅ 已 emit 任务进度 for 任务: {}", name);
            emitted.insert(name.clone());
            futures.push(emit_progress_for_collection(
                app.clone(),
                mongo_clients.local.clone(),
                mongo_clients.remote.clone(),
                name,
                config.clone(),
                None,
            ));
        }
    } else {
        eprintln!("查询 {} 集合失败", &config.mongodb.databases.mission);
    }

    // ② 远程独有任务（agent 在 remote 直接生成的，只注册在 remote mission.tasks）：
    // 轻量查询远程任务名（2s 超时），给未在本地的任务也发初始进度。
    let remote_names = timeout(REMOTE_PROGRESS_TIMEOUT, async {
        let remote_db = mongo_clients.remote.database(&config.mongodb.databases.mission);
        let remote_coll = remote_db.collection::<Document>("tasks");
        let mut cursor = remote_coll.find(filter.clone(), None).await.map_err(|e| e.to_string())?;
        let mut names = Vec::new();
        while let Some(Ok(doc)) = cursor.next().await {
            if let Ok(name) = doc.get_str("name") {
                names.push(name.to_string());
            }
        }
        Ok::<_, String>(names)
    })
    .await;

    match remote_names {
        Ok(Ok(remote_names)) => {
            for name in remote_names {
                if !emitted.contains(&name) {
                    println!("✅ 已 emit 远程任务进度 for 任务: {}", name);
                    futures.push(emit_progress_for_collection(
                        app.clone(),
                        mongo_clients.local.clone(),
                        mongo_clients.remote.clone(),
                        name,
                        config.clone(),
                        Some(true),
                    ));
                }
            }
        }
        _ => {
            eprintln!("⚠️ 远程任务初始进度跳过（隧道不可用）");
        }
    }

    // 并行 emit，避免某个远程任务卡住时阻塞其他任务
    join_all(futures).await;
}


/// 高性能 watcher：channel + worker + 去重 + 最小 emit 间隔
pub async fn watch_simulation_db_changes(
    app: AppHandle,
    client_local: Arc<Client>,
    client_remote: Arc<Client>,
    config: AppConfig,
) {
    let db_mission = client_local.database(&config.mongodb.databases.mission);
    let tasks_coll = db_mission.collection::<Document>("tasks");
    let db_local = client_local.database(&config.mongodb.databases.simulation);
    let db_remote = client_remote.database(&config.mongodb.databases.simulation);

    let options = ChangeStreamOptions::builder()
        .full_document(Some(FullDocumentType::UpdateLookup))
        .build();

    let (tx, mut rx) = channel::<String>(1024);
    let last_emit_map = Arc::new(Mutex::new(HashMap::<String, Instant>::new()));
    let min_emit_interval = Duration::from_millis(500);

    let app_clone = app.clone();
    let cl_clone = client_local.clone();
    let cr_clone = client_remote.clone();
    let last_emit_map_clone = last_emit_map.clone();
    let config_clone = config.clone();

    tokio::spawn(async move {
        while let Some(coll_name) = rx.recv().await {
            let mut last_emit = last_emit_map_clone.lock().await;
            let now = Instant::now();

            if let Some(prev) = last_emit.get(&coll_name) {
                if now.duration_since(*prev) < min_emit_interval {
                    continue;
                }
            }
            last_emit.insert(coll_name.clone(), now);
            drop(last_emit);

            let app2 = app_clone.clone();
            let cl2 = cl_clone.clone();
            let cr2 = cr_clone.clone();
            let cfg2 = config_clone.clone();
            if let Err(e) = emit_progress_for_collection(app2, cl2, cr2, coll_name.clone(), cfg2, None).await {
                eprintln!("emit 失败 [{}]: {}", coll_name, e);
            }
        }
    });

    let spawn_watcher = |source: &'static str,
                         mut stream: ChangeStream<ChangeStreamEvent<Document>>,
                         tx: Sender<String>| {
        tokio::spawn(async move {
            while let Some(Ok(change)) = stream.next().await {
                if let Some(coll_name) = extract_collection_name(&change, source) {
                    let _ = tx.send(coll_name).await;
                }
            }
        });
    };

    if let Ok(mission_stream) = tasks_coll.watch([], Some(options.clone())).await {
        spawn_watcher("mission", mission_stream, tx.clone());
    } else {
        eprintln!("启动 mission watcher 失败");
    }

    if let Ok(db_stream_local) = db_local.watch([], Some(options.clone())).await {
        spawn_watcher("db", db_stream_local, tx.clone());
    }
    // 远程 change stream 也走隧道：加超时，隧道不可用时快速跳过而不是卡住启动
    if let Ok(Ok(db_stream_remote)) = timeout(REMOTE_PROGRESS_TIMEOUT, db_remote.watch([], Some(options.clone()))).await {
        spawn_watcher("db", db_stream_remote, tx.clone());
    } else {
        eprintln!("⚠️ 远程 simulation_db 变更流不可用（隧道不通？），远程任务进度需手动刷新");
    }

    println!("{} + {} watcher 已启动", &config.mongodb.databases.mission, &config.mongodb.databases.simulation);
}