use std::{collections::HashMap, sync::Arc};
use std::error::Error;

use bson::{doc, Document};
use futures_util::{StreamExt, stream::TryStreamExt};
use mongodb::{
    Client, Collection, Database,
    options::{ChangeStreamOptions, FullDocumentType},
};
use serde::Serialize;
use tauri::{
    AppHandle, command, Emitter, Result as TauriResult, State,
    async_runtime::JoinHandle,
};
use tokio::sync::RwLock;
use tokio::sync::OnceCell;


use crate::mongo_manager::MongoClients;



#[command]
pub async fn frontend_ready(
    app: AppHandle,
    mongo_clients: State<'_, MongoClients>,
) -> Result<(), String> {
    let clients = mongo_clients.inner().clone();

    start_all_task_watchers(app, clients)
        .await
        .map_err(|e| format!("启动监听失败: {}", e))
}

#[derive(Debug, Serialize, Clone)]
struct TaskProgress {
    collection: String,
    success: usize,
    total: usize,
    priority_success: Option<usize>,
    priority_total: Option<usize>,
    is_remote: bool, // 额外加个字段，用于前端查看是否用的是 remote
}

type TaskMap = Arc<RwLock<HashMap<String, JoinHandle<()>>>>;


static TASKS_WATCH_STARTED: OnceCell<()> = OnceCell::const_new();

pub async fn start_all_task_watchers(app_handle: AppHandle, clients: MongoClients) -> Result<(), Box<dyn Error>> {
    let client = &clients.local;
    let client_remote = &clients.remote;

    let task_coll = client.database("simulation_mission").collection::<Document>("tasks");

    let filter = doc! { "status": { "$ne": "deactive" } };
    let mut cursor = task_coll.find(Some(filter), None).await?;
    let watchers: TaskMap = Arc::new(RwLock::new(HashMap::new()));

    while let Ok(Some(task)) = cursor.try_next().await {
        if let Some(name) = task.get_str("name").ok() {
            let is_remote = task.get_bool("isRemote").unwrap_or(false);
            start_or_restart_watcher(&app_handle, name.to_string(), is_remote, &client, &client_remote, watchers.clone()).await;
        }
    }

    // 监听 tasks 表的变更（是否更新 is_remote）
    if TASKS_WATCH_STARTED.set(()).is_ok() {
        let watchers_clone = watchers.clone();
        let app_clone = app_handle.clone();
        let client_clone = client.clone();
        let client_remote_clone = client_remote.clone();

        tauri::async_runtime::spawn(async move {
            watch_tasks_changes(
                app_clone,
                client_clone,
                client_remote_clone,
                watchers_clone,
            )
            .await;
        });
    } else {
        println!("watch_tasks_changes 已启动，跳过重复启动");
    }

    Ok(())
}


async fn start_single_watcher(app_handle: AppHandle, db: Database, collection_name: String, is_remote: bool) {
    let collection: Collection<Document> = db.collection(&collection_name);

    loop {    //自动重启监听逻辑
        // 初次推送一次状态
        if let Err(err) = emit_task_progress(&app_handle, &collection, &collection_name, is_remote).await {
            eprintln!("初始推送失败 [{}]: {}", collection_name, err);
        } else {
            println!("初始推送成功 [{}]", collection_name);  // 成功推送日志
        }

        let options = ChangeStreamOptions::default();
        let mut change_stream = match collection.watch([], options).await {
            Ok(stream) => {
                println!("成功开始监听集合 [{}]", collection_name);  // 成功开始监听日志
                stream
            },
            Err(err) => {
                eprintln!("监听集合 [{}] 失败: {}", collection_name, err);
                // 等待一段时间后重试
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };


        while let Some(event) = change_stream.next().await {
            match event {
                Ok(_) => {
                    if let Err(err) = emit_task_progress(&app_handle, &collection, &collection_name,is_remote).await {
                        eprintln!("推送失败 [{}]: {}", collection_name, err);
                    }
                }
                Err(err) => {
                    eprintln!("监听失败 [{}]: {}", collection_name, err);
                    // 中断当前监听，重新启动
                    break;
                }
            }
        }
        // 等待再重新监听，防止频繁死循环
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

}

async fn emit_task_progress(
    app_handle: &AppHandle,
    collection: &Collection<Document>,
    collection_name: &str,
    is_remote: bool,
) -> TauriResult<()> {
    let total = collection.count_documents(None, None).await.unwrap_or(0) as usize;
    let success = collection
        .count_documents(doc! { "status": "success" }, None)
        .await
        .unwrap_or(0) as usize;

    let priority_filter = doc! {
        "priority": {
            "$exists": true,
            "$ne": 0
        }
    };

    let priority_total = collection
        .count_documents(priority_filter.clone(), None)
        .await
        .unwrap_or(0) as usize;

    let priority_success = collection
        .count_documents(doc! { 
            "status": "success",
            "priority": {
                "$exists": true,
                "$ne": 0
            }
        }, None)
        .await
        .unwrap_or(0) as usize;

    let payload = TaskProgress {
        collection: collection_name.to_string(),
        success,
        total,
        is_remote,
        priority_success: Some(priority_success),
        priority_total: Some(priority_total),
    };
    let result = app_handle.emit("task-progress-update", payload.clone());
    // println!(
    //     "emit [{}] -> success={}, total={} | result = {:?}",
    //     collection_name, payload.success, payload.total, result
    // );
    
    result?; // 保持函数签名不变
    Ok(())
}

async fn start_or_restart_watcher(
    app: &AppHandle,
    name: String,
    is_remote: bool,
    local_client: &Client,
    remote_client: &Client,
    task_map: TaskMap,
) {
    let db = if is_remote {
        remote_client.database("simulation_db")
    } else {
        local_client.database("simulation_db")
    };

    let mut map = task_map.write().await;
    if let Some(handle) = map.remove(&name) {
        handle.abort(); // 停掉旧的
    }

    let app_clone = app.clone();
    let name_clone = name.clone();
    let handle = tauri::async_runtime::spawn(async move {
        start_single_watcher(app_clone, db, name_clone,is_remote).await;
    });

    map.insert(name, handle);
}


pub async fn watch_tasks_changes(
    app: AppHandle,
    client: Arc<Client>,
    client_remote: Arc<Client>,
    task_map: TaskMap,
) {
    let task_coll = client.database("simulation_mission").collection::<Document>("tasks");

    let options = ChangeStreamOptions::builder()
        .full_document(Some(FullDocumentType::UpdateLookup))
        .build();

    let mut stream = match task_coll.watch([], Some(options)).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("监听 tasks 失败: {}", e);
            return;
        }
    };

    while let Some(event) = stream.next().await {
        if let Ok(change) = event {
            if let Some(doc) = change.full_document {
                if let Ok(name) = doc.get_str("name") {
                    // 注意这里字段名和实际保持一致
                    let is_remote = doc.get_bool("isRemote").unwrap_or(false);
                    let status = doc.get_str("status").unwrap_or("active");
                    if status != "deactive" {
                        println!("任务更新: {} -> is_remote={}", name, is_remote);
                        start_or_restart_watcher(
                            &app,
                            name.to_string(),
                            is_remote,
                            &client,
                            &client_remote,
                            task_map.clone(),
                        ).await;
                    } else {
                        println!("跳过 deactive 任务: {}", name);
                    }
                    if status == "deactive" {
                        let mut map = task_map.write().await;
                        if let Some(handle) = map.remove(name) {
                            println!("停止监听 deactive 任务: {}", name);
                            handle.abort();
                        }
                        continue;
                    }
                }
            }
        }
    }
}

