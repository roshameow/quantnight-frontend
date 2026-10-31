//! Shared create/generate coordination. The original Python template owner and
//! mongo_datum writer remain behind GenerationRuntime; there is no alternate
//! candidate builder, authentication, DB connection, launcher or taskctl loop.

use crate::{
    cleanup_sync::{failure, outcome, summary_value, wire_document, TaskOutcome},
    lifecycle_policy,
    mission_lock::MissionLocks,
    task_store::{Host, TaskStore},
};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct CreateRequest {
    pub name: String,
    pub templatefile: String,
    pub task_type: String,
    pub auth_profile: String,
    pub template: String,
    pub target_remote: bool,
    pub force: bool,
    pub expected_task_id: Option<String>,
    pub mission_config: Option<Document>,
}
impl CreateRequest {
    pub fn new(name: impl Into<String>, templatefile: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            templatefile: templatefile.into(),
            task_type: "regular".into(),
            auth_profile: "user1".into(),
            template: String::new(),
            target_remote: false,
            force: false,
            expected_task_id: None,
            mission_config: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
    pub replace: bool,
    pub timeout_seconds: u64,
}
impl GenerateRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            expected_task_id: None,
            replace: false,
            timeout_seconds: 900,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct CreationMetadata {
    /// Validated by the original template_binding/read_binding owner, not by a
    /// second Rust interpretation of research manifests or dataset fields.
    pub recommended_max_concurrent: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerationErrorKind {
    Ordinary,
    Timeout,
    DatabaseUncertain,
    Interrupted,
}
#[derive(Clone, Debug)]
pub struct GenerationFailure {
    pub kind: GenerationErrorKind,
    pub message: String,
}
impl GenerationFailure {
    pub fn new(kind: GenerationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    fn database(message: impl Into<String>) -> Self {
        Self::new(GenerationErrorKind::DatabaseUncertain, message)
    }
    fn ordinary(message: impl std::fmt::Display) -> Self {
        Self::new(GenerationErrorKind::Ordinary, message.to_string())
    }
    fn python_type(&self) -> &'static str {
        match self.kind {
            GenerationErrorKind::Ordinary => "RuntimeError",
            GenerationErrorKind::Timeout => "TimeoutExpired",
            GenerationErrorKind::DatabaseUncertain => "PyMongoError",
            GenerationErrorKind::Interrupted => "KeyboardInterrupt",
        }
    }
}
#[derive(Clone, Debug)]
pub struct GenerationCommand {
    pub argv: Vec<String>,
    pub timeout_seconds: u64,
    pub task_id: ObjectId,
    pub generation_id: String,
}
#[derive(Clone, Debug)]
pub struct GeneratedOutput {
    pub stdout: String,
}

#[async_trait]
pub trait GenerationRuntime: Send + Sync {
    /// Read-only original local writer inspection, never a simulation/tmux probe.
    async fn inspect_generation(&self, _root: &Path, _task: &Document)
        -> Result<GenerationProcesses, String> {
        Err("original generation process inspection unavailable".into())
    }

    async fn creation_metadata(
        &self,
        root: &Path,
        desired: &Document,
    ) -> Result<CreationMetadata, GenerationFailure>;
    /// Ok requires confirmed original writer exit0. A lost child/response is not
    /// success: use Timeout/DatabaseUncertain unless interruption is confirmed.
    async fn generate(
        &self,
        root: &Path,
        command: &GenerationCommand,
    ) -> Result<GeneratedOutput, GenerationFailure>;
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}
fn nonce() -> String {
    bson::Uuid::new().to_string().replace('-', "")
}
fn python_int(value: i64) -> Bson {
    i32::try_from(value)
        .map(Bson::Int32)
        .unwrap_or(Bson::Int64(value))
}
fn tail(value: &str, count: usize) -> String {
    let start = value
        .char_indices()
        .rev()
        .nth(count.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    value[start..].to_string()
}
fn expected_id(value: Option<&str>) -> Result<Option<ObjectId>, String> {
    value
        .map(|s| ObjectId::parse_str(s).map_err(|_| "invalid expected task ObjectId".into()))
        .transpose()
}
async fn active(store: &dyn TaskStore, name: &str) -> Result<Option<Document>, String> {
    store
        .find_task(
            Host::Local,
            doc! {"name": name, "status": {"$ne": "deactive"}},
            true,
        )
        .await
}
fn identity(task: &Document, name: &str, expected: Option<ObjectId>) -> Result<ObjectId, String> {
    let id = task
        .get_object_id("_id")
        .map_err(|_| "task must retain its original ObjectId")?;
    if task.get_str("name").ok() != Some(name) {
        return Err("task name binding changed".into());
    }
    task.get_str("status")
        .map_err(|_| "malformed task status")?;
    if expected.is_some_and(|expected| expected != id) {
        return Err("task identity changed before mutation".into());
    }
    lifecycle_policy::assert_observation_closed(task)?;
    Ok(id)
}
fn create_event(status: &str, at: &str) -> Document {
    doc! {"id": nonce(), "command": "create", "status": status, "at": at, "detail": {}}
}
/// Stronger snapshot fencing protects original identity/config even if a
/// legacy writer fails to advance updatedAt. Never overwrites a new token.
fn snapshot_filter(task: &Document, command: &str) -> Result<Document, String> {
    let name = task.get_str("name").map_err(|_| "task name unavailable")?;
    let mut query = lifecycle_policy::task_binding_query(name, task, command)?;
    for (key, value) in task {
        query.insert(key, value.clone());
    }
    for key in [
        "acceptedObservation",
        "generation",
        "events",
        "templatefile",
        "template",
        "auth_profile",
        "taskType",
        "targetRemote",
        "runId",
        "startConfig",
        "mission_config",
        "config",
    ] {
        if !task.contains_key(key) {
            query.insert(key, doc! {"$exists": false});
        }
    }
    Ok(query)
}
async fn unchanged(store: &dyn TaskStore, task: &Document) -> Result<(), String> {
    let name = task.get_str("name").map_err(|_| "task name unavailable")?;
    let current = active(store, name)
        .await?
        .ok_or("task disappeared before mutation")?;
    if current != *task {
        return Err("task identity/state changed before mutation".into());
    }
    identity(
        &current,
        name,
        Some(
            task.get_object_id("_id")
                .map_err(|_| "task ObjectId unavailable")?,
        ),
    )?;
    Ok(())
}
pub(crate) fn initial_config(name: &str, kind: &str, concurrency: i64) -> Result<Document, String> {
    // Only task-management defaults are ported. Priority has no research
    // binding; its original rounds/default validation remains with its owner.
    match kind {
        "regular" => Ok(
            doc! {"max_concurrent": python_int(concurrency), "max_multi_simulation_children": 10,
            "tag": {"name": name, "tags": [name], "regular": {"description": "research campaign"}}},
        ),
        "super" => Ok(
            doc! {"max_concurrent": python_int(concurrency), "max_multi_simulation_children": 1,
            "tag": {"name": name, "tags": [name], "selection": {"description": "research campaign"},
                "combo": {"description": "research campaign"}}},
        ),
        _ => Err("Priority creation metadata must not invent a research binding".into()),
    }
}

pub async fn create(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn GenerationRuntime,
    request: &CreateRequest,
) -> TaskOutcome {
    match create_inner(root, store, runtime, request).await {
        Ok((created, task)) => outcome(
            "create",
            &request.name,
            true,
            json!({"created": created, "taskId": task.get_object_id("_id").unwrap().to_hex(), "taskRecord": wire_document(&task)}),
        ),
        Err(error) => outcome(
            "create",
            &request.name,
            false,
            json!({"errorType": error.python_type(), "error": error.message}),
        ),
    }
}
async fn create_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn GenerationRuntime,
    request: &CreateRequest,
) -> Result<(bool, Document), GenerationFailure> {
    lifecycle_policy::validate_name(&request.name).map_err(GenerationFailure::ordinary)?;
    if !["regular", "super", "priority"].contains(&request.task_type.as_str()) {
        return Err(GenerationFailure::ordinary("invalid task type"));
    }
    let expected =
        expected_id(request.expected_task_id.as_deref()).map_err(GenerationFailure::ordinary)?;
    let _lock = MissionLocks::acquire(root, &[&request.name], false)
        .map_err(GenerationFailure::ordinary)?;
    let mut desired = doc! {"name": &request.name, "template": &request.template,
    "templatefile": &request.templatefile, "auth_profile": &request.auth_profile,
    "status": "pending", "isRemote": false, "targetRemote": request.target_remote, "taskType": &request.task_type};
    if let Some(existing) = active(store, &request.name)
        .await
        .map_err(GenerationFailure::database)?
    {
        identity(&existing, &request.name, expected).map_err(GenerationFailure::ordinary)?;
        let mut conflicts = vec![];
        for (key, value) in &desired {
            if !["status", "isRemote", "targetRemote"].contains(&key.as_str())
                && existing.get(key) != Some(value)
            {
                conflicts.push(key.clone());
            }
        }
        if let Some(config) = &request.mission_config {
            if existing
                .get_document("mission_config")
                .ok()
                .map(wire_document)
                .as_ref()
                != Some(&wire_document(config))
            {
                conflicts.push("mission_config".into());
            }
        }
        if !conflicts.is_empty() && !request.force {
            return Err(GenerationFailure::ordinary(format!(
                "task exists with conflicting immutable fields: {}",
                conflicts.join(", ")
            )));
        }
        let stamp = now();
        let event = create_event("idempotent", &stamp);
        let mut fields = doc! {"targetRemote": request.target_remote, "updatedAt": &stamp};
        if request.force {
            for (key, value) in &desired {
                if !["name", "status", "isRemote"].contains(&key.as_str()) {
                    fields.insert(key, value.clone());
                }
            }
            if let Some(config) = &request.mission_config {
                fields.insert("mission_config", config.clone());
                fields.insert("startConfig", config.clone());
            }
        }
        let count = store
            .update_task(
                Host::Local,
                snapshot_filter(&existing, "create").map_err(GenerationFailure::ordinary)?,
                doc! {"$set": fields.clone(), "$push": {"events": event.clone()}},
            )
            .await
            .map_err(GenerationFailure::database)?;
        if count != 1 {
            return Err(GenerationFailure::ordinary(
                "create task snapshot CAS not confirmed",
            ));
        }
        let mut saved = existing;
        saved.extend(fields);
        match saved.get_mut("events") {
            Some(Bson::Array(events)) => events.push(event.into()),
            None => {
                saved.insert("events", vec![Bson::Document(event)]);
            }
            _ => return Err(GenerationFailure::ordinary("malformed task event history")),
        }
        unchanged(store, &saved)
            .await
            .map_err(GenerationFailure::database)?;
        return Ok((false, saved));
    }
    if expected.is_some() {
        return Err(GenerationFailure::ordinary(
            "expected task identity missing; refusing to create a replacement",
        ));
    }
    let metadata = runtime.creation_metadata(root, &desired).await?;
    let prepared_config = metadata
        .recommended_max_concurrent
        .map(|value| initial_config(&request.name, &request.task_type, value))
        .transpose()
        .map_err(GenerationFailure::ordinary)?;
    if let Some(config) = request.mission_config.clone().or(prepared_config) {
        desired.insert("startConfig", config.clone());
        desired.insert("mission_config", config);
    }
    // Metadata may await a Python owner: do not adopt/overwrite a new same-name
    // task that appeared meanwhile. No DB transaction/unique-name claim is invented.
    if active(store, &request.name)
        .await
        .map_err(GenerationFailure::database)?
        .is_some()
    {
        return Err(GenerationFailure::ordinary(
            "task appeared during creation metadata; retry only against its exact identity",
        ));
    }
    let id = ObjectId::new();
    let stamp = now();
    desired.extend(doc! {"_id": id, "createdAt": &stamp, "updatedAt": &stamp,
    "runId": format!("mission:{}", request.name), "events": [create_event("pending", &stamp)]});
    let inserted = store
        .insert_task(Host::Local, desired.clone())
        .await
        .map_err(GenerationFailure::database)?;
    if inserted != Bson::ObjectId(id) {
        return Err(GenerationFailure::database(
            "create insert identity not confirmed; no automatic retry",
        ));
    }
    unchanged(store, &desired)
        .await
        .map_err(GenerationFailure::database)?;
    Ok((true, desired))
}

/// Original generation_state.begin_generation query/update with deterministic
/// token/clock/PID inputs for offline AST oracle tests. No database IO here.
pub fn generation_begin_plan(
    task: &Document,
    token: &str,
    phase: &str,
    producer_pid: u32,
    at: &str,
) -> Result<(Document, Document), String> {
    if !matches!(
        task.get_str("status").ok(),
        Some("pending" | "waiting" | "ready" | "generation_failed")
    ) {
        return Err("task is running, generating, or requires generation reconciliation".into());
    }
    let mut query = doc! {"_id": task.get("_id").ok_or("'_id'")?.clone(), "name": task.get("name").ok_or("'name'")?.clone(), "status": task.get("status").unwrap().clone()};
    if let Some(updated) = task.get("updatedAt") {
        query.insert("updatedAt", updated.clone());
    }
    let generation = doc! {"id": token, "phase": phase, "producerPid": python_int(i64::from(producer_pid)), "startedAt": at};
    Ok((
        query,
        doc! {"$set": {"status": "generating", "generation": &generation, "updatedAt": at},
        "$push": {"events": {"command": "generation-begin", "status": "generating", "at": at, "detail": generation}}},
    ))
}
/// Original finish token CAS. `status` is the operation outcome, not always
/// the resulting task status: a failed generation restores a captured pending
/// or waiting task, while its phase/event keep the failure. Legacy ready and
/// other failure inputs retain their old behavior. Preparing->pending is unchanged.
/// Caller fields cannot override phase or the token/identity query.
pub fn generation_finish_plan(
    task: &Document,
    token: &str,
    status: &str,
    mut fields: Document,
    at: &str,
) -> Result<(Document, Document), String> {
    // Python rejects duplicate positional/keyword formal arguments before the
    // function body runs, even if `status` itself would be invalid. Keep that
    // boundary when adapting **fields to a Rust Document.
    for key in fields.keys() {
        if ["tasks", "task", "token", "status"].contains(&key.as_str()) {
            return Err(format!(
                "finish_generation() got multiple values for argument '{key}'"
            ));
        }
    }
    let phase = match status {
        "pending" => "prepared",
        "ready" => "complete",
        "generation_failed" | "generation_unknown" => status,
        _ => return Err("invalid generation outcome".into()),
    };
    let query = doc! {"_id": task.get("_id").ok_or("'_id'")?.clone(), "name": task.get("name").ok_or("'name'")?.clone(), "status": "generating", "generation.id": token};
    let task_status = match (status, task.get_str("status").ok()) {
        ("generation_failed", Some(previous @ ("pending" | "waiting"))) => previous,
        _ => status,
    };
    fields.extend(doc! {"status": task_status, "generation.phase": phase, "generation.finishedAt": at, "updatedAt": at});
    Ok((
        query,
        doc! {"$set": fields, "$push": {"events": {"command": "generation-finish", "status": status, "at": at, "detail": {"generationId": token}}}},
    ))
}
fn owned(original: &Document, generation: &Document, current: &Document) -> Result<(), String> {
    if current.get_str("status").ok() != Some("generating")
        || current.get_document("generation").ok() != Some(generation)
    {
        return Err("generation ownership changed; refusing stale state update".into());
    }
    for key in original.keys().chain(current.keys()) {
        if !["status", "updatedAt", "generation", "events"].contains(&key.as_str())
            && original.get(key) != current.get(key)
        {
            return Err(format!("generation task binding changed: {key}"));
        }
    }
    lifecycle_policy::assert_observation_closed(current)
}
async fn current_claim(
    store: &dyn TaskStore,
    original: &Document,
    generation: &Document,
) -> Result<Document, GenerationFailure> {
    let name = original
        .get_str("name")
        .map_err(GenerationFailure::ordinary)?;
    let current = active(store, name)
        .await
        .map_err(GenerationFailure::database)?
        .ok_or_else(|| GenerationFailure::ordinary("generation task disappeared"))?;
    owned(original, generation, &current).map_err(GenerationFailure::ordinary)?;
    Ok(current)
}
async fn finish(
    store: &dyn TaskStore,
    original: &Document,
    generation: &Document,
    status: &str,
    fields: Document,
) -> Result<(), GenerationFailure> {
    finish_at(store, original, generation, status, fields, &now(), None).await
}
async fn finish_at(
    store: &dyn TaskStore, original: &Document, generation: &Document,
    status: &str, fields: Document, at: &str, expected_snapshot: Option<&Document>,
) -> Result<(), GenerationFailure> {
    let current = current_claim(store, original, generation).await?;
    if expected_snapshot.is_some_and(|expected| expected != &current) {
        return Err(GenerationFailure::ordinary("generation snapshot changed during refresh"));
    }
    let token = generation
        .get_str("id")
        .map_err(GenerationFailure::ordinary)?;
    let (mut query, update) = generation_finish_plan(original, token, status, fields, at)
        .map_err(GenerationFailure::ordinary)?;
    query.extend(
        snapshot_filter(&current, "generation-finish").map_err(GenerationFailure::ordinary)?,
    );
    let count = store
        .update_task(Host::Local, query, update)
        .await
        .map_err(GenerationFailure::database)?;
    if count != 1 {
        return Err(GenerationFailure::ordinary(
            "generation ownership changed; refusing stale state update",
        ));
    }
    Ok(())
}
fn command(
    task: &Document,
    request: &GenerateRequest,
    token: &str,
) -> Result<GenerationCommand, GenerationFailure> {
    let template = task
        .get_str("templatefile")
        .map_err(GenerationFailure::ordinary)?;
    let profile = task
        .get_str("auth_profile")
        .map_err(GenerationFailure::ordinary)?;
    let mut argv = vec![
        "uv".into(),
        "run".into(),
        "-m".into(),
        "backend.mongodb.mongo_datum".into(),
        "--template".into(),
        template.into(),
        "--alpha_filename".into(),
        request.name.clone(),
        "--auth_profile".into(),
        profile.into(),
    ];
    if task.get_str("taskType").ok() == Some("super") {
        argv.extend(["--alpha_type".into(), "super".into()]);
    }
    if request.replace {
        argv.push("--replace".into());
    }
    Ok(GenerationCommand {
        argv,
        timeout_seconds: request.timeout_seconds,
        task_id: task
            .get_object_id("_id")
            .map_err(GenerationFailure::ordinary)?,
        generation_id: token.into(),
    })
}

/// Pure counterpart of cmd_generate's original BaseException classification.
/// None means the interruption inventory could not be observed, not zero rows.
pub fn generation_failure_status(
    kind: &GenerationErrorKind,
    interrupted_total: Option<u64>,
) -> &'static str {
    match kind {
        GenerationErrorKind::Timeout | GenerationErrorKind::DatabaseUncertain => {
            "generation_unknown"
        }
        GenerationErrorKind::Ordinary => "generation_failed",
        GenerationErrorKind::Interrupted if interrupted_total == Some(0) => "generation_failed",
        GenerationErrorKind::Interrupted => "generation_unknown",
    }
}

/// A response-only refresh result. It is never persisted as task metadata.
#[derive(Debug)]
pub struct GenerationRefresh {
    pub task: Option<Document>,
    pub changed: bool,
    pub diagnostic: Option<String>,
}
#[derive(Debug, Clone)]
pub enum GenerationProcesses {
    Stopped,
    Live(String),
    Unverified(String),
}
fn refresh(task: Option<Document>, diagnostic: Option<String>) -> GenerationRefresh {
    if let Some(message) = &diagnostic {
        log::warn!("Generation refresh {:?}: {}", task.as_ref().and_then(|t| t.get("_id")), message);
    }
    GenerationRefresh { task, changed: false, diagnostic }
}
pub fn generation_refresh_candidate(task: &Document) -> bool {
    task.get_str("status").ok() == Some("generating")
}

/// Recognize the original maintenance_ops::watch_locked/update audit shape.
/// This is historical notification metadata, not a status transition or a
/// claim that the watcher is still alive. Reattachment may legitimately change
/// the recipient/producer, so validate each recorded receipt, not today's route.
fn refresh_watch_event(task: &Document, generation: &Document, event: &Document) -> Option<()> {
    if event.get_str("status").ok()? != "ok" || event.get_str("id").ok()?.is_empty() { return None; }
    let receipt = event.get_document("detail").ok()?;
    let target = receipt.get_document("target").ok()?;
    if target.len() != 1 || !["taskId", "sessionId"].iter()
        .any(|key| target.get_str(key).is_ok_and(|value| !value.is_empty())) { return None; }
    let key = receipt.get_str("itemKey").ok()?;
    if key.is_empty() { return None; }
    let remote = match task.get("isRemote") {
        None | Some(Bson::Null) => false, Some(Bson::Boolean(v)) => *v, _ => return None,
    };
    // These are the original watcher's legacy launch facts, not a new launch
    // requirement or a value accepted from the receipt being validated.
    let launch = task.get_str("launchAttemptId").ok().filter(|v| !v.is_empty())
        .or_else(|| task.get_str("startedAt").ok().filter(|v| !v.is_empty())).unwrap_or("legacy");
    crate::start::notification_receipt_bound(receipt, task, remote,
        &doc! {"target":target,"itemKey":key},
        &doc! {"launchId":launch,"generationId":generation.get_str("id").ok()?,
            "generationPhase":generation.get_str("phase").ok()?}).ok()
}

/// Only existing unambiguous adjacent generation/create facts, not an event
/// sourcing engine. In particular a failed finish event records the outcome,
/// not the restored control status, so it cannot establish a prior status.
fn refresh_previous_status(task: &Document, generation: &Document) -> Option<&'static str> {
    let events = task.get_array("events").ok()?;
    let mut relevant: Vec<&Document> = Vec::new();
    let mut previous_at = None;
    for event in events {
        let row = event.as_document()?;
        let at = chrono::DateTime::parse_from_rfc3339(row.get_str("at").ok()?).ok()?;
        if previous_at.is_some_and(|previous| at < previous) { return None; }
        previous_at = Some(at);
        // Only the original non-status-mutating watch receipt after THIS
        // generation's begin is transparent. Unknown/state-changing events
        // are retained below and therefore cannot manufacture a prior status.
        if row.get_str("command").ok() == Some("watch-attach")
            && relevant.last().is_some_and(|begin|
                begin.get_str("command").ok() == Some("generation-begin")
                && begin.get_document("detail").ok() == Some(generation)) {
            refresh_watch_event(task, generation, row)?;
            continue;
        }
        if row.get_str("command").ok() == Some("create")
            && row.get_str("status").ok() == Some("idempotent")
            && row.get_document("detail").ok()?.is_empty() { continue; }
        relevant.push(row);
    }
    let begin = *relevant.last()?;
    if begin.get_str("command").ok()? != "generation-begin"
        || begin.get_str("status").ok()? != "generating"
        || begin.get_document("detail").ok()? != generation { return None; }
    let began = chrono::DateTime::parse_from_rfc3339(generation.get_str("startedAt").ok()?).ok()?;
    let event_at = chrono::DateTime::parse_from_rfc3339(begin.get_str("at").ok()?).ok()?;
    if began > event_at { return None; }
    let token = generation.get_str("id").ok()?;
    if relevant.iter().filter(|e| e.get_str("command").ok() == Some("generation-begin")
        && e.get_document("detail").ok().and_then(|d| d.get_str("id").ok()) == Some(token)).count() != 1 { return None; }
    let previous = *relevant.get(relevant.len().checked_sub(2)?)?;
    let before = chrono::DateTime::parse_from_rfc3339(previous.get_str("at").ok()?).ok()?;
    if before > began { return None; }
    match (previous.get_str("command").ok()?, previous.get_str("status").ok()?) {
        ("create", "pending") if relevant.len() == 2
            && !previous.get_str("id").ok()?.is_empty()
            && previous.get_document("detail").ok()?.is_empty() => Some("pending"),
        ("generation-finish", status @ ("pending" | "ready")) => {
            let old = *relevant.get(relevant.len().checked_sub(3)?)?;
            let detail = old.get_document("detail").ok()?;
            let old_token = detail.get_str("id").ok()?;
            if old.get_str("command").ok()? != "generation-begin"
                || old.get_str("status").ok()? != "generating"
                || old_token == token || old_token.is_empty()
                || previous.get_document("detail").ok()?.get_str("generationId").ok()? != old_token
                || detail.get_str("phase").ok()? != if status == "pending" { "preparing" } else { "writing" }
                || chrono::DateTime::parse_from_rfc3339(old.get_str("at").ok()?).ok()? > before { return None; }
            Some(if status == "pending" { "pending" } else { "ready" })
        }
        _ => None,
    }
}

