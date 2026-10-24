use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::stream::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId, Document},
};
use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, Emitter, State};
use tokio::time::timeout;
use toml;
use dirs::data_dir;

use crate::config::{run_bash_script, run_bash_script_observed_with_env, run_process_observed, run_python_command, AppConfig,
    BashScriptEvent, BashScriptObserver};
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

    // Same ordinary-task generation CAS used by taskctl/preparation. A second
    // UI/agent invocation cannot enter generation while another owner holds it.
    let generation_id = ObjectId::new().to_hex();
    let claim = tasks.update_one(
        doc! {"_id": obj_id, "status": {"$in": ["pending", "waiting", "ready", "generation_failed"]}},
        doc! {"$set": {"status": "generating", "updatedAt": bson::DateTime::now(),
            "generation": {"id": &generation_id, "phase": "writing", "producerPid": std::process::id() as i64,
                           "startedAt": bson::DateTime::now()}}}, None).await.map_err(|e| e.to_string())?;
    if claim.matched_count != 1 { return Err("生成认领失败：任务正在运行、生成或等待状态核验".into()); }
    let generated: Result<u64, String> = async {
        run_python_command(&config, "generate_list", &replacements).await?;
        let count = client.database("simulation_db").collection::<Document>(name)
            .count_documents(None, None).await.map_err(|e| e.to_string())?;
        if count == 0 { return Err("生成器退出成功但候选集合为空".into()); }
        Ok(count)
    }.await;
    let query = doc! {"_id": obj_id, "status": "generating", "generation.id": &generation_id};
    let fields = match &generated {
        Ok(count) => doc! {"status": "ready", "expectedCount": *count as i64,
            "generation.phase": "complete", "generation.finishedAt": bson::DateTime::now()},
        Err(error) => {
            let status = if error.contains("超时") || error.to_lowercase().contains("timeout") {
                "generation_unknown"
            } else { "generation_failed" };
            doc! {"status": status, "generation.phase": status,
                "generation.finishedAt": bson::DateTime::now(), "lastError": error}
        }
    };
    let updated = tasks.update_one(query, doc! {"$set": fields}, None).await.map_err(|e| e.to_string())?;
    if updated.matched_count != 1 { return Err("生成归属已变化；保留当前状态".into()); }
    generated.map(|_| ())

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


// Discovery and binding share Python's pure catalog with generation. Do not
// replicate sidecar/hash/root-containment logic here or execute template code.
async fn run_template_catalog(
    root: &Path, action: &str, kind: &str, name: Option<&str>, directory: Option<&str>,
) -> Result<JsonValue, String> {
    let root = root.canonicalize().map_err(|e| format!("模板根目录解析失败：{e}"))?;
    let mut command = tokio::process::Command::new(root.join(".venv/bin/python"));
    command.current_dir(&root).env("PYTHONPATH", &root).kill_on_drop(true)
        .args(["-m", "templates.catalog", action, "--root"])
        .arg(&root).args(["--kind", kind]);
    if let Some(name) = name { command.args(["--name", name]); }
    if let Some(directory) = directory { command.args(["--directory", directory]); }
    let output = timeout(Duration::from_secs(30), command.output()).await
        .map_err(|_| "模板目录查询超时".to_string())?
        .map_err(|e| format!("无法启动 templates.catalog：{e}"))?;
    let value: JsonValue = serde_json::from_slice(&output.stdout)
        .map_err(|_| format!("templates.catalog 未返回有效 JSON：{}", String::from_utf8_lossy(&output.stderr)))?;
    if !output.status.success() || value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("模板目录查询失败：{}", value));
    }
    Ok(value)
}

fn latest_catalog_name(value: &JsonValue, kind: &str) -> Result<String, String> {
    let entry = value.get("entry").filter(|v| v.is_object())
        .ok_or_else(|| "该目录没有已发布模板；可手动选择旧模板名称".to_string())?;
    if entry.get("taskType").and_then(|v| v.as_str()) != Some(kind) {
        return Err("模板类型与请求存在差异".into());
    }
    entry.get("name").and_then(|v| v.as_str()).filter(|name| !name.is_empty())
        .map(str::to_owned).ok_or_else(|| "模板目录结果缺少裸名称".into())
}

#[command]
pub async fn read_latest_py_file(
    folder_path: String, task_type: Option<String>, config: State<'_, AppConfig>,
) -> Result<String, String> {
    latest_template_from_directory(Path::new(&config.env.working_dir), &folder_path, task_type.as_deref()).await
}

