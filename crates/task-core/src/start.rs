//! Shared start lifecycle. Professional validators, original launch scripts and
//! watcher remain owned by their original runtimes, never a taskctl recursion.
use async_trait::async_trait;
use bson::Document;
use std::path::Path;

#[derive(Clone, Debug)]
pub enum NotificationTarget {
    Session(String),
    Worker(String),
}
#[derive(Clone, Debug, Default)]
pub struct NotificationOptions {
    pub target: Option<NotificationTarget>,
    pub no_notify: bool,
    pub item_key: Option<String>,
    pub review_seconds: Option<i64>,
    pub watch_timeout: Option<i64>,
}
#[derive(Clone, Debug)]
pub struct StartRequest {
    pub name: String,
    pub config_json: Option<String>,
    pub remote: Option<bool>,
    pub expected_task_id: Option<String>,
    /// Captured actual execution side, separate from a requested destination.
    pub expected_remote: Option<bool>,
    pub expected_launch_attempt_id: Option<String>,
    pub notification: NotificationOptions,
}
impl StartRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config_json: None,
            remote: None,
            expected_task_id: None,
            expected_remote: None,
            expected_launch_attempt_id: None,
            notification: NotificationOptions::default(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct StartCommand {
    pub argv: Vec<String>,
    pub timeout_seconds: u64,
    pub binding: Document,
}
#[derive(Clone, Debug)]
pub struct LaunchOutput {
    pub stdout: String,
    pub stderr: String,
    /// Extracted by the original inspector owner, never inferred from text here.
    pub observed_contexts: Vec<Document>,
}
#[async_trait]
pub trait StartRuntime: Send + Sync {
    async fn inspect_execution(&self, _root:&Path, _store:&dyn crate::task_store::TaskStore,
        _task:&Document, _remote:bool, _requested:Option<&Document>, _remaining:std::time::Duration)
        -> Result<Option<serde_json::Value>,String> { Ok(None) }
    async fn verify_stopped(&self,_root:&Path,_task:&Document,_remaining:std::time::Duration)
        -> Result<Option<crate::pause::StopOutput>,String> {Ok(None)}
    async fn stop_owned_residue(&self,_root:&Path,_task:&Document,_remaining:std::time::Duration)
        -> Result<crate::pause::StopOutput,String> {Err("original owned-residue stop owner unavailable".into())}
    async fn priority_config(
        &self,
        root: &Path,
        task: &Document,
        selected: Option<Document>,
    ) -> Result<Document, String>;
    async fn validate_super_config(&self, root: &Path, config: &Document) -> Result<(), String>;
    async fn bind_regular_source(
        &self,
        root: &Path,
        task: &Document,
        config: &Document,
    ) -> Result<Document, String>;
    async fn verify_priority_extension(
        &self,
        root: &Path,
        task: &Document,
        local_count: u64,
        remote_count: u64,
    ) -> Result<Document, String>;
    async fn continuation(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
    ) -> Result<Option<Document>, String>;
    async fn preflight_notification(&self, root: &Path, spec: &Document) -> Result<(), String>;
    async fn assert_consumer_absent(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
    ) -> Result<(), String>;
    async fn validate_execution_binding(
        &self,
        root: &Path,
        binding: &Document,
        remote: bool,
    ) -> Result<(), String>;
    async fn launch(&self, root: &Path, command: &StartCommand) -> Result<LaunchOutput, String>;
    async fn attach_watcher(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
    ) -> Result<Document, String>;
}

use crate::{
    cleanup_sync::{digest, failure, outcome, summary_value, wire_document, TaskOutcome},
    create_generate::initial_config,
    lifecycle_policy,
    mission_lock::MissionLocks,
    task_store::{id_projection, CollectionSummary, Host, TaskStore},
};
use bson::{doc, oid::ObjectId, Bson};
use serde_json::{json, Value};

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}
fn nonce() -> String {
    bson::Uuid::new().to_string().replace('-', "")
}
fn tail(s: &str, n: usize) -> String {
    let start = s.char_indices().rev().nth(n - 1).map_or(0, |(i, _)| i);
    s[start..].into()
}
fn integer(v: Option<&Bson>) -> Option<i64> {
    match v {
        Some(Bson::Int32(n)) => Some(i64::from(*n)),
        Some(Bson::Int64(n)) => Some(*n),
        _ => None,
    }
}
fn boolean(task: &Document, key: &str) -> Result<bool, String> {
    match task.get(key) {
        None | Some(Bson::Null) => Ok(false),
        Some(Bson::Boolean(v)) => Ok(*v),
        _ => Err(format!("invalid boolean task field: {key}")),
    }
}
fn kind(task: &Document) -> Result<&str, String> {
    match task.get("taskType") {
        None => Ok("regular"),
        Some(Bson::String(v)) if ["regular", "super", "priority"].contains(&v.as_str()) => Ok(v),
        _ => Err("invalid task type".into()),
    }
}
fn profile(task: &Document) -> Result<&str, String> {
    match task.get("auth_profile") {
        None => Ok("user1"),
        Some(Bson::String(v)) => Ok(v),
        _ => Err("invalid task auth_profile".into()),
    }
}
fn task_id(task: &Document) -> Result<ObjectId, String> {
    task.get_object_id("_id")
        .map_err(|_| "task must retain its original ObjectId".into())
}
fn required_str<'a>(task: &'a Document, key: &str) -> Result<&'a str, String> {
    task.get_str(key)
        .map_err(|_| format!("missing/invalid {key}"))
}