/// Shared GUI/CLI list reconciliation. No-op branches perform zero writes.
/// All observation and finalization is per exact local task under the ORIGINAL
/// nonblocking mutation lock. A remote execution intent never routes the writer.
pub async fn reconcile_generation_for_list(
    root: &Path, store: &dyn TaskStore, runtime: &dyn GenerationRuntime, observed: &Document,
) -> Result<GenerationRefresh, String> {
    if !generation_refresh_candidate(observed) { return Ok(refresh(Some(observed.clone()), None)); }
    let id = observed.get_object_id("_id").map_err(|_| "generation refresh requires exact local ObjectId")?;
    let name = observed.get_str("name").map_err(|_| "generation refresh requires exact name")?;
    lifecycle_policy::validate_name(name)?;
    let expected_generation = observed.get_document("generation").map_err(|_| "generation record missing/malformed")?;
    if expected_generation.get_str("phase").ok() != Some("writing") {
        return Ok(refresh(Some(observed.clone()), Some("generation phase is not writing; original preparation/unknown owner retained".into())));
    }
    let token = expected_generation.get_str("id").map_err(|_| "generation id missing/malformed")?;
    if token.is_empty() { return Err("generation id empty".into()); }
    let Some(_lock) = MissionLocks::try_acquire(root, &[name], false)? else {
        // An ordinary owner holding this lock is not a refresh error. Keep
        // automatic refresh silent, without even reading or probing the task.
        return Ok(refresh(Some(observed.clone()), None));
    };
    let query = doc! {"_id":id,"name":name};
    let Some(task) = store.find_task(Host::Local, query.clone(), false).await? else {
        return Ok(refresh(None, Some("exact local task disappeared; no same-name adoption".into())));
    };
    if task.get_object_id("_id").ok() != Some(id) || task.get_str("name").ok() != Some(name) {
        return Err("store returned a foreign task identity".into());
    }
    if task.get_str("status").ok() != Some("generating")
        || task.get_document("generation").ok() != Some(expected_generation) {
        return Ok(refresh(Some(task), Some("generation changed since list snapshot; not reconciled".into())));
    }
    identity(&task, name, Some(id))?;
    let generation = task.get_document("generation").unwrap().clone();
    // Do not pass a malformed parent to an inspector that might interpret zero
    // or a missing PID as absence. Birth metadata is not a migration prerequisite.
    let pid = generation.get("producerPid").and_then(|v| match v {
        Bson::Int32(v) => Some(i64::from(*v)), Bson::Int64(v) => Some(*v), _ => None,
    }).filter(|v| *v > 1 && *v <= i64::from(u32::MAX));
    if pid.is_none() { return Err("generation producerPid missing/malformed; owner absence unverified".into()); }
    match runtime.inspect_generation(root, &task).await? {
        GenerationProcesses::Stopped => (),
        GenerationProcesses::Live(reason) | GenerationProcesses::Unverified(reason) =>
            return Ok(refresh(Some(task), Some(reason))),
    }
    let (total, inventory_error) = match store.has_rows(Host::Local, name).await {
        Ok(false) => (Some(0), None), Ok(true) => (Some(1), None), Err(e) => (None, Some(e)),
    };
    let previous = refresh_previous_status(&task, &generation);
    let mut status = generation_failure_status(&GenerationErrorKind::Interrupted, total);
    let mut error = "generation_interrupted: original owner absent and local mongo_datum writer absence verified".to_owned();
    if let Some(e) = inventory_error { error.push_str(&format!("; local collection probe unconfirmed: {e}")); }
    if total == Some(1) { error.push_str("; nonempty collection does not prove complete generation"); }
    if previous.is_none() {
        status = generation_failure_status(&GenerationErrorKind::DatabaseUncertain, None);
        error.push_str("; previous_status_unverified: existing create/generation facts are incomplete or ambiguous");
    }
    let mut original = task.clone();
    if let Some(previous) = previous { original.insert("status", previous); }
    let at = now();
    let fields = doc! {"lastError": tail(&error, 4000)};
    // Use the same planner to derive the intended read-back, never a second
    // status algorithm. finish_at retains current_claim and full snapshot CAS.
    let (_, intended) = generation_finish_plan(&original, token, status, fields.clone(), &at)?;
    let finished = finish_at(store, &original, &generation, status, fields, &at, Some(&task)).await;
    let current = store.find_task(Host::Local, query, false).await?;
    let applied = current.as_ref().is_some_and(|row| {
        row.get_object_id("_id").ok() == Some(id) && row.get_str("name").ok() == Some(name)
            && row.get_document("generation").ok().is_some_and(|g|
                g.get_str("id").ok() == Some(token)
                && g.get_str("finishedAt").ok() == Some(at.as_str())
                && g.get_str("phase").ok() == Some(status))
            && row.get("status") == intended.get_document("$set").ok().and_then(|d| d.get("status"))
            && row.get("lastError") == intended.get_document("$set").ok().and_then(|d| d.get("lastError"))
    });
    if applied {
        return Ok(GenerationRefresh { task: current, changed: true, diagnostic: None });
    }
    let reason = match finished {
        Ok(()) => "generation finalization read-back changed/unconfirmed; retained current exact record".into(),
        Err(e) => format!("generation reconciliation unconfirmed: {}", e.message),
    };
    Ok(refresh(current, Some(reason)))
}

