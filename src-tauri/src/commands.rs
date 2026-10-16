use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use futures_util::stream::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId, Document},
};
use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tokio::time::timeout;
use toml;
use dirs::data_dir;

use crate::config::{run_bash_script, run_python_command, AppConfig};
use crate::mongo_manager::MongoClients;

use serde_json::from_str;
use serde_json::Value as JsonValue;

#[derive(Debug, Deserialize)]
pub struct NewTask {
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub auth_profile: Option<String>,
    pub status: Option<String>,
    pub is_remote: Option<bool>,
    pub task_type: Option<String>,
    #[serde(default, alias = "config")]
    pub mission_config: Option<JsonValue>,
}

#[derive(Debug, Serialize)]
pub struct CreatedTask {
    pub _id: String,
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub auth_profile: Option<String>,
    pub status: String,
    pub is_remote: Option<bool>,
    pub task_type: Option<String>,
    pub mission_config: Option<JsonValue>,
}

// Pure helpers: validate before launching a script or writing a task record.
fn mission_config_document(value: &JsonValue) -> Result<Document, String> {
    if !value.is_object() {
        return Err("配置必须是 JSON 对象".to_string());
    }
    bson::to_document(value).map_err(|e| format!("config 转 BSON 失败: {}", e))
}

fn priority_config_update(config: &str, is_remote: bool) -> Result<Document, String> {
    let value: JsonValue = serde_json::from_str(config)
        .map_err(|e| format!("config 解析成 JSON 失败: {}", e))?;
    let document = mission_config_document(&value)?;
    // Prepared before launch, persisted only after the requested host's script succeeds.
    // CP/run host is independent of each root's immutable source_remote.
    Ok(doc! { "status": "running", "mission_config": document,
        "isRemote": is_remote, "targetRemote": is_remote })
}

#[command]
pub async fn create_task(
    new_task: NewTask,
    clients: State<'_, Arc<MongoClients>>
) -> Result<CreatedTask, String> {
    let saved_config = new_task.mission_config.as_ref().map(mission_config_document).transpose()?;
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let coll = db.collection::<Document>("tasks");

    let status = new_task.status.unwrap_or_else(|| "waiting".to_string());
    let is_remote = new_task.is_remote.unwrap_or(false);
    let task_type = new_task.task_type.clone().unwrap_or_else(|| "regular".to_string());
    let auth_profile = new_task.auth_profile.clone().unwrap_or_else(|| "user1".to_string());

    let mut task_doc = doc! {
        "name": &new_task.name,
        "template": &new_task.template,
        "templatefile": &new_task.templatefile,
        "auth_profile": &auth_profile,
        "status": &status,
        "isRemote": is_remote,
        "taskType": &task_type,
    };
    if let Some(config) = saved_config {
        task_doc.insert("mission_config", config);
    }

    let result = coll
        .insert_one(task_doc, None)
        .await
        .map_err(|e| format!("插入失败: {}", e))?;

    let inserted_id = result
        .inserted_id
        .as_object_id()
        .ok_or_else(|| "插入失败，无法获取ObjectId".to_string())?;

    Ok(CreatedTask {
        _id: inserted_id.to_hex(),
        name: new_task.name,
        template: new_task.template,
        templatefile: new_task.templatefile,
        auth_profile: Some(auth_profile),
        status,
        is_remote: Some(is_remote),
        task_type: Some(task_type),
        mission_config: new_task.mission_config,
    })
}