/// No Extended JSON fallback or nonfinite->null coercion in launch configs.
pub fn strict_json(value: &Bson) -> Result<Value, String> {
    Ok(match value {
        Bson::Null | Bson::Undefined => Value::Null,
        Bson::Boolean(v) => json!(v),
        Bson::String(v) | Bson::Symbol(v) | Bson::JavaScriptCode(v) => json!(v),
        Bson::JavaScriptCodeWithScope(v) => json!(v.code),
        Bson::Int32(v) => json!(v),
        Bson::Int64(v) => json!(v),
        Bson::Double(v) => {
            Value::Number(serde_json::Number::from_f64(*v).ok_or("non-finite start config number")?)
        }
        Bson::Array(v) => Value::Array(v.iter().map(strict_json).collect::<Result<_, _>>()?),
        Bson::Document(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), strict_json(v)?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err("start config contains non-JSON BSON value".into()),
    })
}
fn from_json(value: Value) -> Result<Bson, String> {
    Ok(match value {
        Value::Null => Bson::Null,
        Value::Bool(v) => Bson::Boolean(v),
        Value::String(v) => Bson::String(v),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                i32::try_from(v).map(Bson::Int32).unwrap_or(Bson::Int64(v))
            } else if v.is_u64() {
                return Err("start config integer exceeds BSON range".into());
            } else {
                Bson::Double(
                    v.as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("non-finite start config number")?,
                )
            }
        }
        Value::Array(v) => Bson::Array(v.into_iter().map(from_json).collect::<Result<_, _>>()?),
        Value::Object(v) => Bson::Document(
            v.into_iter()
                .map(|(k, v)| Ok((k, from_json(v)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}
fn parse_json(s: &str) -> Result<Value, String> {
    // serde_json may otherwise round oversized integer tokens to f64. Such
    // integers cannot be persisted in BSON: reject explicitly, never corrupt.
    let bytes = s.as_bytes();
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    let mut integer_negative_zero = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if quoted {
            if escaped {
                escaped = false
            } else if c == b'\\' {
                escaped = true
            } else if c == b'"' {
                quoted = false
            };
            i += 1;
            continue;
        }
        if c == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if c == b'-' || c.is_ascii_digit() {
            let begin = i;
            i += 1;
            while i < bytes.len() && !b",]} \t\r\n".contains(&bytes[i]) {
                i += 1;
            }
            let number = &s[begin..i];
            if number == "-0" {
                integer_negative_zero.push((begin, i));
            }
            if !number.contains(['.', 'e', 'E']) && number.parse::<i64>().is_err() {
                return Err("start config integer invalid or exceeds BSON range".into());
            }
        } else {
            i += 1;
        }
    }
    let parsed = serde_json::from_str(s).map_err(|e| format!("invalid start config JSON: {e}"))?;
    if integer_negative_zero.is_empty() {
        return Ok(parsed);
    }
    // Python json.loads('-0') is integer 0, whereas serde_json retains f64
    // negative zero. Normalize ONLY these valid integer tokens, after original
    // JSON validation; strings, -0.0 and -0e0 retain their original semantics.
    let mut normalized = s.to_owned();
    for (begin, end) in integer_negative_zero.into_iter().rev() {
        normalized.replace_range(begin..end, "0");
    }
    serde_json::from_str(&normalized).map_err(|e| format!("invalid start config JSON: {e}"))
}
pub(crate) fn config_document(value: Value) -> Result<Document, String> {
    match from_json(value)? {
        Bson::Document(d) => Ok(d),
        _ => Err("start config must be a JSON object".into()),
    }
}
fn normalize_config(config: &Document) -> Result<Document, String> {
    config_document(strict_json(&Bson::Document(config.clone()))?)
}
fn saved_value(value: &Bson) -> Result<Document, String> {
    match value {
        Bson::String(s) | Bson::Symbol(s) | Bson::JavaScriptCode(s) => {
            config_document(parse_json(s)?)
        }
        Bson::JavaScriptCodeWithScope(s) => config_document(parse_json(&s.code)?),
        v => config_document(strict_json(v)?),
    }
}

/// Ordinary/SUPER original key-presence priority: null/invalid saved values do
/// not fall through to older configs. No defaults are merged into user config.
pub fn saved_start_config(task: &Document, explicit: Option<&str>) -> Result<Document, String> {
    if let Some(s) = explicit {
        return config_document(parse_json(s)?);
    }
    for key in ["startConfig", "mission_config", "config"] {
        if let Some(v) = task.get(key) {
            return saved_value(v);
        }
    }
    let kind = kind(task)?;
    let concurrency = match kind {
        "regular" => 8,
        "super" => 3,
        _ => return Err("Priority defaults belong to original priority owner".into()),
    };
    initial_config(required_str(task, "name")?, kind, concurrency)
}
/// None requests the original Priority owner default. Explicit config wins,
/// then non-null mission_config/startConfig; legacy `config` is NOT a fallback.
pub fn priority_config_input(
    task: &Document,
    explicit: Option<&str>,
) -> Result<Option<Document>, String> {
    if let Some(s) = explicit {
        let value = parse_json(s)?;
        return Ok(Some(if let Value::String(s) = value {
            config_document(parse_json(&s)?)?
        } else {
            config_document(value)?
        }));
    }
    for key in ["mission_config", "startConfig"] {
        if let Some(v) = task.get(key) {
            if v != &Bson::Null {
                return saved_value(v).map(Some);
            }
        }
    }
    Ok(None)
}
pub(crate) fn preserves(before: &Document, after: &Document) -> Result<(), String> {
    if before.iter().any(|(k, v)| after.get(k) != Some(v)) {
        return Err("original validator changed or dropped supplied configuration".into());
    }
    Ok(())
}
async fn active(store: &dyn TaskStore, name: &str) -> Result<Option<Document>, String> {
    store
        .find_task(
            Host::Local,
            doc! {"name":name,"status":{"$ne":"deactive"}},
            true,
        )
        .await
}
fn identity(task: &Document, request: &StartRequest) -> Result<(), String> {
    let id = task_id(task)?;
    task.get_str("status").map_err(|_| "invalid task status")?;
    if task.get_str("name").ok() != Some(&request.name) {
        return Err("task name changed before launch".into());
    }
    if let Some(expected) = &request.expected_task_id {
        if ObjectId::parse_str(expected).map_err(|_| "invalid expected task ObjectId")? != id {
            return Err("task identity changed before launch".into());
        }
    }
    if let Some(expected) = &request.expected_launch_attempt_id {
        let actual = match task.get("launchAttemptId") {
            None | Some(Bson::Null) => "",
            Some(Bson::String(v)) => v,
            _ => return Err("invalid launch identity".into()),
        };
        if actual != expected {
            return Err("launch identity changed before mutation".into());
        }
    }
    lifecycle_policy::assert_observation_closed(task)
}
fn control_filter(task: &Document, command: &str) -> Result<Document, String> {
    let mut query =
        lifecycle_policy::task_binding_query(required_str(task, "name")?, task, command)?;
    for key in [
        "generation",
        "targetRemote",
        "startupBinding",
        "acceptedObservation",
        "taskType",
        "auth_profile",
        "runId",
        "startConfig",
        "mission_config",
        "config",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "stopConfirmationBasis",
        "launchAttemptAt",
        "launchSettledAttemptId",
    ] {
        query.insert(
            key,
            task.get(key)
                .cloned()
                .unwrap_or_else(|| doc! {"$exists":false}.into()),
        );
    }
    Ok(query)
}
async fn unchanged(store: &dyn TaskStore, expected: &Document) -> Result<(), String> {
    let current = active(store, required_str(expected, "name")?)
        .await?
        .ok_or("task disappeared during start preflight")?;
    if control_filter(expected, "start")?
        .keys()
        .any(|key| expected.get(key) != current.get(key))
    {
        return Err("task identity/state changed during start preflight".into());
    }
    Ok(())
}
async fn update(
    store: &dyn TaskStore,
    task: &Document,
    command: &str,
    mut fields: Document,
    detail: Document,
) -> Result<Document, String> {
    let stamp = now();
    fields.insert("updatedAt", &stamp);
    let event = doc! {"id":nonce(),"command":command,"status":fields.get_str("status").unwrap_or("ok"),"at":stamp,"detail":detail};
    let count = store
        .update_task(
            Host::Local,
            control_filter(task, command)?,
            doc! {"$set":&fields,"$push":{"events":&event}},
        )
        .await?;
    if count != 1 {
        return Err(
            "start control CAS not confirmed; preserve original task/launch, do not restart".into(),
        );
    }
    let mut saved = task.clone();
    saved.extend(fields);
    match saved.get_mut("events") {
        Some(Bson::Array(events)) => events.push(event.into()),
        None => {
            saved.insert("events", vec![Bson::Document(event)]);
        }
        _ => return Err("invalid task event history".into()),
    }
    Ok(saved)
}
fn launch_binding(task: &Document, remote: bool) -> Result<Document, String> {
    Ok(doc! {"taskId":task_id(task)?.to_hex(),"remote":remote,
    "launchAttemptId":task.get("launchAttemptId").cloned().unwrap_or(Bson::Null),"tmuxContext":task.get("tmuxContext").cloned().unwrap_or(Bson::Null)})
}
fn same_claim(expected: &Document, current: &Document) -> Result<(), String> {
    for key in [
        "_id",
        "name",
        "status",
        "isRemote",
        "updatedAt",
        "launchAttemptId",
        "launchAttemptAt",
        "taskType",
        "auth_profile",
        "runId",
        "startConfig",
        "mission_config",
        "config",
        "acceptedObservation",
        "generation",
        "startupBinding",
        "tmuxContext",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "stopConfirmationBasis",
    ] {
        if expected.get(key) != current.get(key) {
            return Err(format!("launch claim changed: {key}; do not restart"));
        }
    }
    lifecycle_policy::assert_observation_closed(current)
}
async fn reread_claim(store: &dyn TaskStore, expected: &Document) -> Result<Document, String> {
    let current = active(store, required_str(expected, "name")?)
        .await?
        .ok_or("launch task disappeared")?;
    same_claim(expected, &current)?;
    Ok(current)
}

/// Original SUPER execution registry, not another local control task. Never
/// writes item rows; do not move this registration after the launcher.
pub async fn register_super_runtime(
    store: &dyn TaskStore,
    task: &Document,
    remote: bool,
) -> Result<(), String> {
    let host = Host::from_remote(remote);
    let name = required_str(task, "name")?;
    let id = task_id(task)?;
    if store
        .find_task(
            host,
            doc! {"name":name,"_id":{"$ne":id},"status":{"$ne":"deactive"}},
            false,
        )
        .await?
        .is_some()
    {
        return Err("SUPER execution runtime same-name task ID conflict".into());
    }
    let mut existing = store
        .find_task(host, doc! {"name":name,"status":{"$ne":"deactive"}}, false)
        .await?;
    if existing.is_none() {
        if !remote {
            return Err("original SUPER control task missing".into());
        }
        if store
            .find_task(host, doc! {"_id":id}, false)
            .await?
            .is_some()
        {
            return Err("SUPER execution runtime task ID conflict".into());
        }
        let seed: Document = [
            "_id",
            "name",
            "taskType",
            "auth_profile",
            "isRemote",
            "launchAttemptId",
            "startupBinding",
        ]
        .iter()
        .filter_map(|key| task.get(*key).map(|v| ((*key).into(), v.clone())))
        .collect();
        if store.insert_task(host, seed).await? != Bson::ObjectId(id) {
            return Err("SUPER runtime insert identity not confirmed".into());
        }
        existing = store.find_task(host, doc! {"_id":id}, false).await?;
    }
    let existing = existing.ok_or("SUPER execution runtime missing after registration")?;
    if ["_id", "name", "taskType", "auth_profile", "isRemote"]
        .iter()
        .any(|key| existing.get(*key) != task.get(*key))
    {
        return Err("SUPER execution runtime identity/profile/side conflict".into());
    }
    let query: Document = [
        "_id",
        "name",
        "taskType",
        "auth_profile",
        "isRemote",
        "launchAttemptId",
        "startupBinding",
        "simulatorRuntime",
    ]
    .iter()
    .map(|key| {
        (
            (*key).into(),
            existing
                .get(*key)
                .cloned()
                .unwrap_or_else(|| doc! {"$exists":false}.into()),
        )
    })
    .collect();
    let startup = task
        .get_document("startupBinding")
        .map_err(|_| "SUPER startup binding unavailable")?;
    let mut pending = startup.clone();
    pending.insert("phase", "pending");
    let fields = doc! {"launchAttemptId":required_str(task,"launchAttemptId")?,"startupBinding":startup,"simulatorRuntime":{"startupRecovery":pending}};
    if store.update_task(host, query, doc! {"$set":fields}).await? != 1 {
        return Err("SUPER runtime registration CAS not confirmed".into());
    }
    Ok(())
}

async fn source_binding(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn StartRuntime,
    task: &Document,
    config: &Document,
) -> Result<Document, String> {
    let name = required_str(task, "name")?;
    let rows = store
        .find_tasks(Host::Local, doc! {"name":name,"status":{"$ne":"deactive"}})
        .await?;
    if rows.len() != 1
        || control_filter(task, "start")?
            .keys()
            .any(|key| rows[0].get(key) != task.get(key))
    {
        return Err("regular source task missing, ambiguous or changed".into());
    }
    unchanged(store, task).await?;
    let bound = normalize_config(&runtime.bind_regular_source(root, task, config).await?)?;
    preserves(config, &bound)?;
    if bound.get_str("source_task_id").ok() != Some(task_id(task)?.to_hex().as_str())
        || bound.get_bool("source_remote") != Ok(true)
    {
        return Err("original source validator returned wrong task identity/host".into());
    }
    Ok(bound)
}
fn extension_proof(
    proof: &Document,
    task: &Document,
    local: u64,
    remote: u64,
) -> Result<(), String> {
    if proof.get_str("reason") != Ok("verified_priority_extension")
        || proof.get_str("sourceTaskId").ok() != Some(task_id(task)?.to_hex().as_str())
        || proof.get_bool("sourceRemote") != Ok(true)
    {
        return Err("Priority extension proof identity/host mismatch".into());
    }
    for (key, expected) in [
        ("localBaseCount", local),
        ("remoteBaseCount", local),
        ("remoteTotalCount", remote),
        ("remotePriorityExtensionCount", remote - local),
    ] {
        if integer(proof.get(key)).and_then(|v| u64::try_from(v).ok()) != Some(expected) {
            return Err(format!("Priority extension proof count mismatch: {key}"));
        }
    }
    Ok(())
}
pub(crate) fn notification_spec(
    task: &Document,
    options: &NotificationOptions,
) -> Result<Option<Document>, String> {
    if options.no_notify {
        return Ok(None);
    }
    let empty = Document::new();
    let previous = match task.get("agentNotification") {
        None | Some(Bson::Null) | Some(Bson::Boolean(false)) => &empty,
        Some(Bson::Document(v)) => v,
        _ => return Err("invalid saved notification metadata".into()),
    };
    let target = match &options.target {
        Some(NotificationTarget::Session(id)) if !id.is_empty() => Some(doc! {"sessionId":id}),
        Some(NotificationTarget::Worker(id)) if !id.is_empty() => Some(doc! {"taskId":id}),
        _ => match previous.get("target") {
            None | Some(Bson::Null) => None,
            Some(Bson::Document(v)) if v.is_empty() => None,
            Some(Bson::Document(v)) => Some(v.clone()),
            _ => return Err("invalid saved notification target".into()),
        },
    };
    let Some(target) = target else {
        return Ok(None);
    };
    if kind(task)? == "priority" {
        return Err("mission watcher requires a task with its own execution collection".into());
    }
    let key = options
        .item_key
        .as_ref()
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            if let Ok(worker) = target.get_str("taskId") {
                format!("worker:{worker}")
            } else {
                format!("mission:{}", task.get_str("name").unwrap_or(""))
            }
        });
    let number = |explicit: Option<i64>, field: &str, default: i64| -> Result<i64, String> {
        let n = match explicit {
            Some(n) => n,
            None => match previous.get(field) {
                None => default,
                Some(v) => integer(Some(v)).ok_or_else(|| format!("invalid {field}"))?,
            },
        };
        if !(60..=86400).contains(&n) {
            return Err(format!("{field} must be 60..86400 seconds"));
        }
        Ok(n)
    };
    Ok(Some(
        doc! {"target":target,"itemKey":key,"timeoutSeconds":number(options.watch_timeout,"timeoutSeconds",7200)?,"reviewSeconds":number(options.review_seconds,"reviewSeconds",1800)?},
    ))
}
pub(crate) fn notification_receipt(
    receipt: &Document,
    task: &Document,
    remote: bool,
    spec: &Document,
) -> Result<(), String> {
    // Start always owns a fresh, persisted launch attempt. Watch's legacy or
    // generation expectation is computed by the ORIGINAL watcher owner.
    let launch = task.get_str("launchAttemptId").map_err(|_| "new launch identity missing")?;
    notification_receipt_bound(receipt, task, remote, spec, &doc! {"launchId": launch})
}

/// Shared receiver check; `expected` comes from the original watcher binding
/// function for standalone watch, not from guessing a same-name task or from
/// the receipt being validated. No second generation/legacy parser lives here.
pub(crate) fn notification_receipt_bound(
    receipt: &Document, task: &Document, remote: bool, spec: &Document, expected: &Document,
) -> Result<(), String> {
    let launch = expected.get_str("launchId").map_err(|_| "expected watcher launch binding missing")?;
    if launch.is_empty() { return Err("expected watcher launch binding empty".into()); }
    let mission = task_id(task)?.to_hex();
    if expected.get("missionId").is_some_and(|v| v.as_str() != Some(mission.as_str()))
        || expected.get("remote").is_some_and(|v| v.as_bool() != Some(remote)) {
        return Err("original watcher expectation task/host mismatch".into());
    }
    let generation = expected.get("generationId").filter(|v| **v != Bson::Null);
    let phase = expected.get("generationPhase").filter(|v| **v != Bson::Null);
    if generation.is_some() != phase.is_some()
        || generation.is_some_and(|v| v.as_str().map_or(true, str::is_empty))
        || phase.is_some_and(|v| v.as_str().map_or(true, str::is_empty)) {
        return Err("original watcher expectation generation binding incomplete".into());
    }
    for key in ["generationId", "generationPhase"] {
        if receipt.get(key).unwrap_or(&Bson::Null) != expected.get(key).unwrap_or(&Bson::Null) {
            return Err("watcher receipt generation binding mismatch".into());
        }
    }
    if receipt.get_str("missionId").ok() != Some(task_id(task)?.to_hex().as_str())
        || receipt.get_str("launchId").ok() != Some(launch)
        || receipt.get_bool("remote") != Ok(remote)
        || receipt.get("target") != spec.get("target")
        || receipt.get("itemKey") != spec.get("itemKey")
    {
        return Err("watcher receipt task/launch/host/target mismatch".into());
    }
    if integer(receipt.get("producerPid")).is_none_or(|v| v <= 1)
        || receipt
            .get_str("producerMarkerPath")
            .map_or(true, |s| s.is_empty())
        || receipt
            .get_str("checkpointPath")
            .map_or(true, |s| s.is_empty())
    {
        return Err("watcher receipt lacks verified producer/marker/checkpoint".into());
    }
    let worker = spec
        .get_document("target")
        .map_err(|_| "notification target unavailable")?
        .contains_key("taskId");
    if receipt.get_bool("requiresLease") != Ok(worker) {
        return Err("watcher receipt lease route mismatch".into());
    }
    Ok(())
}

/// Only a new Priority producer may choose its recorded destination on first
/// launch. This is positive create-event evidence, not status/pane inference.
fn pristine_priority_target(task: &Document, remote: bool) -> Result<bool, String> {
    if task.get_str("taskType").ok() != Some("priority") || task.get_str("status").ok() != Some("pending")
        || task.get_bool("isRemote").ok() != Some(false) || task.get_bool("targetRemote").ok() != Some(remote) {
        return Ok(false);
    }
    let name = task.get_str("name").map_err(|_| "Priority name missing")?;
    if task.get_str("runId").ok() != Some(format!("mission:{name}").as_str()) { return Ok(false); }
    for key in ["launchAttemptId","launchAttemptAt","startedAt","pausedAt","completedAt","consumer_run_id",
        "consumerPid","heartbeatAt","simulatorRuntime","startupBinding","startPreflight","stopConfirmedAt",
        "tmuxContext","launchHistory","launchAttempts","launchReceipt","startReceipt","acceptedObservation","generation"] {
        if task.get(key).is_some_and(|value| *value != Bson::Null) { return Ok(false); }
    }
    let Some(created) = task.get_str("createdAt").ok().and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()) else { return Ok(false); };
    let Some(events) = task.get_array("events").ok().filter(|events| !events.is_empty()) else { return Ok(false); };
    let mut previous = created;
    let current = chrono::Utc::now();
    for (index, event) in events.iter().enumerate() {
        let Some(event) = event.as_document() else { return Ok(false); };
        let Some(stamp) = event.get_str("at").ok().and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()) else { return Ok(false); };
        if event.get_str("command").ok() != Some("create")
            || event.get_str("status").ok() != Some(if index == 0 {"pending"} else {"idempotent"})
            || event.get_str("id").map_or(true, str::is_empty)
            || event.get_document("detail").map_or(true, |value| !value.is_empty())
            || stamp < previous || stamp > current { return Ok(false); }
        previous = stamp;
    }
    Ok(true)
}

