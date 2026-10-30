//! Shared inspection/evidence coordinators. Long observations are unlocked;
//! only exact terminal reconciliation owns the original lifecycle EX lock.
//! Python retains professional Priority/dataset algorithms; no taskctl callback.
use crate::{
    cleanup_sync::{outcome, OperationsRuntime, TaskOutcome},
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    start::saved_start_config,
    task_store::{CanonicalNamespace, CollectionSummary, Host, TaskStore},
};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[async_trait]
pub trait InspectionRuntime: OperationsRuntime {
    async fn inspect_execution(&self,_root:&Path,_store:&dyn TaskStore,_task:&Document,_host:Host)
        -> Result<Option<Value>,String>{Ok(None)}
    /// Original MongoConfig profile mapping; no client/URI/GUI guess/fallback.
    async fn canonical_namespace(
        &self,
        _root: &Path,
        _profile: &str,
    ) -> Result<CanonicalNamespace, String> {
        Err("original canonical namespace mapping unavailable".into())
    }
    /// Original Priority checkpoint algorithm, including run-host reads and
    /// optional exact source-ID inspection. Return original fields incl. `ok`.
    /// No control writes; checkpoint invalidity is ok=false, CLI exit=0.
    async fn priority_snapshot(
        &self,
        _root: &Path,
        _task: &Document,
        _host: Host,
        _inspect_items: bool,
    ) -> Result<Value, String> {
        Err("original Priority checkpoint owner unavailable".into())
    }
    async fn priority_default_config(
        &self,
        _root: &Path,
        _task: &Document,
    ) -> Result<Document, String> {
        Err("original Priority default config owner unavailable".into())
    }
    /// Original verify_registration owner against LOCAL generated collection.
    async fn verify_registration(&self, _root: &Path, _task: &Document) -> Result<Value, String> {
        Err("original dataset verification owner unavailable".into())
    }
}
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    pub include_deactive: bool,
    pub remote: bool,
}
/// Existing producer identity, optional for ordinary status callers.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchObservation {
    pub task_id: String,
    pub launch_id: String,
    pub remote: bool,
}
#[derive(Debug, Clone)]
pub struct StatusRequest {
    pub name: String,
    pub remote: Option<bool>,
    pub inspect_items: bool,
    pub watch_observation: Option<WatchObservation>,
}
impl StatusRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            remote: None,
            inspect_items: false,
            watch_observation: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ResultsRequest {
    pub name: String,
    pub limit: i64,
    pub offset: i64,
    pub output: Option<PathBuf>,
}
impl ResultsRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            limit: 20,
            offset: 0,
            output: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct CollectRequest {
    pub name: String,
    pub output: Option<PathBuf>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceAction {
    VerifyRegistration,
    ArchiveResearch,
}
impl EvidenceAction {
    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::VerifyRegistration => "verify-registration",
            Self::ArchiveResearch => "archive-research",
        }
    }
}
#[derive(Debug, Clone)]
pub struct EvidenceRequest {
    pub name: String,
    pub action: EvidenceAction,
    pub expected_task_id: Option<String>,
    pub expected_remote: Option<bool>,
}