async fn latest_template_from_directory(root: &Path, folder_path: &str, task_type: Option<&str>) -> Result<String, String> {
    // Old callers may omit taskType. No change to stored task.templatefile IDs.
    let kind = task_type.unwrap_or_else(|| {
        let folder = PathBuf::from(folder_path);
        if folder == root.join("super_templates") || folder == root.join("templates/generated/super") {
            "super"
        } else { "regular" }
    });
    let result = run_template_catalog(root, "latest", kind, None, Some(folder_path)).await?;
    latest_catalog_name(&result, kind)
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
    #[serde(default, rename = "stopConfirmationBasis")]
    pub stop_confirmation_basis: Option<String>,

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

// Keep the existing delete button/lifecycle. Prepared research templates carry
// an optional sidecar; archive through the same Python control interface before
// making any destructive Mongo change. Legacy tasks need no new workflow.
async fn archive_prepared_research(task: &Document, config: &AppConfig) -> Result<(), String> {
    let template = task.get_str("templatefile").unwrap_or("");
    if template.is_empty() { return Ok(()); }
    let kind = task.get_str("taskType").unwrap_or("regular");
    if kind == "priority" { return Ok(()); } // Priority has its own unchanged templates.
    let root = PathBuf::from(&config.env.working_dir);
    let result = run_template_catalog(&root, "binding", kind, Some(template), None).await
        .map_err(|e| format!("研究绑定核验失败；任务记录保留：{e}"))?;
    let binding = result.get("binding").ok_or("研究绑定查询缺少 binding；任务记录保留")?;
    if !binding.is_null() && !binding.is_object() {
        return Err("研究绑定类型应为对象或 null；任务记录保留".into());
    }
    // Refine descriptors are not dataset manifests. Keep the original binding's kind.
    if binding.get("kind").and_then(|v| v.as_str()) != Some("dataset") { return Ok(()); }
    if task.get_str("status").ok() == Some("running") {
        return Err("当前研究任务正在运行；完成整任务暂停后可删除".into());
    }
    let name = task.get_str("name").map_err(|e| e.to_string())?;
    let id = task.get_object_id("_id").map_err(|e| e.to_string())?.to_hex();
    let remote = if task.get_bool("isRemote").unwrap_or(false) { "true" } else { "false" };
    let (stdout, stderr) = run_taskctl(&root,
        &["archive-research", "--name", name, "--expected-task-id", &id, "--expected-remote", remote],
        Duration::from_secs(90)).await
        .map_err(|e| format!("研究统计归档失败；任务记录保留：{e}"))?;
    let receipt: JsonValue = serde_json::from_str(&stdout)
        .map_err(|e| format!("研究统计归档回执 JSON 解析失败；任务记录保留：{e}\nstdout: {stdout}\nstderr: {stderr}"))?;
    if receipt.get("ok").and_then(|v| v.as_bool()) != Some(true)
        || receipt.get("command").and_then(|v| v.as_str()) != Some("archive-research")
        || receipt.get("task").and_then(|v| v.as_str()) != Some(name)
        || receipt.get("archivePath").and_then(|v| v.as_str()).map_or(true, str::is_empty) {
        return Err(format!("研究统计归档失败；任务记录保留：{receipt}\nstdout: {stdout}\nstderr: {stderr}"));
    }
    Ok(())
}

#[command]
pub async fn delete_task(id: String, clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>) -> Result<(), String> {
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let obj_id = ObjectId::parse_str(&id).map_err(|e| e.to_string())?;

    // 先在本地 mission.tasks 找；找不到则视为 remote 端注册的任务（agent 直接生成的），路由到远程删除
    let probe = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| e.to_string())?;
    if matches!(probe.as_ref().and_then(|d| d.get_str("status").ok()), Some("generating" | "generation_unknown")) {
        return Err("当前处于候选生成阶段；完成生成核验后可删除".into());
    }
    if probe.is_none() {
        return delete_remote_registered_task(&clients, obj_id).await;
    }
    archive_prepared_research(probe.as_ref().unwrap(), &app_config).await?;

    let mut delete_filter = doc! { "_id": obj_id };
    for field in ["name", "status", "updatedAt", "isRemote", "templatefile", "taskType", "auth_profile"] {
        match probe.as_ref().unwrap().get(field) {
            Some(value) => { delete_filter.insert(field, value.clone()); },
            None => { delete_filter.insert(field, doc! { "$exists": false }); },
        }
    }
    let update_result = tasks
        .update_one(
            delete_filter,
            doc! { "$set": { "status": "deactive" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    if update_result.matched_count == 0 {
        return Err("任务身份或状态已改变；任务记录保留".to_string());
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


// Same local Python/root/PYTHONPATH transport previously used by archive-research.
// taskctl owns remote transport, configuration, identity CAS and launch evidence.
async fn run_taskctl(root: &Path, args: &[&str], budget: Duration) -> Result<(String, String), String> {
    let mut command = tokio::process::Command::new(root.join(".venv/bin/python"));
    command.current_dir(root).env("PYTHONPATH", root).kill_on_drop(true)
        .arg(".agents/skills/manage-quantnight-frontend/scripts/taskctl.py").args(args);
    run_process_observed(command, budget, false, None).await
}

async fn start_via_taskctl(
    task_name: &str, config: &str, is_remote: bool, task_id: &str,
    app_config: &AppConfig, budget: Duration,
) -> Result<(), String> {
    ObjectId::parse_str(task_id).map_err(|e| format!("启动任务身份无效：{e}"))?;
    let value: JsonValue = serde_json::from_str(config)
        .map_err(|e| format!("启动配置 JSON 无效：{e}"))?;
    if !value.is_object() { return Err("启动配置必须是 JSON 对象".into()); }
    let (stdout, stderr) = run_taskctl(Path::new(&app_config.env.working_dir),
        &["start", "--name", task_name, "--config", config,
          if is_remote { "--remote" } else { "--no-remote" },
          "--expected-task-id", task_id, "--no-notify"], budget).await
        .map_err(|e| format!("taskctl start 未获确认；未重试或写入 running，请核对原任务状态：{e}"))?;
    let receipt: JsonValue = serde_json::from_str(&stdout)
        .map_err(|e| format!("taskctl start 回执 JSON 无效；未重试：{e}\nstdout: {stdout}\nstderr: {stderr}"))?;
    if receipt.get("schemaVersion").and_then(JsonValue::as_u64) != Some(1)
        || receipt.get("command").and_then(JsonValue::as_str) != Some("start")
        || receipt.get("task").and_then(JsonValue::as_str) != Some(task_name)
        || receipt.get("ok").and_then(JsonValue::as_bool) != Some(true)
        || receipt.get("taskId").and_then(JsonValue::as_str) != Some(task_id)
        || receipt.get("remote").and_then(JsonValue::as_bool) != Some(is_remote) {
        return Err(format!("taskctl start 失败或回执任务/执行端不匹配；未重试或写入 running\nstdout: {stdout}\nstderr: {stderr}"));
    }
    // Startup confirmation is not task completion or platform acceptance.
    Ok(())
}

#[command]
pub async fn start_task(
    task_name: String, config: String, is_remote: bool, task_id: String,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    start_via_taskctl(&task_name, &config, is_remote, &task_id, &app_config, Duration::from_secs(300)).await
}

#[command]
pub async fn start_super_task(
    task_name: String, config: String, is_remote: bool, task_id: String,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    start_via_taskctl(&task_name, &config, is_remote, &task_id, &app_config, Duration::from_secs(300)).await
}

#[command]
pub async fn start_priority_task(
    task_name: String, config: String, is_remote: bool, task_id: String,
    app_config: State<'_, AppConfig>,
) -> Result<(), String> {
    start_via_taskctl(&task_name, &config, is_remote, &task_id, &app_config, Duration::from_secs(300)).await
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

// Diagnostic metadata shares the original capability/runtime read. It never
// changes task state, receipt authority or the supervisor's retry schedule.
const RUNTIME_HISTORY_MARKER: &str = "QUANTNIGHT_RUNTIME_HISTORY ";
const BUSINESS_DIAGNOSTIC_MARKER: &str = "QUANTNIGHT_BUSINESS_DIAGNOSTIC ";
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BusinessDiagnostic {
    schema_version: u8, task_id: String, name: String, is_remote: bool, auth_profile: String,
    source: String, source_detail: Option<String>, consumer_run_id: String, launch_attempt_id: Option<String>,
    launch_matched: bool, heartbeat_at: i64, occurred_at: i64,
    error_class: String, reason: String, phase: String, scope: String,
    timeout_seconds: Option<f64>, recovery_state: String, next_retry_at: Option<i64>,
}

fn safe_diagnostic_text(value: &str) -> String {
    // Remove complete capability URLs (including webhook paths), not only query
    // strings. Defense in depth even when the original writer already sanitizes.
    let urls = regex::Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://[^\s<>"']+"#).unwrap();
    let secrets = regex::Regex::new(r"(?i)\b(authorization|cookie|password|passwd|token|secret|api[_-]?key)\s*[:=]\s*[^,;]+|\bBearer\s+\S+").unwrap();
    let text = urls.replace_all(value, "[redacted-uri]");
    secrets.replace_all(&text, "[redacted-credential]").chars()
        .map(|c| if c.is_control() { ' ' } else { c }).take(1024).collect()
}

fn diagnostic_time(doc: &Document, key: &str) -> Option<i64> {
    match doc.get(key)? {
        bson::Bson::DateTime(t) => Some(t.timestamp_millis()),
        bson::Bson::String(s) => {
            let t = chrono::DateTime::parse_from_rfc3339(s).ok()?;
            (t.offset().local_minus_utc() == 0).then_some(t.timestamp_millis())
        },
        _ => None,
    }
}

fn diagnostic_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 100 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn validated_business_diagnostic(runtime: &Document, task: &Document, id: ObjectId,
    name: &str, remote: bool, now: i64) -> Option<BusinessDiagnostic> {
    let profile = task.get_str("auth_profile").ok()?;
    let kind = task.get_str("taskType").ok()?;
    if !diagnostic_id(profile) || task.get_object_id("_id").ok()? != id ||
        task.get_str("name").ok()? != name || task.get_bool("isRemote").ok()? != remote ||
        task.get_str("status").ok()? == "deactive" || runtime.get_str("auth_profile").ok()? != profile ||
        runtime.get_str("taskType").ok()? != kind ||
        runtime.get_str("dist").ok()? != if remote { "remote" } else { "local" } { return None; }
    if kind == "regular" {
        if runtime.get_str("_id").ok()? != format!("{profile}:{name}") ||
            runtime.get_str("task").ok()? != name || runtime.get_str("source_collection").ok()? != name ||
            runtime.get_str("source_task_id").ok()? != id.to_hex() ||
            runtime.get_bool("source_remote").ok()? != remote ||
            runtime.get_str("heartbeat_clock").ok()? != "utc" ||
            dispatch_integer(runtime, "protocol_version")? < 3 { return None; }
    } else if kind != "super" { return None; }
    u32::try_from(dispatch_integer(runtime, "pid")?).ok().filter(|pid| *pid > 1)?;
    let d = runtime.get_document("runtimeDiagnostic").ok()?;
    if dispatch_integer(d, "schemaVersion")? != 1 { return None; }
    let run = runtime.get_str("consumer_run_id").ok()?;
    if run.len() != 32 || !run.bytes().all(|b| b.is_ascii_hexdigit()) ||
        d.get_str("consumerRunId").ok()? != run { return None; }
    let heartbeat = diagnostic_time(runtime, "heartbeatAt")?;
    let occurred = diagnostic_time(d, "occurredAt")?;
    if occurred <= 0 || occurred > now || heartbeat < occurred || heartbeat > now + 5000 { return None; }
    // A newer normal start fences old evidence, even if an old consumer wrote
    // a recently timestamped heartbeat. Missing legacy launch proof is history.
    let launch = d.get_str("launchAttemptId").ok().filter(|v| diagnostic_id(v));
    let task_launch = task.get_str("launchAttemptId").ok();
    if launch.zip(task_launch).map_or(false, |(a,b)| a != b) { return None; }
    let mut launch_matched = launch.zip(task_launch).map_or(false, |(a,b)| a == b);
    if kind == "super" {
        let binding = task.get_document("startupBinding").ok()?;
        if binding.get_str("taskId").ok()? != id.to_hex() || binding.get_str("name").ok()? != name ||
            binding.get_str("auth_profile").ok()? != profile || binding.get_bool("remote").ok()? != remote ||
            binding.get_str("consumer_run_id").ok()? != run { return None; }
        launch_matched = launch_matched && binding.get_str("launchAttemptId").ok() == launch;
        if !launch_matched { return None; }
    }
    for key in ["launchAttemptAt", "startedAt"] {
        if diagnostic_time(task, key).map_or(false, |at| occurred < at) { return None; }
        if task.contains_key(key) && diagnostic_time(task, key).is_none() { launch_matched = false; }
    }
    let class = d.get_str("errorClass").ok()?;
    let phase = d.get_str("phase").ok()?;
    let scope = d.get_str("scope").ok()?;
    if !diagnostic_id(class) || !diagnostic_id(phase) ||
        !["item", "accepted_receipts", "new_dispatch", "consumer"].contains(&scope) { return None; }
    let reason = d.get_str("reason").ok()?;
    let recovery = d.get_str("recoveryState").ok()?;
    if reason.trim().is_empty() || reason.len() > 8192 || recovery.trim().is_empty() || recovery.len() > 1024 { return None; }
    let budget = match d.get("timeoutSeconds") {
        None | Some(bson::Bson::Null) => None,
        Some(bson::Bson::Double(n)) if n.is_finite() && *n > 0.0 && *n <= 86400.0 => Some(*n),
        _ => Some(dispatch_integer(d, "timeoutSeconds").filter(|n| *n > 0 && *n <= 86400)? as f64),
    };
    let retry = match d.get("nextRetryAt") {
        None | Some(bson::Bson::Null) => None,
        _ => Some(diagnostic_time(d, "nextRetryAt")?),
    };
    if retry.map_or(false, |t| t < occurred || t > 8_640_000_000_000_000) { return None; }
    Some(BusinessDiagnostic { schema_version: 1, task_id: id.to_hex(), name: name.into(), is_remote: remote,
        auth_profile: profile.into(), source: if kind == "super" { "simulatorRuntime" } else { "priority_capability" }.into(),
        source_detail: None, consumer_run_id: run.into(), launch_attempt_id: launch.map(str::to_owned), launch_matched,
        heartbeat_at: heartbeat, occurred_at: occurred, error_class: class.into(), reason: safe_diagnostic_text(reason),
        phase: phase.into(), scope: scope.into(), timeout_seconds: budget, recovery_state: safe_diagnostic_text(recovery), next_retry_at: retry })
}

fn validated_history_diagnostic(stdout: &str, task: &Document, id: ObjectId,
    name: &str, remote: bool, now: i64) -> Option<BusinessDiagnostic> {
    // Only the trusted original checker emits this envelope; captured pane text
    // itself is never relayed as a status/identity marker. History is not proof
    // of a task exit cause and never becomes a current dispatch/recovery state.
    let markers: Vec<_> = stdout.lines().filter(|l| l.contains("QUANTNIGHT_RUNTIME_HISTORY")).collect();
    if markers.len() != 1 || !markers[0].starts_with(RUNTIME_HISTORY_MARKER) || markers[0].len() > 8192 { return None; }
    let raw: JsonValue = serde_json::from_str(markers[0].strip_prefix(RUNTIME_HISTORY_MARKER)?).ok()?;
    let d = raw.as_object()?;
    if d.len() != 10 || d.get("schemaVersion")?.as_u64()? != 1 || d.get("name")?.as_str()? != name ||
        d.get("isRemote")?.as_bool()? != remote || operation_task_identity(task, name, remote).ok()? != id { return None; }
    let process = runtime_process_output(stdout, "").ok()?;
    let negative = regex::Regex::new(r"^❌ Pane [0-9]+ \(pid=[1-9][0-9]*\) (is running: (bash|sh|zsh|sleep)|exited \(retained tmux pane\))$").ok()?;
    if process.lines().count() != 1 || !negative.is_match(process.trim()) { return None; }
    let sid = d.get("sessionId")?.as_str()?;
    if !regex::Regex::new(r"^\$[0-9]+$").ok()?.is_match(sid) { return None; }
    let created = d.get("sessionCreatedAt")?.as_i64()?;
    let occurred = d.get("occurredAt")?.as_i64()?;
    if created <= 0 || occurred < created || occurred > now { return None; }
    // A retained pane from before task creation/start cannot supply this task's
    // history. Legacy launches with no timestamp remain explicitly unconfirmed.
    for key in ["createdAt", "startedAt", "launchAttemptAt"] {
        if diagnostic_time(task, key).map_or(false, |at| created + 1000 < at || occurred < at) { return None; }
    }
    let class = d.get("errorClass")?.as_str()?;
    let phase = d.get("phase")?.as_str()?;
    let reason = d.get("reason")?.as_str()?;
    if !class.starts_with("simulation_") || !diagnostic_id(class) ||
        !["simulation_recovery_phase", "startup_recovery"].contains(&phase) || reason.trim().is_empty() || reason.len() > 4096 { return None; }
    let budget = if d.get("timeoutSeconds")?.is_null() { None } else {
        Some(d.get("timeoutSeconds")?.as_i64().filter(|v| *v > 0 && *v <= 86400)?)
    };
    Some(BusinessDiagnostic { schema_version: 1, task_id: id.to_hex(), name: name.into(), is_remote: remote,
        auth_profile: task.get_str("auth_profile").ok()?.into(), source: "tmux_log".into(),
        source_detail: Some(format!("exact session {sid}; createdAt={created}; bounded fixed logger record; launch unconfirmed")),
        consumer_run_id: "unavailable".into(), launch_attempt_id: None, launch_matched: false,
        heartbeat_at: occurred, occurred_at: occurred, error_class: class.into(), reason: safe_diagnostic_text(reason),
        phase: phase.into(), scope: "accepted_receipts".into(), timeout_seconds: budget.map(|n| n as f64),
        recovery_state: "历史启动恢复错误；当前恢复状态与退出因果未核验".into(), next_retry_at: None })
}

async fn read_runtime_evidence(clients: &MongoClients, task_id: &str, name: &str, remote: bool)
    -> (Option<DispatchState>, Option<BusinessDiagnostic>) {
    let read = async {
        let id = ObjectId::parse_str(task_id).ok()?;
        let filter = doc! { "_id": id, "name": name, "status": { "$ne": "deactive" } };
        let local = clients.local.database("simulation_mission").collection::<Document>("tasks");
        let task = match local.find_one(filter.clone(), None).await.ok()? {
            Some(task) => task,
            None if remote => clients.remote.database("simulation_mission").collection::<Document>("tasks")
                .find_one(filter.clone(), None).await.ok()??,
            None => return None,
        };
        let profile = task.get_str("auth_profile").ok()?;
        if task.get_bool("isRemote").ok()? != remote { return None; }
        let client = if remote { &clients.remote } else { &clients.local };
        let runtime = if task.get_str("taskType").ok()? == "super" {
            let execution = if remote { client.database("simulation_mission").collection::<Document>("tasks")
                .find_one(filter, None).await.ok()?? } else { task.clone() };
            // Execution-side proof must be exactly the original control launch.
            for key in ["auth_profile", "isRemote", "startupBinding", "launchAttemptId"] {
                if execution.get(key) != task.get(key) { return None; }
            }
            execution.get_document("simulatorRuntime").ok()?.clone()
        } else {
            client.database("simulation_mission").collection::<Document>("priority_consumer_capabilities")
                .find_one(doc! { "_id": format!("{profile}:{name}"), "source_task_id": id.to_hex(), "source_remote": remote }, None)
                .await.ok()??
        };
        let now = bson::DateTime::now().timestamp_millis();
        Some((validated_dispatch_state(&runtime, &task, id, name, remote, now),
            validated_business_diagnostic(&runtime, &task, id, name, remote, now)))
    };
    timeout(Duration::from_secs(2), read).await.ok().flatten().unwrap_or((None, None))
}

#[cfg(test)]
async fn read_dispatch_state(clients: &MongoClients, task_id: &str, name: &str, remote: bool) -> Option<DispatchState> {
    read_runtime_evidence(clients, task_id, name, remote).await.0
}

fn business_diagnostic_output(stdout: &str, diagnostic: Option<BusinessDiagnostic>) -> String {
    let mut raw = stdout.lines().filter(|line| !line.contains("QUANTNIGHT_BUSINESS_DIAGNOSTIC"))
        .collect::<Vec<_>>().join("\n");
    if let Some(diagnostic) = diagnostic {
        if let Ok(json) = serde_json::to_string(&diagnostic) { raw.push_str(&format!("\n{BUSINESS_DIAGNOSTIC_MARKER}{json}")); }
    }
    raw
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

const TASK_STAGE_PREFIX: &str = "QUANTNIGHT_TASK_STAGE ";
const RUNTIME_BUDGET: Duration = Duration::from_secs(12);
const PAUSE_SCRIPT_BUDGET: Duration = Duration::from_secs(75);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptTaskStage {
    operation: String,
    task_name: String,
    target_remote: bool,
    stage: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskOperationStage {
    operation: String,
    task_id: String,
    name: String,
    is_remote: bool,
    request_id: String,
    stage: String,
    message: String,
}

#[derive(Clone)]
struct TaskOperationProgress {
    operation: &'static str,
    task_id: String,
    name: String,
    remote: bool,
    request_id: String,
    tmux_binding: Option<String>,
    last: Arc<Mutex<(String, String)>>,
    sink: Arc<dyn Fn(TaskOperationStage) + Send + Sync>,
}

impl TaskOperationProgress {
    fn new(app: AppHandle, operation: &'static str, task_id: Option<&str>, name: &str,
        remote: bool, request_id: Option<String>) -> Self {
        Self { operation, task_id: task_id.unwrap_or_default().into(), name: name.into(), remote,
            request_id: request_id.unwrap_or_default(), tmux_binding: None,
            last: Arc::new(Mutex::new(("initial".into(), "原生桥已接收请求".into()))),
            sink: Arc::new(move |event| {
                if let Err(error) = app.emit("task-operation-stage", event) {
                    eprintln!("task-operation-stage emit failed: {error}");
                }
            }) }
    }

    fn stage(&self, stage: &str, message: &str) {
        // Serialize updates and emission, so no stale callback can overtake a
        // receipt/control-write phase. stdout cannot supply native identity.
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let message = safe_diagnostic_text(message);
        *last = (stage.into(), message.clone());
        // Legacy callers have no correlation token. Keep internal diagnostics
        // and business behavior, but do not emit an unrouteable UI event.
        if self.request_id.is_empty() { return; }
        (self.sink)(TaskOperationStage { operation: self.operation.into(), task_id: self.task_id.clone(),
            name: self.name.clone(), is_remote: self.remote, request_id: self.request_id.clone(),
            stage: stage.into(), message });
    }

    fn failure(&self, cause: impl std::fmt::Display) -> String {
        let (stage, message) = self.last.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let error = safe_diagnostic_text(&format!("{} stage={}（{}）：{}", self.operation, stage, message, cause));
        // Keep the actual failed step, never collapse it into generic unknown.
        self.stage(&stage, &error);
        error
    }

    fn script_stage(&self, line: &str) {
        // Parse only a bounded, exact line/schema and operation-specific fixed
        // IDs. Markers are diagnostic hints, NEVER stop/dispatch authority.
        if line.len() > 4096 { return; }
        let Some(json) = line.strip_prefix(TASK_STAGE_PREFIX) else { return; };
        let Ok(event) = serde_json::from_str::<ScriptTaskStage>(json) else { return; };
        if event.operation != self.operation || event.task_name != self.name ||
            event.target_remote != self.remote || event.message.trim().is_empty() ||
            event.message.len() > 1024 || event.message.chars().any(char::is_control) {
            return;
        }
        let allowed: &[&str] = match self.operation {
            "runtime" => &["config", "ssh", "remote_environment", "tmux", "process", "receipt", "session_absent", "complete"],
            "stop" => &["config", "ssh", "remote_environment", "tmux", "process", "interrupt", "session_exit",
                "process_exit", "receipt", "session_absent", "complete"],
            _ => &[],
        };
        if allowed.contains(&event.stage.as_str()) { self.stage(&event.stage, &event.message); }
    }

    fn observer(&self) -> BashScriptObserver {
        let progress = self.clone();
        Arc::new(move |event| match event {
            BashScriptEvent::Config => progress.stage("config", "读取原脚本配置与参数"),
            BashScriptEvent::Launch => progress.stage("launch", "启动本次操作的本地脚本子进程"),
            BashScriptEvent::Wait => progress.stage("wait", "等待脚本 stdout/stderr 与退出"),
            BashScriptEvent::Stdout(line) => progress.script_stage(&line),
        })
    }
}

// Both commands and their offline regression tests take this same route to
// the original runner. No mock transport/config or second runner is involved.
async fn execute_task_script(progress: &TaskOperationProgress, config: &AppConfig,
    budget: Duration) -> Result<(String, String), String> {
    let (key, diagnostic) = match progress.operation {
        "runtime" => ("check_task_status", true),
        "stop" => ("pause_task", false),
        _ => return Err(progress.failure("原生任务操作类型无效")),
    };
    // Explicitly replace any inherited binding; never adopt Pi/parent authority.
    let environment = [("QUANTNIGHT_TMUX_BINDING", progress.tmux_binding.as_deref().unwrap_or("{}"))];
    run_bash_script_observed_with_env(key, &[("task_name", &progress.name)], progress.remote,
        config, budget, diagnostic, Some(progress.observer()), &environment).await
        .map_err(|e| progress.failure(e))
}

// Forward only persisted backend evidence. Backend validates it and selects the
// sole system tmux. No native resolver or inferred neverStarted/newLaunch flags.
fn task_script_binding(task: &Document, remote: bool) -> Result<String, String> {
    let id = task.get_object_id("_id").map_err(|e| e.to_string())?;
    serde_json::to_string(&doc! { "taskId": id.to_hex(), "remote": remote,
        "launchAttemptId": task.get("launchAttemptId").cloned().unwrap_or(bson::Bson::Null),
        "tmuxContext": task.get("tmuxContext").cloned().unwrap_or(bson::Bson::Null) })
        .map_err(|e| format!("任务执行绑定序列化失败：{e}"))
}

fn operation_task_identity(task: &Document, name: &str, remote: bool) -> Result<ObjectId, String> {
    if task.get_str("name").ok() != Some(name) || task.get_str("status").ok() == Some("deactive") {
        return Err("原任务名称/活动身份已变化".into());
    }
    if task.get_bool("isRemote").unwrap_or(false) != remote {
        return Err("执行目标已经变更；请求停留在身份核验阶段".into());
    }
    task.get_object_id("_id").map_err(|e| format!("任务ID无效：{e}"))
}

async fn operation_task(clients: &MongoClients, task_id: Option<&str>, name: &str,
    budget: Duration) -> Result<Document, String> {
    let mut filter = doc! { "name": name, "status": { "$ne": "deactive" } };
    if let Some(id) = task_id {
        filter.insert("_id", ObjectId::parse_str(id).map_err(|e| format!("任务ID无效：{e}"))?);
    }
    let tasks = clients.local.database("simulation_mission").collection::<Document>("tasks");
    timeout(budget, tasks.find_one(filter, None)).await
        .map_err(|_| format!("本地控制面任务身份查询超时（{} ms）；请求停留在身份核验阶段", budget.as_millis()))?
        .map_err(|e| format!("本地控制面任务身份查询失败：{e}"))?
        .ok_or_else(|| "本地控制面缺少原任务记录；请求停留在身份核验阶段".into())
}

// Keep legacy pane text intact while removing diagnostic protocol lines before
// the existing strict process/dispatch parser sees the response.
fn runtime_process_output(stdout: &str, stderr: &str) -> Result<String, String> {
    let stdout = stdout.lines().filter(|line| !line.starts_with(TASK_STAGE_PREFIX)
        && !line.starts_with("QUANTNIGHT_TMUX_CONTEXT ") && !line.contains("QUANTNIGHT_RUNTIME_HISTORY"))
        .collect::<Vec<_>>().join("\n");
    if stdout.trim().is_empty() {
        return Err(format!("运行时脚本未返回 Pane 检查结果（stdout 为空或仅有阶段信息）；stderr: {stderr}"));
    }
    Ok((if stderr.trim().is_empty() { stdout } else { format!("{stdout}\n{stderr}") })
        .lines().map(safe_diagnostic_text).collect::<Vec<_>>().join("\n"))
}

#[command]
pub async fn check_task_status(
    task_name: String,
    is_remote: bool,
    task_id: Option<String>,
    request_id: Option<String>,
    app: AppHandle,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
) -> Result<String, String> {
    let started = Instant::now();
    let mut progress = TaskOperationProgress::new(app, "runtime", task_id.as_deref(),
        &task_name, is_remote, request_id);
    progress.stage("initial", "运行检查已接收；总预算12秒");
    // Final aggregate guard includes identity, script readers/exit/cleanup and
    // dispatch. Dropping an in-flight diagnostic also cleans its private group.
    timeout(RUNTIME_BUDGET, async {
        progress.stage("identity", "查询本地控制面的原任务ID、名称与执行目标");
        let task = operation_task(&clients, task_id.as_deref(), &task_name, Duration::from_secs(2)).await
            .map_err(|e| progress.failure(e))?;
        let id = operation_task_identity(&task, &task_name, is_remote).map_err(|e| progress.failure(e))?;
        progress.task_id = id.to_hex();
        progress.tmux_binding = Some(task_script_binding(&task, is_remote).map_err(|e| progress.failure(e))?);
        // Reserve two seconds for dispatch evidence and cleanup. This probe must
        // finish before the UI's 15s deadline; it never inherits the generic 300s.
        let budget = RUNTIME_BUDGET.saturating_sub(started.elapsed())
            .saturating_sub(Duration::from_secs(2)).min(Duration::from_secs(8));
        let (stdout, stderr) = execute_task_script(&progress, &app_config, budget).await?;
        progress.stage("receipt_validate", "检查运行时脚本输出；保留原 Pane 文本");
        let process_output = runtime_process_output(&stdout, &stderr).map_err(|e| progress.failure(e))?;
        progress.stage("dispatch_read", "有界读取实际执行库的派发证据");
        let remaining = RUNTIME_BUDGET.saturating_sub(started.elapsed());
        let (dispatch, diagnostic) = timeout(remaining, read_runtime_evidence(&clients, &id.to_hex(), &task_name, is_remote))
            .await.unwrap_or((None, None));
        let diagnostic = diagnostic.or_else(|| if stderr.trim().is_empty() {
            validated_history_diagnostic(&stdout, &task, id, &task_name, is_remote, bson::DateTime::now().timestamp_millis())
        } else { None });
        let output = business_diagnostic_output(&dispatch_status_output(&process_output, dispatch), diagnostic);
        progress.stage("completed", "运行时检查已返回；状态仍由原 Pane/原生派发证据解析");
        Ok(output)
    }).await.map_err(|_| progress.failure("运行检查总预算12秒耗尽（包含身份查询、脚本stdout/stderr、退出与派发证据）"))?
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
struct ExecutionStopProofV1 {
    schema_version: u32,
    task_name: String,
    target_remote: bool,
    session_absent: bool,
    runners_exited: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionAbsentStopProofV2 {
    schema_version: u32,
    task_name: String,
    target_remote: bool,
    session_absent: bool,
    // Unit requires an explicit JSON null, not an omitted Option or a bool.
    runners_exited: (),
    confirmation_basis: StopConfirmationBasis,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ExecutionStopProof {
    Exited(ExecutionStopProofV1),
    Absent(SessionAbsentStopProofV2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StopConfirmationBasis {
    RunnersExited,
    SessionAbsent,
}

impl StopConfirmationBasis {
    fn as_str(self) -> &'static str {
        match self { Self::RunnersExited => "runners_exited", Self::SessionAbsent => "session_absent" }
    }
    fn description(self) -> &'static str {
        match self {
            Self::RunnersExited => "原会话与已捕获执行进程退出均已核验",
            Self::SessionAbsent => "已确认目标主机上精确tmux会话缺失",
        }
    }
}

// Only the unique final receipt binds a stop basis. A runtime stage/event or
// plain legacy absence line is NEVER pause/control-write authority.
fn verified_stop_proof(stdout: &str, name: &str, remote: bool) -> Result<StopConfirmationBasis, String> {
    let receipts: Vec<_> = stdout.lines().filter_map(|line|
        line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED ")).collect();
    if receipts.len() != 1 || stdout.lines().last().and_then(|line|
        line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED ")) != receipts.first().copied() {
        return Err(match receipts.len() {
            0 => "暂停回执校验失败：缺少结构化暂停回执".into(),
            1 => "暂停回执校验失败：结构化暂停回执后仍有输出，最终行应为暂停回执".into(),
            count => format!("暂停回执校验失败：收到{count}条结构化暂停回执，要求唯一回执"),
        });
    }
    if receipts[0].len() > 8192 { return Err("暂停回执校验失败：最终回执超出8192字节上限".into()); }
    let proof: ExecutionStopProof = serde_json::from_str(receipts[0])
        .map_err(|e| format!("暂停回执解析失败：{e}"))?;
    match proof {
        ExecutionStopProof::Exited(ExecutionStopProofV1 { schema_version: 1, task_name,
            target_remote, session_absent: true, runners_exited: true })
            if task_name == name && target_remote == remote => Ok(StopConfirmationBasis::RunnersExited),
        ExecutionStopProof::Absent(SessionAbsentStopProofV2 { schema_version: 2, task_name,
            target_remote, session_absent: true, runners_exited: (),
            confirmation_basis: StopConfirmationBasis::SessionAbsent })
            if task_name == name && target_remote == remote => Ok(StopConfirmationBasis::SessionAbsent),
        proof => Err(format!("暂停回执校验失败：任务身份、执行主机或停止依据存在字段差异；期望 task_name={name:?}, target_remote={remote}, session_absent=true，V1 runners_exited=true 或 V2 runners_exited=null/confirmation_basis=session_absent；收到 {proof:?}")),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTaskStop {
    confirmed: bool,
    task_id: String,
    task_name: String,
    is_remote: bool,
    stop_confirmed_at: i64,
    confirmation_basis: StopConfirmationBasis,
}

fn confirmed_pause_update(id: ObjectId, remote: bool, at: bson::DateTime,
    basis: StopConfirmationBasis) -> Document {
    doc! { "status": "paused", "pausedAt": at, "stopConfirmedAt": at, "updatedAt": at,
        "stopConfirmedRemote": remote, "stopConfirmedTaskId": id.to_hex(),
        "stopConfirmationBasis": basis.as_str() }
}

fn pause_control_write(id: ObjectId, name: &str, original: &Document,
    remote: bool, at: bson::DateTime, basis: StopConfirmationBasis) -> (Document, Document) {
    let mut filter = doc! { "_id": id, "name": name };
    // Preserve the same task/host AND the captured control revision. This uses
    // existing updatedAt/launch/evidence fields, not a new version mechanism.
    for key in ["status", "isRemote", "updatedAt", "launchAttemptId", "launchAttemptAt", "startedAt",
        "generation", "startupBinding", "tmuxContext", "stopConfirmedAt", "stopConfirmedRemote",
        "stopConfirmedTaskId", "stopConfirmationBasis"] {
        filter.insert(key, original.get(key).cloned().unwrap_or_else(|| bson::Bson::Document(doc! { "$exists": false })));
    }
    (filter, doc! { "$set": confirmed_pause_update(id, remote, at, basis) })
}

fn validate_stop_output(progress: &TaskOperationProgress, stdout: &str, stderr: &str) -> Result<StopConfirmationBasis, String> {
    progress.stage("receipt_validate", "验证唯一最终暂停回执、任务/执行端与V1进程退出或V2会话缺失依据");
    verified_stop_proof(stdout, &progress.name, progress.remote)
        .map_err(|e| progress.failure(format!("{e}；stderr: {stderr}；stdout: {stdout}")))
}

fn confirmed_stop_after_write(progress: &TaskOperationProgress, id: ObjectId, at: bson::DateTime,
    basis: StopConfirmationBasis, matched_count: u64) -> Result<ConfirmedTaskStop, String> {
    if matched_count != 1 {
        return Err(progress.failure(format!("{}；暂停回写匹配失败：原任务身份/执行目标/控制版本已变化", basis.description())));
    }
    progress.stage("completed", &format!("{}；控制面CAS回写已确认", basis.description()));
    Ok(ConfirmedTaskStop { confirmed: true, task_id: id.to_hex(), task_name: progress.name.clone(),
        is_remote: progress.remote, stop_confirmed_at: at.timestamp_millis(), confirmation_basis: basis })
}

#[command]
pub async fn pause_task(task_name: String, is_remote: bool, task_id: Option<String>,
    request_id: Option<String>, app: AppHandle,
    clients: State<'_, Arc<MongoClients>>, app_config: State<'_, AppConfig>) -> Result<ConfirmedTaskStop, String> {
    let mut progress = TaskOperationProgress::new(app, "stop", task_id.as_deref(),
        &task_name, is_remote, request_id);
    progress.stage("initial", "暂停请求已接收；停止操作次数1次，脚本预算75秒");
    progress.stage("identity", "核对本地控制面原任务与执行目标");
    let task = operation_task(&clients, task_id.as_deref(), &task_name, Duration::from_secs(5)).await
        .map_err(|e| progress.failure(e))?;
    let id = operation_task_identity(&task, &task_name, is_remote).map_err(|e| progress.failure(e))?;
    progress.task_id = id.to_hex();
    progress.tmux_binding = Some(task_script_binding(&task, is_remote).map_err(|e| progress.failure(e))?);
    if matches!(task.get_str("status").ok(), Some("generating" | "generation_unknown")) {
        return Err(progress.failure("当前处于候选生成阶段；请先核验生成作业"));
    }
    let (stdout, stderr) = execute_task_script(&progress, &app_config, PAUSE_SCRIPT_BUDGET).await?;
    let basis = validate_stop_output(&progress, &stdout, &stderr)?;
    let at = bson::DateTime::now();
    let (filter, update) = pause_control_write(id, &task_name, &task, is_remote, at, basis);
    progress.stage("control_write", &format!("{}；按原任务ID/名称/执行目标/控制版本CAS回写暂停（5秒）", basis.description()));
    let tasks = clients.local.database("simulation_mission").collection::<Document>("tasks");
    let result = timeout(Duration::from_secs(5), tasks.update_one(filter, update, None)).await
        .map_err(|_| progress.failure(format!("{}；控制记录回写超时（预算5秒）", basis.description())))?
        .map_err(|e| progress.failure(format!("{}；控制记录回写失败：{e}", basis.description())))?;
    confirmed_stop_after_write(&progress, id, at, basis, result.matched_count)
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
    fn pause_copy_reports_receipt_gap_and_field_values() {
        let valid = proof(json!({}));
        for (raw, expected) in [
            (String::new(), "缺少结构化暂停回执"),
            (valid.clone() + &valid, "收到2条结构化暂停回执，要求唯一回执"),
            (valid.clone() + "more output\n", "结构化暂停回执后仍有输出，最终行应为暂停回执"),
            (format!("QUANTNIGHT_PAUSE_CONFIRMED {}", "x".repeat(8193)), "最终回执超出8192字节上限"),
        ] {
            let error = verified_stop_proof(&raw, "pipeline__priority", true).unwrap_err();
            assert_eq!(error, format!("暂停回执校验失败：{expected}"));
            assert!(!error.contains('不'));
        }
        for (changes, actual) in [
            (json!({"task_name":"other"}), "task_name: \"other\""),
            (json!({"target_remote":false}), "target_remote: false"),
            (json!({"session_absent":false}), "session_absent: false"),
            (json!({"runners_exited":false}), "runners_exited: false"),
            (json!({"schema_version":3}), "schema_version: 3"),
        ] {
            let error = verified_stop_proof(&proof(changes), "pipeline__priority", true).unwrap_err();
            assert!(error.starts_with("暂停回执校验失败：任务身份、执行主机或停止依据存在字段差异"), "{error}");
            assert!(error.contains("期望 task_name=\"pipeline__priority\", target_remote=true"), "{error}");
            assert!(error.contains(actual), "{error}");
            assert!(!error.contains('不'));
        }
        let error = verified_stop_proof("QUANTNIGHT_PAUSE_CONFIRMED {broken}", "pipeline__priority", true).unwrap_err();
        assert!(error.starts_with("暂停回执解析失败："));
        assert_eq!(StopConfirmationBasis::SessionAbsent.description(), "已确认目标主机上精确tmux会话缺失");
        assert_eq!(StopConfirmationBasis::RunnersExited.description(), "原会话与已捕获执行进程退出均已核验");
    }

    #[test]
    fn task_list_preserves_persisted_stop_evidence_for_remounts() {
        let id = ObjectId::new();
        let at = bson::DateTime::now();
        let mut record = confirmed_pause_update(id, true, at, StopConfirmationBasis::RunnersExited);
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
        let update = confirmed_pause_update(id, true, at, StopConfirmationBasis::RunnersExited);
        assert_eq!(update.get_str("status").unwrap(), "paused");
        assert_eq!(update.get_str("stopConfirmedTaskId").unwrap(), id.to_hex());
        assert!(update.get_bool("stopConfirmedRemote").unwrap());
        assert_eq!(*update.get_datetime("stopConfirmedAt").unwrap(), at);
        let response = serde_json::to_value(ConfirmedTaskStop { confirmed: true,
            task_id: id.to_hex(), task_name: "pipeline__priority".into(), is_remote: true,
            stop_confirmed_at: at.timestamp_millis(), confirmation_basis: StopConfirmationBasis::RunnersExited }).unwrap();
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

#[cfg(all(test, unix))]
mod template_catalog_tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    // Fake process IO only: never invokes real Python, taskctl, Mongo or a service.
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("quantnight-template-{}", ObjectId::new()));
            fs::create_dir_all(root.join(".venv/bin")).unwrap();
            let root = root.canonicalize().unwrap();
            let executable = root.join(".venv/bin/python");
            fs::write(&executable, r#"#!/bin/sh
printf '%s\n' "$@" >> calls
if [ "$1" = '-m' ]; then
  [ "$2" = 'templates.catalog' ] || exit 91
  [ "$4" = '--root' ] || exit 92
  [ "$5" = "$PWD" ] || exit 93
  [ "$6" = '--kind' ] || exit 94
  cat catalog.json
  exit "$(cat catalog.exit)"
fi
[ "$1" = '.agents/skills/manage-quantnight-frontend/scripts/taskctl.py' ] || exit 95
cat archive.json
exit "$(cat archive.exit)"
"#).unwrap();
            fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(root.join("catalog.exit"), "0").unwrap();
            fs::write(root.join("archive.exit"), "0").unwrap();
            Self(root)
        }
        fn catalog(&self, value: JsonValue) {
            fs::write(self.0.join("catalog.json"), value.to_string()).unwrap();
        }
        fn receipt(&self, value: JsonValue) {
            fs::write(self.0.join("archive.json"), value.to_string()).unwrap();
        }
        fn calls(&self) -> String { fs::read_to_string(self.0.join("calls")).unwrap_or_default() }
        fn config(&self) -> AppConfig {
            serde_json::from_value(json!({"env":{"working_dir":self.0},
                "python":{"scripts":{}}, "bash":{"scripts":{}},
                "mongodb":{"local_uri":"unused", "remote_uri":"unused",
                    "databases":{"mission":"unused", "simulation":"unused", "alpha":"unused"}}})).unwrap()
        }
        fn task(&self) -> Document {
            doc! {"_id": ObjectId::new(), "name":"task", "templatefile":"same_name",
                "taskType":"super", "status":"paused", "isRemote":true}
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[tokio::test]
    async fn latest_uses_catalog_kind_directory_and_bare_name() {
        let f = Fixture::new();
        f.catalog(json!({"ok":true,"entry":{"name":"published","taskType":"super","format":"descriptor"}}));
        let folder = f.0.join("templates/generated/super").to_string_lossy().into_owned();
        assert_eq!(latest_template_from_directory(&f.0, &folder, None).await.unwrap(), "published");
        assert!(f.calls().contains(&format!("--kind\nsuper\n--directory\n{folder}\n")));
        f.catalog(json!({"ok":true,"entry":null}));
        assert!(latest_template_from_directory(&f.0, &folder, Some("super")).await.is_err());
        f.catalog(json!({"ok":true,"entry":{"name":"wrong_kind","taskType":"regular"}}));
        assert!(latest_template_from_directory(&f.0, &folder, Some("super")).await.is_err());
    }

    #[tokio::test]
    async fn dataset_binding_archives_original_task_and_exact_target_before_delete() {
        let f = Fixture::new();
        f.catalog(json!({"ok":true,"binding":{"kind":"dataset","loader":"frozen_research","extension":"kept"}}));
        f.receipt(json!({"ok":true,"command":"archive-research","task":"task","archivePath":"/fake/archive.json"}));
        let task = f.task();
        archive_prepared_research(&task, &f.config()).await.unwrap();
        let calls = f.calls();
        assert!(calls.contains("--kind\nsuper\n--name\nsame_name\n"));
        assert!(calls.contains(&format!("archive-research\n--name\ntask\n--expected-task-id\n{}\n--expected-remote\ntrue\n", task.get_object_id("_id").unwrap().to_hex())));
        assert!(calls.find("templates.catalog").unwrap() < calls.find("archive-research").unwrap());
    }

    #[tokio::test]
    async fn archive_rejects_invalid_binding_collision_process_error_and_wrong_receipt() {
        for binding in [json!({"ok":false,"error":"ambiguous template name collision"}),
                        json!({"ok":true}), json!({"ok":true,"binding":false})] {
            let f = Fixture::new(); f.catalog(binding);
            assert!(archive_prepared_research(&f.task(), &f.config()).await.is_err());
            assert!(!f.calls().contains("archive-research"));
        }
        for receipt in [json!({"ok":false}),
            json!({"ok":true,"command":"archive-research","task":"other","archivePath":"/fake"}),
            json!({"ok":true,"command":"verify-registration","task":"task","archivePath":"/fake"}),
            json!({"ok":true,"command":"archive-research","task":"task","archivePath":""})] {
            let f = Fixture::new();
            f.catalog(json!({"ok":true,"binding":{"kind":"dataset"}})); f.receipt(receipt);
            assert!(archive_prepared_research(&f.task(), &f.config()).await.is_err());
        }
        let f = Fixture::new(); f.catalog(json!({"ok":true,"binding":null}));
        fs::write(f.0.join("catalog.exit"), "1").unwrap();
        assert!(archive_prepared_research(&f.task(), &f.config()).await.is_err());
    }

    #[tokio::test]
    async fn missing_legacy_binding_refine_and_priority_do_not_archive_dataset() {
        for binding in [JsonValue::Null, json!({"loader":"frozen_refine","schemaVersion":3})] {
            let f = Fixture::new(); f.catalog(json!({"ok":true,"binding":binding}));
            archive_prepared_research(&f.task(), &f.config()).await.unwrap();
            assert!(!f.calls().contains("archive-research"));
        }
        let f = Fixture::new(); let mut task = f.task(); task.insert("taskType", "priority");
        archive_prepared_research(&task, &f.config()).await.unwrap();
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn running_dataset_requires_pause_and_nonzero_archive_is_not_success() {
        let f = Fixture::new(); f.catalog(json!({"ok":true,"binding":{"kind":"dataset"}}));
        let mut task = f.task(); task.insert("status", "running");
        assert!(archive_prepared_research(&task, &f.config()).await.is_err());
        assert!(!f.calls().contains("archive-research"));
        task.insert("status", "paused");
        f.receipt(json!({"ok":true,"command":"archive-research","task":"task","archivePath":"/fake"}));
        fs::write(f.0.join("archive.exit"), "1").unwrap();
        assert!(archive_prepared_research(&task, &f.config()).await.is_err());
    }
    #[tokio::test]
    #[ignore = "set QUANTNIGHT_CATALOG_SOURCE and QUANTNIGHT_TEST_PYTHON for offline cross-repo CLI contract"]
    async fn actual_catalog_cli_in_temporary_repository() {
        let source = std::env::var("QUANTNIGHT_CATALOG_SOURCE").unwrap();
        let python = std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap();
        let f = Fixture::new();
        fs::remove_file(f.0.join(".venv/bin/python")).unwrap();
        std::os::unix::fs::symlink(python, f.0.join(".venv/bin/python")).unwrap();
        fs::create_dir_all(f.0.join("templates/generated/regular")).unwrap();
        fs::create_dir_all(f.0.join("templates/generated/super")).unwrap();
        fs::create_dir_all(f.0.join("super_templates")).unwrap();
        fs::write(f.0.join("templates/__init__.py"), "").unwrap();
        fs::copy(source, f.0.join("templates/catalog.py")).unwrap();
        for (kind, sha) in [
            ("regular", "d95d5532c10b09a6348b70512ce53fe3a3228c28000d123c20970c882001b4b8"),
            ("super", "f7a4d0ead7dff04008c915c6d41d69f9ee407a863e04fa4361ae77d638327375"),
        ] {
            let manifest = format!("{{\"schemaVersion\":1,\"taskType\":\"{kind}\",\"provenance\":{{\"kind\":\"dataset\"}}}}");
            fs::write(f.0.join(format!("{kind}.json")), manifest).unwrap();
            let folder = f.0.join(format!("templates/generated/{kind}"));
            fs::write(folder.join("same_name.research.json"), json!({
                "taskType":kind,"loader":"frozen_research","manifest":format!("{kind}.json"),
                "sha256":sha,"kind":"dataset","extension":"preserved"}).to_string()).unwrap();
            // These newer modules/claims must never be executed or selected.
            for name in ["helper.py", "__init__.py", ".claim.py", "incomplete.tmp.py"] {
                fs::write(folder.join(name), "raise RuntimeError('must not execute')\n").unwrap();
            }
            assert_eq!(latest_template_from_directory(&f.0, folder.to_str().unwrap(), Some(kind)).await.unwrap(), "same_name");
            let binding = run_template_catalog(&f.0, "binding", kind, Some("same_name"), None).await.unwrap();
            assert_eq!(binding["binding"]["taskType"], kind);
            assert_eq!(binding["binding"]["extension"], "preserved");
        }
        let regular = f.0.join("templates/generated/regular");
        fs::write(f.0.join("templates/same_name.py"), "async def generate_parms(session): pass\n").unwrap();
        assert!(latest_template_from_directory(&f.0, regular.to_str().unwrap(), Some("regular")).await.unwrap_err().contains("collision"));
        fs::remove_file(f.0.join("templates/same_name.py")).unwrap();
        fs::write(f.0.join("regular.json"), "tampered").unwrap();
        assert!(run_template_catalog(&f.0, "binding", "regular", Some("same_name"), None).await.is_err());
        // A legacy Python entry is still manually resolvable when generated is empty.
        fs::remove_file(regular.join("same_name.research.json")).unwrap();
        fs::write(f.0.join("templates/legacy.py"), "raise RuntimeError('not executed')\nasync def generate_parms(session): pass\n").unwrap();
        assert!(latest_template_from_directory(&f.0, regular.to_str().unwrap(), Some("regular")).await.is_err());
        assert_eq!(latest_template_from_directory(&f.0, f.0.join("templates").to_str().unwrap(), Some("regular")).await.unwrap(), "legacy");
        assert!(run_template_catalog(&f.0, "binding", "regular", Some("legacy"), None).await.unwrap()["binding"].is_null());
    }

}

#[cfg(test)]
mod task_stage_diagnostics_tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    use serde_json::json;

    fn progress(operation: &'static str) -> (TaskOperationProgress, Arc<Mutex<Vec<TaskOperationStage>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        (TaskOperationProgress { operation, task_id: "0123456789abcdef01234567".into(),
            name: "exact__priority".into(), remote: true, request_id: "request-native-1".into(), tmux_binding: None,
            last: Arc::new(Mutex::new(("initial".into(), "原生请求".into()))),
            sink: Arc::new(move |event| recorded.lock().unwrap().push(event)) }, events)
    }

    fn stage(operation: &str, phase: &str) -> JsonValue {
        json!({"operation": operation, "task_name": "exact__priority", "target_remote": true,
            "stage": phase, "message": "正在核对精确任务的执行步骤"})
    }

    fn shell_line(value: &JsonValue) -> String {
        format!("printf '%s\\n' '{}{}'\n", TASK_STAGE_PREFIX, value)
    }

    fn stop_receipt() -> String {
        format!("QUANTNIGHT_PAUSE_CONFIRMED {}", json!({"schema_version":1,
            "task_name":"exact__priority", "target_remote":true,
            "session_absent":true, "runners_exited":true}))
    }

    #[test]
    fn bounded_exact_schema_identity_cannot_forge_scope_or_control_authority() {
        let (p, events) = progress("runtime");
        p.script_stage(&format!("{TASK_STAGE_PREFIX}{}", stage("runtime", "ssh")));
        for change in [json!({"operation":"stop"}), json!({"task_name":"exact"}),
            json!({"target_remote":false}), json!({"target_remote":"true"}),
            json!({"stage":"control_write"}), json!({"stage":"paused"}), json!({"stage":"interrupt"}),
            json!({"message":""}), json!({"message":"\n"}), json!({"message":"x".repeat(1025)}),
            json!({"requestId":"spoof"}), json!({"taskId":"spoof"}), json!({"confirmed":true})] {
            let mut value = stage("runtime", "ssh");
            for (key, val) in change.as_object().unwrap() { value[key] = val.clone(); }
            p.script_stage(&format!("{TASK_STAGE_PREFIX}{value}"));
        }
        for line in ["QUANTNIGHT_TASK_STAGE {broken}".to_string(),
            format!(" {}{}", TASK_STAGE_PREFIX, stage("runtime", "ssh")),
            format!("{}{}", TASK_STAGE_PREFIX, "x".repeat(4097)),
            "QUANTNIGHT_PAUSE_CONFIRMED {\"confirmed\":true}".into(),
            format!("{TASK_STAGE_PREFIX}{{\"operation\":\"runtime\",\"operation\":\"stop\"}}"),
            format!("{TASK_STAGE_PREFIX}{}", "[".repeat(300))] { p.script_stage(&line); }
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(serde_json::to_value(&events[0]).unwrap(), json!({"operation":"runtime",
            "taskId":"0123456789abcdef01234567", "name":"exact__priority", "isRemote":true,
            "requestId":"request-native-1", "stage":"ssh", "message":"正在核对精确任务的执行步骤"}));
        drop(events);
        let (p, events) = progress("stop");
        for phase in ["config", "ssh", "tmux", "process", "interrupt", "session_exit", "process_exit", "receipt", "complete"] {
            p.script_stage(&format!("{TASK_STAGE_PREFIX}{}", stage("stop", phase)));
        }
        assert_eq!(events.lock().unwrap().len(), 9);
    }

    #[tokio::test]
    async fn native_operation_real_runner_reports_stages_and_preserves_legacy_pane() {
        let pane = "✅ Pane 0 (pid=123) is running: python";
        let script = shell_line(&stage("runtime", "ssh")) + &shell_line(&stage("runtime", "process"))
            + &format!("printf '%s\\n' '{pane}'\n");
        let fake = FakeBash::new(&script);
        let (p, events) = progress("runtime");
        let (stdout, stderr) = execute_task_script(&p, &fake.config, Duration::from_secs(2)).await.unwrap();
        assert_eq!(runtime_process_output(&stdout, &stderr).unwrap(), pane);
        let events = events.lock().unwrap();
        assert_eq!(events.iter().map(|e| e.stage.as_str()).collect::<Vec<_>>(),
            ["config", "launch", "wait", "ssh", "process"]);
        assert!(events.iter().all(|e| e.task_id == p.task_id && e.request_id == p.request_id && e.is_remote));
    }

    #[tokio::test]
    async fn native_nonzero_timeout_keep_last_script_stage_and_stderr() {
        for (tail, cause, budget) in [
            ("echo exact-ssh-denied >&2; exit 7", "exact-ssh-denied", Duration::from_secs(2)),
            ("echo exact-ssh-stall >&2; exec /bin/sleep 30", "exact-ssh-stall", Duration::from_millis(100)),
        ] {
            let fake = FakeBash::new(&(shell_line(&stage("runtime", "ssh")) + tail));
            let (p, events) = progress("runtime");
            let error = execute_task_script(&p, &fake.config, budget).await.unwrap_err();
            assert!(error.contains("stage=ssh") && error.contains(cause), "{error}");
            let events = events.lock().unwrap();
            assert_eq!(events.last().unwrap().stage, "ssh");
            assert!(events.last().unwrap().message.contains(cause));
            assert!(!events.iter().any(|e| ["control_write", "completed"].contains(&e.stage.as_str())));
        }
    }

    #[tokio::test]
    async fn native_configuration_launch_errors_emit_specific_failed_step() {
        let mut fake = FakeBash::new(":");
        fake.config.bash.scripts.remove("check_task_status");
        let (p, events) = progress("runtime");
        let error = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap_err();
        assert!(error.contains("stage=config") && error.contains("未找到 check_task_status"));
        assert_eq!(events.lock().unwrap().last().unwrap().stage, "config");
        let mut fake = FakeBash::new(":");
        fake.config.bash.scripts.get_mut("check_task_status").unwrap().command = Some("/not/a/command".into());
        let error = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap_err();
        assert!(error.contains("stage=launch") && error.contains("启动命令失败"));
        assert_eq!(events.lock().unwrap().last().unwrap().stage, "launch");
    }

    #[tokio::test]
    async fn empty_runtime_fake_stop_success_never_confirmed() {
        for source in [":", "echo 'QUANTNIGHT_TASK_STAGE {}'", "echo 'QUANTNIGHT_TMUX_CONTEXT {}'"] {
            let fake = FakeBash::new(source);
            let (p, _) = progress("runtime");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap();
            p.stage("receipt_validate", "核验运行时输出");
            let error = runtime_process_output(&out, &err).map_err(|e| p.failure(e)).unwrap_err();
            assert!(error.contains("stage=receipt_validate") && error.contains("为空或仅有阶段信息"));
        }
        for source in [":", "echo 已暂停", "echo 'QUANTNIGHT_PAUSE_CONFIRMED {broken}'",
            "echo 'QUANTNIGHT_PAUSE_CONFIRMED {\"schema_version\":1,\"confirmed\":true}'"] {
            let fake = FakeBash::new(source);
            let (p, events) = progress("stop");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap();
            let error = validate_stop_output(&p, &out, &err).unwrap_err();
            assert!(error.contains("stage=receipt_validate"), "{error}");
            assert!(!events.lock().unwrap().iter().any(|e| e.stage == "control_write"));
        }
    }

    #[tokio::test]
    async fn stop_receipt_final_and_control_write_builder_retains_cas() {
        let receipt = stop_receipt();
        let script = shell_line(&stage("stop", "process_exit")) + &format!("printf '%s\\n' '{receipt}'");
        let fake = FakeBash::new(&script);
        let (p, events) = progress("stop");
        let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap();
        validate_stop_output(&p, &out, &err).unwrap();
        assert_eq!(out.lines().last(), Some(receipt.as_str()));
        let id = ObjectId::parse_str(&p.task_id).unwrap();
        let at = bson::DateTime::now();
        let (filter, update) = pause_control_write(id, &p.name, &doc! {"isRemote":true, "status":"running"}, true, at, StopConfirmationBasis::RunnersExited);
        assert_eq!(filter.get_object_id("_id").unwrap(), id);
        assert_eq!(filter.get_str("name").unwrap(), "exact__priority");
        assert_eq!(filter.get_str("status").unwrap(), "running");
        assert_eq!(filter.get_bool("isRemote").unwrap(), true);
        assert_eq!(filter.get_document("updatedAt").unwrap(), &doc! {"$exists":false});
        assert_eq!(update, doc! {"$set": confirmed_pause_update(id, true, at, StopConfirmationBasis::RunnersExited)});
        let (null_filter, _) = pause_control_write(id, &p.name, &doc! {"isRemote":bson::Bson::Null}, false, at, StopConfirmationBasis::RunnersExited);
        assert_eq!(null_filter.get("isRemote"), Some(&bson::Bson::Null));
        p.stage("control_write", "仅构造原CAS；本测试不连接数据库");
        let error = p.failure("synthetic write rejected: original host changed");
        assert!(error.contains("stage=control_write") && error.contains("host changed"));
        assert_eq!(events.lock().unwrap().last().unwrap().stage, "control_write");
        assert!(verified_stop_proof(&(out + "\n" + TASK_STAGE_PREFIX + &stage("stop", "complete").to_string()),
            &p.name, true).is_err());
    }

    #[tokio::test]
    async fn legacy_missing_request_id_suppresses_only_events_not_operation_or_diagnostics() {
        for operation in ["runtime", "stop"] {
            let result_line = if operation == "stop" { stop_receipt() }
                else { "✅ Pane 0 (pid=123) is running: python".into() };
            let fake = FakeBash::new(&(shell_line(&stage(operation, "process"))
                + &format!("printf '%s\\n' '{result_line}'")));
            let (mut p, events) = progress(operation);
            p.request_id.clear();
            p.stage("initial", "legacy调用不发送缺少requestId的事件");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1)).await.unwrap();
            assert_eq!(p.last.lock().unwrap().0, "process");
            assert!(p.failure("synthetic diagnostic").contains("stage=process"));
            if operation == "stop" { validate_stop_output(&p, &out, &err).unwrap(); }
            else { assert_eq!(runtime_process_output(&out, &err).unwrap(), result_line); }
            assert!(events.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn native_binding_rejects_changed_host_identity_and_budgets_fit_ui() {
        let id = ObjectId::new();
        let mut task = doc! {"_id":id, "name":"exact__priority", "isRemote":true, "status":"running"};
        assert_eq!(operation_task_identity(&task, "exact__priority", true).unwrap(), id);
        assert!(operation_task_identity(&task, "exact__priority", false).unwrap_err().contains("执行目标已经变更"));
        assert!(operation_task_identity(&task, "exact", true).is_err());
        task.insert("status", "deactive");
        assert!(operation_task_identity(&task, "exact__priority", true).is_err());
        assert!(RUNTIME_BUDGET < Duration::from_secs(15));
        assert!(PAUSE_SCRIPT_BUDGET > Duration::from_secs(60));
        assert!(PAUSE_SCRIPT_BUDGET + Duration::from_secs(10) < Duration::from_secs(120));
    }
}

#[cfg(test)]
mod session_absent_v2_tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    use serde_json::json;

    fn progress(operation: &'static str, remote: bool) -> (TaskOperationProgress, Arc<Mutex<Vec<TaskOperationStage>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        (TaskOperationProgress { operation, task_id: "0123456789abcdef01234567".into(),
            name: "exact__priority".into(), remote, request_id: "session-absent-v2-request".into(), tmux_binding: None,
            last: Arc::new(Mutex::new(("initial".into(), "请求已接收".into()))),
            sink: Arc::new(move |event| sink_events.lock().unwrap().push(event)) }, events)
    }
    fn stage(operation: &str, remote: bool, phase: &str) -> JsonValue {
        json!({"operation":operation, "task_name":"exact__priority", "target_remote":remote,
            "stage":phase, "message":"精确tmux会话不存在；不代表历史孤儿进程退出或平台取消"})
    }
    fn proof(version: u32, remote: bool) -> JsonValue {
        let mut proof = json!({"schema_version":version, "task_name":"exact__priority", "target_remote":remote,
            "session_absent":true, "runners_exited":true});
        if version == 2 {
            proof["runners_exited"] = JsonValue::Null;
            proof["confirmation_basis"] = json!("session_absent");
        }
        proof
    }
    fn receipt(proof: &JsonValue) -> String { format!("QUANTNIGHT_PAUSE_CONFIRMED {proof}") }
    fn print_line(line: &str) -> String {
        let escaped = line.replace('\'', "'\"'\"'");
        format!("printf '%s\\n' '{escaped}'\n")
    }

    #[tokio::test]
    async fn real_runner_absent_runtime_is_success_and_stage_never_writes_control() {
        for remote in [false, true] {
            let legacy = format!("❌ Session 'exact__priority' 不存在 ({})", if remote { "remote" } else { "local" });
            let script = print_line(&format!("{TASK_STAGE_PREFIX}{}", stage("runtime", remote, "session_absent")))
                + &print_line(&legacy) + "exit 0\n";
            let fake = FakeBash::new(&script);
            let (progress, events) = progress("runtime", remote);
            let (out, err) = execute_task_script(&progress, &fake.config, Duration::from_secs(2)).await.unwrap();
            assert_eq!(runtime_process_output(&out, &err).unwrap(), legacy);
            assert_eq!(dispatch_status_output(&runtime_process_output(&out, &err).unwrap(), None), legacy);
            assert_eq!(progress.last.lock().unwrap().0, "session_absent");
            assert!(events.lock().unwrap().iter().any(|event| event.stage == "session_absent"
                && event.message == stage("runtime", remote, "session_absent")["message"].as_str().unwrap()));
            assert!(!events.lock().unwrap().iter().any(|event| ["control_write", "paused"].contains(&event.stage.as_str())));
            // Legacy absence output is not itself a structured pause receipt.
            assert!(verified_stop_proof(&out, &progress.name, remote).is_err());
        }
    }

    #[tokio::test]
    async fn real_runner_v2_and_v1_preserve_distinct_basis_through_metadata_and_cas_gate() {
        for remote in [false, true] {
            for (version, expected) in [(1, StopConfirmationBasis::RunnersExited), (2, StopConfirmationBasis::SessionAbsent)] {
                let final_line = receipt(&proof(version, remote));
                let script = print_line(&format!("{TASK_STAGE_PREFIX}{}", stage("stop", remote, if version == 2 { "session_absent" } else { "receipt" })))
                    + &print_line(&final_line);
                let fake = FakeBash::new(&script);
                let (progress, events) = progress("stop", remote);
                let (out, err) = execute_task_script(&progress, &fake.config, Duration::from_secs(2)).await.unwrap();
                let basis = validate_stop_output(&progress, &out, &err).unwrap();
                assert_eq!(basis, expected);
                assert_eq!(out.lines().last(), Some(final_line.as_str()));
                assert!(!events.lock().unwrap().iter().any(|e| e.stage == "control_write"));
                let id = ObjectId::parse_str(&progress.task_id).unwrap();
                let at = bson::DateTime::now();
                let original = doc! {"_id":id, "name":&progress.name, "isRemote":remote, "status":"running",
                    "updatedAt":"original-control-revision", "launchAttemptId":"original-launch"};
                let (filter, update) = pause_control_write(id, &progress.name, &original, remote, at, basis);
                assert_eq!(filter.get_str("updatedAt").unwrap(), "original-control-revision");
                assert_eq!(filter.get_str("launchAttemptId").unwrap(), "original-launch");
                assert_eq!(filter.get_bool("isRemote").unwrap(), remote);
                let set = update.get_document("$set").unwrap();
                assert_eq!(set.get_str("stopConfirmationBasis").unwrap(), basis.as_str());
                assert!(!set.contains_key("runners_exited") && !set.contains_key("platformCancelled"));
                let mut persisted = set.clone();
                persisted.insert("_id", id); persisted.insert("name", &progress.name); persisted.insert("isRemote", remote);
                let mapped: Task = bson::from_document(persisted).unwrap();
                assert_eq!(serde_json::to_value(mapped).unwrap()["stopConfirmationBasis"], basis.as_str());
                progress.stage("control_write", basis.description());
                let rejected = confirmed_stop_after_write(&progress, id, at, basis, 0).unwrap_err();
                assert!(rejected.contains("stage=control_write") && rejected.contains("控制版本已变化"));
                assert!(rejected.contains(basis.description()) && rejected.contains("暂停回写匹配失败"));
                assert!(!rejected.contains('不'));
                assert!(!events.lock().unwrap().iter().any(|e| e.stage == "completed"));
                // This is only the pure real-command gate, not a fake DB write.
                let confirmed = confirmed_stop_after_write(&progress, id, at, basis, 1).unwrap();
                let wire = serde_json::to_value(confirmed).unwrap();
                assert_eq!(wire["confirmed"], true);
                assert_eq!(wire["taskId"], id.to_hex());
                assert_eq!(wire["isRemote"], remote);
                assert_eq!(wire["confirmationBasis"], basis.as_str());
                if version == 2 {
                    assert_eq!(events.lock().unwrap().last().unwrap().message,
                        "已确认目标主机上精确tmux会话缺失；控制面CAS回写已确认");
                }
            }
        }
    }

    #[tokio::test]
    async fn real_runner_strict_v2_rejects_forged_missing_wrong_type_duplicate_and_nonfinal_proofs() {
        let valid = proof(2, true);
        let mut bad = Vec::new();
        for change in [json!({"schema_version":true}), json!({"schema_version":2.0}), json!({"schema_version":3}),
            json!({"task_name":"exact"}), json!({"target_remote":false}), json!({"target_remote":"true"}),
            json!({"session_absent":false}), json!({"session_absent":"true"}),
            json!({"runners_exited":true}), json!({"runners_exited":false}), json!({"runners_exited":"null"}),
            json!({"confirmation_basis":"runners_exited"}), json!({"confirmation_basis":null}),
            json!({"confirmation_basis":false}), json!({"extra":"forged"})] {
            let mut invalid = valid.clone();
            for (key, value) in change.as_object().unwrap() { invalid[key] = value.clone(); }
            bad.push(receipt(&invalid));
        }
        for missing in ["schema_version", "task_name", "target_remote", "session_absent", "runners_exited", "confirmation_basis"] {
            let mut invalid = valid.clone(); invalid.as_object_mut().unwrap().remove(missing);
            bad.push(receipt(&invalid));
        }
        bad.push(format!("QUANTNIGHT_PAUSE_CONFIRMED {}", valid.to_string().replacen("{", "{\"schema_version\":2,", 1)));
        bad.push(receipt(&valid) + "\n" + &receipt(&valid));
        bad.push(receipt(&valid) + "\n" + TASK_STAGE_PREFIX + &stage("stop", true, "complete").to_string());
        bad.push(format!("{TASK_STAGE_PREFIX}{}", stage("stop", true, "session_absent")));
        for stdout in bad {
            let script = stdout.lines().map(print_line).collect::<String>();
            let fake = FakeBash::new(&script);
            let (progress, events) = progress("stop", true);
            let (out, err) = execute_task_script(&progress, &fake.config, Duration::from_secs(2)).await.unwrap();
            let error = validate_stop_output(&progress, &out, &err).unwrap_err();
            assert!(error.contains("stage=receipt_validate"), "{stdout}: {error}");
            assert!(!events.lock().unwrap().iter().any(|e| ["control_write", "completed"].contains(&e.stage.as_str())));
        }
        // V1 remains exact: adding the V2 basis or changing true->null is invalid.
        for change in [json!({"confirmation_basis":"session_absent"}), json!({"confirmation_basis":null}),
            json!({"runners_exited":null})] {
            let mut invalid = proof(1, true);
            for (key, value) in change.as_object().unwrap() { invalid[key] = value.clone(); }
            assert!(verified_stop_proof(&receipt(&invalid), "exact__priority", true).is_err());
        }
    }

    #[tokio::test]
    async fn real_runner_progress_cannot_fake_control_write_or_confirmation() {
        let (progress, events) = progress("runtime", true);
        let script = print_line(&format!("{TASK_STAGE_PREFIX}{}", stage("runtime", true, "session_absent")))
            + &print_line(&format!("{TASK_STAGE_PREFIX}{}", stage("runtime", true, "control_write")))
            + &print_line(&format!("{TASK_STAGE_PREFIX}{}", stage("stop", true, "session_absent")))
            + &print_line(&receipt(&proof(2, true)))
            + &print_line("❌ Session 'exact__priority' 不存在 (remote)");
        let fake = FakeBash::new(&script);
        let (out, _) = execute_task_script(&progress, &fake.config, Duration::from_secs(2)).await.unwrap();
        assert_eq!(progress.last.lock().unwrap().0, "session_absent");
        assert!(!events.lock().unwrap().iter().any(|e| ["control_write", "paused", "completed"].contains(&e.stage.as_str())));
        assert!(verified_stop_proof(&out, &progress.name, true).is_err());
    }

    #[test]
    fn control_cas_retains_original_status_revision_host_launch_and_absent_fields() {
        let id = ObjectId::new(); let at = bson::DateTime::now();
        let original = doc! {"status":"running", "isRemote":true, "updatedAt":17_i64,
            "launchAttemptId":"launch-v1", "generation":{"id":"generation-v1"}, "stopConfirmedAt":bson::Bson::Null};
        let (filter, set) = pause_control_write(id, "exact__priority", &original, true, at, StopConfirmationBasis::SessionAbsent);
        for key in ["status", "isRemote", "updatedAt", "launchAttemptId", "generation", "stopConfirmedAt"] {
            assert_eq!(filter.get(key), original.get(key));
        }
        for key in ["startedAt", "stopConfirmationBasis", "stopConfirmedTaskId"] {
            assert_eq!(filter.get_document(key).unwrap(), &doc! {"$exists":false});
        }
        assert_eq!(set.get_document("$set").unwrap().get_datetime("updatedAt").unwrap(), &at);
        // Legacy records without basis still deserialize instead of inventing it.
        let task: Task = bson::from_document(doc! {"_id":id, "name":"legacy", "status":"paused"}).unwrap();
        assert!(serde_json::to_value(task).unwrap()["stopConfirmationBasis"].is_null());
    }
}

#[cfg(test)]
mod business_diagnostics_tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    const NOW: i64 = 1_790_000_000_000;
    fn fixture() -> (ObjectId, Document, Document) {
        let id = ObjectId::parse_str("0123456789abcdef01234567").unwrap();
        let task = doc! {"_id":id,"name":"fixture","isRemote":false,"taskType":"regular","status":"running","auth_profile":"user1", "launchAttemptId":"launch1"};
        let runtime = doc! {"_id":"user1:fixture","task":"fixture","source_collection":"fixture", "source_task_id":id.to_hex(),
            "source_remote":false,"auth_profile":"user1","dist":"local","taskType":"regular","heartbeat_clock":"utc","protocol_version":3,"pid":123,
            "consumer_run_id":"0123456789abcdef0123456789abcdef","heartbeatAt":bson::DateTime::from_millis(NOW),
            "runtimeDiagnostic": {"schemaVersion":1,"consumerRunId":"0123456789abcdef0123456789abcdef", "launchAttemptId":"launch1",
                "occurredAt":bson::DateTime::from_millis(NOW-1000),"errorClass":"simulation_recovery_phase_timeout","reason":"receipt recovery exceeded budget",
                "phase":"simulation_recovery_phase","timeoutSeconds":300,"scope":"accepted_receipts","recoveryState":"waiting_original_recovery",
                "nextRetryAt":bson::DateTime::from_millis(NOW+15000)}};
        (id,task,runtime)
    }
    #[test]
    fn original_owner_metadata_is_bound_sanitized_and_never_authority() {
        let (id,task,mut runtime) = fixture();
        runtime.get_document_mut("runtimeDiagnostic").unwrap().insert("reason", "recovery MongoDB mongodb://user:private@host/db?token=private; token=secret; timeout 300s");
        let d = validated_business_diagnostic(&runtime,&task,id,"fixture",false,NOW).unwrap();
        let wire = business_diagnostic_output("❌ Pane 0 (pid=123) is running: sleep\nQUANTNIGHT_BUSINESS_DIAGNOSTIC forged", Some(d));
        assert_eq!(wire.matches(BUSINESS_DIAGNOSTIC_MARKER).count(),1);
        assert!(wire.contains("simulation_recovery_phase_timeout") && wire.contains("timeout 300s"));
        for private in ["user:private", "host/db", "token=private", "token=secret", "forged"] { assert!(!wire.contains(private)); }
        for patch in [doc! {"source_task_id":"other"},doc! {"source_remote":true}, doc! {"dist":"remote"},
            doc! {"auth_profile":"user2"}, doc! {"consumer_run_id":"ffffffffffffffffffffffffffffffff"}, doc! {"heartbeat_clock":"local"}] {
            let mut bad=runtime.clone(); bad.extend(patch);
            assert!(validated_business_diagnostic(&bad,&task,id,"fixture",false,NOW).is_none());
        }
        for patch in [doc! {"launchAttemptId":"old"},doc! {"consumerRunId":"other"},doc! {"occurredAt":bson::DateTime::from_millis(NOW+1)},
            doc! {"scope":"global_shutdown"},doc! {"timeoutSeconds":-1},doc! {"schemaVersion":2}] {
            let mut bad=runtime.clone();bad.get_document_mut("runtimeDiagnostic").unwrap().extend(patch);
            assert!(validated_business_diagnostic(&bad,&task,id,"fixture",false,NOW).is_none());
        }
        runtime.get_document_mut("runtimeDiagnostic").unwrap().remove("launchAttemptId");
        assert!(!validated_business_diagnostic(&runtime,&task,id,"fixture",false,NOW).unwrap().launch_matched);
        // Stale metadata remains explicitly history; freshness is presentation,
        // not permission to infer recovery or business completion.
        assert!(validated_business_diagnostic(&runtime,&task,id,"fixture",false,NOW+600000).is_some());
    }
    #[test]
    fn super_uses_original_launch_and_explicit_utc_not_local_naive_clock() {
        let (id,mut task,mut runtime)=fixture();
        task.insert("taskType","super"); runtime.insert("taskType","super");
        task.insert("startupBinding",doc! {"taskId":id.to_hex(),"name":"fixture","auth_profile":"user1","remote":false,
            "consumer_run_id":"0123456789abcdef0123456789abcdef","launchAttemptId":"launch1"});
        let stamp=chrono::DateTime::from_timestamp_millis(NOW).unwrap().to_rfc3339();
        runtime.insert("heartbeatAt",stamp);
        assert!(validated_business_diagnostic(&runtime,&task,id,"fixture",false,NOW).is_some());
        for key in ["taskId","name","auth_profile","consumer_run_id","launchAttemptId"] {
            let mut bad=task.clone();bad.get_document_mut("startupBinding").unwrap().insert(key,"other");
            assert!(validated_business_diagnostic(&runtime,&bad,id,"fixture",false,NOW).is_none());
        }
        runtime.insert("heartbeatAt","2026-09-22T00:00:00");
        assert!(validated_business_diagnostic(&runtime,&task,id,"fixture",false,NOW).is_none());
    }
    #[test]
    fn history_envelope_cannot_supply_current_state_or_rewrite_identity() {
        let (id, task,_) = fixture();
        let record=serde_json::json!({"schemaVersion":1,"name":"fixture","isRemote":false,"sessionId":"$5",
            "sessionCreatedAt":NOW-300000,"occurredAt":NOW-1000,"errorClass":"simulation_recovery_phase_timeout",
            "reason":"timeout 300s; original receipts retained","phase":"simulation_recovery_phase","timeoutSeconds":300});
        let pane="❌ Pane 0 (pid=123) is running: sleep";
        let wire=format!("{pane}\n{RUNTIME_HISTORY_MARKER}{record}");
        let d=validated_history_diagnostic(&wire,&task,id,"fixture",false,NOW).unwrap();
        assert!(!d.launch_matched && d.next_retry_at.is_none());
        assert_eq!(runtime_process_output(&wire,"").unwrap(),pane);
        for change in [serde_json::json!({"name":"other"}),serde_json::json!({"isRemote":true}),
            serde_json::json!({"occurredAt":NOW-400000}),serde_json::json!({"occurredAt":NOW+1}),
            serde_json::json!({"confirmed":true}),serde_json::json!({"sessionId":"$3147|old"})] {
            let mut bad=record.clone();bad.as_object_mut().unwrap().extend(change.as_object().unwrap().clone());
            assert!(validated_history_diagnostic(&format!("{pane}\n{RUNTIME_HISTORY_MARKER}{bad}"),&task,id,"fixture",false,NOW).is_none());
        }
        for bad in [wire.replace("❌", "✅"),format!("{wire}\n{RUNTIME_HISTORY_MARKER}{record}"),wire.replace(pane,""),format!("{wire}\nPermission denied")] {
            assert!(validated_history_diagnostic(&bad,&task,id,"fixture",false,NOW).is_none());
        }
    }

    // Cross-repo original shell + native transport, with only filesystem/process
    // IO faked. No real tmux, SSH, DB, platform or service operations.
    #[tokio::test]
    #[ignore = "requires QUANTNIGHT_RUNTIME_SOURCE pointing to original shell"]
    async fn original_shell_fake_os_native_runner_history_route() {
        use std::os::unix::fs::PermissionsExt;
        let source=PathBuf::from(std::env::var("QUANTNIGHT_RUNTIME_SOURCE").expect("source path required"));
        let python=std::env::var("QUANTNIGHT_TEST_PYTHON").expect("existing Python interpreter required");
        let polluted="export PATH=\"$PWD/bin:/usr/bin:/bin\"; export TMUX=pi-polluted TMUX_PANE=%999 TMUX_TMPDIR=/pi-polluted; ";
        let fake=FakeBash::new(&(polluted.to_string()+"exec /bin/bash ./check_tmux_python.sh fixture"));
        let directory=Path::new(&fake.config.env.working_dir).canonicalize().unwrap();
        let root=directory.as_path();
        fs::copy(&source,root.join("check_tmux_python.sh")).unwrap();
        fs::copy(source.parent().unwrap().join("pause_task.sh"),root.join("pause_task.sh")).unwrap();
        fs::write(root.join("script_config.sh"),format!("SCRIPT_CONFIG_PYTHON='{python}'\nget_local_workdir() {{ pwd; }}\n")).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir(root.join("native")).unwrap();
        let native=root.join("native/tmux");
        // Execute the actual backend resolver; substitute only its host binary
        // candidates with our executable fixture, never a frontend resolver.
        let owner=fs::read_to_string(source.parent().unwrap().join("script_config.py")).unwrap();
        let support=owner.split("# BEGIN NATIVE TMUX SUPPORT\n").nth(1).unwrap()
            .split("# END NATIVE TMUX SUPPORT").next().unwrap();
        let mut lines: Vec<String>=support.lines().map(str::to_owned).collect();
        let candidates=lines.iter_mut().find(|line|line.starts_with("NATIVE_TMUX_CANDIDATES =")).unwrap();
        *candidates=format!("NATIVE_TMUX_CANDIDATES = ({},)",serde_json::to_string(&native).unwrap());
        let support=lines.join("\n");
        fs::write(root.join("script_config.py"),format!("{support}\n\ndef tmux_support_source():\n    return {}\n",serde_json::to_string(&support).unwrap())).unwrap();
        let os_fake = format!(r##"#!{python}
import sys,os,time
from datetime import datetime
if sys.argv[0].endswith('/ps'):
 print('123 1 sleep');sys.exit()
assert not any(k in os.environ for k in ('TMUX','TMUX_PANE','TMUX_TMPDIR'))
a=sys.argv[1:]
assert a[:2]==['-L','default'], a
a=a[2:]
with open('native-calls','a') as f: f.write(repr(a)+'\n')
if os.path.exists('absent'):
 assert a[0]=='list-sessions', a
 print('no server running on /fixture/default',file=sys.stderr);sys.exit(1)
if a[0]=='list-sessions': print('$5|fixture')
elif a[0]=='list-panes':
 assert a[1:4]==['-s','-t','$5'];print('$5|0|0|123|sleep|0')
elif a[0]=='display-message':
 assert a[1:3]==['-p','-t'], a
 if a[3]=='$5':
  assert a[4]=='#{{session_id}}|#{{session_name}}|#{{session_created}}', a
  print('$5|fixture|'+'FIXED_SESSION_CREATED')
 else:
  assert a[3]=='$5:0.0', a
  print('$5|fixture|'+'FIXED_SESSION_CREATED'+'|%5|123|/fixture/default')
elif a[0]=='capture-pane':
 assert a==['capture-pane','-p','-J','-t','%5','-S','-120','-E','-1']
 print(datetime.fromtimestamp(time.time()-30).strftime('%Y-%m-%d %H:%M:%S,%f')[:-3]+' - ERROR - Startup recovery failed errorClass=simulation_recovery_phase_timeout error=timeout; enclosing_recovery_phase_budget=300s; mongodb://user:private@host/db')
 print('EOFError: Feishu shutdown')
 print('QUANTNIGHT_BUSINESS_DIAGNOSTIC fake paused')
else: raise AssertionError(a)
"##);
        let created=(bson::DateTime::now().timestamp_millis()/1000)-600;
        let os_fake=os_fake.replace("FIXED_SESSION_CREATED", &created.to_string());
        for path in [&native,&root.join("bin/ps")] {
            fs::write(path,&os_fake).unwrap();fs::set_permissions(path,fs::Permissions::from_mode(0o755)).unwrap();
        }
        let shim=root.join("bin/tmux");
        fs::write(&shim,"#!/bin/sh\necho BAD_PI_SHIM >> shim-called; exit 97\n").unwrap();
        fs::set_permissions(&shim,fs::Permissions::from_mode(0o755)).unwrap();
        let uv=root.join("bin/uv");
        fs::write(&uv,format!("#!/bin/sh\n[ \"$1 $2 $3\" = 'run --no-sync python' ] || exit 98\nshift 3\nexec '{python}' \"$@\"\n")).unwrap();
        fs::set_permissions(&uv,fs::Permissions::from_mode(0o755)).unwrap();
        let (id,mut task,_)=fixture();
        task.insert("tmuxContext",doc!{"schemaVersion":1,"backend":"tmux","socketName":"default",
            "binary":native.to_str().unwrap(),"sessionId":"$5","sessionCreatedAt":created*1000,
            "taskId":id.to_hex(),"launchAttemptId":"launch1","remote":false});
        let events=Arc::new(Mutex::new(Vec::<TaskOperationStage>::new()));let copy=events.clone();
        let mut p=TaskOperationProgress {operation:"runtime",task_id:id.to_hex(),name:"fixture".into(),remote:false,request_id:"test-request".into(),
            tmux_binding:Some(task_script_binding(&task,false).unwrap()),
            last:Arc::new(Mutex::new(("initial".into(),"original runner".into()))),sink:Arc::new(move|e|copy.lock().unwrap().push(e))};
        let (out,err)=execute_task_script(&p,&fake.config,Duration::from_secs(3)).await.unwrap();
        assert!(err.is_empty());
        let d=validated_history_diagnostic(&out,&task,id,"fixture",false,bson::DateTime::now().timestamp_millis()).unwrap();
        let wire=business_diagnostic_output(&runtime_process_output(&out,&err).unwrap(),Some(d));
        assert!(wire.contains("simulation_recovery_phase_timeout") && wire.contains("300"));
        assert!(!wire.contains("private") && !wire.contains("EOFError") && !wire.contains("fake paused"));
        assert!(events.lock().unwrap().iter().all(|e|e.task_id==id.to_hex() && e.request_id=="test-request"));
        // Same persisted binding, real pause bootstrap and original V2 proof.
        fs::write(root.join("absent"),"").unwrap();
        fs::write(root.join("fake.sh"),polluted.to_string()+"exec /bin/bash ./pause_task.sh fixture false").unwrap();
        p.operation="stop";
        let (out,err)=execute_task_script(&p,&fake.config,Duration::from_secs(5)).await.unwrap();
        let basis=validate_stop_output(&p,&out,&err).unwrap();
        assert!(matches!(basis,StopConfirmationBasis::SessionAbsent));
        let (filter,_)=pause_control_write(id,"fixture",&task,false,bson::DateTime::now(),basis);
        assert_eq!(filter.get_document("tmuxContext").unwrap(),task.get_document("tmuxContext").unwrap());
        assert!(!root.join("shim-called").exists(),"Pi PATH shim must never be invoked");
        assert!(!fs::read_to_string(root.join("native-calls")).unwrap().contains("kill-session"));
    }

}

#[cfg(all(test, unix))]
mod taskctl_start_tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    const ID: &str = "0123456789abcdef01234567";
    const CONFIG: &str = r#"{"max_concurrent":3,"rounds":{"enabled":true},"marker":"space ' quote"}"#;

    fn fixture(body: &str) -> FakeBash {
        let mut f = FakeBash::new(":");
        f.config.env.working_dir = Path::new(&f.config.env.working_dir).canonicalize().unwrap().to_string_lossy().into_owned();
        let root = Path::new(&f.config.env.working_dir);
        fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let path = root.join(".venv/bin/python");
        fs::write(&path, format!(r#"#!/bin/sh
[ "$PYTHONPATH" = "$PWD" ] || exit 94
[ "$1" = '.agents/skills/manage-quantnight-frontend/scripts/taskctl.py' ] || exit 95
printf '%s\n' "$@" >> calls
{body}
"#)).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("control.json"), r#"{"status":"paused"}"#).unwrap();
        f
    }
    fn success(remote: bool) -> JsonValue {
        json!({"schemaVersion":1,"command":"start","task":"fixture","ok":true,"taskId":ID,"remote":remote})
    }
    fn output(value: &str) -> String {
        format!("printf '%s\\n' '{}'", value.replace('\'', "'\\''"))
    }
    async fn start(f: &FakeBash, remote: bool, budget: Duration) -> Result<(), String> {
        start_via_taskctl("fixture", CONFIG, remote, ID, &f.config, budget).await
    }
    fn assert_one_start_no_control_write(f: &FakeBash, remote: bool) {
        let root = Path::new(&f.config.env.working_dir);
        assert_eq!(fs::read_to_string(root.join("calls")).unwrap(), format!(
            ".agents/skills/manage-quantnight-frontend/scripts/taskctl.py\nstart\n--name\nfixture\n--config\n{CONFIG}\n{}\n--expected-task-id\n{ID}\n--no-notify\n",
            if remote {"--remote"} else {"--no-remote"}));
        assert_eq!(fs::read_to_string(root.join("control.json")).unwrap(), r#"{"status":"paused"}"#);
    }
    #[tokio::test]
    async fn real_local_cli_transport_preserves_full_config_id_host_and_disables_notification() {
        for remote in [false, true] {
            let f = fixture(&output(&success(remote).to_string()));
            start(&f, remote, Duration::from_secs(10)).await.unwrap();
            assert_one_start_no_control_write(&f, remote);
        }
    }
    #[tokio::test]
    async fn false_wrong_identity_schema_and_invalid_json_never_retry_or_write_running() {
        let mut receipts = vec!["not-json".into(), "{}".into()];
        for change in [json!({"ok":false,"error":"execution_context_unverified: specific-host-error"}),
            json!({"taskId":"replacement-task"}), json!({"remote":true}), json!({"remote":"false"}),
            json!({"task":"other"}), json!({"command":"status"}), json!({"schemaVersion":2}),
            json!({"ok":"true"})] {
            let mut receipt = success(false);
            receipt.as_object_mut().unwrap().extend(change.as_object().unwrap().clone());
            receipts.push(receipt.to_string());
        }
        for receipt in receipts {
            let f = fixture(&(output(&receipt) + "\necho precise-stderr >&2"));
            let error = start(&f, false, Duration::from_secs(10)).await.unwrap_err();
            assert!(error.contains(&receipt) && error.contains("precise-stderr"), "{error}");
            assert_one_start_no_control_write(&f, false);
        }
    }
    #[tokio::test]
    async fn nonzero_and_timeout_preserve_partial_diagnostics_and_do_not_retry() {
        for (tail, expected, budget) in [
            ("exit 17", "17", Duration::from_secs(10)),
            ("exec /bin/sleep 30", "2000 ms", Duration::from_secs(2)),
        ] {
            let f = fixture(&(output(&success(false).to_string()) + "\necho host-binding-failure >&2\n" + tail));
            let error = start(&f, false, budget).await.unwrap_err();
            assert!(error.contains(expected) && error.contains("host-binding-failure") && error.contains("stdout"), "{error}");
            assert_one_start_no_control_write(&f, false);
        }
    }
    #[tokio::test]
    async fn malformed_id_config_and_missing_python_fail_without_launch() {
        let f = fixture("exit 99");
        for (id, config) in [("", CONFIG), (ID, "bad-json"), (ID, "[]")] {
            assert!(start_via_taskctl("fixture", config, false, id, &f.config, Duration::from_secs(1)).await.is_err());
        }
        let root = Path::new(&f.config.env.working_dir);
        assert!(!root.join("calls").exists());
        fs::remove_file(root.join(".venv/bin/python")).unwrap();
        let error = start(&f, false, Duration::from_secs(1)).await.unwrap_err();
        assert!(error.contains("启动命令失败"), "{error}");
        assert!(!root.join("calls").exists());
    }
}