pub async fn generate(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn GenerationRuntime,
    request: &GenerateRequest,
) -> TaskOutcome {
    if let Err(e) = lifecycle_policy::validate_name(&request.name) {
        return failure("generate", &request.name, e);
    }
    let expected = match expected_id(request.expected_task_id.as_deref()) {
        Ok(v) => v,
        Err(e) => return failure("generate", &request.name, e),
    };
    let _lock = match MissionLocks::acquire(root, &[&request.name], false) {
        Ok(v) => v,
        Err(e) => return failure("generate", &request.name, e),
    };
    let task = match active(store, &request.name).await {
        Ok(Some(v)) => v,
        Ok(None) => return failure("generate", &request.name, "create the task first"),
        Err(e) => return failure("generate", &request.name, e),
    };
    if let Err(e) = identity(&task, &request.name, expected) {
        return failure("generate", &request.name, e);
    }
    let token = nonce();
    let stamp = now();
    let (mut query, update) =
        match generation_begin_plan(&task, &token, "writing", std::process::id(), &stamp) {
            Ok(v) => v,
            Err(e) => return failure("generate", &request.name, e),
        };
    let generation = update
        .get_document("$set")
        .unwrap()
        .get_document("generation")
        .unwrap()
        .clone();
    match snapshot_filter(&task, "generation-begin") {
        Ok(fence) => query.extend(fence),
        Err(e) => return failure("generate", &request.name, e),
    }
    // Begin is outside the recovery block, as in the original owner: if its
    // response is lost, do not assume a claim or dispatch the child.
    match store.update_task(Host::Local, query, update).await {
        Ok(1) => (),
        Ok(_) => {
            return failure(
                "generate",
                &request.name,
                "generation claim lost; another operation changed the task",
            )
        }
        Err(e) => {
            return outcome(
                "generate",
                &request.name,
                false,
                json!({"errorType": "PyMongoError", "error": e, "generationId": token, "generationClaimUnconfirmed": true}),
            )
        }
    }
    let work = async {
        current_claim(store, &task, &generation).await?;
        let output = runtime
            .generate(root, &command(&task, request, &token)?)
            .await?;
        current_claim(store, &task, &generation).await?;
        let counts = store
            .summary(Host::Local, &request.name)
            .await
            .map_err(GenerationFailure::database)?;
        if !counts.exists || counts.total == 0 {
            return Err(GenerationFailure::ordinary(
                "generation returned success but collection is empty",
            ));
        }
        current_claim(store, &task, &generation).await?;
        let run_id = task
            .get("runId")
            .cloned()
            .unwrap_or_else(|| Bson::String(format!("mission:{}", request.name)));
        store
            .update_items(
                Host::Local,
                &request.name,
                doc! {},
                doc! {"$set": {"runId": run_id}},
            )
            .await
            .map_err(GenerationFailure::database)?;
        let expected_count = i64::try_from(counts.total).map_err(|_| {
            GenerationFailure::database("generated count cannot be represented in BSON")
        })?;
        finish(
            store,
            &task,
            &generation,
            "ready",
            doc! {"expectedCount": python_int(expected_count), "generatedAt": now()},
        )
        .await?;
        Ok::<_, GenerationFailure>((counts, output.stdout))
    }
    .await;
    match work {
        Ok((counts, stdout)) => outcome(
            "generate",
            &request.name,
            true,
            json!({"counts": summary_value(&counts), "stdout": tail(&stdout, 2000)}),
        ),
        Err(error) => {
            let interrupted_total = if error.kind == GenerationErrorKind::Interrupted {
                store
                    .summary(Host::Local, &request.name)
                    .await
                    .ok()
                    .map(|counts| counts.total)
            } else {
                None
            };
            let status = generation_failure_status(&error.kind, interrupted_total);
            let reconcile = finish(
                store,
                &task,
                &generation,
                status,
                doc! {"lastError": tail(&error.message, 4000)},
            )
            .await;
            match reconcile {
                Ok(()) => outcome(
                    "generate",
                    &request.name,
                    false,
                    json!({"errorType": error.python_type(), "error": error.message,
                    "generationId": token, "generationStatus": status}),
                ),
                Err(final_error) => outcome(
                    "generate",
                    &request.name,
                    false,
                    json!({"errorType": error.python_type(), "error": error.message,
                    "generationId": token, "generationReconciliationError": final_error.message, "generationReconciliationUnconfirmed": true}),
                ),
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GuiCreatedTask {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub auth_profile: Option<String>,
    pub status: String,
    pub is_remote: Option<bool>,
    pub task_type: Option<String>,
    pub mission_config: Option<Value>,
}
pub fn gui_created_task(result: &TaskOutcome) -> Result<GuiCreatedTask, String> {
    if result.exit_code != 0 || result.value["ok"] != true || result.value["command"] != "create" {
        return Err(result.value["error"]
            .as_str()
            .unwrap_or("create not confirmed")
            .into());
    }
    let task = &result.value["taskRecord"];
    let string = |key: &str| {
        task[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("create receipt lacks {key}"))
    };
    Ok(GuiCreatedTask {
        id: string("_id")?,
        name: string("name")?,
        template: string("template")?,
        templatefile: string("templatefile")?,
        auth_profile: task["auth_profile"].as_str().map(str::to_owned),
        status: string("status")?,
        is_remote: task["isRemote"].as_bool(),
        task_type: task["taskType"].as_str().map(str::to_owned),
        mission_config: task.get("mission_config").cloned(),
    })
}
pub fn gui_generated(result: &TaskOutcome) -> Result<(), String> {
    if result.exit_code == 0 && result.value["ok"] == true && result.value["command"] == "generate"
    {
        Ok(())
    } else {
        Err(result.value["error"]
            .as_str()
            .unwrap_or("generation not confirmed")
            .into())
    }
}