#[command]
pub async fn generate_list(id: String, clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,) -> Result<(), String> {

    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");
    let obj_id = ObjectId::parse_str(&id).map_err(|e| e.to_string())?;

    let task_doc = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    // 从任务中读取 template 和 name 字段
    let template = task_doc.get_str("templatefile").map_err(|_| "任务中缺少 template 字段".to_string())?;
    let name = task_doc.get_str("name").map_err(|_| "任务中缺少 name 字段".to_string())?;
    let auth_profile = task_doc.get_str("auth_profile").unwrap_or("user1");
    let task_type = task_doc.get_str("taskType").unwrap_or("regular"); // 默认为 regular
    let is_super = task_type.eq_ignore_ascii_case("super");
    // 执行python脚本
    let mut replacements = HashMap::new();
    replacements.insert("{template}", template);
    replacements.insert("{name}", name);
    replacements.insert("{auth_profile}", auth_profile);
    replacements.insert("{extra_args}", if is_super { "--alpha_type super" } else { "" });

    run_python_command(&config, "generate_list", &replacements).await?;
    // 可选：更新状态
    tasks
        .update_one(
            doc! { "_id": obj_id, "status": { "$ne": "deactive" } },
            doc! { "$set": { "status": "ready" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;


    Ok(())
}

#[derive(Deserialize)]
pub struct Updates {
    is_remote: bool,
}

#[tauri::command]
pub async fn update_remote_status(id: String, updates: Updates,clients: State<'_, Arc<MongoClients>>) -> Result<(), String> {
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let collection = db.collection::<mongodb::bson::Document>("tasks");

    // 转换 id
    let obj_id = ObjectId::parse_str(&id)
        .map_err(|e| format!("解析 ObjectId 失败: {}", e))?;

    // 更新任务
    collection
        .update_one(
            doc! { "_id": obj_id },
            doc! { "$set": { "isRemote": updates.is_remote } },
            None,
        )
        .await
        .map_err(|e| format!("更新失败: {}", e))?;

    Ok(())
}


#[command]
pub fn read_latest_py_file(folder_path: String) -> Result<String, String> {
    let path = PathBuf::from(folder_path);
    if !path.is_dir() {
        return Err("路径不是文件夹".into());
    }

    let mut latest_file: Option<(SystemTime, PathBuf)> = None;

    for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let modified = metadata.modified().map_err(|e| e.to_string())?;
        let file_path = entry.path();

        if file_path.extension().map(|ext| ext == "py").unwrap_or(false) {
            if let Some((latest_time, _)) = &latest_file {
                if modified > *latest_time {
                    latest_file = Some((modified, file_path));
                }
            } else {
                latest_file = Some((modified, file_path));
            }
        }
    }

    if let Some((_, path)) = latest_file {
        if let Some(file_name) = path.file_name() {
            Ok(file_name.to_string_lossy().to_string())
        } else {
            Err("无法获取文件名".into())
        }
    } else {
        Err("没有找到 .py 文件".into())
    }
}

#[command]
pub async fn sync_remote_task(alpha_mission_list: String, config: State<'_, AppConfig>) -> Result<(), String> {

    let (_, _) = run_bash_script("sync_remote_task", &[("alpha_mission_list", &alpha_mission_list)], false, &config).await?;
    Ok(())
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    #[serde(rename = "_id")]
    pub id: ObjectId, // 用 id 显得更自然一些，但必须 rename "_id"
    pub name: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub templatefile: String,
    pub auth_profile: Option<String>,
    pub status: String,
    #[serde(rename = "isRemote")] // 👈 加上这个
    pub is_remote: Option<bool>, // 可选字段
    #[serde(rename = "taskType")] // 👈 加上这个
    pub task_type: Option<String>, // 或者 task_type: Option<String>
    #[serde(rename = "mission_config")]
    pub mission_config: Option<JsonValue>, // 加上这一行，接收嵌套的 config
    #[serde(rename = "startConfig")]
    pub start_config: Option<JsonValue>,
    pub config: Option<JsonValue>,
    #[serde(default, rename = "stopConfirmedAt")]
    pub stop_confirmed_at: Option<bson::DateTime>,
    #[serde(default, rename = "stopConfirmedRemote")]
    pub stop_confirmed_remote: Option<bool>,
    #[serde(default, rename = "stopConfirmedTaskId")]
    pub stop_confirmed_task_id: Option<String>,

}

/// 远程任务列表缓存：短 TTL，避免每次本地操作都穿透 SSH 隧道查远程库。
/// 缓存的是“远程独有任务”（不含本地任务），读取时再与本地合并去重。
/// 隧道不可用/超时/失败时返回空，不影响本地任务展示。
struct RemoteTaskCache {
    fetched_at: Instant,
    remote_only_tasks: Vec<Task>,
}

static REMOTE_TASK_CACHE: OnceLock<Mutex<Option<RemoteTaskCache>>> = OnceLock::new();

/// 远程查询 TTL：窗口内重复调用直接读缓存，不穿透隧道
const REMOTE_TASK_CACHE_TTL: Duration = Duration::from_secs(10);
/// 远程查询硬超时：隧道慢/挂时快速失败，避免每次操作被拖住（驱动默认 server selection 可达 30s）
const REMOTE_TASK_TIMEOUT: Duration = Duration::from_secs(2);

fn clear_remote_task_cache() {
    if let Some(cache) = REMOTE_TASK_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

/// 从远程 mission.tasks 拉取任务列表（带 TTL 缓存 + 超时）。
/// 远程不可达/超时时静默返回空，不影响本地任务展示。
async fn fetch_remote_tasks(clients: &Arc<MongoClients>, local_uri: &str, remote_uri: &str) -> Vec<Task> {
    // 本地/远程指向同一个库时无需重复查询
    if remote_uri == local_uri {
        return Vec::new();
    }

    // 命中缓存直接返回
    {
        let cache = REMOTE_TASK_CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(guard) = cache.lock() {
            if let Some(c) = guard.as_ref() {
                if c.fetched_at.elapsed() < REMOTE_TASK_CACHE_TTL {
                    return c.remote_only_tasks.clone();
                }
            }
        }
    }

    // 网络请求限定时间，避免隧道不可用时每次操作都卡住
    let result = timeout(REMOTE_TASK_TIMEOUT, async {
        let remote_client = &clients.remote;
        let remote_db = remote_client.database("simulation_mission");
        let remote_collection = remote_db.collection::<Document>("tasks");

        let mut remote_cursor = remote_collection.find(None, None).await.map_err(|e| e.to_string())?;
        let mut remote_tasks = Vec::new();
        while let Some(mut document) = remote_cursor.try_next().await.map_err(|e| e.to_string())? {
            // 远程来源统一标记 isRemote = true
            document.insert("isRemote", true);
            if let Ok(task) = bson::from_document::<Task>(document) {
                remote_tasks.push(task);
            }
        }
        Ok::<Vec<Task>, String>(remote_tasks)
    })
    .await;

    match result {
        Ok(Ok(remote_tasks)) => {
            let cache = REMOTE_TASK_CACHE.get_or_init(|| Mutex::new(None));
            if let Ok(mut guard) = cache.lock() {
                *guard = Some(RemoteTaskCache {
                    fetched_at: Instant::now(),
                    remote_only_tasks: remote_tasks.clone(),
                });
            }
            remote_tasks
        }
        _ => {
            // 超时或连接失败：静默跳过，不影响本地任务展示
            eprintln!("⚠️ 远程任务查询失败或超时（{}s），本次跳过远程聚合", REMOTE_TASK_TIMEOUT.as_secs());
            Vec::new()
        }
    }
}

/// 删除 remote 端注册的任务（agent 在 mac-mini 直接生成的，仅存在于 remote mission.tasks）
async fn delete_remote_registered_task(
    clients: &Arc<MongoClients>,
    obj_id: ObjectId,
) -> Result<(), String> {
    let remote_client = Arc::clone(&clients.remote);
    let result = timeout(REMOTE_TASK_TIMEOUT, async move {
        let tasks = remote_client.database("simulation_mission").collection::<Document>("tasks");

        let task_doc = tasks
            .find_one(doc! { "_id": obj_id }, None)
            .await
            .map_err(|e| format!("连接远程任务库失败: {}", e))?
            .ok_or_else(|| "未找到指定的任务".to_string())?;

        // 标记 deactive（软删）
        tasks
            .update_one(
                doc! { "_id": obj_id },
                doc! { "$set": { "status": "deactive" } },
                None,
            )
            .await
            .map_err(|e| e.to_string())?;

        // 删除远程 simulation_db 中的同名 collection（尽力而为，失败不阻塞删除主流程）
        let task_name = task_doc
            .get_str("name")
            .map_err(|_| "任务中缺少 name 字段".to_string())?
            .to_string();
        let sim_data_db = remote_client.database("simulation_db");
        if let Ok(names) = sim_data_db.list_collection_names(None).await {
            if names.contains(&task_name) {
                let _ = sim_data_db.collection::<Document>(&task_name).drop(None).await;
            }
        }
        Ok::<(), String>(())
    })
    .await;

    match result {
        Ok(res) => {
            clear_remote_task_cache();
            res
        }
        Err(_) => Err("连接远程任务库超时，请检查远程 MongoDB / 隧道是否可用".to_string()),
    }
}

#[command]
pub async fn get_all_tasks(
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Vec<Task>, String> {
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let collection = db.collection::<Document>("tasks");

    // 查找所有本地任务
    let mut cursor = collection.find(None, None).await.map_err(|e| e.to_string())?;

    // 用一个 Vec 来存储所有任务
    let mut tasks = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    // 遍历 cursor，获取每一个 Document
    while let Some(document) = cursor.try_next().await.map_err(|e| e.to_string())? {
        // 假设你有一个 `Task` 类型，使用 `from_document` 方法将 Document 转换为 Task
        if let Ok(task) = bson::from_document::<Task>(document) {
            seen_names.insert(task.name.clone());
            tasks.push(task);
        }
    }

    // 聚合远程任务列表（remote 端 agent 直接生成的任务只注册在 remote mission.tasks）。
    // 带 TTL 缓存 + 超时，远程不可用/慢时不再拖慢每次操作。
    let remote_tasks = fetch_remote_tasks(&clients, &config.mongodb.local_uri, &config.mongodb.remote_uri).await;
    for task in remote_tasks {
        // 同名任务以本地为准（本地创建后 sync 过去的），不重复显示
        if !seen_names.contains(&task.name) {
            seen_names.insert(task.name.clone());
            tasks.push(task);
        }
    }

    Ok(tasks)
}

#[command]
pub async fn delete_task(id: String, clients: State<'_, Arc<MongoClients>>) -> Result<(), String> {
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let obj_id = ObjectId::parse_str(&id).map_err(|e| e.to_string())?;

    // 先在本地 mission.tasks 找；找不到则视为 remote 端注册的任务（agent 直接生成的），路由到远程删除
    let probe = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| e.to_string())?;
    if probe.is_none() {
        return delete_remote_registered_task(&clients, obj_id).await;
    }

    let update_result = tasks
        .update_one(
            doc! { "_id": obj_id },
            doc! { "$set": { "status": "deactive" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    if update_result.matched_count == 0 {
        return Err("未找到指定的任务".to_string());
    }


    // 找到该任务，获取 name 字段
    let task_doc = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| format!("查询任务失败: {}", e))?
        .ok_or_else(|| "未找到指定的任务".to_string())?;
    let task_name = task_doc
        .get_str("name")
        .map_err(|_| "任务中缺少 name 字段".to_string())?
        .to_string();
    // 删除 simulation_db 中的同名 collection（如果存在）
    let sim_data_db = client.database("simulation_db");
    if sim_data_db.list_collection_names(None).await
        .map_err(|e| e.to_string())?
        .contains(&task_name) 
    {
        sim_data_db
            .collection::<Document>(&task_name)
            .drop(None)
            .await
            .map_err(|e| format!("删除 simulation_db 中 collection 失败: {}", e))?;
    }
    // 如果是远程任务，尝试连接远程 Mongo 并删除同名 collection（尽力而为：超时/失败只告警，不阻塞删除主流程）
    let is_remote = task_doc.get_bool("isRemote").unwrap_or(false);
    if is_remote {
        let remote_client = Arc::clone(&clients.remote);
        let remote_task_name = task_name.clone();
        let _ = timeout(REMOTE_TASK_TIMEOUT, async move {
            let remote_db = remote_client.database("simulation_db");
            if let Ok(names) = remote_db.list_collection_names(None).await {
                if names.contains(&remote_task_name) {
                    let _ = remote_db.collection::<Document>(&remote_task_name).drop(None).await;
                }
            }
        })
        .await;
    }

    Ok(())
}


#[command]
pub async fn start_task(
    task_name: String,
    config: String,
    is_remote: bool,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let task_doc = tasks
        .find_one(doc! { "name": &task_name, "status": { "$ne": "deactive" } }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    let auth_profile = task_doc.get_str("auth_profile").unwrap_or("user1");
    let is_remote_str = if is_remote { "true" } else { "false" };

    let (_, _) = run_bash_script(
        "start_task",
        &[
            ("task_name", &task_name),
            ("config", &config),
            ("is_remote", is_remote_str),
            ("auth_profile", auth_profile),
        ],
        is_remote,
        &app_config,
    )
    .await?;

    // 解析 config 并准备写入 mission_config
    let mut set_doc = doc! { "status": "running" };

    if let Ok(json_val) = from_str::<serde_json::Value>(&config) {
        if let Ok(bson_val) = bson::to_bson(&json_val) {
            if let bson::Bson::Document(doc_inner) = bson_val {
                set_doc.insert("mission_config", doc_inner);
            }
        } else {
            eprintln!("config 转 BSON 失败");
        }
    } else {
        eprintln!("config 解析成 JSON 失败");
    }

    // 组合更新
    let update = doc! { "$set": set_doc };

    tasks
        .update_one(
            doc! { "name": &task_name, "status": { "$ne": "deactive" } },
            update,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub async fn start_super_task(
    task_name: String,
    config: String,
    is_remote: bool,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let task_doc = tasks
        .find_one(doc! { "name": &task_name, "status": { "$ne": "deactive" } }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    let auth_profile = task_doc.get_str("auth_profile").unwrap_or("user1");
    let is_remote_str = if is_remote { "true" } else { "false" };

    let (_, _) = run_bash_script(
        "start_super_task",
        &[
            ("task_name", &task_name),
            ("config", &config),
            ("is_remote", is_remote_str),
            ("auth_profile", auth_profile),
        ],
        is_remote,
        &app_config,
    )
    .await?;

    // 保存启动配置，供任务卡片展示并发参数。
    // Super 任务此前只更新 status，导致重新读取任务后没有 mission_config。
    let mut set_doc = doc! { "status": "running" };

    if let Ok(json_val) = from_str::<serde_json::Value>(&config) {
        if let Ok(bson_val) = bson::to_bson(&json_val) {
            if let bson::Bson::Document(doc_inner) = bson_val {
                set_doc.insert("mission_config", doc_inner);
            }
        } else {
            eprintln!("config 转 BSON 失败");
        }
    } else {
        eprintln!("config 解析成 JSON 失败");
    }

    tasks
        .update_one(
            doc! { "name": &task_name, "status": { "$ne": "deactive" } },
            doc! { "$set": set_doc },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub async fn start_priority_task(
    task_name: String,
    config: String,
    is_remote: bool,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    let set_doc = priority_config_update(&config, is_remote)?;
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let task_doc = tasks
        .find_one(doc! { "name": &task_name, "status": { "$ne": "deactive" } }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    let auth_profile = task_doc.get_str("auth_profile").unwrap_or("user1");
    let is_remote_str = if is_remote { "true" } else { "false" };

    // 调用后端脚本启动任务
    let (_, _) = run_bash_script(
        "start_priority_task",
        &[
            ("task_name", &task_name),
            ("config", &config),
            ("is_remote", is_remote_str),
            ("auth_profile", auth_profile),
        ],
        is_remote,
        &app_config,
    )
    .await?;

    // Persist exactly the validated object sent to the runner.
    // Script success means requested config, not an applied-revision acknowledgement.
    // 组合更新
    let update = doc! { "$set": set_doc };

    tasks
        .update_one(
            doc! { "name": &task_name, "status": { "$ne": "deactive" } },
            update,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}


const DISPATCH_MARKER: &str = "QUANTNIGHT_DISPATCH_STATE ";
const DISPATCH_MAX_AGE_MS: i64 = 120_000;
const LEGACY_RECONCILIATION_REASON_PREFIX: &str = "请求结果待核验；暂停新请求，已接受任务继续采集。";

// Native-owned, allowlisted wire data. Never forward arbitrary capability BSON
// or a marker printed by Bash as authority. Process stdout stays independent.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchState {
    schema_version: u8,
    task_id: String,
    name: String,
    is_remote: bool,
    auth_profile: String,
    submission_state: String,
    submission_reason: Option<String>,
    heartbeat_at: i64,
    consumer_run_id: String,
    pid: u32,
    quota: DispatchQuota,
    reset_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DispatchQuota {
    limit: Option<i64>,
    remaining: Option<i64>,
    stop_floor: Option<i64>,
    budget_confirmed: bool,
}

fn dispatch_integer(doc: &Document, key: &str) -> Option<i64> {
    match doc.get(key)? {
        bson::Bson::Int32(n) => Some(i64::from(*n)),
        bson::Bson::Int64(n) => Some(*n),
        _ => None,
    }
}

fn dispatch_counter(doc: &Document, key: &str) -> Option<Option<i64>> {
    match doc.get(key)? {
        bson::Bson::Null => Some(None),
        _ => dispatch_integer(doc, key).filter(|n| (0..=9_007_199_254_740_991).contains(n)).map(Some),
    }
}

fn dispatch_reason(doc: &Document, key: &str) -> Option<Option<String>> {
    match doc.get(key)? {
        bson::Bson::Null => Some(None),
        bson::Bson::String(s) if !s.trim().is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control) => Some(Some(s.clone())),
        _ => None,
    }
}

fn dispatch_reset(doc: &Document) -> Option<Option<i64>> {
    match doc.get("reset_at")? {
        bson::Bson::Null => Some(None),
        bson::Bson::String(s) => {
            let date = chrono::DateTime::parse_from_rfc3339(s).ok()?;
            // No naive/local-clock compatibility for dispatch evidence.
            (date.offset().local_minus_utc() == 0 && date.timestamp_millis() > 0)
                .then_some(Some(date.timestamp_millis()))
        }
        _ => None,
    }
}

fn validated_dispatch_state(cap: &Document, task: &Document, id: ObjectId,
    name: &str, remote: bool, now: i64) -> Option<DispatchState> {
    let profile = task.get_str("auth_profile").ok()?;
    if profile.is_empty() || profile.len() > 100 || !profile.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) ||
        task.get_object_id("_id").ok()? != id || task.get_str("name").ok()? != name ||
        task.get_str("status").ok()? != "running" ||
        task.get_bool("isRemote").ok()? != remote ||
        task.get_str("taskType").ok()? != "regular" ||
        cap.get_str("_id").ok()? != format!("{profile}:{name}") ||
        cap.get_str("task").ok()? != name || cap.get_str("source_collection").ok()? != name ||
        cap.get_str("source_task_id").ok()? != id.to_hex() ||
        cap.get_bool("source_remote").ok()? != remote || cap.get_str("auth_profile").ok()? != profile ||
        cap.get_str("taskType").ok()? != "regular" ||
        cap.get_str("dist").ok()? != (if remote { "remote" } else { "local" }) ||
        dispatch_integer(cap, "protocol_version")? < 3 || cap.get_str("heartbeat_clock").ok()? != "utc" {
        return None;
    }
    let heartbeat = cap.get_datetime("heartbeatAt").ok()?.timestamp_millis();
    if heartbeat <= 0 || heartbeat > now + 5_000 || now - heartbeat >= DISPATCH_MAX_AGE_MS { return None; }
    let pid = u32::try_from(dispatch_integer(cap, "pid")?).ok().filter(|n| *n > 1)?;
    let run = cap.get_str("consumer_run_id").ok()?;
    if run.len() != 32 || !run.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let state = cap.get_str("submissionState").ok()?;
    if !["ready", "waiting_reconciliation", "waiting_quota_reset"].contains(&state) { return None; }
    let supplied_reason = dispatch_reason(cap, "submissionReason")?;
    if state != "ready" && supplied_reason.is_none() { return None; }
    let quota = cap.get_document("quota").ok()?;
    let reason = dispatch_reason(quota, "reason")?;
    if quota.get_str("dispatch_state").ok()? != state { return None; }
    // One exact historical writer format, not arbitrary display text. Keep the
    // quota's raw reason authoritative and normalize the DTO to that same raw
    // value. No trimming, partial matches, or compatibility for other states.
    let exact_legacy_reason = state == "waiting_reconciliation" &&
        match (supplied_reason.as_deref(), reason.as_deref()) {
            (Some(supplied), Some(raw)) => supplied == format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{raw}"),
            _ => false,
        };
    if supplied_reason != reason && !exact_legacy_reason { return None; }
    let stopped = quota.get_bool("stopped").ok()?;
    if stopped != (state == "waiting_quota_reset") { return None; }
    let limit = dispatch_counter(quota, "limit")?;
    let remaining = dispatch_counter(quota, "remaining")?;
    let floor = dispatch_counter(quota, "stop_floor")?;
    if limit == Some(0) || remaining.zip(limit).map_or(false, |(r, l)| r > l) ||
        floor.zip(limit).map_or(false, |(f, l)| f > l) ||
        (state == "waiting_quota_reset" && (limit.is_none() || remaining.is_none() || floor.is_none())) { return None; }
    let reset_at = dispatch_reset(cap)?;
    if dispatch_reset(quota)? != reset_at { return None; }
    Some(DispatchState { schema_version: 1, task_id: id.to_hex(), name: name.into(), is_remote: remote,
        auth_profile: profile.into(), submission_state: state.into(), submission_reason: reason,
        heartbeat_at: heartbeat, consumer_run_id: run.into(), pid,
        quota: DispatchQuota { limit, remaining, stop_floor: floor, budget_confirmed: quota.get_bool("budget_confirmed").ok()? }, reset_at })
}

async fn read_dispatch_state(clients: &MongoClients, task_id: &str, name: &str, remote: bool) -> Option<DispatchState> {
    let id = ObjectId::parse_str(task_id).ok()?;
    // One total deadline covers control identity + execution capability. Remote
    // capability is NEVER read from local Mongo, including on tunnel failure.
    timeout(Duration::from_secs(2), async {
        let filter = doc! { "_id": id, "name": name, "status": { "$ne": "deactive" } };
        let local = clients.local.database("simulation_mission").collection::<Document>("tasks");
        let task = match local.find_one(filter.clone(), None).await.ok()? {
            Some(task) => task,
            None if remote => clients.remote.database("simulation_mission").collection::<Document>("tasks")
                .find_one(filter, None).await.ok()??,
            None => return None,
        };
        let profile = task.get_str("auth_profile").ok()?;
        let client = if remote { &clients.remote } else { &clients.local };
        let cap = client.database("simulation_mission").collection::<Document>("priority_consumer_capabilities")
            .find_one(doc! { "_id": format!("{profile}:{name}"), "source_task_id": id.to_hex(), "source_remote": remote }, None)
            .await.ok()??;
        validated_dispatch_state(&cap, &task, id, name, remote, bson::DateTime::now().timestamp_millis())
    }).await.ok().flatten()
}

fn dispatch_status_output(stdout: &str, dispatch: Option<DispatchState>) -> String {
    // Strip ALL untrusted marker-bearing lines, not just the first/last one.
    // Keep non-marker diagnostics so an error cannot become a positive probe.
    let mut raw = stdout.lines().filter(|line| !line.contains("QUANTNIGHT_DISPATCH_STATE"))
        .collect::<Vec<_>>().join("\n").trim().to_string();
    if let Some(dispatch) = dispatch {
        if let Ok(json) = serde_json::to_string(&dispatch) {
            raw.push_str(&format!("\n{DISPATCH_MARKER}{json}"));
        }
    }
    raw
}

#[command]
pub async fn check_task_status(
    task_name: String,
    is_remote: bool,
    task_id: Option<String>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<String, String> {
    let (stdout, stderr) = run_bash_script(
        "check_task_status", &[ ("task_name", &task_name) ], is_remote, &app_config,
    ).await.map_err(|e| e.to_string())?;
    let dispatch = match task_id {
        Some(id) => read_dispatch_state(&clients, &id, &task_name, is_remote).await,
        None => None, // Legacy callers still receive their process-only string.
    };
    // A successful shell exit with stderr diagnostics is still not a clean
    // positive process observation. Preserve them for the strict UI parser.
    let process_output = if stderr.trim().is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
    Ok(dispatch_status_output(&process_output, dispatch))
}

#[command]
pub async fn update_priority_task(
    task_name: String,
    config: String,
    is_remote: bool,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    let set_doc = priority_config_update(&config, is_remote)?;

    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let task_doc = tasks
        .find_one(doc! { "name": &task_name, "status": { "$ne": "deactive" } }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    let auth_profile = task_doc.get_str("auth_profile").unwrap_or("user1");
    let is_remote_str = if is_remote { "true" } else { "false" };

    let (_, _) = run_bash_script(
        "update_priority_task",
        &[
            ("task_name", &task_name),
            ("config", &config),
            ("is_remote", is_remote_str),
            ("auth_profile", auth_profile),
        ],
        is_remote,
        &app_config,
    )
    .await?;

    tasks
        .update_one(
            doc! { "name": &task_name, "status": { "$ne": "deactive" } },
            doc! { "$set": set_doc },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub fn get_button_mappings() -> Result<String, String> {
    let config_path = get_config_toml_path()?;
    let config_content = fs::read_to_string(&config_path).map_err(|e| {
        eprintln!("Failed to read config.toml from {:?}: {}", config_path, e);
        e.to_string()
    })?;
    
    // Parse TOML content
    let config: toml::Value = toml::from_str(&config_content).map_err(|e| {
        eprintln!("Failed to parse config.toml: {}", e);
        e.to_string()
    })?;
    
    // Create button mappings from config
    let mut mappings = Vec::new();
    
    // Python scripts
    if let Some(python) = config.get("python").and_then(|v| v.as_table()) {
        if let Some(scripts) = python.get("scripts").and_then(|v| v.as_table()) {
            for (key, value) in scripts {
                let script_command = if let Some(command) = value.get("command").and_then(|v| v.as_str()) {
                    command.to_string()
                } else if let Some(module) = value.get("module").and_then(|v| v.as_str()) {
                    format!("python3 -m {}", module)
                } else {
                    key.to_string()
                };
                
                let args = if let Some(args_array) = value.get("args").and_then(|v| v.as_array()) {
                    let args_str: Vec<String> = args_array.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();
                    args_str.join(" ")
                } else {
                    String::new()
                };
                
                mappings.push(serde_json::json!({
                    "button_id": key,
                    "button_label": get_button_label(key),
                    "script_key": key,
                    "script_type": "python",
                    "script_command": format!("{} {}", script_command, args),
                    "description": get_script_description(key),
                    "enabled": true
                }));
            }
        }
    }
    
    // Bash scripts
    if let Some(bash) = config.get("bash").and_then(|v| v.as_table()) {
        if let Some(scripts) = bash.get("scripts").and_then(|v| v.as_table()) {
            for (key, value) in scripts {
                let script_command = if let Some(script) = value.get("script").and_then(|v| v.as_str()) {
                    format!("bash {}", script)
                } else if let Some(command) = value.get("command").and_then(|v| v.as_str()) {
                    command.to_string()
                } else {
                    key.to_string()
                };
                
                let args = if let Some(args_array) = value.get("args").and_then(|v| v.as_array()) {
                    let args_str: Vec<String> = args_array.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();
                    args_str.join(" ")
                } else {
                    String::new()
                };
                
                mappings.push(serde_json::json!({
                    "button_id": key,
                    "button_label": get_button_label(key),
                    "script_key": key,
                    "script_type": "bash",
                    "script_command": format!("{} {}", script_command, args),
                    "description": get_script_description(key),
                    "enabled": true
                }));
            }
        }
    }
    
    Ok(serde_json::to_string(&mappings).map_err(|e| e.to_string())?)
}

#[command]
pub fn save_button_mappings(mappings: String) -> Result<(), String> {
    // Parse the mappings from JSON
    let _mappings_value: serde_json::Value = serde_json::from_str(&mappings).map_err(|e| e.to_string())?;
    
    // Read the current config.toml
    let config_path = get_config_toml_path()?;
    let config_content = fs::read_to_string(&config_path).map_err(|e| {
        eprintln!("Failed to read config.toml from {:?}: {}", config_path, e);
        e.to_string()
    })?;
    
    // Parse TOML content
    let mut config: toml::Value = toml::from_str(&config_content).map_err(|e| {
        eprintln!("Failed to parse config.toml: {}", e);
        e.to_string()
    })?;
    
    // Update the config with new button mappings
    // We'll store the mappings as a JSON string in the TOML file
    if let Some(table) = config.as_table_mut() {
        // Remove existing button mappings if any
        table.remove("button_mappings");
        
        // Add new button mappings as a string
        let mut button_mappings_table = toml::value::Table::new();
        button_mappings_table.insert("mappings".to_string(), toml::Value::String(mappings.clone()));
        table.insert("button_mappings".to_string(), toml::Value::Table(button_mappings_table));
    }
    
    // Write the updated config back to file
    let updated_content = toml::to_string_pretty(&config).map_err(|e| {
        eprintln!("Failed to serialize updated config: {}", e);
        e.to_string()
    })?;
    
    fs::write(&config_path, updated_content).map_err(|e| {
        eprintln!("Failed to write config.toml: {}", e);
        e.to_string()
    })?;
    
    eprintln!("Button mappings saved successfully to config.toml");
    
    Ok(())
}

// Helper function to get the config.toml path (reuse from frontend_config)
fn get_config_toml_path() -> Result<PathBuf, String> {
    // In development, use the relative path
    #[cfg(debug_assertions)]
    {
        return Ok(PathBuf::from("./config.toml"));
    }
    
    // In production, use the app data directory
    #[cfg(not(debug_assertions))]
    {
        // 获取用户数据目录
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let config_dir = PathBuf::from(home).join("Library/Application Support/quantnight");
                
                // 确保目录存在
                if let Err(_) = std::fs::create_dir_all(&config_dir) {
                    return Err("Failed to create config directory".to_string());
                }
                
                return Ok(config_dir.join("config.toml"));
            }
        }
        
        #[cfg(target_os = "windows")]
        {
            if let Ok(app_data) = std::env::var("APPDATA") {
                let config_dir = PathBuf::from(app_data).join("quantnight");
                
                // 确保目录存在
                if let Err(_) = std::fs::create_dir_all(&config_dir) {
                    return Err("Failed to create config directory".to_string());
                }
                
                return Ok(config_dir.join("config.toml"));
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let config_dir = PathBuf::from(home).join(".local/share/quantnight");
                
                // 确保目录存在
                if let Err(_) = std::fs::create_dir_all(&config_dir) {
                    return Err("Failed to create config directory".to_string());
                }
                
                return Ok(config_dir.join("config.toml"));
            }
        }
        
        // 回退到当前目录
        Ok(PathBuf::from("config.toml"))
    }
}

fn get_button_label(script_key: &str) -> String {
    match script_key {
        "generate_list" => "生成列表".to_string(),
        "sync_remote_task" => "发送到远程".to_string(),
        "start_task" => "启动".to_string(),
        "start_super_task" => "启动超级任务".to_string(),
        "start_priority_task" => "启动优先任务".to_string(),
        "update_priority_task" => "更新优先任务".to_string(),
        "pause_task" => "暂停".to_string(),
        "check_task_status" => "检查任务状态".to_string(),
        "alpha_correlation" => "Alpha相关性".to_string(),
        _ => script_key.to_string(),
    }
}

fn get_script_description(script_key: &str) -> String {
    match script_key {
        "generate_list" => "从模板生成alpha列表".to_string(),
        "sync_remote_task" => "同步远程任务列表".to_string(),
        "start_task" => "启动常规任务".to_string(),
        "start_super_task" => "启动超级任务".to_string(),
        "start_priority_task" => "启动优先任务".to_string(),
        "update_priority_task" => "更新优先任务".to_string(),
        "pause_task" => "暂停运行中的任务".to_string(),
        "check_task_status" => "检查任务状态".to_string(),
        "alpha_correlation" => "计算Alpha相关性".to_string(),
        _ => format!("执行脚本: {}", script_key),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionStopProof {
    schema_version: u32,
    task_name: String,
    target_remote: bool,
    session_absent: bool,
    runners_exited: bool,
}

// Exit zero or a printed success message is insufficient. The stop script
// must verify both the exact session and its recorded execution processes.
fn verified_stop_proof(stdout: &str, name: &str, remote: bool) -> Result<(), String> {
    let receipts: Vec<_> = stdout.lines().filter_map(|line|
        line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED ")).collect();
    if receipts.len() != 1 || stdout.lines().last().and_then(|line|
        line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED ")) != receipts.first().copied() {
        return Err("停止状态未确认：缺少唯一的最终执行退出核验回执，请更新停止脚本后核对；不会自动重试停止".into());
    }
    let proof: ExecutionStopProof = serde_json::from_str(receipts[0])
        .map_err(|e| format!("停止状态未确认：回执无效：{e}"))?;
    if proof.schema_version != 1 || proof.task_name != name || proof.target_remote != remote
        || !proof.session_absent || !proof.runners_exited {
        return Err("停止状态未确认：任务/执行目标不匹配，或会话/执行进程尚未确认退出".into());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTaskStop {
    confirmed: bool,
    task_id: String,
    task_name: String,
    is_remote: bool,
    stop_confirmed_at: i64,
}

fn confirmed_pause_update(id: ObjectId, remote: bool, at: bson::DateTime) -> Document {
    doc! { "status": "paused", "pausedAt": at, "stopConfirmedAt": at,
        "stopConfirmedRemote": remote, "stopConfirmedTaskId": id.to_hex() }
}

#[command]
pub async fn pause_task(task_name: String, is_remote: bool, task_id: Option<String>,
    clients: State<'_, Arc<MongoClients>>, app_config: State<'_, AppConfig>) -> Result<ConfirmedTaskStop, String> {
    let tasks = clients.local.database("simulation_mission").collection::<Document>("tasks");
    let mut filter = doc! { "name": &task_name, "status": { "$ne": "deactive" } };
    if let Some(id) = task_id {
        filter.insert("_id", ObjectId::parse_str(id).map_err(|e| format!("任务ID无效：{e}"))?);
    }
    let task = timeout(Duration::from_secs(5), tasks.find_one(filter, None)).await
        .map_err(|_| "停止请求未发出：任务身份查询超时".to_string())?
        .map_err(|e| e.to_string())?.ok_or("停止请求未发出：原任务不存在")?;
    let id = task.get_object_id("_id").map_err(|e| e.to_string())?;
    if task.get_bool("isRemote").unwrap_or(false) != is_remote {
        return Err("停止请求未发出：执行目标已经变更".into());
    }
    let (stdout, _) = run_bash_script("pause_task", &[("task_name", &task_name)],
        is_remote, &app_config).await?;
    verified_stop_proof(&stdout, &task_name, is_remote)?;
    let at = bson::DateTime::now();
    let result = timeout(Duration::from_secs(5), tasks.update_one(
        doc! { "_id": id, "name": &task_name, "status": { "$ne": "deactive" },
            "isRemote": task.get("isRemote").cloned().unwrap_or(bson::Bson::Null) },
        doc! { "$set": confirmed_pause_update(id, is_remote, at) }, None)).await
        .map_err(|_| "执行退出已核验，但控制记录回写超时；停止状态未完成确认，请核对，不要盲目重试".to_string())?
        .map_err(|e| format!("执行退出已核验，但控制记录回写失败：{e}"))?;
    if result.matched_count != 1 {
        return Err("执行退出已核验，但原任务身份/执行目标已变化；未回写暂停".into());
    }
    Ok(ConfirmedTaskStop { confirmed: true, task_id: id.to_hex(), task_name,
        is_remote, stop_confirmed_at: at.timestamp_millis() })
}

#[command]
pub fn restart_app() -> Result<(), String> {
    // Use std::process to restart the application
    std::process::exit(0); // This will cause the application to exit, and the process manager can restart it
}



#[cfg(test)]
mod verified_pause_tests {
    use super::*;
    use serde_json::json;

    fn proof(extra: JsonValue) -> String {
        let mut value = json!({ "schema_version": 1, "task_name": "pipeline__priority",
            "target_remote": true, "session_absent": true, "runners_exited": true });
        if let Some(overrides) = extra.as_object() {
            for (key, replacement) in overrides { value[key] = replacement.clone(); }
        }
        format!("stop log\nQUANTNIGHT_PAUSE_CONFIRMED {value}\n")
    }

    #[test]
    fn only_exact_verified_execution_receipt_confirms_stop() {
        let valid = proof(json!({}));
        assert!(verified_stop_proof(&valid, "pipeline__priority", true).is_ok());
        assert!(verified_stop_proof("✅ 已关停", "pipeline__priority", true).is_err());
        assert!(verified_stop_proof("", "pipeline__priority", true).is_err());
        assert!(verified_stop_proof(&(valid.clone() + &valid), "pipeline__priority", true).is_err());
        assert!(verified_stop_proof(&(valid.clone() + "more output\n"), "pipeline__priority", true).is_err());
        assert!(verified_stop_proof(&proof(json!({"unexpected": true})), "pipeline__priority", true).is_err());
        for changes in [json!({"session_absent": false}), json!({"runners_exited": false}),
            json!({"schema_version": 2}), json!({"task_name": "other"}),
            json!({"target_remote": false}), json!({"runners_exited": "true"})] {
            assert!(verified_stop_proof(&proof(changes), "pipeline__priority", true).is_err());
        }
        assert!(verified_stop_proof("QUANTNIGHT_PAUSE_CONFIRMED {broken}", "pipeline__priority", true).is_err());
    }

    #[test]
    fn task_list_preserves_persisted_stop_evidence_for_remounts() {
        let id = ObjectId::new();
        let at = bson::DateTime::now();
        let mut record = confirmed_pause_update(id, true, at);
        record.insert("_id", id);
        record.insert("name", "pipeline__priority");
        record.insert("isRemote", true);
        let task: Task = bson::from_document(record).unwrap();
        let wire = serde_json::to_value(task).unwrap();
        assert_eq!(wire["stopConfirmedTaskId"], id.to_hex());
        assert_eq!(wire["stopConfirmedRemote"], true);
        assert!(!wire["stopConfirmedAt"].is_null());
    }

    #[test]
    fn persisted_and_returned_stop_identity_are_fenced_and_explicit() {
        let id = ObjectId::new();
        let at = bson::DateTime::now();
        let update = confirmed_pause_update(id, true, at);
        assert_eq!(update.get_str("status").unwrap(), "paused");
        assert_eq!(update.get_str("stopConfirmedTaskId").unwrap(), id.to_hex());
        assert!(update.get_bool("stopConfirmedRemote").unwrap());
        assert_eq!(*update.get_datetime("stopConfirmedAt").unwrap(), at);
        let response = serde_json::to_value(ConfirmedTaskStop { confirmed: true,
            task_id: id.to_hex(), task_name: "pipeline__priority".into(), is_remote: true,
            stop_confirmed_at: at.timestamp_millis() }).unwrap();
        assert_eq!(response["taskId"], id.to_hex());
        assert_eq!(response["isRemote"], true);
        assert_eq!(response["confirmed"], true);
        assert_eq!(response["stopConfirmedAt"], at.timestamp_millis());
    }
}

#[cfg(test)]
mod priority_config_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_accepts_pipeline_config_and_legacy_config_alias() {
        for key in ["mission_config", "config"] {
            let mut payload = json!({
                "name": "custom_research__priority", "template": "", "templatefile": "pipeline",
                "task_type": "priority"
            });
            payload[key] = json!({
                "refine_templates": [{"name":"group_op_add", "enabled":true,
                    "params":{"num_variants":2}, "max_variants":4}],
                "max_variants_per_alpha": 40
            });
            let task: NewTask = serde_json::from_value(payload).unwrap();
            assert_eq!(task.templatefile, "pipeline");
            let saved = mission_config_document(task.mission_config.as_ref().unwrap()).unwrap();
            assert_eq!(saved.get_array("refine_templates").unwrap().len(), 1);
        }
        let legacy: NewTask = serde_json::from_value(json!({
            "name":"sign_flip__priority", "template":"", "templatefile":"sign_flip"
        })).unwrap();
        assert!(legacy.mission_config.is_none());
    }

    #[test]
    fn update_persists_exact_nested_object_and_explicit_empty_steps() {
        let value = json!({"refine_templates":[], "max_variants_per_alpha":40,
            "extension":{"preserve":true}});
        let update = priority_config_update(&value.to_string(), false).unwrap();
        assert_eq!(update.get_str("status").unwrap(), "running");
        let saved = update.get_document("mission_config").unwrap();
        assert!(saved.get_array("refine_templates").unwrap().is_empty());
        assert_eq!(serde_json::to_value(saved).unwrap(), value);
        let legacy = priority_config_update("{\"interval\":600}", false).unwrap();
        assert!(!legacy.get_document("mission_config").unwrap().contains_key("refine_templates"));
    }

    #[test]
    fn invalid_top_level_config_is_rejected_before_script_execution() {
        for value in ["[]", "null", "false", "3", "broken"] {
            assert!(priority_config_update(value, true).is_err());
        }
        assert!(mission_config_document(&json!([])).is_err());
    }

    #[test]
    fn requested_cp_host_is_preserved_independently_of_root_origin_and_round_mode() {
        for enabled in [true, false] {
            for remote in [true, false] {
                let value = json!({"rounds":{"enabled":enabled}, "source_tasks":["remote-source"],
                    "extra":{"source_remote":!remote}});
                let update = priority_config_update(&value.to_string(), remote).unwrap();
                assert_eq!(update.get_bool("isRemote").unwrap(), remote);
                assert_eq!(update.get_bool("targetRemote").unwrap(), remote);
                assert_eq!(serde_json::to_value(update.get_document("mission_config").unwrap()).unwrap(), value);
            }
        }
    }

    #[test]
    fn task_read_model_exposes_all_saved_config_sources() {
        for key in ["mission_config", "startConfig", "config"] {
            let mut document = doc! {"_id": ObjectId::new(), "name":"legacy__priority", "status":"pending"};
            document.insert(key, doc! {"refine_templates": []});
            let task: Task = bson::from_document(document).unwrap();
            let exposed = serde_json::to_value(task).unwrap();
            assert_eq!(exposed[key]["refine_templates"], json!([]));
        }
    }
}

#[cfg(test)]
mod dispatch_state_tests {
    use super::*;
    const NOW: i64 = 1_792_113_200_000;
    const PANE: &str = "✅ Pane 0 (pid=21733) is running: python3.12";

    // Same BSON contract as _refresh_priority_capability/_submission_evidence:
    // original source ID, UTC BSON heartbeat, uuid.hex run, guard.snapshot().
    fn fixture(remote: bool, state: &str) -> (ObjectId, Document, Document) {
        let id = ObjectId::new();
        let reason = match state {
            "ready" => bson::Bson::Null,
            "waiting_quota_reset" => "daily simulation safety stop reached: remaining=496, floor=500".into(),
            _ => "simulation_quota_unconfirmed: unknown response without trustworthy daily quota headers".into(),
        };
        let quota = doc! { "dispatch_state": state, "reason": reason.clone(), "budget_phase": if state == "waiting_quota_reset" { "CONFIRMED" } else { "UNCONFIRMED" },
            "budget_epoch": 0, "limit": 5000, "remaining": if state == "waiting_quota_reset" { 496 } else { 3965 }, "stop_floor": 500,
            "reset": "3600", "reset_at": "2026-10-16T02:13:20+00:00", "stop_after_used": 4500,
            "stopped": state == "waiting_quota_reset", "stop_reason": if state == "waiting_quota_reset" { reason.clone() } else { bson::Bson::Null },
            "headers_seen": true, "budget_confirmed": state == "waiting_quota_reset",
            "blocked_reason": if state == "waiting_reconciliation" { reason.clone() } else { bson::Bson::Null } };
        let task = doc! { "_id": id, "name": "source", "status": "running", "isRemote": remote,
            "auth_profile": "user1", "taskType": "regular" };
        let cap = doc! { "_id": "user1:source", "task": "source", "source_collection": "source",
            "source_task_id": id.to_hex(), "source_remote": remote, "auth_profile": "user1",
            "taskType": "regular", "dist": if remote { "remote" } else { "local" },
            "protocol_version": 3, "heartbeat_clock": "utc", "heartbeatAt": bson::DateTime::from_millis(NOW),
            "pid": 21740, "consumer_run_id": "0123456789abcdef0123456789abcdef",
            "submissionState": state, "submissionReason": reason, "quota": quota,
            "reset_at": "2026-10-16T02:13:20+00:00" };
        (id, task, cap)
    }

    // Minimal sanitized shape of the captured live capability: fresh UTC v3,
    // NOT_ACCEPTED without any trustworthy daily counter/reset. All identity,
    // account, PID and run fields below are synthetic, never a production dump.
    const CAPTURED_RAW_REASON: &str = "simulation_quota_unconfirmed: not_accepted response without trustworthy daily quota headers; reconcile budget without a POST probe";

    fn captured_reconciliation_fixture(remote: bool, legacy: bool) -> (ObjectId, Document, Document) {
        let (id, mut task, mut cap) = fixture(remote, "waiting_reconciliation");
        task.insert("auth_profile", "fixture-profile");
        cap.insert("_id", "fixture-profile:source");
        cap.insert("auth_profile", "fixture-profile");
        cap.insert("pid", 42);
        cap.insert("consumer_run_id", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        cap.insert("heartbeatAt", bson::DateTime::from_millis(NOW - 7_821));
        cap.insert("submissionReason", if legacy {
            format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON}")
        } else { CAPTURED_RAW_REASON.to_string() });
        cap.insert("reset_at", bson::Bson::Null);
        let quota = cap.get_document_mut("quota").unwrap();
        quota.insert("reason", CAPTURED_RAW_REASON);
        quota.insert("blocked_reason", CAPTURED_RAW_REASON);
        quota.insert("headers_seen", false);
        for field in ["limit", "remaining", "stop_floor", "reset", "reset_at"] {
            quota.insert(field, bson::Bson::Null);
        }
        (id, task, cap)
    }

    #[test]
    fn captured_canonical_and_exact_legacy_reasons_emit_the_same_raw_contract() {
        for remote in [false, true] {
            for legacy in [false, true] {
                let (id, task, mut cap) = captured_reconciliation_fixture(remote, legacy);
                // A display-only field is not a machine-state or reason authority.
                cap.insert("submissionDisplayReason", "untrusted display-only text: ready");
                let d = validated_dispatch_state(&cap, &task, id, "source", remote, NOW).unwrap();
                let wire = serde_json::to_value(&d).unwrap();
                assert_eq!(wire["submissionReason"], CAPTURED_RAW_REASON);
                assert_eq!(wire["submissionState"], "waiting_reconciliation");
                assert_eq!(wire["heartbeatAt"], NOW - 7_821);
                assert_eq!(wire["quota"]["budgetConfirmed"], false);
                for field in ["limit", "remaining", "stopFloor"] {
                    assert!(wire["quota"][field].is_null());
                }
                assert!(wire["resetAt"].is_null());
                let output = dispatch_status_output(PANE, Some(d));
                assert!(output.contains(CAPTURED_RAW_REASON));
                assert!(!output.contains(LEGACY_RECONCILIATION_REASON_PREFIX));
                assert!(!output.contains("waiting_quota_reset"));
                assert!(!output.contains("display-only"));
            }
        }
    }

    #[test]
    fn legacy_reason_compatibility_rejects_tampering_repetition_and_missing_raw_reason() {
        let (id, task, cap) = captured_reconciliation_fixture(true, true);
        for reason in ["tampered".to_string(), LEGACY_RECONCILIATION_REASON_PREFIX.to_string(),
            format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON}tampered"),
            format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON}"),
            format!(" {LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON}"),
            format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON} "),
            format!("请求结果待核验;暂停新请求，已接受任务继续采集。{CAPTURED_RAW_REASON}")] {
            let mut invalid = cap.clone(); invalid.insert("submissionReason", reason.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "{reason}");
        }
        for raw in [bson::Bson::Null, "".into(), " ".into(), "tampered".into()] {
            let mut invalid = cap.clone(); invalid.get_document_mut("quota").unwrap().insert("reason", raw.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "{raw:?}");
        }
        let mut missing = cap.clone(); missing.get_document_mut("quota").unwrap().remove("reason");
        assert!(validated_dispatch_state(&missing, &task, id, "source", true, NOW).is_none());
        for state in ["ready", "waiting_quota_reset"] {
            let (id, task, mut other) = fixture(true, state);
            let raw = other.get_document("quota").unwrap().get("reason").unwrap().clone();
            other.insert("submissionReason", format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{}", raw.as_str().unwrap_or("")));
            assert!(validated_dispatch_state(&other, &task, id, "source", true, NOW).is_none(), "{state}");
        }
    }

    #[test]
    fn legacy_reason_compatibility_keeps_identity_clock_and_quota_fences() {
        let (id, task, cap) = captured_reconciliation_fixture(true, true);
        for changes in [doc! { "source_task_id": ObjectId::new().to_hex() }, doc! { "source_remote": false },
            doc! { "task": "other" }, doc! { "source_collection": "other" }, doc! { "auth_profile": "other" },
            doc! { "_id": "other:source" }, doc! { "dist": "local" }, doc! { "taskType": "super" },
            doc! { "protocol_version": 2 }, doc! { "heartbeat_clock": "local" },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW - DISPATCH_MAX_AGE_MS) },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW + 5001) },
            doc! { "pid": 0 }, doc! { "consumer_run_id": "bad" }, doc! { "submissionState": "ready" }] {
            let mut invalid = cap.clone(); invalid.extend(changes.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "{changes:?}");
        }
        for changes in [doc! { "dispatch_state": "ready" }, doc! { "stopped": true },
            doc! { "remaining": -1 }, doc! { "budget_confirmed": "false" }, doc! { "reset_at": "bad" }] {
            let mut invalid = cap.clone(); invalid.get_document_mut("quota").unwrap().extend(changes.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "{changes:?}");
        }
        assert!(validated_dispatch_state(&cap, &task, ObjectId::new(), "source", true, NOW).is_none());
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_none());
        let mut paused = task.clone(); paused.insert("status", "paused");
        assert!(validated_dispatch_state(&cap, &paused, id, "source", true, NOW).is_none());
    }

    #[test]
    fn native_dispatch_dto_is_explicit_and_unknown_budget_is_not_quota_exhaustion() {
        for remote in [false, true] {
            for state in ["ready", "waiting_reconciliation", "waiting_quota_reset"] {
                let (id, task, cap) = fixture(remote, state);
                let d = validated_dispatch_state(&cap, &task, id, "source", remote, NOW).unwrap();
                let wire = serde_json::to_value(&d).unwrap();
                assert_eq!(wire["taskId"], id.to_hex());
                assert_eq!(wire["submissionState"], state);
                assert_eq!(wire["heartbeatAt"], NOW);
                assert_eq!(wire["isRemote"], remote);
                assert_eq!(wire["pid"], 21740);
                assert_eq!(wire["quota"]["budgetConfirmed"], state == "waiting_quota_reset");
                assert!(wire["resetAt"].is_i64());
                let output = dispatch_status_output(PANE, Some(d));
                assert!(output.starts_with(PANE));
                assert_eq!(output.matches(DISPATCH_MARKER).count(), 1);
            }
        }
    }

    #[test]
    fn legitimate_bootstrap_and_unknown_reset_do_not_require_fabricated_counters() {
        let (id, task, mut cap) = fixture(false, "ready");
        cap.insert("reset_at", bson::Bson::Null);
        let quota = cap.get_document_mut("quota").unwrap();
        for field in ["limit", "remaining", "stop_floor", "reset_at"] { quota.insert(field, bson::Bson::Null); }
        let d = validated_dispatch_state(&cap, &task, id, "source", false, NOW).unwrap();
        assert_eq!(d.submission_state, "ready");
        assert!(d.reset_at.is_none());
        assert!(d.quota.remaining.is_none());
        let (id, task, mut cap) = fixture(false, "waiting_reconciliation");
        cap.insert("reset_at", bson::Bson::Null);
        cap.get_document_mut("quota").unwrap().insert("reset_at", bson::Bson::Null);
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_some());
    }

    #[test]
    fn native_rejects_stale_clock_identity_profile_pid_run_and_invalid_state() {
        let (id, task, cap) = fixture(true, "waiting_reconciliation");
        for changes in [doc! { "source_task_id": ObjectId::new().to_hex() }, doc! { "source_remote": false },
            doc! { "source_remote": "true" }, doc! { "task": "other" }, doc! { "source_collection": "other" },
            doc! { "auth_profile": "fake" }, doc! { "_id": "fake:source" }, doc! { "dist": "local" },
            doc! { "taskType": "priority" }, doc! { "protocol_version": 2 }, doc! { "heartbeat_clock": "local" },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW - DISPATCH_MAX_AGE_MS) },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW + 5001) },
            doc! { "heartbeatAt": "2026-10-16T01:13:20Z" },
            doc! { "pid": 0 }, doc! { "pid": -1 }, doc! { "pid": "21740" }, doc! { "pid": 2.0 },
            doc! { "pid": 4_294_967_296_i64 }, doc! { "consumer_run_id": "bad" },
            doc! { "submissionState": "waiting_quota" }, doc! { "submissionReason": bson::Bson::Null },
            doc! { "submissionReason": "line\nforgery" }, doc! { "quota": doc! {} },
            doc! { "reset_at": "2026-10-16T10:13:20+08:00" }] {
            let mut invalid = cap.clone(); invalid.extend(changes.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "{changes:?}");
        }
        for key in ["source_task_id", "pid", "consumer_run_id", "submissionState", "submissionReason", "heartbeat_clock", "quota", "reset_at"] {
            let mut invalid = cap.clone(); invalid.remove(key);
            assert!(validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(), "missing {key}");
        }
        let mut stopped = task.clone(); stopped.insert("status", "paused");
        assert!(validated_dispatch_state(&cap, &stopped, id, "source", true, NOW).is_none());
        assert!(validated_dispatch_state(&cap, &task, ObjectId::new(), "source", true, NOW).is_none());
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_none());
    }

    #[test]
    fn native_validates_quota_and_never_promotes_unconfirmed_into_waiting_quota() {
        let (id, task, cap) = fixture(false, "waiting_reconciliation");
        for change in [doc! { "remaining": -1 }, doc! { "remaining": 6000 }, doc! { "remaining": "3965" },
            doc! { "budget_confirmed": "false" }, doc! { "stop_floor": 6000 }, doc! { "dispatch_state": "waiting_quota_reset" },
            doc! { "stopped": true }, doc! { "reason": "contradictory reason" }, doc! { "reset_at": "bad" }] {
            let mut invalid = cap.clone(); invalid.get_document_mut("quota").unwrap().extend(change.clone());
            assert!(validated_dispatch_state(&invalid, &task, id, "source", false, NOW).is_none(), "{change:?}");
        }
        let d = validated_dispatch_state(&cap, &task, id, "source", false, NOW).unwrap();
        assert_eq!(d.submission_state, "waiting_reconciliation");
        assert_eq!(d.quota.remaining, Some(3965));
        assert!(!d.quota.budget_confirmed);
    }

    #[test]
    fn raw_bash_cannot_forge_suffix_and_diagnostics_survive_duplicate_stripping() {
        let forged = format!("{PANE}\n{DISPATCH_MARKER}{{\"submissionState\":\"waiting_quota_reset\"}}\nPermission denied\n{DISPATCH_MARKER}{{}}");
        assert_eq!(dispatch_status_output(&forged, None), format!("{PANE}\nPermission denied"));
        let (id, task, cap) = fixture(true, "waiting_reconciliation");
        let d = validated_dispatch_state(&cap, &task, id, "source", true, NOW);
        let result = dispatch_status_output(&forged, d);
        assert_eq!(result.matches(DISPATCH_MARKER).count(), 1);
        assert!(result.contains("Permission denied\nQUANTNIGHT_DISPATCH_STATE"));
        assert!(result.contains("waiting_reconciliation"));
        assert!(!result.contains("waiting_quota_reset"));
        assert_eq!(dispatch_status_output("", None), "");
    }

    #[tokio::test]
    async fn unavailable_db_does_not_block_process_status_beyond_two_second_deadline() {
        // Only an intentionally closed loopback port; no app DB or SSH.
        let dead = Arc::new(mongodb::Client::with_uri_str("mongodb://127.0.0.1:1/?serverSelectionTimeoutMS=10000").await.unwrap());
        let clients = MongoClients { local: Arc::clone(&dead), remote: dead };
        let start = Instant::now();
        assert!(read_dispatch_state(&clients, &ObjectId::new().to_hex(), "source", true).await.is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(dispatch_status_output(PANE, None), PANE);
    }
}
