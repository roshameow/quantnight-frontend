use std::sync::Arc;

use bson::{doc, Document};
use futures_util::stream::StreamExt;
use mongodb::{
    Client, Collection,
    options::{ChangeStreamOptions, FullDocumentType},
};
use serde::Serialize;
use tauri::{State, Emitter, AppHandle, Result as TauriResult};
use crate::mongo_manager::MongoClients;


#[derive(Debug, Serialize, Clone)]
struct TaskProgress {
    collection: String,
    success: usize,
    total: usize,
    priority_success: Option<usize>,
    priority_total: Option<usize>,
    is_remote: bool,
}

static TASKS_WATCH_STARTED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// 前端 ready 时调用，启动 watcher
#[tauri::command]
pub async fn frontend_ready(
    app: AppHandle,
    mongo_clients: State<'_, Arc<MongoClients>>, // 这里是 State 类型
) -> Result<(), String> {
    // 从 State 中获取实际的 Arc<MongoClients> 对象
    let mongo_clients = mongo_clients.clone();  // 这里提取出 Arc<MongoClients>
    
    let local = mongo_clients.local.clone();
    let remote = mongo_clients.remote.clone();

    // 传递正确的类型
    emit_all_collections_once(app.clone(), mongo_clients).await;

    if TASKS_WATCH_STARTED.set(()).is_ok() {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            watch_simulation_db_changes(app_clone, local, remote).await;
        });
        println!("🔥 simulation_db 全局 watcher 已启动");
    } else {
        println!("全局 watcher 已启动，跳过重复启动");
    }

    Ok(())
}


/// 根据 collection 名称，从 simulation_mission.tasks 中查询 isRemote
pub async fn get_is_remote_for_collection(
    client_mission: Arc<Client>,
    collection_name: &str,
) -> bool {
    let db_mission = client_mission.database("simulation_mission");
    let tasks_coll = db_mission.collection::<mongodb::bson::Document>("tasks");

    let filter = doc! {
        "name": collection_name,
        "status": { "$ne": "deactive" }
    };

    match tasks_coll.find_one(filter, None).await {
        Ok(Some(task_doc)) => {
            let is_remote = task_doc
                .get_bool("isRemote")
                .unwrap_or(false);

            // println!(
            //     "🔍 get_is_remote_for_collection: [{}] => is_remote={}",
            //     collection_name, is_remote
            // );
            is_remote
        }
        Ok(None) => {
            println!(
                "⚠️ 任务 [{}] 未在 tasks 中找到，默认 is_remote=false",
                collection_name
            );
            false
        }
        Err(e) => {
            eprintln!(
                "❌ 查询 tasks 失败 [{}]: {}，默认 is_remote=false",
                collection_name, e
            );
            false
        }
    }
}


/// 监听 simulation_db 下所有 collections
pub async fn watch_simulation_db_changes(
    app: AppHandle,
    client_local: Arc<Client>,
    client_remote: Arc<Client>,
) {
    let db_local = client_local.database("simulation_db");

    // 1️⃣ 启动时先 emit 所有现有 collections —— 用同步 await，而不是 spawn
    if let Ok(collections) = db_local.list_collection_names(None).await {
        for coll_name in collections {
            if coll_name.starts_with("system.") { continue; }
            let is_remote = get_is_remote_for_collection(
                client_local.clone(),
                &coll_name
            ).await;

            // 这里不要 spawn，直接 await
            if let Err(e) = emit_progress_for_collection(
                app.clone(),
                client_local.clone(),
                client_remote.clone(),
                coll_name.clone(),
                is_remote, // 或者根据你的业务判断是否 remote
            ).await {
                eprintln!("初始 emit 失败 [{}]: {}", coll_name, e);
            } else {
                println!("初始 emit 成功 [{}]", coll_name);
            }
        }
    }

    // 2️⃣ change stream 监听新增/更新
    let options = ChangeStreamOptions::builder()
        .full_document(Some(FullDocumentType::UpdateLookup))
        .build();

    let mut stream = match db_local.watch([], Some(options)).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("监听 simulation_db 失败: {}", e);
            return;
        }
    };

    println!("开始监听 simulation_db 下所有集合…");

    while let Some(event) = stream.next().await {
        match event {
            Ok(change) => {
                let ns = match change.ns { Some(ns) => ns, None => continue };
                let coll_name = match ns.coll { Some(name) => name, None => continue };
                if coll_name.starts_with("system.") { continue; }

                let doc = match change.full_document { Some(d) => d, None => continue };
                // println!("🔔 检测到 collection [{}] 有变更，开始 emit 进度…", doc);
                let is_remote = doc.get_bool("isRemote").unwrap_or(false);

                let app_clone = app.clone();
                let local = client_local.clone();
                let remote = client_remote.clone();
                let coll_name_cloned = coll_name.clone();

                tokio::spawn(async move {
                    if let Err(e) = emit_progress_for_collection(
                        app_clone,
                        local,
                        remote,
                        coll_name_cloned,
                        is_remote,
                    ).await {
                        eprintln!("emit 失败 [{}]: {}", coll_name, e);
                    }
                });
            }
            Err(e) => {
                eprintln!("simulation_db watcher 失败: {}", e);
                break;
            }
        }
    }
}