async fn resolve_priority_start_config(root:&Path,runtime:&dyn StartRuntime,task:&Document,explicit:Option<&str>)->Result<Document,String>{
    let selected=priority_config_input(task,explicit)?;
    let validated=normalize_config(&runtime.priority_config(root,task,selected.clone()).await?)?;
    if let Some(selected)=&selected{preserves(selected,&validated)?;}
    if validated.get_str("auth_profile")!=Ok(profile(task)?){return Err("Priority validator returned conflicting auth profile".into());}
    Ok(validated)
}

// Missing/null is explicit observed absence; malformed evidence is not legacy.
fn observed_launch(task:&Document)->Result<Value,String> {
    match task.get("launchAttemptId") {
        None|Some(Bson::Null)=>Ok(Value::Null),
        Some(Bson::String(value)) if !value.trim().is_empty()=>Ok(json!(value)),
        _=>Err("original launch identity malformed".into()),
    }
}

// Known pending launch state survives explicit pause; missing optional evidence
// on other legacy tasks is NOT a global new admission requirement.
pub(crate) fn launch_attempt_unsettled(task:&Document)->bool {
    (task.get_str("status").ok()==Some("launch_unknown") || task.contains_key("launchSettledAttemptId")) &&
        (task.get_str("launchAttemptId").map_or(true,|v|v.trim().is_empty()) || task.get("launchSettledAttemptId")!=task.get("launchAttemptId"))
}

