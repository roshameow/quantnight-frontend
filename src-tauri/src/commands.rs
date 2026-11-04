use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mongodb::bson::{doc, oid::ObjectId, Document};
use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, Emitter, State};
use tokio::time::timeout;
use toml;

use crate::config::AppConfig;
use crate::mongo_manager::MongoClients;
pub use quantnight_task_core::create_generate::GuiCreatedTask as CreatedTask;
pub use quantnight_task_core::pause::ConfirmedTaskStop;
use quantnight_task_core::{
    cleanup_sync::TaskOutcome,
    create_generate::{gui_created_task, gui_generated, CreateRequest, GenerateRequest},
    pause::{PauseOptions, StopBudget},
    start::StartRequest,
};
use quantnight_task_core::{
    runtime::{CallResult, RuntimeEvent, RuntimeObserver, TaskEndpoint},
    task_diagnostics::{operation_task_identity, TaskOperationProgress, TaskOperationStage},
    task_request::{DeleteRequest, NamedRequest, PauseRequest, TaskRequest},
};

#[cfg(test)]
use quantnight_task_core::task_control::TaskControl;

/// A checked application endpoint; GUI state owns only its own subscription.
/// Runtime selection is fixed at bootstrap, never on a failed button call.
pub struct TaskControlState<T = TaskEndpoint> {
    endpoint: tokio::sync::OnceCell<Result<T, String>>,
    pub(crate) subscription:
        tokio::sync::Mutex<Option<quantnight_task_core::runtime::ProgressSubscription>>,
}
impl<T> TaskControlState<T> {
    // Same cached checked-bootstrap container used by production and original
    // regression fixtures. Generic fixtures retain the real TaskControl helper;
    // production stores only TaskEndpoint, never a second TaskControl or pool.
    async fn initialize(
        &self,
        init: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<&T, String> {
        self.endpoint
            .get_or_init(|| init)
            .await
            .as_ref()
            .map_err(Clone::clone)
    }
}
#[cfg(test)]
impl TaskControlState<TaskControl> {
    fn default() -> Self {
        Self {
            endpoint: tokio::sync::OnceCell::new(),
            subscription: tokio::sync::Mutex::new(None),
        }
    }
}
impl TaskControlState {
    pub fn from_endpoint(endpoint: TaskEndpoint) -> Self {
        Self {
            endpoint: tokio::sync::OnceCell::new_with(Some(Ok(endpoint))),
            subscription: tokio::sync::Mutex::new(None),
        }
    }
    pub(crate) fn endpoint(&self) -> Result<&TaskEndpoint, String> {
        crate::dev_identity::admit_gui()?;
        self.endpoint
            .get()
            .ok_or("GUI task endpoint was not initialized")?
            .as_ref()
            .map_err(Clone::clone)
    }
    async fn get(
        &self,
        _config: &AppConfig,
        _clients: &Arc<MongoClients>,
    ) -> Result<&TaskEndpoint, String> {
        self.endpoint()
    }
}

async fn execute_task(
    endpoint: &TaskEndpoint,
    request: TaskRequest,
    observer: Option<RuntimeObserver>,
) -> Result<TaskOutcome, String> {
    let result = endpoint
        .execute(request, observer)
        .await
        .map_err(|e| e.to_string())?;
    Ok(TaskOutcome {
        value: result.value,
        exit_code: result.exit_code,
    })
}

async fn task_by_id(endpoint: &TaskEndpoint, id: &str) -> Result<Document, String> {
    let result = endpoint
        .execute(TaskRequest::TaskById { id: id.into() }, None)
        .await
        .map_err(|e| e.to_string())?;
    if result.exit_code != 0 {
        return Err(format!("原任务读取未确认：{}", result.value));
    }
    quantnight_task_core::runtime::wire_document::decode(result.value)
}

fn core_receipt(result: &TaskOutcome, command: &str, name: &str) -> Result<(), String> {
    let value = &result.value;
    if result.exit_code != 0
        || value["schemaVersion"].as_u64() != Some(1)
        || value["command"].as_str() != Some(command)
        || value["task"].as_str() != Some(name)
        || value["ok"].as_bool() != Some(true)
    {
        return Err(format!(
            "共享 TaskControl {command} 未确认；未自动重试：{value}"
        ));
    }
    Ok(())
}

fn required_task_id(id: Option<&str>) -> Result<&str, String> {
    let id = id.ok_or("任务ID缺失；请刷新列表，禁止按同名接管任务")?;
    ObjectId::parse_str(id).map_err(|_| "任务ID无效；禁止按同名接管任务")?;
    Ok(id)
}

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

// Pure helpers: validate before launching a script or writing a task record.
fn mission_config_document(value: &JsonValue) -> Result<Document, String> {
    if !value.is_object() {
        return Err("配置必须是 JSON 对象".to_string());
    }
    bson::to_document(value).map_err(|e| format!("config 转 BSON 失败: {}", e))
}

fn priority_config_document(config: &str) -> Result<Document, String> {
    let value: JsonValue =
        serde_json::from_str(config).map_err(|e| format!("配置 JSON 无效：{e}"))?;
    mission_config_document(&value)
}

fn create_request(new_task: NewTask) -> Result<CreateRequest, String> {
    let mut request = CreateRequest::new(new_task.name, new_task.templatefile);
    request.template = new_task.template;
    request.auth_profile = new_task.auth_profile.unwrap_or_else(|| "user1".into());
    request.task_type = new_task.task_type.unwrap_or_else(|| "regular".into());
    request.target_remote = new_task.is_remote.unwrap_or(false);
    request.mission_config = new_task
        .mission_config
        .as_ref()
        .map(mission_config_document)
        .transpose()?;
    // Legacy status is accepted in the DTO, never used as control-write authority.
    let _ = new_task.status;
    Ok(request)
}

#[command]
pub async fn create_task(
    new_task: NewTask,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<CreatedTask, String> {
    let request = create_request(new_task)?;
    let result = execute_task(
        control.get(&config, &clients).await?,
        TaskRequest::Create(request.clone()),
        None,
    )
    .await?;
    core_receipt(&result, "create", &request.name)?;
    gui_created_task(&result)
}

#[command]
pub async fn generate_list(
    id: String,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<(), String> {
    required_task_id(Some(&id))?;
    let core = control.get(&config, &clients).await?;
    let task = task_by_id(core, &id).await?;
    let mut request = GenerateRequest::new(task.get_str("name").map_err(|e| e.to_string())?);
    request.expected_task_id = Some(id);
    request.timeout_seconds = 900; // Original generator budget, not a candidate cap.
    let result = execute_task(core, TaskRequest::Generate(request.clone()), None).await?;
    core_receipt(&result, "generate", &request.name)?;
    gui_generated(&result)
}

#[derive(Deserialize)]
pub struct Updates {
    is_remote: bool,
}

#[tauri::command]
pub async fn update_remote_status(id: String, updates: Updates) -> Result<(), String> {
    required_task_id(Some(&id))?;
    let target = if updates.is_remote {
        "远程"
    } else {
        "本地"
    };
    Err(format!("禁止通过开关改写实际执行端为{target}；新任务请在创建时选择目标，初次远程执行须使用安全同步。已有任务不会自动迁移"))
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
pub async fn sync_remote_task(
    alpha_mission_list: String,
    task_id: Option<String>,
    expected_remote: Option<bool>,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<(), String> {
    let id = required_task_id(task_id.as_deref())?;
    let core = control.get(&config, &clients).await?;
    let task = task_by_id(core, id).await?;
    if task.get_str("name").ok() != Some(&alpha_mission_list) {
        return Err("原任务名称已变化".into());
    }
    let remote = expected_remote.unwrap_or(task.get_bool("isRemote").unwrap_or(false));
    let result = execute_task(
        core,
        TaskRequest::Sync(NamedRequest {
            name: alpha_mission_list.clone(),
            expected_task_id: Some(id.into()),
            expected_remote: Some(remote),
        }),
        None,
    )
    .await?;
    core_receipt(&result, "sync", &alpha_mission_list)
}

pub use quantnight_task_core::gui_task_queries::Task;

#[command]
pub async fn get_all_tasks(
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<Vec<Task>, String> {
    let endpoint = control.get(&config, &clients).await?;
    let result = endpoint
        .execute(TaskRequest::GuiTasks, None)
        .await
        .map_err(|e| e.to_string())?;
    if result.exit_code != 0 {
        return Err(format!("任务列表读取未确认：{}", result.value));
    }
    quantnight_task_core::gui_task_queries::decode_list(result.value)
}

#[command]
pub async fn delete_task(
    id: String,
    expected_remote: Option<bool>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<JsonValue, String> {
    required_task_id(Some(&id))?;
    let core = control.get(&app_config, &clients).await?;
    // No remote-only fallback and no name adoption. The common owner archives.
    let task = task_by_id(core, &id).await?;
    let name = task.get_str("name").map_err(|e| e.to_string())?;
    let remote = expected_remote.unwrap_or(task.get_bool("isRemote").unwrap_or(false));
    let result = execute_task(
        core,
        TaskRequest::Delete(DeleteRequest {
            name: name.into(),
            expected_task_id: Some(id.clone()),
            expected_remote: Some(remote),
            confirm: None,
        }),
        None,
    )
    .await?;
    core_receipt(&result, "delete", name)?;
    let receipt = &result.value;
    if receipt["controlHidden"] != true
        || receipt["localDeleted"] != true
        || (remote && receipt["remoteDeleted"] != true)
        || receipt["itemsPreserved"] != true
        || receipt["canonicalResultsDeleted"] != false
    {
        return Err(format!("安全归档回执未完成；保留原任务核验：{receipt}"));
    }
    if receipt["taskId"].as_str() != Some(id.as_str())
        || receipt["remote"].as_bool() != Some(remote)
        || receipt["unresolvedItemCount"].as_u64().is_none()
        || receipt["receiptAssessmentUnknownCount"].as_u64().is_none()
        || receipt["receiptAssessmentUnknownCount"].as_u64()
            > receipt["unresolvedItemCount"].as_u64()
        || receipt["platformRequestsCancelled"] != false
        || receipt["capacityReleased"] != false
        || !receipt["receiptPath"]
            .as_str()
            .is_some_and(|path| !path.is_empty())
        || !["localArchive", "remoteArchive"].iter().all(|key| {
            receipt.get(*key).is_some_and(|value| {
                value.is_null() || value.as_str() == Some(format!("{name}__archive_{id}").as_str())
            })
        })
    {
        return Err(format!("安全归档回执未完成；保留原任务核验：{receipt}"));
    }
    // Return only the validated compact contract, never task config/attempts.
    Ok(serde_json::json!({
        "schemaVersion": 1, "command": "delete", "task": name, "ok": true,
        "taskId": id, "remote": remote,
        "controlHidden": true, "localDeleted": true, "remoteDeleted": receipt["remoteDeleted"],
        "itemsPreserved": true, "canonicalResultsDeleted": false,
        "receiptPath": receipt["receiptPath"], "localArchive": receipt["localArchive"],
        "remoteArchive": receipt["remoteArchive"], "unresolvedItemCount": receipt["unresolvedItemCount"],
        "receiptAssessmentUnknownCount": receipt["receiptAssessmentUnknownCount"],
        "platformRequestsCancelled": false, "capacityReleased": false
    }))
}

fn start_request(
    task_name: &str,
    config: Option<&str>,
    remote: bool,
    task_id: &str,
    expected_remote: Option<bool>,
) -> Result<StartRequest, String> {
    required_task_id(Some(task_id))?;
    if let Some(config) = config {
        priority_config_document(config)?;
    }
    let mut request = StartRequest::new(task_name);
    // None is the original core's saved-config selection, never {} or JSON null.
    request.config_json = config.map(str::to_owned);
    request.remote = Some(remote);
    request.expected_remote = Some(expected_remote.unwrap_or(remote));
    request.expected_task_id = Some(task_id.into());
    request.notification.no_notify = true;
    Ok(request)
}

fn start_receipt(result: &TaskOutcome, request: &StartRequest) -> Result<(), String> {
    core_receipt(result, "start", &request.name)?;
    if result.value["taskId"].as_str() != request.expected_task_id.as_deref()
        || result.value["remote"].as_bool() != request.remote
    {
        return Err(format!(
            "共享 TaskControl start 回执身份/执行端不匹配；未重试：{}",
            result.value
        ));
    }
    Ok(())
}

// A failed business outcome remains a rejected command for old void callers.
// Encode the ORIGINAL versioned object as complete JSON, not text fragments.
fn gui_start_outcome(result: TaskOutcome, request: &StartRequest) -> Result<JsonValue, String> {
    start_receipt(&result, request).map_err(|error| {
        if result.value["schemaVersion"] == 1
            && result.value["command"] == "start"
            && result.value["task"].as_str() == Some(request.name.as_str())
            && result.value["ok"] == false
        {
            result.value.to_string()
        } else {
            error
        }
    })?;
    Ok(result.value)
}

async fn start_via_core(core: &TaskEndpoint, request: &StartRequest) -> Result<JsonValue, String> {
    // Transport intent is not host migration authorization. Shared core repeats
    // authoritative identity/CAS checks under its original mission lock.
    let task = task_by_id(core, required_task_id(request.expected_task_id.as_deref())?).await?;
    operation_task_identity(
        &task,
        &request.name,
        request.expected_remote.ok_or("原始执行端缺失")?,
    )?;
    let result = timeout(
        Duration::from_secs(300),
        execute_task(core, TaskRequest::Start(request.clone()), None),
    )
    .await
    .map_err(|_| "启动总预算300秒耗尽；结果待核验，未自动重试或写入running")??;
    gui_start_outcome(result, request)
}

#[command]
pub async fn start_task(
    task_name: String,
    config: Option<String>,
    is_remote: bool,
    task_id: String,
    expected_remote: Option<bool>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<JsonValue, String> {
    let request = start_request(
        &task_name,
        config.as_deref(),
        is_remote,
        &task_id,
        expected_remote,
    )?;
    start_via_core(control.get(&app_config, &clients).await?, &request).await
}

#[command]
pub async fn start_super_task(
    task_name: String,
    config: Option<String>,
    is_remote: bool,
    task_id: String,
    expected_remote: Option<bool>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<JsonValue, String> {
    let request = start_request(
        &task_name,
        config.as_deref(),
        is_remote,
        &task_id,
        expected_remote,
    )?;
    start_via_core(control.get(&app_config, &clients).await?, &request).await
}

#[command]
pub async fn start_priority_task(
    task_name: String,
    config: Option<String>,
    is_remote: bool,
    task_id: String,
    expected_remote: Option<bool>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<JsonValue, String> {
    let request = start_request(
        &task_name,
        config.as_deref(),
        is_remote,
        &task_id,
        expected_remote,
    )?;
    start_via_core(control.get(&app_config, &clients).await?, &request).await
}

fn gui_operation_progress(
    app: AppHandle,
    operation: &'static str,
    task_id: Option<&str>,
    name: &str,
    remote: bool,
    request_id: Option<String>,
) -> TaskOperationProgress {
    TaskOperationProgress::new(
        Arc::new(move |event| {
            if let Err(error) = app.emit("task-operation-stage", event) {
                eprintln!("task-operation-stage emit failed: {error}");
            }
        }),
        operation,
        task_id,
        name,
        remote,
        request_id,
    )
}

pub(crate) const PROGRESS_GAP_NOTICE: &str = "过程更新滞后/合并，最终结果仍由原回执确认";

fn operation_observer(progress: TaskOperationProgress) -> RuntimeObserver {
    let bash = progress.observer();
    Arc::new(move |event| match event {
        RuntimeEvent::Bash(event) => bash(event),
        RuntimeEvent::PauseStage(event) => {
            if event.stage != "failed" {
                progress.stage(&event.stage, &event.message);
            }
        }
        RuntimeEvent::GuiProgress { name, payload } if name == "task-operation-stage" => {
            // Native diagnostic stages come from the shared helper, not Bash.
            // Keep the GUI's original request token; do not expose transport ids.
            if payload["name"].as_str() != Some(progress.name.as_str())
                || payload["isRemote"].as_bool() != Some(progress.remote)
                || payload["operation"].as_str() != Some(progress.operation)
            {
                return;
            }
            let mut scoped = progress.clone();
            if scoped.task_id.is_empty() {
                if let Some(id) = payload["taskId"]
                    .as_str()
                    .filter(|id| ObjectId::parse_str(id).is_ok())
                {
                    scoped.task_id = id.into();
                }
            } else if payload["taskId"].as_str() != Some(scoped.task_id.as_str()) {
                return;
            }
            if let (Some(stage), Some(message)) =
                (payload["stage"].as_str(), payload["message"].as_str())
            {
                scoped.stage(stage, message);
            }
        }
        RuntimeEvent::ProgressGap { .. } => progress.stage("wait", PROGRESS_GAP_NOTICE),
        RuntimeEvent::GuiProgress { .. } => {}
    })
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
    control: State<'_, TaskControlState>,
) -> Result<String, String> {
    let progress = gui_operation_progress(
        app,
        "runtime",
        task_id.as_deref(),
        &task_name,
        is_remote,
        request_id,
    );
    let endpoint = control
        .get(&app_config, &clients)
        .await
        .map_err(|e| progress.failure(e))?;
    let result = endpoint
        .execute(
            TaskRequest::TaskDiagnostics {
                name: task_name,
                task_id,
                remote: is_remote,
            },
            Some(operation_observer(progress.clone())),
        )
        .await
        .map_err(|e| progress.failure(e))?;
    if result.exit_code != 0 {
        return Err(progress.failure(result.value));
    }
    result
        .value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| progress.failure("运行检查未返回原诊断字符串"))
}

#[command]
pub async fn update_priority_task(
    task_name: String,
    config: String,
    is_remote: bool,
    task_id: Option<String>,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<(), String> {
    let id = required_task_id(task_id.as_deref())?;
    priority_config_document(&config)?;
    let request = quantnight_task_core::priority_update::UpdatePriorityRequest {
        name: task_name.clone(),
        config_json: config,
        remote: Some(is_remote),
        expected_task_id: Some(id.into()),
    };
    let core = control.get(&app_config, &clients).await?;
    let result = timeout(
        Duration::from_secs(300),
        execute_task(core, TaskRequest::PriorityUpdate(request.clone()), None),
    )
    .await
    .map_err(|_| "Priority配置发布总预算300秒耗尽；结果未确认，未重试或写running")??;
    core_receipt(&result, "priority-update", &task_name)?;
    if result.value["taskId"].as_str() != Some(id)
        || result.value["configPublished"] != true
        || result.value["adoptionVerified"] != false
        || result.value["remote"].as_bool() != Some(is_remote)
    {
        return Err(format!(
            "Priority配置发布回执身份/确认字段无效：{}",
            result.value
        ));
    }
    Ok(()) // Configuration publication is NOT running/adoption evidence.
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

// Optional frozen join fields. Omitted launch check preserves old direct callers.
fn gui_pause_options<'a>(
    id: &'a str,
    remote: bool,
    exact: Option<&'a str>,
    absent: Option<bool>,
    remaining_ms: Option<u64>,
    elapsed: Duration,
) -> Result<PauseOptions<'a>, String> {
    required_task_id(Some(id))?;
    if exact.is_some_and(|value| value.trim().is_empty()) {
        return Err("expectedLaunchAttemptId must be nonempty".into());
    }
    let expected_launch_absent = absent.unwrap_or(false);
    if expected_launch_absent && exact.is_some() {
        return Err("exact launch and expectedLaunchAbsent are mutually exclusive".into());
    }
    let remaining = match remaining_ms {
        Some(ms) if ms > 120_000 => return Err("remainingMs exceeds original pause UI120s".into()),
        Some(ms) => {
            let left = Duration::from_millis(ms).saturating_sub(elapsed);
            if left.is_zero() {
                return Err("pause caller deadline expired; stop not sent".into());
            }
            Some(left)
        }
        None => None,
    };
    Ok(PauseOptions {
        expected_task_id: Some(id),
        expected_remote: Some(remote),
        expected_launch_attempt_id: exact,
        expected_launch_absent,
        remaining,
        ..Default::default()
    })
}

fn gui_pause_outcome(result: CallResult, id: &str, name: &str, remote: bool) -> Result<ConfirmedTaskStop, String> {
    core_receipt(&TaskOutcome { value: result.value, exit_code: result.exit_code }, "pause", name)?;
    let stop = result.confirmed_stop.ok_or("共享核心未返回已确认停止回执")?;
    if !stop.confirmed || stop.task_id != id || stop.task_name != name || stop.is_remote != remote
        || stop.stop_confirmed_at <= 0 {
        return Err("共享核心暂停回执任务ID、名称、执行端或停止确认时间无效；未重试".into());
    }
    // Return the original typed proof; no client timestamp/second read can
    // substitute for the shared pause owner's actual confirmation.
    Ok(stop)
}

#[command]
pub async fn pause_task(
    task_name: String,
    is_remote: bool,
    task_id: Option<String>,
    request_id: Option<String>,
    expected_launch_attempt_id: Option<String>,
    expected_launch_absent: Option<bool>,
    remaining_ms: Option<u64>,
    app: AppHandle,
    clients: State<'_, Arc<MongoClients>>,
    app_config: State<'_, AppConfig>,
    control: State<'_, TaskControlState>,
) -> Result<ConfirmedTaskStop, String> {
    let started = Instant::now();
    let id = required_task_id(task_id.as_deref())?;
    let admission = gui_pause_options(
        id,
        is_remote,
        expected_launch_attempt_id.as_deref(),
        expected_launch_absent,
        remaining_ms,
        Duration::ZERO,
    )?;
    let progress = gui_operation_progress(app, "stop", Some(id), &task_name, is_remote, request_id);
    progress.stage(
        "initial",
        "暂停请求已接收；尚未获得原任务锁/尚未执行停止；原停止路径上限75秒",
    );
    let core = if let Some(left) = admission.remaining {
        // Original binding owner uses an 8s cap; keep it inside caller remaining.
        timeout(
            left.min(Duration::from_secs(8)),
            control.get(&app_config, &clients),
        )
        .await
        .map_err(|_| progress.failure("控端绑定获取超时；停止未发送"))?
        .map_err(|error| progress.failure(error))?
    } else {
        control
            .get(&app_config, &clients)
            .await
            .map_err(|error| progress.failure(error))?
    };
    let options = gui_pause_options(
        id,
        is_remote,
        expected_launch_attempt_id.as_deref(),
        expected_launch_absent,
        remaining_ms,
        started.elapsed(),
    )
    .map_err(|error| progress.failure(error))?;
    let left = options.remaining;
    let expected_launch_absent = options.expected_launch_absent;
    let request = TaskRequest::Pause(PauseRequest {
        name: task_name.clone(),
        expected_task_id: Some(id.into()),
        expected_remote: Some(is_remote),
        expected_launch_attempt_id,
        expected_launch_absent,
        remaining_ms: left.map(|duration| duration.as_millis() as u64),
        budget: StopBudget::Gui,
    });
    let operation = core.execute(request, Some(operation_observer(progress.clone())));
    let result = if let Some(left) = left {
        timeout(left, operation)
            .await
            .map_err(|_| progress.failure("本次暂停剩余预算耗尽；最终停止回执待核验，未重发"))?
    } else {
        operation.await
    }
    .map_err(|error| progress.failure(error))?;
    gui_pause_outcome(result, id, &task_name, is_remote).map_err(|error| progress.failure(error))
}

#[command]
pub fn restart_app() -> Result<(), String> {
    // Use std::process to restart the application
    std::process::exit(0); // This will cause the application to exit, and the process manager can restart it
}



#[cfg(test)]
use quantnight_task_core::pause::{verified_stop_proof, StopConfirmationBasis};
#[cfg(test)]
#[path = "task_control_tests.rs"]
mod task_control_tests;
#[cfg(test)]
use task_control_tests::{pause_route, proof_line};
#[cfg(test)]
mod verified_pause_tests {
    use super::*;
    use serde_json::json;

    fn proof(extra: JsonValue) -> String {
        let mut value = json!({ "schema_version": 1, "task_name": "pipeline__priority",
            "target_remote": true, "session_absent": true, "runners_exited": true });
        if let Some(overrides) = extra.as_object() {
            for (key, replacement) in overrides {
                value[key] = replacement.clone();
            }
        }
        format!("stop log\nQUANTNIGHT_PAUSE_CONFIRMED {value}\n")
    }

    #[test]
    fn only_exact_verified_execution_receipt_confirms_stop() {
        let valid = proof(json!({}));
        assert!(verified_stop_proof(&valid, "pipeline__priority", true).is_ok());
        assert!(verified_stop_proof("✅ 已关停", "pipeline__priority", true).is_err());
        assert!(verified_stop_proof("", "pipeline__priority", true).is_err());
        assert!(
            verified_stop_proof(&(valid.clone() + &valid), "pipeline__priority", true).is_err()
        );
        assert!(verified_stop_proof(
            &(valid.clone() + "more output\n"),
            "pipeline__priority",
            true
        )
        .is_err());
        assert!(verified_stop_proof(
            &proof(json!({"unexpected": true})),
            "pipeline__priority",
            true
        )
        .is_err());
        for changes in [
            json!({"session_absent": false}),
            json!({"runners_exited": false}),
            json!({"schema_version": 2}),
            json!({"task_name": "other"}),
            json!({"target_remote": false}),
            json!({"runners_exited": "true"}),
        ] {
            assert!(verified_stop_proof(&proof(changes), "pipeline__priority", true).is_err());
        }
        assert!(verified_stop_proof(
            "QUANTNIGHT_PAUSE_CONFIRMED {broken}",
            "pipeline__priority",
            true
        )
        .is_err());
    }

    #[test]
    fn pause_copy_reports_receipt_gap_and_field_values() {
        let valid = proof(json!({}));
        for (raw, expected) in [
            (String::new(), "缺少结构化暂停回执"),
            (
                valid.clone() + &valid,
                "收到2条结构化暂停回执，要求唯一回执",
            ),
            (
                valid.clone() + "more output\n",
                "结构化暂停回执后仍有输出，最终行应为暂停回执",
            ),
            (
                format!("QUANTNIGHT_PAUSE_CONFIRMED {}", "x".repeat(8193)),
                "最终回执超出8192字节上限",
            ),
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
            let error =
                verified_stop_proof(&proof(changes), "pipeline__priority", true).unwrap_err();
            assert!(
                error.starts_with("暂停回执校验失败：任务身份、执行主机或停止依据存在字段差异"),
                "{error}"
            );
            assert!(
                error.contains("期望 task_name=\"pipeline__priority\", target_remote=true"),
                "{error}"
            );
            assert!(error.contains(actual), "{error}");
            assert!(!error.contains('不'));
        }
        let error = verified_stop_proof(
            "QUANTNIGHT_PAUSE_CONFIRMED {broken}",
            "pipeline__priority",
            true,
        )
        .unwrap_err();
        assert!(error.starts_with("暂停回执解析失败："));
    }

    #[tokio::test]
    async fn task_list_preserves_persisted_stop_evidence_for_remounts() {
        let id = ObjectId::new();
        let original =
            doc! {"_id":id,"name":"pipeline__priority","status":"running","isRemote":true};
        let route = pause_route(
            original.clone(),
            proof_line(
                "pipeline__priority",
                true,
                StopConfirmationBasis::RunnersExited,
            ),
            1,
        )
        .await;
        assert_eq!(route.outcome.exit_code, 0);
        let mut record = original;
        record.extend(route.writes[0].1.get_document("$set").unwrap().clone());
        let task: Task = bson::from_document(record).unwrap();
        let wire = serde_json::to_value(task).unwrap();
        assert_eq!(wire["stopConfirmedTaskId"], id.to_hex());
        assert_eq!(wire["stopConfirmedRemote"], true);
        assert!(!wire["stopConfirmedAt"].is_null());
    }

    #[tokio::test]
    async fn persisted_and_returned_stop_identity_are_fenced_and_explicit() {
        let id = ObjectId::new();
        let original =
            doc! {"_id":id,"name":"pipeline__priority","status":"running","isRemote":true};
        let route = pause_route(
            original,
            proof_line(
                "pipeline__priority",
                true,
                StopConfirmationBasis::RunnersExited,
            ),
            1,
        )
        .await;
        let update = route.writes[0].1.get_document("$set").unwrap();
        assert_eq!(update.get_str("status").unwrap(), "paused");
        assert_eq!(update.get_str("stopConfirmedTaskId").unwrap(), id.to_hex());
        assert!(update.get_bool("stopConfirmedRemote").unwrap());
        let at = update.get_datetime("stopConfirmedAt").unwrap();
        let response = serde_json::to_value(route.outcome.confirmed_stop.unwrap()).unwrap();
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
        }))
        .unwrap();
        assert!(legacy.mission_config.is_none());
    }

    #[test]
    fn update_persists_exact_nested_object_and_explicit_empty_steps() {
        let value = json!({"refine_templates":[], "max_variants_per_alpha":40,
            "extension":{"preserve":true}});
        let update = priority_config_document(&value.to_string()).unwrap();
        assert!(!update.contains_key("status"));
        let saved = &update;
        assert!(saved.get_array("refine_templates").unwrap().is_empty());
        assert_eq!(serde_json::to_value(saved).unwrap(), value);
        let legacy = priority_config_document("{\"interval\":600}").unwrap();
        assert!(!legacy.contains_key("refine_templates"));
    }

    #[test]
    fn invalid_top_level_config_is_rejected_before_script_execution() {
        for value in ["[]", "null", "false", "3", "broken"] {
            assert!(priority_config_document(value).is_err());
        }
        assert!(mission_config_document(&json!([])).is_err());
    }

    #[test]
    fn requested_cp_host_is_preserved_independently_of_root_origin_and_round_mode() {
        for enabled in [true, false] {
            for remote in [true, false] {
                let value = json!({"rounds":{"enabled":enabled}, "source_tasks":["remote-source"],
                    "extra":{"source_remote":!remote}});
                let update = priority_config_document(&value.to_string()).unwrap();
                assert!(
                    !update.contains_key("isRemote")
                        && !update.contains_key("targetRemote")
                        && !update.contains_key("status")
                );
                assert_eq!(serde_json::to_value(update).unwrap(), value);
            }
        }
    }

    #[test]
    fn task_read_model_exposes_all_saved_config_sources() {
        for key in ["mission_config", "startConfig", "config"] {
            let mut document =
                doc! {"_id": ObjectId::new(), "name":"legacy__priority", "status":"pending"};
            document.insert(key, doc! {"refine_templates": []});
            let task: Task = bson::from_document(document).unwrap();
            let exposed = serde_json::to_value(task).unwrap();
            assert_eq!(exposed[key]["refine_templates"], json!([]));
        }
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
            let root =
                std::env::temp_dir().join(format!("quantnight-template-{}", ObjectId::new()));
            fs::create_dir_all(root.join(".venv/bin")).unwrap();
            let root = root.canonicalize().unwrap();
            let executable = root.join(".venv/bin/python");
            fs::write(
                &executable,
                r#"#!/bin/sh
printf '%s\n' "$@" >> calls
if [ "$1" = '-m' ]; then
  [ "$2" = 'templates.catalog' ] || exit 91
  [ "$4" = '--root' ] || exit 92
  [ "$5" = "$PWD" ] || exit 93
  [ "$6" = '--kind' ] || exit 94
  cat catalog.json
  exit "$(cat catalog.exit)"
fi
[ "$1" = '-B' ] && [ "$2" = '-m' ] || exit 95
if [ "$3" = 'backend.mongodb.mongo_config' ]; then
  printf '%s\n' '{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}'
  exit 0
fi
[ "$3" = 'util.dataset_mission' ] && [ "$4" = '--native-archive-statistics' ] || exit 96
printf '%s' "$5" > archive-request.json
cat archive.json
exit "$(cat archive.exit)"
"#,
            )
            .unwrap();
            fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(root.join("credentials.yaml"), "synthetic fixture only").unwrap();
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
        fn calls(&self) -> String {
            fs::read_to_string(self.0.join("calls")).unwrap_or_default()
        }
        fn config(&self) -> AppConfig {
            serde_json::from_value(json!({"env":{"working_dir":self.0},
                "python":{"scripts":{}}, "bash":{"scripts":{}},
                "mongodb":{"local_uri":"unused", "remote_uri":"unused",
                    "databases":{"mission":"unused", "simulation":"unused", "alpha":"unused"}}}))
            .unwrap()
        }
        fn task(&self) -> Document {
            doc! {"_id": ObjectId::new(), "name":"task", "templatefile":"same_name",
            "taskType":"super", "status":"paused", "isRemote":true}
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn latest_uses_catalog_kind_directory_and_bare_name() {
        let f = Fixture::new();
        f.catalog(json!({"ok":true,"entry":{"name":"published","taskType":"super","format":"descriptor"}}));
        let folder =
            f.0.join("templates/generated/super")
                .to_string_lossy()
                .into_owned();
        assert_eq!(
            latest_template_from_directory(&f.0, &folder, None)
                .await
                .unwrap(),
            "published"
        );
        assert!(f
            .calls()
            .contains(&format!("--kind\nsuper\n--directory\n{folder}\n")));
        f.catalog(json!({"ok":true,"entry":null}));
        assert!(latest_template_from_directory(&f.0, &folder, Some("super"))
            .await
            .is_err());
        f.catalog(json!({"ok":true,"entry":{"name":"wrong_kind","taskType":"regular"}}));
        assert!(latest_template_from_directory(&f.0, &folder, Some("super"))
            .await
            .is_err());
    }

    async fn original_archive(f: &Fixture, task: &Document) -> Result<JsonValue, String> {
        use quantnight_task_core::{
            backend_binding::validate_backend_binding, cleanup_sync::OperationsRuntime,
            operations_runtime::OriginalOperationsRuntime, task_store::Host,
        };
        let binding = validate_backend_binding(&f.config()).await?;
        let runtime = OriginalOperationsRuntime {
            binding: &binding,
            observer: None,
        };
        runtime
            .archive_research(
                binding.root(),
                task,
                Host::Remote,
                task.get_str("name").unwrap(),
            )
            .await
    }
    fn archive_receipt(result: JsonValue) -> JsonValue {
        json!({"schemaVersion":1,"command":"native-archive-statistics","ok":true,"result":result})
    }

    #[tokio::test]
    async fn dataset_binding_archives_original_task_and_exact_target_before_delete() {
        let f = Fixture::new();
        f.receipt(archive_receipt(json!({"archivePath":"/fake/archive.json"})));
        let task = f.task();
        assert_eq!(
            original_archive(&f, &task).await.unwrap()["archivePath"],
            "/fake/archive.json"
        );
        let request: JsonValue =
            serde_json::from_str(&fs::read_to_string(f.0.join("archive-request.json")).unwrap())
                .unwrap();
        assert_eq!(
            request["task"]["_id"]["$oid"],
            task.get_object_id("_id").unwrap().to_hex()
        );
        assert_eq!(request["task"]["name"], "task");
        assert_eq!(request["task"]["taskType"], "super");
        assert_eq!(request["task"]["templatefile"], "same_name");
        assert_eq!(request["remote"], true);
        assert_eq!(request["evidenceCollection"], "task");
        assert!(request["configurationBinding"].is_object());
        let calls = f.calls();
        assert!(
            calls.find("backend.mongodb.mongo_config").unwrap()
                < calls.find("util.dataset_mission").unwrap()
        );
        assert!(!calls.contains("taskctl.py"));
    }

    #[tokio::test]
    async fn archive_rejects_invalid_binding_collision_process_error_and_wrong_receipt() {
        // Dataset catalog/binding now belongs to original dataset_mission, not
        // GUI. Errors from that owner and bad native receipts remain fatal.
        for receipt in [
            json!({"ok":false,"error":"ambiguous template name collision"}),
            json!({"ok":true}),
            json!({"schemaVersion":1,"command":"native-archive-statistics","ok":false}),
            json!({"schemaVersion":1,"command":"archive-research","ok":true,"result":null}),
            json!({"schemaVersion":2,"command":"native-archive-statistics","ok":true,"result":null}),
            json!({"schemaVersion":1,"command":"native-archive-statistics","ok":"true","result":null}),
        ] {
            let f = Fixture::new();
            f.receipt(receipt);
            assert!(original_archive(&f, &f.task()).await.is_err());
        }
        let f = Fixture::new();
        fs::write(f.0.join("archive.json"), "broken-json").unwrap();
        assert!(original_archive(&f, &f.task()).await.is_err());
    }

    #[tokio::test]
    async fn missing_legacy_binding_refine_and_priority_do_not_archive_dataset() {
        // Absence/refine/priority no-op is interpreted by the original owner;
        // Rust forwards metadata and preserves its null result, not sidecars.
        for kind in ["regular", "super", "priority"] {
            let f = Fixture::new();
            f.receipt(archive_receipt(JsonValue::Null));
            let mut task = f.task();
            task.insert("taskType", kind);
            assert_eq!(original_archive(&f, &task).await.unwrap(), JsonValue::Null);
            let request: JsonValue = serde_json::from_str(
                &fs::read_to_string(f.0.join("archive-request.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(request["task"]["taskType"], kind);
            assert!(!f.calls().contains("templates.catalog"));
        }
    }

    #[tokio::test]
    async fn running_dataset_requires_pause_and_nonzero_archive_is_not_success() {
        task_control_tests::assert_running_delete_refused().await;
        let f = Fixture::new();
        f.receipt(archive_receipt(json!({"archivePath":"/fake"})));
        fs::write(f.0.join("archive.exit"), "1").unwrap();
        assert!(original_archive(&f, &f.task()).await.is_err());
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
            (
                "regular",
                "d95d5532c10b09a6348b70512ce53fe3a3228c28000d123c20970c882001b4b8",
            ),
            (
                "super",
                "f7a4d0ead7dff04008c915c6d41d69f9ee407a863e04fa4361ae77d638327375",
            ),
        ] {
            let manifest = format!("{{\"schemaVersion\":1,\"taskType\":\"{kind}\",\"provenance\":{{\"kind\":\"dataset\"}}}}");
            fs::write(f.0.join(format!("{kind}.json")), manifest).unwrap();
            let folder = f.0.join(format!("templates/generated/{kind}"));
            fs::write(
                folder.join("same_name.research.json"),
                json!({
                "taskType":kind,"loader":"frozen_research","manifest":format!("{kind}.json"),
                "sha256":sha,"kind":"dataset","extension":"preserved"})
                .to_string(),
            )
            .unwrap();
            // These newer modules/claims must never be executed or selected.
            for name in ["helper.py", "__init__.py", ".claim.py", "incomplete.tmp.py"] {
                fs::write(
                    folder.join(name),
                    "raise RuntimeError('must not execute')\n",
                )
                .unwrap();
            }
            assert_eq!(
                latest_template_from_directory(&f.0, folder.to_str().unwrap(), Some(kind))
                    .await
                    .unwrap(),
                "same_name"
            );
            let binding = run_template_catalog(&f.0, "binding", kind, Some("same_name"), None)
                .await
                .unwrap();
            assert_eq!(binding["binding"]["taskType"], kind);
            assert_eq!(binding["binding"]["extension"], "preserved");
        }
        let regular = f.0.join("templates/generated/regular");
        fs::write(
            f.0.join("templates/same_name.py"),
            "async def generate_parms(session): pass\n",
        )
        .unwrap();
        assert!(
            latest_template_from_directory(&f.0, regular.to_str().unwrap(), Some("regular"))
                .await
                .unwrap_err()
                .contains("collision")
        );
        fs::remove_file(f.0.join("templates/same_name.py")).unwrap();
        fs::write(f.0.join("regular.json"), "tampered").unwrap();
        assert!(
            run_template_catalog(&f.0, "binding", "regular", Some("same_name"), None)
                .await
                .is_err()
        );
        // A legacy Python entry is still manually resolvable when generated is empty.
        fs::remove_file(regular.join("same_name.research.json")).unwrap();
        fs::write(
            f.0.join("templates/legacy.py"),
            "raise RuntimeError('not executed')\nasync def generate_parms(session): pass\n",
        )
        .unwrap();
        assert!(
            latest_template_from_directory(&f.0, regular.to_str().unwrap(), Some("regular"))
                .await
                .is_err()
        );
        assert_eq!(
            latest_template_from_directory(
                &f.0,
                f.0.join("templates").to_str().unwrap(),
                Some("regular")
            )
            .await
            .unwrap(),
            "legacy"
        );
        assert!(
            run_template_catalog(&f.0, "binding", "regular", Some("legacy"), None)
                .await
                .unwrap()["binding"]
                .is_null()
        );
    }
}

#[cfg(all(test, unix))]
mod shared_start_tests {
    use super::*;
    use crate::config::{bash_test_support::FakeBash, run_process_observed};
    use serde_json::json;
    const ID: &str = "0123456789abcdef01234567";
    const CONFIG: &str =
        r#"{"max_concurrent":3,"rounds":{"enabled":true},"marker":"space ' quote"}"#;
    fn success(remote: bool) -> JsonValue {
        json!({"schemaVersion":1,"command":"start","task":"fixture","ok":true,"taskId":ID,"remote":remote})
    }
    #[test]
    fn real_shared_request_preserves_full_config_id_host_and_disables_notification() {
        for remote in [false, true] {
            let request = start_request("fixture", Some(CONFIG), remote, ID, None).unwrap();
            assert_eq!(request.config_json.as_deref(), Some(CONFIG));
            assert_eq!(request.expected_task_id.as_deref(), Some(ID));
            assert_eq!(request.remote, Some(remote));
            assert_eq!(request.expected_remote, Some(remote));
            assert!(request.notification.no_notify);
            assert!(request.notification.target.is_none());
            start_receipt(
                &TaskOutcome {
                    value: success(remote),
                    exit_code: 0,
                },
                &request,
            )
            .unwrap();
        }
    }
    #[test]
    fn typed_start_success_returns_original_value_and_business_failure_still_rejects() {
        let request = start_request("fixture", None, false, ID, Some(false)).unwrap();
        let already = json!({"schemaVersion":1,"command":"start","task":"fixture","ok":true,
            "taskId":ID,"remote":false,"launchAttemptId":"original-launch","outcome":"already_running",
            "startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_requested","requestSatisfied":true});
        assert_eq!(
            gui_start_outcome(
                TaskOutcome {
                    value: already.clone(),
                    exit_code: 0
                },
                &request
            )
            .unwrap(),
            already
        );
        for comparison in ["different", "unverified"] {
            let mut failure = already.clone();
            failure["ok"] = json!(false);
            failure["requestSatisfied"] = json!(false);
            failure["configComparison"] = json!(comparison);
            failure["error"] = json!("explicit configuration was not applied");
            // Even ok=false/exit=0 cannot resolve as legacy void success.
            let rejected = gui_start_outcome(
                TaskOutcome {
                    value: failure.clone(),
                    exit_code: 0,
                },
                &request,
            )
            .unwrap_err();
            assert_eq!(
                serde_json::from_str::<JsonValue>(&rejected).unwrap(),
                failure
            );
        }
        let mut foreign = already.clone();
        foreign["taskId"] = json!("foreign-task");
        assert!(gui_start_outcome(
            TaskOutcome {
                value: foreign,
                exit_code: 0
            },
            &request
        )
        .is_err());
        assert!(gui_start_outcome(
            TaskOutcome {
                value: already,
                exit_code: 1
            },
            &request
        )
        .is_err());
    }

    #[test]
    fn optional_gui_config_reaches_same_original_selectors_as_cli_none_using_raw_task_fixture() {
        // Same JSON input table as actual StartTaskModal -> TaskManager tests.
        // Raw task fields represent DB shape, not the lossy list Task DTO.
        let table: JsonValue =
            serde_json::from_str(include_str!("../../tests/fixtures/startConfigParity.json"))
                .unwrap();
        fn literal(value: &JsonValue) -> bson::Bson {
            // Fixture storage only: do not interpret opaque business $oid/$numberInt.
            match value {
                JsonValue::Null => bson::Bson::Null,
                JsonValue::Bool(v) => bson::Bson::Boolean(*v),
                JsonValue::String(v) => bson::Bson::String(v.clone()),
                JsonValue::Number(v) => v
                    .as_i64()
                    .map(bson::Bson::Int64)
                    .unwrap_or_else(|| bson::Bson::Double(v.as_f64().unwrap())),
                JsonValue::Array(v) => bson::Bson::Array(v.iter().map(literal).collect()),
                JsonValue::Object(v) => {
                    bson::Bson::Document(v.iter().map(|(k, v)| (k.clone(), literal(v))).collect())
                }
            }
        }
        fn plain(value: &bson::Bson) -> JsonValue {
            match value {
                bson::Bson::Document(d) => {
                    JsonValue::Object(d.iter().map(|(k, v)| (k.clone(), plain(v))).collect())
                }
                bson::Bson::Array(v) => JsonValue::Array(v.iter().map(plain).collect()),
                bson::Bson::String(v) => json!(v),
                bson::Bson::Boolean(v) => json!(v),
                bson::Bson::Int32(v) => json!(v),
                bson::Bson::Int64(v) => json!(v),
                bson::Bson::Double(v) => json!(v),
                bson::Bson::Null => JsonValue::Null,
                other => panic!("unexpected business fixture value: {other:?}"),
            }
        }
        fn assert_selected(selected: &Document, row: &JsonValue) {
            let expected = row.get("edit").unwrap_or(&row["expectedCore"]["value"]);
            assert_eq!(
                &plain(&bson::Bson::Document(selected.clone())),
                expected,
                "{}",
                row["id"]
            );
        }
        for row in table["rows"].as_array().unwrap() {
            let kind = row["kind"].as_str().unwrap();
            let mut task = literal(&row["rawTask"]).as_document().unwrap().clone();
            task.insert("name", "priority-a");
            task.insert("taskType", kind);
            let explicit = row.get("edit").map(JsonValue::to_string);
            let gui =
                start_request("priority-a", explicit.as_deref(), false, ID, Some(false)).unwrap();
            let mut cli = StartRequest::new("priority-a");
            cli.config_json = explicit.clone();
            assert_eq!(gui.config_json, cli.config_json, "{}", row["id"]);
            if kind == "priority" {
                let gui_selected = quantnight_task_core::start::priority_config_input(
                    &task,
                    gui.config_json.as_deref(),
                );
                let cli_selected = quantnight_task_core::start::priority_config_input(
                    &task,
                    cli.config_json.as_deref(),
                );
                assert_eq!(gui_selected, cli_selected, "{}", row["id"]);
                if row["expectedCore"]["selection"] == "original_owner_default" {
                    assert!(gui_selected.unwrap().is_none());
                } else {
                    assert_selected(&gui_selected.unwrap().unwrap(), row);
                }
            } else {
                let gui_selected = quantnight_task_core::start::saved_start_config(
                    &task,
                    gui.config_json.as_deref(),
                );
                let cli_selected = quantnight_task_core::start::saved_start_config(
                    &task,
                    cli.config_json.as_deref(),
                );
                assert_eq!(gui_selected, cli_selected, "{}", row["id"]);
                if row["expectedCore"]["selection"] == "reject" {
                    assert!(gui_selected.is_err());
                } else {
                    assert_selected(&gui_selected.unwrap(), row);
                }
            }
        }
    }

    #[test]
    fn optional_gui_config_keeps_explicit_null_array_and_invalid_json_errors() {
        assert!(start_request("fixture", None, false, ID, Some(false))
            .unwrap()
            .config_json
            .is_none());
        for value in ["null", "[]", "false", "broken", "", "\"null\""] {
            assert!(
                start_request("fixture", Some(value), false, ID, Some(false)).is_err(),
                "{value}"
            );
        }
        let opaque = r#"{"opaque":{"$oid":"business","$numberInt":"01","array":[null,true,1]}}"#;
        assert_eq!(
            start_request("fixture", Some(opaque), true, ID, Some(true))
                .unwrap()
                .config_json
                .as_deref(),
            Some(opaque)
        );
    }

    #[test]
    fn false_wrong_identity_schema_and_invalid_json_never_retry_or_write_running() {
        let request = start_request("fixture", Some(CONFIG), false, ID, None).unwrap();
        let mut receipts = vec![JsonValue::Null, json!({}), json!("not-json")];
        for change in [
            json!({"ok":false,"error":"execution_context_unverified: specific-host-error"}),
            json!({"taskId":"replacement-task"}),
            json!({"remote":true}),
            json!({"remote":"false"}),
            json!({"task":"other"}),
            json!({"command":"status"}),
            json!({"schemaVersion":2}),
            json!({"ok":"true"}),
        ] {
            let mut receipt = success(false);
            receipt
                .as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
            receipts.push(receipt);
        }
        for receipt in receipts {
            let result = TaskOutcome {
                value: receipt.clone(),
                exit_code: 0,
            };
            assert!(start_receipt(&result, &request)
                .unwrap_err()
                .contains(&receipt.to_string()));
        }
        assert!(start_receipt(
            &TaskOutcome {
                value: success(false),
                exit_code: 1
            },
            &request
        )
        .is_err());
    }
    #[tokio::test]
    async fn nonzero_and_timeout_preserve_partial_diagnostics_and_do_not_retry() {
        // Same shared process owner used by native start; no Python taskctl shim.
        for (tail, expected, budget) in [
            ("exit 17", "17", Duration::from_secs(10)),
            ("exec /bin/sleep 30", "2000 ms", Duration::from_secs(2)),
        ] {
            let f = FakeBash::new(&format!(
                "echo called >> calls\necho partial-stdout\necho host-binding-failure >&2\n{tail}"
            ));
            let root = Path::new(&f.config.env.working_dir);
            let mut command = tokio::process::Command::new("/bin/bash");
            command.current_dir(root).arg("fake.sh");
            let error = run_process_observed(command, budget, false, None)
                .await
                .unwrap_err();
            assert!(
                error.contains(expected)
                    && error.contains("host-binding-failure")
                    && error.contains("partial-stdout"),
                "{error}"
            );
            assert_eq!(fs::read_to_string(root.join("calls")).unwrap(), "called\n");
        }
    }
    #[tokio::test]
    async fn malformed_id_config_and_missing_process_fail_without_launch() {
        for (id, config) in [("", CONFIG), (ID, "bad-json"), (ID, "[]")] {
            assert!(start_request("fixture", Some(config), false, id, None).is_err());
        }
        let command = tokio::process::Command::new("/no-such-quantnight-test-process");
        let error = run_process_observed(command, Duration::from_secs(1), false, None)
            .await
            .unwrap_err();
        assert!(error.contains("启动命令失败"), "{error}");
    }
}

#[cfg(test)]
mod gui_pause_join_tests {
    use super::*;
    const ID: &str = "0123456789abcdef01234567";
    fn pause_result(id: &str, remote: bool) -> CallResult {
        CallResult {
            value: serde_json::json!({"schemaVersion":1,"command":"pause","task":"exact","ok":true}),
            exit_code: 0,
            confirmed_stop: Some(ConfirmedTaskStop { confirmed:true,task_id:id.into(),task_name:"exact".into(),
                is_remote:remote,stop_confirmed_at:1_794_000_000_123,
                confirmation_basis:StopConfirmationBasis::RunnersExited }),
        }
    }

    #[test]
    fn progress_gap_is_wait_notice_not_business_failure_or_stop_evidence() {
        let events = Arc::new(Mutex::new(Vec::<TaskOperationStage>::new()));
        let recorded = events.clone();
        let id = "0123456789abcdef01234567";
        let progress = TaskOperationProgress::new(Arc::new(move |stage| recorded.lock().unwrap().push(stage)),
            "stop",Some(id),"exact",false,Some("original-ui-token".into()));
        let observer = operation_observer(progress);
        observer(RuntimeEvent::ProgressGap {dropped:64});
        let rows = events.lock().unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].stage,"wait");
        assert_eq!(rows[0].message,PROGRESS_GAP_NOTICE);
        assert_eq!(rows[0].request_id,"original-ui-token");
        assert_eq!(rows[0].task_id,id);
        assert!(!["failed","completed","control_write"].contains(&rows[0].stage.as_str()));
        // A lost/coalesced observation is independent of the original final proof.
        let stop = gui_pause_outcome(pause_result(id,false),id,"exact",false).unwrap();
        assert!(stop.confirmed);
    }

    #[test]
    fn pause_typed_proof_remains_original_and_exactly_bound_after_shared_call() {
        let id = "0123456789abcdef01234567";
        let original = pause_result(id, true);
        let wire = serde_json::to_value(original.confirmed_stop.as_ref().unwrap()).unwrap();
        let stop = gui_pause_outcome(original, id, "exact", true).unwrap();
        assert_eq!(serde_json::to_value(stop).unwrap(), wire);
        assert!(gui_pause_outcome(pause_result(id,true),id,"exact",false).is_err());
        assert!(gui_pause_outcome(pause_result(id,true),"1123456789abcdef01234567","exact",true).is_err());
        assert!(gui_pause_outcome(pause_result(id,true),id,"replacement",true).is_err());
        let mut absent = pause_result(id,true); absent.confirmed_stop = None;
        assert!(gui_pause_outcome(absent,id,"exact",true).is_err());
        let mut invalid = pause_result(id,true); invalid.confirmed_stop.as_mut().unwrap().stop_confirmed_at = 0;
        assert!(gui_pause_outcome(invalid,id,"exact",true).is_err());
        let mut failed = pause_result(id,true); failed.value["ok"] = serde_json::json!(false);
        assert!(gui_pause_outcome(failed,id,"exact",true).is_err());
    }

    #[test]
    fn exact_absent_and_legacy_options_remain_distinct_and_remaining_is_consumed() {
        let legacy =
            gui_pause_options(ID, false, None, None, None, Duration::from_secs(1)).unwrap();
        assert!(!legacy.expected_launch_absent);
        assert!(legacy.expected_launch_attempt_id.is_none());
        assert!(legacy.remaining.is_none());
        let exact = gui_pause_options(
            ID,
            true,
            Some("launch-original"),
            Some(false),
            Some(120000),
            Duration::from_millis(9000),
        )
        .unwrap();
        assert_eq!(exact.expected_launch_attempt_id, Some("launch-original"));
        assert!(!exact.expected_launch_absent);
        assert_eq!(exact.remaining, Some(Duration::from_millis(111000)));
        let absent = gui_pause_options(
            ID,
            true,
            None,
            Some(true),
            Some(85000),
            Duration::from_secs(8),
        )
        .unwrap();
        assert!(absent.expected_launch_absent);
        assert!(absent.expected_launch_attempt_id.is_none());
        assert_eq!(absent.remaining, Some(Duration::from_secs(77)));
        for (exact, absent, remaining, elapsed) in [
            (Some(""), None, None, 0),
            (Some("  "), None, None, 0),
            (Some("launch"), Some(true), None, 0),
            (None, None, Some(120001), 0),
            (None, None, Some(0), 0),
            (None, Some(true), Some(80), 80),
            (None, None, Some(80), 90),
        ] {
            assert!(gui_pause_options(
                ID,
                false,
                exact,
                absent,
                remaining,
                Duration::from_millis(elapsed)
            )
            .is_err());
        }
        assert_eq!(StopBudget::Gui.seconds(), 75);
        assert_eq!(StopBudget::Cli.seconds(), 60);
    }
    #[test]
    fn frozen_gui_optional_argument_types_reject_malformed_values_not_convert_to_absence() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Arguments {
            expected_launch_attempt_id: Option<String>,
            expected_launch_absent: Option<bool>,
            remaining_ms: Option<u64>,
        }
        for raw in [
            r#"{"expectedLaunchAttemptId":false}"#,
            r#"{"expectedLaunchAttemptId":{}}"#,
            r#"{"expectedLaunchAbsent":"true"}"#,
            r#"{"remainingMs":-1}"#,
            r#"{"remainingMs":1.5}"#,
            r#"{"remainingMs":"10"}"#,
            r#"{"remainingMs":true}"#,
        ] {
            assert!(serde_json::from_str::<Arguments>(raw).is_err(), "{raw}");
        }
        let args: Arguments =
            serde_json::from_str(r#"{"expectedLaunchAbsent":true,"remainingMs":100000}"#).unwrap();
        let options = gui_pause_options(
            ID,
            false,
            args.expected_launch_attempt_id.as_deref(),
            args.expected_launch_absent,
            args.remaining_ms,
            Duration::ZERO,
        )
        .unwrap();
        assert!(options.expected_launch_absent);
        assert!(options.expected_launch_attempt_id.is_none());
    }
}

#[cfg(test)]
mod generated_start_ipc_tests {
    use super::*;
    use serde_json::json;

    fn rejected_by_real_handler(command: &str, body: JsonValue) -> JsonValue {
        // Register the ORIGINAL three generated wrappers. No copied test command,
        // no production bootstrap, and deliberately no managed MongoClients state.
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![
                crate::commands::start_task,
                crate::commands::start_super_task,
                crate::commands::start_priority_task,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build Tauri MockRuntime application only");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview only");
        let response = tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        );
        match response {
            Err(error) => error,
            Ok(_) => {
                panic!("command must not succeed or reach business work without clients state")
            }
        }
    }
    fn scalar_arguments() -> JsonValue {
        json!({"taskName":"ipc-config-fixture", "taskId":"0123456789abcdef01234567", "isRemote":false})
    }
    fn assert_unmanaged_clients(command: &str, body: JsonValue) {
        let error = rejected_by_real_handler(command, body);
        let expected = format!("state not managed for field `clients` on command `{command}`. You must call `.manage()` before using this command");
        assert_eq!(
            error.as_str(),
            Some(expected.as_str()),
            "must reach original State extraction, not stop at config decoding: {error}"
        );
    }

    #[test]
    fn generated_start_handlers_decode_omitted_null_and_explicit_config_before_unmanaged_state() {
        for command in ["start_task", "start_super_task", "start_priority_task"] {
            assert_unmanaged_clients(command, scalar_arguments()); // config key is really omitted
            let mut null = scalar_arguments();
            null["config"] = JsonValue::Null;
            assert_unmanaged_clients(command, null); // nullable Option is None, not JSON text "null"
            let mut explicit = scalar_arguments();
            explicit["config"] =
                json!(r#"{"max_concurrent":3,"opaque":{"$oid":"literal","values":[true,null]}}"#);
            explicit["expectedRemote"] = json!(false);
            assert_unmanaged_clients(command, explicit);
            // String DTO decoding is not business JSON validation. The command
            // body cannot run here; original start_request(Some(...)) tests own
            // rejection of these contents after managed-state extraction.
            for text in ["null", "[]", "broken-json"] {
                let mut body = scalar_arguments();
                body["config"] = json!(text);
                assert_unmanaged_clients(command, body);
            }
        }
    }

    #[test]
    fn generated_start_handlers_reject_wrong_config_types_before_state_or_business_entry() {
        for command in ["start_task", "start_super_task", "start_priority_task"] {
            for invalid in [json!({}), json!([]), json!(false), json!(17)] {
                let mut body = scalar_arguments();
                body["config"] = invalid;
                let error = rejected_by_real_handler(command, body);
                let text = error.as_str().expect("original macro rejection string");
                assert!(
                    text.contains("invalid args `config`") && text.contains(command),
                    "{text}"
                );
                assert!(
                    !text.contains("state not managed"),
                    "config must fail before State extraction"
                );
            }
            let mut missing = scalar_arguments();
            missing.as_object_mut().unwrap().remove("taskName");
            let error = rejected_by_real_handler(command, missing);
            let text = error.as_str().unwrap();
            assert!(
                text.contains("missing required key taskName"),
                "required scalar still required: {text}"
            );
            assert!(!text.contains("state not managed"));
        }
    }
}