/// 对单个 collection emit 任务进度
pub async fn emit_progress_for_collection(
    app: AppHandle,
    client_local: Arc<Client>,
    client_remote: Arc<Client>,
    collection_name: String,
    is_remote: bool,
) -> TauriResult<()> {
    // println!("📊 Emitting progress for collection: {}, is_remote: {}", collection_name, is_remote);
    
    let db = if is_remote { 
        client_remote.database("simulation_db") 
    } else { 
        client_local.database("simulation_db") 
    };
    let collection = db.collection::<Document>(&collection_name);

    emit_task_progress(&app, &collection, &collection_name, is_remote).await
}


/// 计算统计并 emit
async fn emit_task_progress(
    app_handle: &AppHandle,
    collection: &Collection<Document>,
    collection_name: &str,
    is_remote: bool,
) -> TauriResult<()> {
    let total = collection.count_documents(None, None).await.unwrap_or(0) as usize;
    let success = collection.count_documents(doc! { "status": "success" }, None)
        .await.unwrap_or(0) as usize;

    let priority_total = collection.count_documents(doc! { "priority": { "$exists": true, "$ne": 0 } }, None)
        .await.unwrap_or(0) as usize;

    let priority_success = collection.count_documents(doc! { 
        "status": "success",
        "priority": { "$exists": true, "$ne": 0 }
    }, None).await.unwrap_or(0) as usize;

    let payload = TaskProgress {
        collection: collection_name.to_string(),
        success,
        total,
        is_remote,
        priority_success: Some(priority_success),
        priority_total: Some(priority_total),
    };

    app_handle.emit("task-progress-update", payload.clone())?;
    // println!("Emitted progress for [{}]: {:?}", collection_name, payload);
    Ok(())
}


async fn emit_all_collections_once(
    app: AppHandle,
    mongo_clients: State<'_, Arc<MongoClients>>,
) {
    let db_local = mongo_clients.local.database("simulation_mission");
    let collection = db_local.collection::<Document>("tasks");

    println!("🔥 初始 emit 任务进度 for tasks 集合");

    // 添加查询条件，过滤掉 status 为 "deactive" 的文档
    let filter = doc! { "status": { "$ne": "deactive" } };

    if let Ok(documents) = collection.find(filter, None).await {
        let mut cursor = documents;
        while let Some(doc) = cursor.next().await {
            match doc {
                Ok(doc) => {
                    // 获取文档中的 'isRemote' 和 'name' 字段
                    let is_remote = doc.get("isRemote").and_then(|b| b.as_bool()).unwrap_or(false);
                    let name = doc.get_str("name").unwrap_or("未知").to_string();
                    println!("✅ 已 emit 任务进度 for 任务: {}, is_remote: {}", name, is_remote);

                    // 调用 emit_progress_for_collection 时传递正确的 is_remote
                    _ = emit_progress_for_collection(
                        app.clone(),
                        mongo_clients.local.clone(),
                        mongo_clients.remote.clone(),
                        name,
                        is_remote,
                    ).await;

                }
                Err(e) => {
                    eprintln!("查询任务文档失败: {}", e);
                }
            }
        }
    } else {
        eprintln!("查询 simulation_mission 集合失败");
    }
}