struct OwnedResidueStop<'a>{runtime:&'a dyn StartRuntime,observation:&'a Value}
#[async_trait]
impl crate::pause::PauseRuntime for OwnedResidueStop<'_> {
    async fn inspect_before_stop(&self,_:&Path,_:&dyn TaskStore,_:&Document,_:std::time::Duration)->Result<Option<Value>,String>{Ok(Some(self.observation.clone()))}
    async fn confirm_stopped_scope(&self,root:&Path,store:&dyn TaskStore,task:&Document,remaining:std::time::Duration)->Result<(),String>{
        let proof=self.runtime.inspect_execution(root,store,task,task.get_bool("isRemote").unwrap_or(false),None,remaining).await?;
        if proof.as_ref().and_then(|p|p["state"].as_str())!=Some("session_absent"){return Err("original residue processes remain live/unconfirmed after stop".into());}
        Ok(())
    }
    async fn stop(&self,_:&Path,_:&Document,_:bool)->Result<crate::pause::StopOutput,String>{Err("residue stop requires remaining original deadline".into())}
    async fn stop_bounded(&self,root:&Path,task:&Document,_:bool,remaining:std::time::Duration)->Result<crate::pause::StopOutput,String>{
        self.runtime.stop_owned_residue(root,task,remaining).await
    }
    async fn verify_stopped(&self,root:&Path,task:&Document,remaining:std::time::Duration)->Result<Option<crate::pause::StopOutput>,String>{
        self.runtime.verify_stopped(root,task,remaining).await
    }
}
pub async fn start(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn StartRuntime,
    request: &StartRequest,
) -> TaskOutcome {
    start_with_deadline(root,store,runtime,request,tokio::time::Instant::now()+std::time::Duration::from_secs(300)).await
}
pub(crate) async fn start_with_deadline(root:&Path,store:&dyn TaskStore,runtime:&dyn StartRuntime,
    request:&StartRequest,deadline:tokio::time::Instant)->TaskOutcome {
    if deadline<=tokio::time::Instant::now(){return failure("start",&request.name,"original start deadline exhausted before mutation");}
    if let Err(e) = lifecycle_policy::validate_name(&request.name) {
        return failure("start", &request.name, e);
    }
    let _lock = match MissionLocks::acquire(root, &[&request.name], false) {
        Ok(v) => v,
        Err(e) => return failure("start", &request.name, e),
    };
    match tokio::time::timeout_at(deadline,start_locked(root, store, runtime, request, &_lock, deadline)).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => failure("start", &request.name, error),
        Err(_) => failure("start", &request.name, "original start total deadline exhausted; no retry or additional launch"),
    }
}
async fn start_locked(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn StartRuntime,
    request: &StartRequest,
    guard: &MissionLocks,
    deadline: tokio::time::Instant,
) -> Result<TaskOutcome, String> {
    let mut task = active(store, &request.name)
        .await?
        .ok_or("task not found")?;
    identity(&task, request)?;
    if matches!(
        task.get_str("status").ok(),
        Some("generating" | "generation_unknown" | "syncing" | "deleting")
    ) {
        return Err(
            "generation/launch is in progress or unconfirmed; reconcile before start".into(),
        );
    }
    observed_launch(&task)?;
    let launch_unresolved = launch_attempt_unsettled(&task);
    let original_kind = kind(&task)?.to_string();
    profile(&task)?; // Existing business profile requirement; no login/config guess.
    let remote = match request.remote {
        Some(v) => v,
        None => {
            boolean(&task, "isRemote")? || (original_kind == "priority" && boolean(&task, "targetRemote")?)
        }
    };
    let actual_remote = boolean(&task, "isRemote")?;
    if request.expected_remote.is_some_and(|expected| expected != actual_remote) {
        return Err("original execution host changed before locked start".into());
    }
    if remote != actual_remote {
        if original_kind != "priority" || !pristine_priority_target(&task, remote)? {
            return Err("start cannot migrate execution host or discard prior receipt ownership; use the original initial sync or resume on the recorded host".into());
        }
        // A creation-time producer destination is not permission to leave an
        // old same-name controller running on the other host.
        runtime.assert_consumer_absent(root, &task, actual_remote).await?;
    }
    let requested = if request.config_json.is_some() {
        if original_kind == "priority" { Some(resolve_priority_start_config(root,runtime,&task,request.config_json.as_deref()).await?) }
        else { Some(saved_start_config(&task,request.config_json.as_deref())?) }
    } else { None };
    if let Some(config)=&requested {
        for (key,expected) in [
            ("mission_tag",Bson::String(request.name.clone())),
            ("dist",Bson::String(if remote{"remote"}else{"local"}.into())),
            ("auth_profile",Bson::String(profile(&task)?.into())),
            ("source_task_id",Bson::String(task_id(&task)?.to_hex())),
            ("source_remote",Bson::Boolean(remote)),
        ] {
            if config.get(key).is_some_and(|actual|*actual!=expected){return Err(format!("original source/task identity config conflicts: {key}"));}
        }
        if let Some(Bson::Document(binding))=config.get("startup_binding"){
            for (key,expected) in [("taskId",Bson::String(task_id(&task)?.to_hex())),("remote",Bson::Boolean(remote)),("auth_profile",Bson::String(profile(&task)?.into()))]{
                if binding.get(key).is_some_and(|actual|*actual!=expected){return Err(format!("original startup identity config conflicts: {key}"));}
            }
        }
    }
    let remaining=deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero(){return Err("start deadline exhausted before original execution observation".into());}
    let observation=runtime.inspect_execution(root,store,&task,remote,requested.as_ref(),remaining).await?;
    if let Some(observed)=observation {
        unchanged(store,&task).await?;
        match observed.get("state").and_then(Value::as_str) {
            Some("live") if observed["identityVerified"]==true && observed["consumerRunning"]==true => {
                let comparison=if request.config_json.is_none(){"not_requested"}else{observed["configComparison"].as_str().unwrap_or("unverified")};
                let satisfied=request.config_json.is_none()||comparison=="equal";
                return Ok(outcome("start",&request.name,satisfied,json!({"taskId":task_id(&task)?.to_hex(),"remote":remote,
                    "launchAttemptId":observed_launch(&task)?,"taskType":original_kind,"authProfile":profile(&task)?,"operationSettled":!launch_unresolved,"outcome":"already_running",
                    "startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":comparison,
                    "requestSatisfied":satisfied,"executionObservation":observed,"controlStatus":task.get_str("status").unwrap_or(""),
                    "controlReconciled":false,"error":if satisfied {Value::Null}else{json!("original consumer remains running; requested config not applied")}})));
            }
            Some("owned_residue") if observed["identityVerified"]==true && observed["ownedResidue"]==true => {
                if launch_unresolved {return Err("original launcher still unresolved; no automatic residue cleanup".into());}
                let stopped=crate::pause::pause_locked(root,store,&OwnedResidueStop{runtime,observation:&observed},&request.name,
                    crate::pause::PauseOptions{expected_task_id:Some(&task_id(&task)?.to_hex()),
                        expected_launch_attempt_id:task.get_str("launchAttemptId").ok(),expected_remote:Some(remote),observer:None,..Default::default()},
                    guard,deadline.min(tokio::time::Instant::now()+std::time::Duration::from_secs(60)),crate::pause::StopIntent::StopAllowed).await;
                if stopped.exit_code!=0 {return Err(format!("owned residue stop unconfirmed: {}",stopped.value));}
                let prior=task.clone();
                task=active(store,&request.name).await?.ok_or("task disappeared after original residue stop")?;
                if ["_id","launchAttemptId","startupBinding","isRemote","taskType","auth_profile","generation","acceptedObservation","runId","startConfig","mission_config","config","templatefile"].iter()
                    .any(|key|task.get(*key)!=prior.get(*key))
                    ||task.get_str("updatedAt").ok()!=stopped.value["controlRevision"].as_str()
                    ||task.get_str("status").ok()!=Some("paused"){
                    return Err("original residue stop snapshot replaced before launch; successor not adopted".into());
                }
                identity(&task,request)?;
                let remaining=deadline.saturating_duration_since(tokio::time::Instant::now());
                let verified=runtime.inspect_execution(root,store,&task,remote,None,remaining).await?;
                if verified.as_ref().and_then(|v|v["state"].as_str())!=Some("session_absent"){
                    return Err("owned residue cleanup did not leave verified original execution absence".into());
                }
            }
            Some("session_absent") => {
                if launch_unresolved {
                    return Err("original launch attempt is unresolved; session absence does not prove launcher settlement".into());
                }
            },
            _=>return Ok(outcome("start",&request.name,false,json!({"outcome":"refused","startedNew":false,"configApplied":false,
                "requestSatisfied":false,"executionObservation":observed,"error":"original execution identity unresolved; existing read/stop remain available"}))),
        }
    } else if launch_unresolved {
        return Err("generation/launch is in progress or unconfirmed; original execution observation required".into());
    }
    let kind=kind(&task)?;
    let profile=profile(&task)?;
    let mut counts: Option<CollectionSummary> = None;
    let mut preflight = Document::new();
    let mut config = if kind == "priority" {
        match requested.clone(){Some(config)=>config,None=>resolve_priority_start_config(root,runtime,&task,None).await?}
    } else {
        let execution = store
            .summary(Host::from_remote(remote), &request.name)
            .await?;
        if execution.total == 0 {
            return Err("execution collection missing or empty".into());
        }
        let mut extension = None;
        if remote {
            let local = store.summary(Host::Local, &request.name).await?;
            let exact = if local.total == execution.total {
                let local_ids = store
                    .rows(Host::Local, &request.name, Some(id_projection()))
                    .await?;
                let remote_ids = store
                    .rows(Host::Remote, &request.name, Some(id_projection()))
                    .await?;
                if local_ids.len() as u64 != local.total
                    || remote_ids.len() as u64 != execution.total
                {
                    return Err("execution ID inventory changed during start preflight".into());
                }
                digest(&local_ids)? == digest(&remote_ids)?
            } else {
                false
            };
            if !exact {
                if kind != "regular"
                    || task.get_bool("isRemote") != Ok(true)
                    || local.total == 0
                    || local.total >= execution.total
                {
                    return Err("remote data is not an exact verified sync of local data".into());
                }
                extension = Some((local.total, execution.total));
            }
        }
        let mut config = saved_start_config(&task, request.config_json.as_deref())?;
        if let Some((local, total)) = extension {
            config = source_binding(root, store, runtime, &task, &config).await?;
            let proof = runtime
                .verify_priority_extension(root, &task, local, total)
                .await?;
            extension_proof(&proof, &task, local, total)?;
            preflight = proof;
            config = source_binding(root, store, runtime, &task, &config).await?;
        }
        counts = Some(execution);
        config
    };
    if kind == "super" {
        runtime.validate_super_config(root, &config).await?;
    }
    let notify = notification_spec(&task, &request.notification)?;
    if let Some(spec) = &notify {
        runtime.preflight_notification(root, spec).await?;
    }
    runtime.assert_consumer_absent(root, &task, remote).await?;
    if kind == "regular" {
        if let Some(continuation) = runtime.continuation(root, &task, remote).await? {
            preflight.insert("datasetContinuation", continuation);
        }
    }
    // Every read-only specialist may await external IO; recheck identity/config
    // before the one launch claim. No specialist owns a control-plane write.
    unchanged(store, &task).await?;
    if deadline.saturating_duration_since(tokio::time::Instant::now()).as_secs()==0 {return Err("start deadline exhausted before launch claim".into());}
    let mut launch = doc! {"launchSettledAttemptId":Bson::Null,"status":"launch_unknown","launchAttemptId":nonce(),"launchAttemptAt":bson::DateTime::now(),"isRemote":remote,
    "stopConfirmedAt":Bson::Null,"stopConfirmedRemote":Bson::Null,"stopConfirmedTaskId":Bson::Null,"stopConfirmationBasis":Bson::Null,"tmuxContext":Bson::Null};
    if kind == "super" {
        let binding = doc! {"taskId":task_id(&task)?.to_hex(),"name":&request.name,"auth_profile":profile,"remote":remote,
        "launchAttemptId":launch.get_str("launchAttemptId").unwrap(),"consumer_run_id":nonce()};
        launch.insert("startupBinding", binding.clone());
        config.insert("startup_binding", binding);
    }
    // Strict conversion happens before side effects; never serialize NaN to null.
    let config_json = strict_json(&Bson::Document(config.clone()))?.to_string();
    let claimed = update(store, &task, "start", launch, Document::new()).await?;
    // This block contains no launcher invocation. A returned preparation error
    // is independently known no-dispatch; cancellation of the whole future is
    // not caught here and cannot manufacture settlement.
    let prepared:Result<(Document,StartCommand),String> = async {
    let mut claimed = reread_claim(store, &claimed).await?;
    if kind == "super" {
        register_super_runtime(store, &claimed, remote).await?;
        claimed = reread_claim(store, &claimed).await?;
    }
    let binding = launch_binding(&claimed, remote)?;
    runtime
        .validate_execution_binding(root, &binding, remote)
        .await?;
    reread_claim(store, &claimed).await?;
    let script = match kind {
        "super" => "start_super_task.sh",
        "priority" => "start_priority_task.sh",
        _ => "start_task.sh",
    };
    let command = StartCommand {
        argv: vec![
            "bash".into(),
            format!("scripts/{script}"),
            request.name.clone(),
            config_json,
            if remote {
                "--remote".into()
            } else {
                "false".into()
            },
            profile.into(),
        ],
        timeout_seconds: deadline.saturating_duration_since(tokio::time::Instant::now()).as_secs().min(120),
        binding,
    };
    if command.timeout_seconds==0 {return Err("start deadline exhausted before launcher; original attempt retained".into());}
    Ok((claimed,command))
    }.await;
    let (mut claimed,command)=match prepared {
        Ok(value)=>value,
        Err(error)=>{
            let settlement:Result<(),String> = async {
                if deadline<=tokio::time::Instant::now(){return Err("original remaining budget exhausted".into());}
                let current=reread_claim(store,&claimed).await?;
                update(store,&current,"start-not-dispatched",
                    doc!{"launchSettledAttemptId":current.get_str("launchAttemptId").map_err(|_|"missing original launch")?},
                    doc!{"launcherInvoked":false,"basis":"not_dispatched"}).await?;
                Ok(())
            }.await;
            return Err(match settlement {
                Ok(())=>format!("{error}; original attempt settled without invoking launcher; this start failed and was not retried"),
                Err(persist)=>format!("{error}; original no-dispatch settlement not confirmed: {persist}; retain original attempt"),
            });
        }
    };
    let output = runtime.launch(root, &command).await?;
    if output.observed_contexts.len() != 1 {
        return Err(
            "execution_context_unverified: launch has no unique observed tmuxContext".into(),
        );
    }
    let observed = &output.observed_contexts[0];
    if observed.get_str("taskId").ok() != Some(task_id(&claimed)?.to_hex().as_str())
        || observed.get("launchAttemptId") != claimed.get("launchAttemptId")
        || observed.get_bool("remote") != Ok(remote)
    {
        return Err("execution_context_unverified: observed task/launch/host mismatch".into());
    }
    let mut observed_binding = launch_binding(&claimed, remote)?;
    observed_binding.insert("tmuxContext", observed.clone());
    runtime
        .validate_execution_binding(root, &observed_binding, remote)
        .await?;
    // Only a successful return from the original synchronous launcher settles
    // its launch action. Timeout/transport rejection never writes this fence.
    // Persist against this exact attempt, before later validation/control ACKs.
    claimed = update(store, &claimed, "start-settled",
        doc! {"launchSettledAttemptId":claimed.get_str("launchAttemptId").map_err(|_|"missing claimed launch")?}, Document::new()).await?;
    let mut fields = doc! {"status":"running","isRemote":remote,"startedAt":now(),"startConfig":&config,"mission_config":&config,"tmuxContext":observed};
    if kind == "priority" {
        fields.insert("targetRemote", remote);
    }
    if !preflight.is_empty() {
        fields.insert("startPreflight", preflight.clone());
    }
    let running = update(store, &claimed, "start", fields, preflight.clone()).await?;
    let mut notification = Bson::Null;
    if let Some(spec) = notify {
        let attachment = async {
            let receipt = runtime
                .attach_watcher(root, &running, remote, &spec)
                .await?;
            notification_receipt(&receipt, &running, remote, &spec)?;
            update(
                store,
                &running,
                "watch-attach",
                doc! {"agentNotification":&receipt},
                receipt.clone(),
            )
            .await?;
            Ok::<Document, String>(receipt)
        }
        .await;
        match attachment {
            Ok(receipt) => notification = Bson::Document(receipt),
            Err(error) => {
                let mut failed = spec;
                failed.extend(doc! {"status":"attach_failed","error":&error});
                let metadata_error = update(
                    store,
                    &running,
                    "watch-attach-failed",
                    doc! {"agentNotification":failed},
                    Document::new(),
                )
                .await
                .err();
                return Ok(outcome(
                    "start",
                    &request.name,
                    false,
                    json!({"taskId":task_id(&task)?.to_hex(),"remote":remote,
                    "taskType":kind,"authProfile":profile,"launchAttemptId":observed_launch(&running)?,"operationSettled":true,
                    "consumerStarted":true,"outcome":"started","startedNew":true,"consumerRunning":true,"configApplied":true,
                    "configComparison":"not_requested","requestSatisfied":false,"notificationAttached":false,"error":error,"notificationMetadataError":metadata_error,
                    "nextAction":"use taskctl watch; do not restart or resimulate accepted work"}),
                ));
            }
        }
    }
    let mut fields = json!({"taskType":kind,"authProfile":profile,"launchAttemptId":observed_launch(&running)?,"operationSettled":true,"outcome":"started","startedNew":true,"consumerRunning":true,"configApplied":true,"configComparison":"not_requested","requestSatisfied":true,"taskId":task_id(&task)?.to_hex(),"remote":remote,"counts":counts.as_ref().map(summary_value),
        "notification":notification.as_document().map(wire_document),"tmuxContext":wire_document(observed),
        "stdout":tail(&output.stdout,16000),"stderr":tail(&output.stderr,4000)});
    if !preflight.is_empty() {
        fields["preflight"] = wire_document(&preflight);
    }
    Ok(outcome("start", &request.name, true, fields))
}