#[derive(Debug)]
struct Error {
    kind: &'static str,
    message: String,
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self {
            kind: if message.starts_with("unhashable type:") {
                "TypeError"
            } else {
                "RuntimeError"
            },
            message,
        }
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
fn invalid(message: impl ToString) -> Error {
    Error {
        kind: "ValueError",
        message: message.to_string(),
    }
}
fn finish(command: &str, name: &str, result: Result<Value, Error>) -> TaskOutcome {
    match result {
        Ok(value) => outcome(command, name, true, value),
        Err(error) => outcome(
            command,
            name,
            false,
            json!({"errorType":error.kind,"error":error.message}),
        ),
    }
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn truthy(value: Option<&Bson>) -> bool {
    match value {
        None | Some(Bson::Null | Bson::Undefined) => false,
        Some(Bson::Boolean(v)) => *v,
        Some(Bson::Int32(v)) => *v != 0,
        Some(Bson::Int64(v)) => *v != 0,
        Some(Bson::Double(v)) => *v != 0.0,
        Some(Bson::String(v) | Bson::Symbol(v) | Bson::JavaScriptCode(v)) => !v.is_empty(),
        Some(Bson::Document(v)) => !v.is_empty(),
        Some(Bson::Array(v)) => !v.is_empty(),
        Some(Bson::Binary(v)) => !v.bytes.is_empty(),
        Some(Bson::JavaScriptCodeWithScope(v)) => !v.code.is_empty(),
        _ => true,
    }
}
fn checked_token(value: Option<&Bson>, choices: &[&str]) -> Result<bool, Error> {
    let kind = match value {
        Some(Bson::Array(_)) => Some("list"),
        Some(Bson::Document(_)) => Some("dict"),
        Some(Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_)) => Some("Code"),
        Some(Bson::RegularExpression(_)) => Some("Regex"),
        Some(Bson::Decimal128(_)) => Some("Decimal128"),
        _ => None,
    };
    if let Some(kind) = kind {
        return Err(format!("unhashable type: '{kind}'").into());
    }
    Ok(match value {
        Some(Bson::String(s) | Bson::Symbol(s)) => choices.contains(&s.as_str()),
        _ => false,
    })
}
fn python_order(left: &Bson, right: &Bson) -> Result<std::cmp::Ordering, Error> {
    use std::cmp::Ordering;
    if left == right {
        return Ok(Ordering::Equal);
    }
    fn number(v: &Bson) -> Option<f64> {
        match v {
            Bson::Double(n) => Some(*n),
            Bson::Int32(n) => Some(*n as f64),
            Bson::Int64(n) => Some(*n as f64),
            Bson::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    if let (Some(a), Some(b)) = (number(left), number(right)) {
        return Ok(a.partial_cmp(&b).unwrap_or(Ordering::Equal));
    }
    if let (Bson::String(a), Bson::String(b)) = (left, right) {
        return Ok(a.cmp(b));
    }
    Err(Error {
        kind: "TypeError",
        message: "result sort keys are not mutually orderable".into(),
    })
}
// taskctl uses json.dumps(default=str) over PyMongo's default naive UTC
// dates, not Extended JSON nor RFC3339 date objects. Keep this CLI wire shape.
fn wire(value: Option<&Bson>) -> Value {
    match value {
        None | Some(Bson::Null | Bson::Undefined) => Value::Null,
        Some(Bson::DateTime(stamp)) => {
            let at =
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(stamp.timestamp_millis());
            at.map(|at| {
                json!(if at.timestamp_subsec_micros() == 0 {
                    at.format("%Y-%m-%d %H:%M:%S").to_string()
                } else {
                    at.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
                })
            })
            .unwrap_or_else(|| json!(stamp.to_string()))
        }
        Some(Bson::Document(d)) => wire_document(d),
        Some(Bson::Array(a)) => Value::Array(a.iter().map(|v| wire(Some(v))).collect()),
        Some(v) => crate::cleanup_sync::wire_document(&doc! {"v":v.clone()})["v"].clone(),
    }
}
fn wire_document(d: &Document) -> Value {
    Value::Object(d.iter().map(|(k, v)| (k.clone(), wire(Some(v)))).collect())
}
fn summary_value(summary: &CollectionSummary) -> Value {
    let mut value =
        json!({"exists":summary.exists,"total":summary.total,"statuses":summary.statuses});
    if summary.exists {
        value["latestHeartbeatAt"] = wire(summary.latest_heartbeat_at.as_ref());
    }
    value
}

fn summary_document(summary: &CollectionSummary) -> Result<Document, Error> {
    let mut result = bson::to_document(&summary_value(summary)).map_err(|e| e.to_string())?;
    if summary.exists {
        result.insert(
            "latestHeartbeatAt",
            summary.latest_heartbeat_at.clone().unwrap_or(Bson::Null),
        );
    }
    Ok(result)
}
fn text_id(value: Option<&Bson>) -> String {
    match wire(value) {
        Value::String(v) => v,
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        v => v.to_string(),
    }
}
async fn task(
    store: &dyn TaskStore,
    name: &str,
    include_deactive: bool,
) -> Result<Document, Error> {
    policy::validate_name(name).map_err(invalid)?;
    let active = store
        .find_task(
            Host::Local,
            doc! {"name":name,"status":{"$ne":"deactive"}},
            true,
        )
        .await?;
    let found = if active.is_none() && include_deactive {
        store
            .find_task(Host::Local, doc! {"name":name}, true)
            .await?
    } else {
        active
    };
    found.ok_or_else(|| Error::from("task not found"))
}
fn host(task: &Document) -> Host {
    Host::from_remote(truthy(task.get("isRemote")))
}
fn write_json(path: &Path, value: &Value) -> Result<(), Error> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list(store: &dyn TaskStore, request: &ListRequest) -> TaskOutcome {
    let result = store
        .list_tasks(Host::from_remote(request.remote), request.include_deactive)
        .await
        .map(|rows| json!({"tasks": rows.iter().map(wire_document).collect::<Vec<_>>()}))
        .map_err(Error::from);
    let mut result = finish("list", "", result);
    result.value["task"] = Value::Null;
    result
}

/// Pure startup applicability checks plus exact execution-registry read. This
/// gate runs BEFORE fresh item counts, including on local execution.
async fn startup_gate(
    store: &dyn TaskStore,
    task: &Document,
) -> Result<(Document, Document), Error> {
    let not_required = || (doc! {"ready":true,"required":false}, doc! {});
    if task.get_str("taskType").ok() != Some("super") {
        return Ok(not_required());
    }
    let status = task.get_str("status").unwrap_or("");
    let mut launched = [
        "launchAttemptId",
        "launchAttemptAt",
        "startupBinding",
        "startedAt",
        "completedAt",
        "launchHistory",
        "launchAttempts",
        "launchReceipt",
        "startReceipt",
        "consumerPid",
        "consumer_run_id",
        "heartbeatAt",
        "simulatorRuntime",
        "acceptedObservation",
        "pausedAt",
    ]
    .iter()
    .any(|k| task.contains_key(*k));
    if !launched {
        match task.get("events") {
            None => (),
            Some(Bson::Array(events)) => {
                for row in events {
                    let command = match row {
                        Bson::Document(row) => row.get("command"),
                        _ => {
                            launched = true;
                            break;
                        }
                    };
                    if checked_token(
                        command,
                        &[
                            "start",
                            "resume",
                            "pause",
                            "watch-terminal",
                            "status-terminal-reconcile",
                            "observe-accepted",
                        ],
                    )? {
                        launched = true;
                        break;
                    }
                }
            }
            _ => launched = true,
        }
    }
    let generated = checked_token(task.get("status"), &["pending", "ready"])?
        && !launched
        && match task.get_document("generation") {
            Ok(g) => {
                g.get_str("id").map(|s| !s.is_empty()).unwrap_or(false)
                    && truthy(g.get("finishedAt"))
                    && checked_token(g.get("phase"), &["prepared", "complete"])?
            }
            Err(_) => false,
        };
    let generation_phase = checked_token(
        task.get("status"),
        &["generating", "generation_failed", "generation_unknown"],
    )? || generated;
    if (status == "paused" && truthy(task.get("acceptedObservation"))) || generation_phase {
        return Ok(not_required());
    }
    let mut pending = doc! {"ready":false,"required":true,"reason":"startup_recovery_unconfirmed"};
    let binding = task.get_document("startupBinding").ok();
    let valid = binding
        .map(|b| {
            b.get_str("taskId").ok() == Some(text_id(task.get("_id")).as_str())
                && b.get("name") == task.get("name")
                && b.get("auth_profile") == task.get("auth_profile")
                && b.get_bool("remote").ok() == Some(host(task).is_remote())
                && b.get_str("auth_profile").map(|v|!v.is_empty()).unwrap_or(false)
                && b.get_str("consumer_run_id").map(|v|!v.is_empty()).unwrap_or(false)
                && b.get_str("launchAttemptId").map(|v|!v.is_empty()).unwrap_or(false)
                && b.get("launchAttemptId") == task.get("launchAttemptId")
        })
        .unwrap_or(false);
    if !valid {
        pending.insert("reason", "startup_binding_missing_or_conflicting");
        return Ok((pending, doc! {}));
    }
    let binding = binding.unwrap();
    if !binding.contains_key("auth_profile") {
        return Err(Error {
            kind: "KeyError",
            message: "'auth_profile'".into(),
        });
    }
    let query = startup_query(task, binding);
    let runtime = store.find_task(host(task), query, false).await?;
    let evidence = runtime
        .as_ref()
        .and_then(|r| r.get_document("simulatorRuntime").ok());
    let proof = evidence.and_then(|r| r.get_document("startupRecovery").ok());
    let proven = proof
        .map(|p| {
            p.get_str("phase").ok() == Some("complete")
                && truthy(p.get("completedAt"))
                && binding.iter().all(|(k, v)| p.get(k) == Some(v))
                && evidence.and_then(|e| e.get("consumer_run_id")) == binding.get("consumer_run_id")
        })
        .unwrap_or(false);
    if !proven {
        return Ok((pending, doc! {}));
    }
    let proof = proof.unwrap().clone();
    let closure_proof = evidence.and_then(|r| r.get_document("lifecycleClosure").ok());
    let closed = closure_proof.map(|p| {
        matches!(p.get("schemaVersion"), Some(Bson::Int32(1) | Bson::Int64(1)))
            && p.get_str("phase").ok() == Some("complete")
            && truthy(p.get("completedAt"))
            && binding.iter().all(|(k, v)| p.get(k) == Some(v))
    }).unwrap_or(false);
    let closure = if closed {
        doc! {"required":true,"ready":true,"proof":closure_proof.unwrap().clone()}
    } else {
        doc! {"required":true,"ready":false,"reason":"lifecycle_closure_unconfirmed"}
    };
    let mut fence = doc! {"startupBinding":binding.clone(),"taskType":"super","auth_profile":binding.get("auth_profile").cloned().unwrap_or(Bson::Null)};
    if !host(task).is_remote() {
        if closed {
            fence.insert("simulatorRuntime.lifecycleClosure", closure_proof.unwrap().clone());
        }
        fence.insert("simulatorRuntime.startupRecovery", proof.clone());
        fence.insert(
            "simulatorRuntime.consumer_run_id",
            binding
                .get("consumer_run_id")
                .cloned()
                .unwrap_or(Bson::Null),
        );
    }
    Ok((doc! {"ready":true,"required":true,"proof":proof,"lifecycleClosure":closure}, fence))
}
fn startup_query(task: &Document, binding: &Document) -> Document {
    doc! {"_id":task.get("_id").cloned().unwrap_or(Bson::Null),"name":task.get("name").cloned().unwrap_or(Bson::Null),
    "taskType":"super","auth_profile":binding.get("auth_profile").cloned().unwrap_or(Bson::Null),
    "isRemote":binding.get("remote").cloned().unwrap_or(Bson::Null),"launchAttemptId":binding.get("launchAttemptId").cloned().unwrap_or(Bson::Null),
    "startupBinding":binding.clone(),"status":{"$ne":"deactive"}}
}
/// Reuse the original control CAS identity for both unlocked read freshness
/// and the short locked reconciliation. Optional proof absence is not invented.
fn original_v1_stop(task:&Document, remote:bool)->bool{
    if !policy::stop_confirmation_valid(task,remote){return false;}
    match task.get("stopConfirmationBasis"){
        Some(Bson::String(basis))=>return basis=="runners_exited",
        None|Some(Bson::Null)=>(),_=>return false,
    }
    let stamp=task.get_datetime("stopConfirmedAt").unwrap().timestamp_millis();
    task.get_array("events").map(|events|events.iter().rev().any(|event|{
        let Some(e)=event.as_document() else{return false;};
        let Ok(detail)=e.get_document("detail") else{return false;};
        e.get_str("command").ok()==Some("pause")&&e.get_str("status").ok()==Some("paused")
            &&e.get_str("at").ok().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok()).map(|v|v.timestamp_millis())==Some(stamp)
            &&detail.get_i32("schema_version").ok()==Some(1)&&detail.get_str("task_name").ok()==task.get_str("name").ok()
            &&detail.get_bool("target_remote").ok()==Some(remote)&&detail.get_bool("session_absent").ok()==Some(true)
            &&detail.get_bool("runners_exited").ok()==Some(true)
    })).unwrap_or(false)
}
fn status_snapshot_query(task: &Document, fence: &Document) -> Result<Document, Error> {
    let name = task.get_str("name").map_err(|e| e.to_string())?;
    let mut query = policy::task_binding_query(name, task, "status-terminal-reconcile")?;
    for key in ["acceptedObservation", "runId", "launchAttemptAt", "startedAt"] {
        query.insert(key, task.get(key).cloned()
            .unwrap_or_else(|| Bson::Document(doc! {"$exists":false})));
    }
    query.extend(fence.clone());
    Ok(query)
}
async fn confirm_status_snapshot(store: &dyn TaskStore, query: &Document) -> Result<(), Error> {
    if store.find_task(Host::Local, query.clone(), false).await?.is_none() {
        return Err("status snapshot changed during observation; no reconciliation performed; refresh original task".into());
    }
    Ok(())
}
fn failed_observation(task: &Document, mut facts: Value, observed_at: &str, error: Error) -> TaskOutcome {
    facts.as_object_mut().unwrap().remove("taskStatus");
    facts["capturedTaskStatus"] = wire(task.get("status"));
    let identity: Document = ["_id", "name", "taskType", "isRemote", "launchAttemptId", "runId", "updatedAt"]
        .iter().filter_map(|key| task.get(*key).map(|value| ((*key).into(), value.clone()))).collect();
    facts["observationIdentity"] = wire_document(&identity);
    facts["observedAt"] = json!(observed_at);
    facts["snapshotCurrent"] = json!(false);
    facts["reconcileSkipped"] = json!(true);
    facts["reconcileError"] = json!(error.message);
    facts["errorType"] = json!(error.kind);
    facts["error"] = facts["reconcileError"].clone();
    facts["observedTerminal"] = facts["terminal"].clone();
    facts["terminal"] = json!(false);
    outcome("status", task.get_str("name").unwrap_or(""), false, facts)
}
fn watch_control(task: &Document) -> Value {
    let observed: Document = ["_id", "name", "taskType", "isRemote", "launchAttemptId", "startedAt",
        "status", "generation", "lastError", "expectedCount"]
        .iter().filter_map(|key| task.get(*key).map(|v| ((*key).into(), v.clone()))).collect();
    wire_document(&observed)
}
fn watch_launch(task: &Document) -> String {
    for key in ["launchAttemptId", "startedAt"] {
        if let Some(value) = task.get(key).filter(|v| truthy(Some(v))) {
            // Python's original marker uses str(BSON datetime) for old launches.
            return match value {
                Bson::DateTime(v) => {
                    let stamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(v.timestamp_millis()).expect("BSON launch datetime");
                    let base = stamp.format("%Y-%m-%d %H:%M:%S").to_string();
                    if stamp.timestamp_subsec_micros() == 0 { base }
                    else { format!("{base}.{:06}", stamp.timestamp_subsec_micros()) }
                },
                Bson::String(v) => v.clone(),
                _ => wire(Some(value)).to_string(),
            };
        }
    }
    "legacy".into()
}
async fn status_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &StatusRequest,
) -> Result<TaskOutcome, Error> {
    policy::validate_name(&request.name).map_err(invalid)?;
    let task = if let Some(watch) = &request.watch_observation {
        let id = ObjectId::parse_str(&watch.task_id).map_err(|e| invalid(e.to_string()))?;
        match store.find_task(Host::Local, doc! {"_id":id}, false).await? {
            Some(task) => task,
            None => return Ok(outcome("status", &request.name, true,
                json!({"watchState":"source_changed","reason":"mission identity/route changed", "terminal":false,"watchControl":{}}))),
        }
    } else { task(store, &request.name, true).await? };
    let observed_at = now();
    let execution_host = host(&task);
    // The producer's existing binding is checked before any possible write.
    // Ordinary status has no watcher context and retains its existing semantics.
    if let Some(watch) = &request.watch_observation {
        let reason = if text_id(task.get("_id")) != watch.task_id || task.get_str("name").ok() != Some(request.name.as_str())
            || execution_host.is_remote() != watch.remote {
            Some("mission identity/route changed")
        } else if watch_launch(&task) != watch.launch_id {
            Some("mission launch changed")
        } else { None };
        let old = task.get_str("status").unwrap_or("");
        if let Some(reason) = reason {
            return Ok(outcome("status", &request.name, true,
                json!({"watchState":"source_changed","reason":reason,"terminal":false,"watchControl":watch_control(&task)})));
        }
        if ["launch_unknown", "syncing", "deleting"].contains(&old) {
            return Ok(failed_observation(&task,
                json!({"terminal":false,"watchControl":watch_control(&task)}), &observed_at,
                "lifecycle mutation unresolved; watcher cannot reconcile it".into()));
        }
        if ["paused", "deactive", "generating", "generation_failed", "generation_unknown"].contains(&old) {
            let state = if ["paused", "deactive"].contains(&old) { "stopped" } else { "source_changed" };
            confirm_status_snapshot(store, &status_snapshot_query(&task, &doc! {})?).await?;
            return Ok(outcome("status", &request.name, true,
                json!({"watchState":state,"terminal":false,"watchControl":watch_control(&task),
                    "reason":"generation began outside simulation watch binding","controlStatus":old})));
        }
    }
    if task.get_str("taskType").ok() == Some("priority") {
        let value = runtime
            .priority_snapshot(
                root,
                &task,
                request
                    .remote
                    .map(Host::from_remote)
                    .unwrap_or(execution_host),
                request.inspect_items,
            )
            .await?;
        if let Err(error) = confirm_status_snapshot(store, &status_snapshot_query(&task, &doc! {})?).await {
            return Ok(failed_observation(&task, value, &observed_at, error));
        }
        let ok = value
            .get("ok")
            .and_then(Value::as_bool)
            .ok_or("Priority snapshot missing boolean ok")?;
        let mut result = outcome("status", &request.name, ok, value);
        result.exit_code = 0;
        return Ok(result);
    }
    let mut startup_read_error=None;
    let (mut startup, mut fence)=match startup_gate(store,&task).await {
        Ok(value)=>value,
        Err(error) if error.kind=="TypeError"=>return Err(error),
        Err(error)=>{
            startup_read_error=Some(error.message.clone());
            (doc!{"ready":false,"required":true,"reason":"startup_evidence_read_error","error":error.message},doc!{})
        }
    };
    let closure = startup.get_document("lifecycleClosure").cloned().unwrap_or_else(|_| {
        if startup.get_bool("required").unwrap_or(false) {
            doc! {"required":true,"ready":false,"reason":"lifecycle_closure_unconfirmed"}
        } else {
            doc! {"required":false,"ready":true}
        }
    });
    startup.remove("lifecycleClosure");
    let mut execution_observation=Value::Null;
    let mut receipt_observation=Value::Null;
    let mut legacy_ready=false;
    let mut legacy_registry = None;
    let local_read = store.summary(Host::Local, &request.name).await;
    let execution_read = if execution_host == Host::Local {
        local_read.clone()
    } else { store.summary(execution_host, &request.name).await };
    if local_read.is_err() || execution_read.is_err() {
        let facts = json!({"remote":execution_host.is_remote(),
            "local":local_read.as_ref().map(summary_value).unwrap_or(Value::Null),
            "execution":execution_read.as_ref().map(summary_value).unwrap_or(Value::Null),
            "readErrors":{"local":local_read.as_ref().err(),"execution":execution_read.as_ref().err()},
            "startupRecovery":wire_document(&startup),"lifecycleClosure":wire_document(&closure)});
        let error = local_read.as_ref().err().or(execution_read.as_ref().err()).unwrap().clone();
        return Ok(failed_observation(&task, facts, &observed_at, error.into()));
    }
    let local = local_read.unwrap();
    let execution = execution_read.unwrap();
    let terminal_states = [
        "success",
        "permanent_error",
        "timeout_error",
        "cancelled",
        "error",
    ];
    let unfinished: u64 = execution
        .statuses
        .iter()
        .filter(|(k, _)| !terminal_states.contains(&k.as_str()))
        .map(|(_, v)| *v)
        .sum();
    if task.get_str("taskType").ok()==Some("super") && startup.get_bool("required").unwrap_or(false)
        && !closure.get_bool("ready").unwrap_or(false) && startup_read_error.is_none()
        && execution.total>0 && unfinished==0 && original_v1_stop(&task,execution_host.is_remote()) {
        if request.watch_observation.is_some() {
            legacy_registry = Some(store.find_task(execution_host,
                doc! {"name":&request.name,"status":{"$ne":"deactive"}}, false).await?);
            fence.insert("simulatorRuntime", task.get("simulatorRuntime").cloned()
                .unwrap_or_else(|| Bson::Document(doc! {"$exists":false})));
        }
        match runtime.inspect_execution(root,store,&task,execution_host).await {
            Ok(Some(observation))=>{
                let absent=observation["state"]=="session_absent";
                execution_observation=observation;
                execution_observation["naturalClosure"]=json!(false);
                execution_observation["consumerStopped"]=json!(absent);
                execution_observation["runnersExited"]=if absent&&original_v1_stop(&task,execution_host.is_remote()){json!(true)}else{Value::Null};
                match store.execution_receipt_assessment(execution_host,&request.name).await {
                    Ok(assessment)=>{
                        legacy_ready=absent&&original_v1_stop(&task,execution_host.is_remote())&&policy::assert_observation_closed(&task).is_ok()
                            &&assessment.get_i64("rowCount").unwrap_or(0)>0&&assessment.get_i64("unresolvedItemCount").ok()==Some(0)
                            &&assessment.get_i64("receiptUnresolvedItemCount").ok()==Some(0)
                            &&assessment.get_i64("assessmentUnknownCount").ok()==Some(0)&&assessment.get_bool("complete").ok()==Some(true);
                        receipt_observation=wire_document(&assessment);
                    },
                    Err(error)=>receipt_observation=json!({"complete":false,"error":error}),
                }
            },
            Ok(None)=>execution_observation=json!({"state":"unconfirmed","reason":"original execution observer unavailable"}),
            Err(error)=>execution_observation=json!({"state":"unconfirmed","error":error}),
        }
        for key in ["stopConfirmedAt","stopConfirmedTaskId","stopConfirmedRemote","stopConfirmationBasis","events"]{
            fence.insert(key,task.get(key).cloned().unwrap_or_else(||Bson::Document(doc!{"$exists":false})));
        }
    }
    legacy_ready=legacy_ready&&receipt_observation["rowCount"].as_u64()==Some(execution.total)&&unfinished==0;
    let terminal = ((startup.get_bool("ready").unwrap_or(false) && closure.get_bool("ready").unwrap_or(false))||legacy_ready)
            && execution.total > 0 && unfinished == 0;
    let mut status = task.get("status").cloned().unwrap_or(Bson::Null);
    let old_status = task.get_str("status").unwrap_or("");
    let query = status_snapshot_query(&task, &fence)?;
    let mut captured = json!({"taskStatus":wire(Some(&status)),"remote":execution_host.is_remote(),
        "local":summary_value(&local),"execution":summary_value(&execution),"terminal":terminal,
        "unfinished":unfinished,"startupRecovery":wire_document(&startup),"lifecycleClosure":wire_document(&closure)});
    captured["observedAt"] = json!(&observed_at);
    if request.watch_observation.is_some() {
        captured["watchControl"] = watch_control(&task);
    }
    if let Some(error)=startup_read_error {
        captured["readErrors"]=json!({"startupRecovery":error});
        return Ok(failed_observation(&task,captured,&observed_at,error.into()));
    }
    if !execution_observation.is_null(){
        captured["executionObservation"]=execution_observation;
        captured["receiptObservation"]=receipt_observation;
        captured["completionVerification"]=json!({"ready":legacy_ready,"basis":if legacy_ready{"legacy_revalidated"}else{"legacy_insufficient"},
            "naturalClosure":false,"runnerProofCreated":false,"taskId":text_id(task.get("_id")),"remote":execution_host.is_remote()});
    }
    if terminal && request.watch_observation.is_some() {
        let recheck: Result<(), Error> = async {
            if let Some(expected) = legacy_registry {
                if store.find_task(execution_host,doc! {"name":&request.name,"status":{"$ne":"deactive"}},false).await? != expected {
                    return Err("legacy execution registry changed before observation delivery".into());
                }
            } else if execution_host.is_remote() && startup.get_bool("required").unwrap_or(false)
                && ["completed", "completed_with_errors"].contains(&old_status) {
                // Existing completion needs only the same proof predicate read,
                // never another remote no-op write or completion timestamp.
                let binding=task.get_document("startupBinding").map_err(|e|e.to_string())?;
                let mut q=startup_query(&task,binding);
                q.insert("simulatorRuntime.startupRecovery",startup.get_document("proof").map_err(|e|e.to_string())?.clone());
                q.insert("simulatorRuntime.lifecycleClosure",closure.get_document("proof").map_err(|e|e.to_string())?.clone());
                q.insert("simulatorRuntime.consumer_run_id",binding.get("consumer_run_id").cloned().unwrap_or(Bson::Null));
                if store.find_task(execution_host,q,false).await?.is_none() {
                    return Err("SUPER execution startup proof changed before terminal CAS".into());
                }
            }
            Ok(())
        }.await;
        if let Err(error)=recheck { return Ok(failed_observation(&task,captured,&observed_at,error)); }
        // Observation protection also applies to an already-completed task.
        if let Err(error) = policy::assert_observation_closed(&task) {
            return Ok(failed_observation(&task, captured, &observed_at, error.into()));
        }
    }
    if terminal && !legacy_ready
        && !(old_status == "paused" && truthy(task.get("acceptedObservation")))
        && !checked_token(
            task.get("status"),
            &[
                "completed",
                "completed_with_errors",
                "deactive",
                "generating",
                "generation_unknown",
                "launch_unknown",
                "syncing",
                "deleting",
            ],
        )?
    {
        let reconciliation: Result<(), Error> = async {
        // Never hold EX/SH across item summaries or the Priority snapshot.
        // Acquiring EX alone is insufficient: a pause/new launch may have
        // completed while this exact original snapshot was being read.
        let _lock = MissionLocks::acquire(root, &[&request.name], false)?;
        confirm_status_snapshot(store, &query).await.map_err(|error| {
            if request.watch_observation.is_some() { Error {kind:error.kind, message:format!("mission changed during terminal reconciliation: {}",error.message)} }
            else { error }
        })?;
        policy::assert_observation_closed(&task)?;
        // The unchanged terminal-state gate above excludes legacy `fail`.
        // A cancelled item still retains the existing completed_with_errors task
        // outcome, but is no longer presented as an expression failure.
        let counts = crate::task_store::item_progress_counts(&execution.statuses);
        let failures = counts["error"] + counts["cancelled"];
        status = Bson::String(
            if failures > 0 {
                "completed_with_errors"
            } else {
                "completed"
            }
            .into(),
        );
        if startup.get_bool("required").unwrap_or(false) && execution_host.is_remote() {
            let binding = task
                .get_document("startupBinding")
                .map_err(|e| e.to_string())?;
            let proof = startup.get_document("proof").map_err(|e| e.to_string())?;
            let mut query = startup_query(&task, binding);
            query.insert("simulatorRuntime.startupRecovery", proof.clone());
            query.insert("simulatorRuntime.lifecycleClosure",
                closure.get_document("proof").map_err(|e| e.to_string())?.clone());
            query.insert(
                "simulatorRuntime.consumer_run_id",
                binding
                    .get("consumer_run_id")
                    .cloned()
                    .unwrap_or(Bson::Null),
            );
            if store
                .update_task(
                    execution_host,
                    query,
                    doc! {"$set":{"simulatorRuntime.startupRecovery":proof.clone()}},
                )
                .await?
                != 1
            {
                return Err("SUPER execution startup proof changed before terminal CAS".into());
            }
        }
        let command = "status-terminal-reconcile";
        let stamp = now();
        let event = doc! {"id":ObjectId::new().to_hex(),"command":command,"status":status.clone(),"at":&stamp,"detail":summary_document(&execution)?};
        if store.update_task(Host::Local,query,doc! {"$set":{"status":status.clone(),"completedAt":&stamp,"updatedAt":&stamp},"$push":{"events":event}}).await? != 1 {
            return Err(if request.watch_observation.is_some() {
                "mission changed during terminal reconciliation".into()
            } else { format!("active task not found: {}",request.name).into() });
        }
        Ok(())
        }.await;
        if let Err(error) = reconciliation { return Ok(failed_observation(&task, captured, &observed_at, error)); }
    } else if let Err(error) = confirm_status_snapshot(store, &query).await {
        return Ok(failed_observation(&task, captured, &observed_at, error));
    }
    let mut value = captured;
    value["taskStatus"] = wire(Some(&status));
    value["snapshotCurrent"] = json!(true);
    if ["completed", "completed_with_errors"].contains(&old_status) && !terminal {
        value["controlStatus"] = json!(old_status);
        value["lifecycleDiagnostic"] = json!({
            "reason":"control_terminal_execution_unconfirmed", "controlStatus":old_status,
            "unfinished":unfinished,
            "action":"inspect original run; no automatic restart or status rewrite"});
    }
    if status.as_str() == Some("paused") {
        let confirmed = policy::stop_confirmation_valid(&task, execution_host.is_remote());
        value["stopConfirmation"] = json!(if confirmed { "confirmed" } else { "unknown" });
        if !confirmed {
            value["taskStatus"] = json!("unknown");
            value["controlStatus"] = json!("paused");
            value["reason"] = json!("pause_unconfirmed");
        }
    }
    if truthy(task.get("acceptedObservation")) {
        value["acceptedObservation"] = wire(task.get("acceptedObservation"));
    }
    Ok(outcome("status", &request.name, true, value))
}
pub async fn status(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &StatusRequest,
) -> TaskOutcome {
    match status_inner(root, store, runtime, request).await {
        Ok(value) => value,
        Err(e) => finish("status", &request.name, Err(e)),
    }
}

async fn results_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &ResultsRequest,
) -> Result<Value, Error> {
    let task = task(store, &request.name, true).await?;
    crate::task_store::result_items_options(request.limit, request.offset).map_err(invalid)?;
    let config = if task.get_str("taskType").ok() == Some("priority")
        && !["startConfig", "mission_config", "config"]
            .iter()
            .any(|k| task.contains_key(*k))
    {
        runtime.priority_default_config(root, &task).await?
    } else {
        saved_start_config(&task, None).map_err(invalid)?
    };
    let tags = match config.get("tag") {
        None => None,
        Some(Bson::Document(tag)) => tag.get("tags"),
        _ => {
            return Err(Error {
                kind: "AttributeError",
                message: "stored tag value has no attribute 'get'".into(),
            })
        }
    };
    let contains = match tags {
        None => false,
        Some(Bson::Array(tags)) => tags.contains(&Bson::String(request.name.clone())),
        Some(Bson::String(tags)) => tags.contains(&request.name),
        Some(Bson::Document(tags)) => tags.contains_key(&request.name),
        _ => {
            return Err(Error {
                kind: "TypeError",
                message: "stored tag tags is not iterable".into(),
            })
        }
    };
    if !contains {
        return Err(invalid(
            "task name absent from stored tag mapping; cannot infer result ownership",
        ));
    }
    let profile = match task.get("auth_profile") {
        None => "user1",
        Some(Bson::String(s)) => s,
        _ => return Err(invalid("invalid auth_profile")),
    };
    let namespace = runtime.canonical_namespace(root, profile).await?;
    CanonicalNamespace::new(namespace.database.clone(), namespace.collection.clone())?;
    let mut items = store
        .result_items_page(host(&task), &request.name, request.limit, request.offset)
        .await?;
    if items.len() > request.limit as usize + 1 {
        return Err("task result page exceeded requested bound".into());
    }
    let has_more = items.len() > request.limit as usize;
    items.truncate(request.limit as usize);
    let mut ids = vec![];
    for item in &items {
        if truthy(item.get("alphaId")) {
            let id = item.get("alphaId").unwrap();
            checked_token(Some(id), &[])?; // dict.fromkeys hashes each truthy ID
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    let mut rows = if ids.is_empty() {
        vec![]
    } else {
        store.canonical_results(&namespace, &ids).await?
    };
    if rows.len() > ids.len() {
        return Err("canonical result page exceeded requested bound".into());
    }
    let missing: Vec<Value> = ids
        .iter()
        .filter(|id| !rows.iter().any(|row| row.get("id") == Some(*id)))
        .map(|id| wire(Some(id)))
        .collect();
    fn fitness(row: &Document) -> Result<Bson, Error> {
        let metrics = row.get("is");
        if !truthy(metrics) {
            return Ok(Bson::Double(f64::NEG_INFINITY));
        }
        let metrics = metrics.and_then(Bson::as_document).ok_or_else(|| Error {
            kind: "AttributeError",
            message: "stored result is value has no attribute 'get'".into(),
        })?;
        Ok(metrics
            .get("fitness")
            .filter(|v| truthy(Some(v)))
            .cloned()
            .unwrap_or(Bson::Double(f64::NEG_INFINITY)))
    }
    // Evaluate every key before sort as Python does; zero remains -infinity.
    let mut keyed = rows
        .into_iter()
        .map(|row| fitness(&row).map(|key| (key, row)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut sort_error = None;
    keyed.sort_by(|(a, _), (b, _)| {
        python_order(b, a).unwrap_or_else(|e| {
            sort_error = Some(e);
            std::cmp::Ordering::Equal
        })
    });
    if let Some(error) = sort_error {
        return Err(error);
    }
    rows = keyed.into_iter().map(|(_, row)| row).collect();
    let mapped: Vec<Value> = items
        .iter()
        .map(|item| {
            let mut value = wire_document(item);
            value.as_object_mut().unwrap().remove("_id");
            value["itemId"] = json!(text_id(item.get("_id")));
            value
        })
        .collect();
    let value = json!({"source":format!("{}.{}",namespace.database,namespace.collection),"sourceKind":"existing_incremental_ingestion","platformFetched":false,
        "taskStatus":wire(task.get("status")),"taskId":text_id(task.get("_id")),"taskDataRemoved":task.get_str("status").ok()==Some("deactive"),
        "offset":request.offset,"limit":request.limit,"pageUnit":"mission_items_by_id","pageItems":items.len(),"pageAlphaIds":ids.len(),"storedOnPage":rows.len(),
        "notInCanonicalStore":missing,"hasMore":has_more,"rows":rows.iter().map(wire_document).collect::<Vec<_>>(),"items":mapped,
        "coverage":"filtered_async_inventory_not_all_simulations","note":"No global count/top-K. Missing canonical rows may reflect ingestion delay/filters. After task cleanup use retained candidate IDs or the ordinary alpha library search."});
    if let Some(output) = &request.output {
        write_json(output, &value)?;
        return Ok(
            json!({"output":output.to_string_lossy(),"pageItems":items.len(),"storedOnPage":rows.len(),"returned":rows.len(),"hasMore":has_more,"platformFetched":false}),
        );
    }
    Ok(value)
}
pub async fn results(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &ResultsRequest,
) -> TaskOutcome {
    finish(
        "results",
        &request.name,
        results_inner(root, store, runtime, request).await,
    )
}

async fn collect_inner(
    root: &Path,
    store: &dyn TaskStore,
    request: &CollectRequest,
) -> Result<Value, Error> {
    let task = task(store, &request.name, false).await?;
    let mut rows = store
        .collect_result_items(host(&task), &request.name)
        .await?;
    fn key(row: &Document) -> (Bson, Bson) {
        (
            row.get("variantId")
                .cloned()
                .unwrap_or(Bson::String(String::new())),
            row.get("payloadHash")
                .or_else(|| row.get("variantHash"))
                .cloned()
                .unwrap_or(Bson::String(String::new())),
        )
    }
    let mut sort_error = None;
    rows.sort_by(|a, b| {
        let (a0, a1) = key(a);
        let (b0, b1) = key(b);
        python_order(&a0, &b0)
            .and_then(|o| {
                if o == std::cmp::Ordering::Equal {
                    python_order(&a1, &b1)
                } else {
                    Ok(o)
                }
            })
            .unwrap_or_else(|e| {
                sort_error = Some(e);
                std::cmp::Ordering::Equal
            })
    });
    if let Some(error) = sort_error {
        return Err(error);
    }
    let output = request.output.clone().unwrap_or_else(|| {
        root.join("reports/missions")
            .join(format!("{}.json", request.name))
    });
    write_json(
        &output,
        &json!({"schemaVersion":1,"task":request.name,"collectedAt":now(),"runId":wire(task.get("runId")),"total":rows.len(),"rows":rows.iter().map(wire_document).collect::<Vec<_>>()}),
    )?;
    Ok(
        json!({"output":output.to_string_lossy(),"total":rows.len(),"alphaIds":rows.iter().filter(|r| truthy(r.get("alphaId"))).count()}),
    )
}
pub async fn collect_results(
    root: &Path,
    store: &dyn TaskStore,
    request: &CollectRequest,
) -> TaskOutcome {
    finish(
        "collect-results",
        &request.name,
        collect_inner(root, store, request).await,
    )
}
async fn evidence_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &EvidenceRequest,
) -> Result<Value, Error> {
    let task = task(store, &request.name, false).await?;
    if request
        .expected_task_id
        .as_ref()
        .map(|s| s != &text_id(task.get("_id")))
        .unwrap_or(false)
    {
        return Err("research task identity changed; refusing archive/verification".into());
    }
    if request
        .expected_remote
        .map(|remote| remote != host(&task).is_remote())
        .unwrap_or(false)
    {
        return Err("research execution target changed".into());
    }
    let value = match request.action {
        EvidenceAction::VerifyRegistration => runtime.verify_registration(root, &task).await?,
        EvidenceAction::ArchiveResearch => {
            runtime
                .archive_research(root, &task, host(&task), &request.name)
                .await?
        }
    };
    if value.is_null() || value.as_object().map(|v| v.is_empty()).unwrap_or(false) {
        Ok(json!({"researchBinding":false}))
    } else if value.is_object() {
        Ok(value)
    } else {
        Err("original research owner returned non-object evidence".into())
    }
}
pub async fn dataset_evidence(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &EvidenceRequest,
) -> TaskOutcome {
    finish(
        request.action.command(),
        &request.name,
        evidence_inner(root, store, runtime, request).await,
    )
}
