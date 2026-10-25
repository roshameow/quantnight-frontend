//! Shared inspection/evidence coordinators. Status is a mutator, not a read.
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
#[derive(Debug, Clone)]
pub struct StatusRequest {
    pub name: String,
    pub remote: Option<bool>,
    pub inspect_items: bool,
}
impl StatusRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            remote: None,
            inspect_items: false,
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
                && truthy(b.get("consumer_run_id"))
                && truthy(b.get("launchAttemptId"))
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
    let mut fence = doc! {"startupBinding":binding.clone(),"taskType":"super","auth_profile":binding.get("auth_profile").cloned().unwrap_or(Bson::Null)};
    if !host(task).is_remote() {
        fence.insert("simulatorRuntime.startupRecovery", proof.clone());
        fence.insert(
            "simulatorRuntime.consumer_run_id",
            binding
                .get("consumer_run_id")
                .cloned()
                .unwrap_or(Bson::Null),
        );
    }
    Ok((doc! {"ready":true,"required":true,"proof":proof}, fence))
}
fn startup_query(task: &Document, binding: &Document) -> Document {
    doc! {"_id":task.get("_id").cloned().unwrap_or(Bson::Null),"name":task.get("name").cloned().unwrap_or(Bson::Null),
    "taskType":"super","auth_profile":binding.get("auth_profile").cloned().unwrap_or(Bson::Null),
    "isRemote":binding.get("remote").cloned().unwrap_or(Bson::Null),"launchAttemptId":binding.get("launchAttemptId").cloned().unwrap_or(Bson::Null),
    "startupBinding":binding.clone(),"status":{"$ne":"deactive"}}
}
async fn status_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn InspectionRuntime,
    request: &StatusRequest,
) -> Result<TaskOutcome, Error> {
    policy::validate_name(&request.name).map_err(invalid)?;
    let _lock = MissionLocks::acquire(root, &[&request.name], false)?;
    let task = task(store, &request.name, true).await?;
    let execution_host = host(&task);
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
        let ok = value
            .get("ok")
            .and_then(Value::as_bool)
            .ok_or("Priority snapshot missing boolean ok")?;
        let mut result = outcome("status", &request.name, ok, value);
        result.exit_code = 0;
        return Ok(result);
    }
    let (startup, fence) = startup_gate(store, &task).await?;
    let local = store.summary(Host::Local, &request.name).await?;
    let execution = store.summary(execution_host, &request.name).await?;
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
    let terminal =
        startup.get_bool("ready").unwrap_or(false) && execution.total > 0 && unfinished == 0;
    let mut status = task.get("status").cloned().unwrap_or(Bson::Null);
    let old_status = task.get_str("status").unwrap_or("");
    if terminal
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
        policy::assert_observation_closed(&task)?;
        let failures: u64 = ["permanent_error", "timeout_error", "cancelled", "error"]
            .iter()
            .map(|k| execution.statuses.get(*k).copied().unwrap_or(0))
            .sum();
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
        let mut query = policy::task_binding_query(&request.name, &task, command)?;
        // Observation is checked AND bound: a concurrent observation claim
        // cannot be erased by a terminal reconciliation.
        query.insert(
            "acceptedObservation",
            task.get("acceptedObservation")
                .cloned()
                .unwrap_or_else(|| Bson::Document(doc! {"$exists":false})),
        );
        query.extend(fence);
        let stamp = now();
        let event = doc! {"id":ObjectId::new().to_hex(),"command":command,"status":status.clone(),"at":&stamp,"detail":summary_document(&execution)?};
        if store.update_task(Host::Local,query,doc! {"$set":{"status":status.clone(),"completedAt":&stamp,"updatedAt":&stamp},"$push":{"events":event}}).await? != 1 {
            return Err(format!("active task not found: {}",request.name).into());
        }
    }
    let mut value = json!({"taskStatus":wire(Some(&status)),"remote":execution_host.is_remote(),"local":summary_value(&local),"execution":summary_value(&execution),"terminal":terminal,"unfinished":unfinished,"startupRecovery":wire_document(&startup)});
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