#[cfg(test)]
mod shared_notification_receipt_tests {
    use super::*;
    #[test]
    fn start_and_watch_share_receiver_guards_without_reimplementing_owner_time_or_generation_rules() {
        let id = ObjectId::new();
        let task = doc! {"_id":id,"name":"fixture","launchAttemptId":"new-launch"};
        let spec = doc! {"target":{"sessionId":"main-fixture"},"itemKey":"mission:fixture"};
        let receipt = doc! {"missionId":id.to_hex(),"launchId":"new-launch","remote":false,
            "target":{"sessionId":"main-fixture"},"itemKey":"mission:fixture","producerPid":42,
            "producerMarkerPath":"/fixture/marker","checkpointPath":"/fixture/checkpoint","requiresLease":false};
        notification_receipt(&receipt, &task, false, &spec).unwrap();
        let mut legacy = task.clone(); legacy.remove("launchAttemptId");
        legacy.insert("startedAt", bson::DateTime::from_millis(1577934245123));
        let expected = doc! {"missionId":id.to_hex(),"remote":false,"launchId":"original-owner-date-string",
            "generationId":"generation-a","generationPhase":"writing"};
        let mut watched = receipt.clone(); watched.insert("launchId", "original-owner-date-string");
        watched.insert("generationId", "generation-a"); watched.insert("generationPhase", "writing");
        notification_receipt_bound(&watched, &legacy, false, &spec, &expected).unwrap();
        assert!(notification_receipt(&watched, &legacy, false, &spec).is_err(), "start still requires its fresh attempt");
        for (key, value) in [("launchId", Bson::String("other".into())),
            ("generationId", Bson::String("other".into())), ("generationPhase", Bson::String("prepared".into())),
            ("producerPid", Bson::Int32(0)), ("remote", Bson::Boolean(true)), ("requiresLease", Bson::Boolean(true))] {
            let mut bad = watched.clone(); bad.insert(key, value);
            assert!(notification_receipt_bound(&bad, &legacy, false, &spec, &expected).is_err(), "{key}");
        }
        let mut incomplete = expected.clone(); incomplete.remove("generationPhase");
        let mut missing = watched.clone(); missing.remove("generationPhase");
        assert!(notification_receipt_bound(&missing, &legacy, false, &spec, &incomplete).is_err());
    }
}
