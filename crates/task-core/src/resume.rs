//! Original taskctl resume: only proven-unposted retry rows return to pending.
//! This operation never starts/stops a consumer or touches the platform.
use crate::{
    cleanup_sync::{outcome, OperationsRuntime, TaskOutcome},
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    task_store::{receipt_projection, Host, TaskStore},
};
use bson::{doc, Bson, Document};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ResumeRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
    pub include_timeout: bool,
    pub recover_processing: bool,
}
struct ResumeError {
    kind: &'static str,
    message: String,
}
impl From<String> for ResumeError {
    fn from(message: String) -> Self {
        Self {
            kind: "RuntimeError",
            message,
        }
    }
}
impl From<&str> for ResumeError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
fn policy_error(message: String) -> ResumeError {
    ResumeError {
        kind: "TypeError",
        message,
    }
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn retry_statuses(request: &ResumeRequest) -> Vec<&'static str> {
    let mut retry = vec!["transient_error", "quota_deferred"];
    if request.include_timeout {
        retry.push("timeout_error");
    }
    if request.recover_processing {
        retry.push("processing");
    }
    retry
}
fn blocked_status(task: &Document) -> Result<bool, ResumeError> {
    // This is the original command's status-set lookup, not a new schema gate.
    let unhashable = match task.get("status") {
        Some(Bson::Array(_)) => Some("list"),
        Some(Bson::Document(_)) => Some("dict"),
        Some(Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_)) => Some("Code"),
        Some(Bson::RegularExpression(_)) => Some("Regex"),
        Some(Bson::Decimal128(_)) => Some("Decimal128"),
        _ => None,
    };
    if let Some(kind) = unhashable {
        return Err(policy_error(format!("unhashable type: '{kind}'")));
    }
    Ok(
        matches!(task.get("status"), Some(Bson::String(status) | Bson::Symbol(status))
        if ["generating", "generation_unknown", "launch_unknown", "syncing", "deleting"].contains(&status.as_str())),
    )
}
fn task_id_text(id: &Bson) -> Option<String> {
    match id {
        Bson::ObjectId(id) => Some(id.to_hex()),
        Bson::String(id) | Bson::Symbol(id) => Some(id.clone()),
        Bson::Int32(id) => Some(id.to_string()),
        Bson::Int64(id) => Some(id.to_string()),
        Bson::Null | Bson::Undefined => Some("None".into()),
        _ => None,
    }
}
fn task_host(task: &Document) -> Result<Host, ResumeError> {
    match task.get("isRemote") {
        None | Some(Bson::Null | Bson::Boolean(false)) => Ok(Host::Local),
        Some(Bson::Boolean(true)) => Ok(Host::Remote),
        _ => Err("invalid task execution host; no endpoint fallback".into()),
    }
}
async fn active_task(store: &dyn TaskStore, name: &str) -> Result<Document, ResumeError> {
    store
        .find_task(
            Host::Local,
            doc! {"name":name,"status":{"$ne":"deactive"}},
            true,
        )
        .await?
        .ok_or_else(|| "task not found".into())
}
const CONTROL_FENCES: &[&str] = &[
    "_id",
    "name",
    "status",
    "isRemote",
    "updatedAt",
    "tmuxContext",
    "launchAttemptId",
    "launchAttemptAt",
    "acceptedObservation",
    "generation",
    "startupBinding",
    "taskType",
    "auth_profile",
];
async fn bound_task(
    store: &dyn TaskStore,
    name: &str,
    expected: &Document,
) -> Result<(), ResumeError> {
    let current = active_task(store, name).await?;
    if CONTROL_FENCES
        .iter()
        .any(|key| current.get(*key) != expected.get(*key))
    {
        return Err(
            "task identity/state/launch/observer binding changed before resume mutation".into(),
        );
    }
    policy::assert_observation_closed(&current)?;
    Ok(())
}
fn snapshot_fields(query: &mut Document, row: &Document, keys: impl Iterator<Item = String>) {
    let mut present_null = Vec::new();
    for key in keys {
        match row.get(&key) {
            Some(value) => {
                query.insert(&key, value.clone());
                // Mongo equality-to-null alone also matches a missing field.
                // Preserve both original value and actual presence in the CAS.
                if matches!(value, Bson::Null) {
                    present_null.push(Bson::Document(doc! {key: {"$exists":true}}));
                }
            }
            None => {
                query.insert(&key, doc! {"$exists":false});
            }
        }
    }
    if !present_null.is_empty() {
        query.insert("$and", present_null);
    }
}
fn control_query(name: &str, task: &Document) -> Result<Document, ResumeError> {
    let mut query = policy::task_binding_query(name, task, "resume")?;
    snapshot_fields(
        &mut query,
        task,
        CONTROL_FENCES.iter().map(|v| v.to_string()),
    );
    Ok(query)
}
fn item_query(row: &Document, projection: &Document) -> Result<Document, ResumeError> {
    if !row.contains_key("_id") {
        return Err("resume item has no original ID; refusing an unbound write".into());
    }
    let mut query = Document::new();
    snapshot_fields(&mut query, row, projection.keys().cloned());
    Ok(query)
}

pub async fn resume(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    request: &ResumeRequest,
) -> TaskOutcome {
    if let Err(error) = policy::validate_name(&request.name) {
        return outcome(
            "resume",
            &request.name,
            false,
            json!({"errorType":"ValueError","error":error}),
        );
    }
    match resume_inner(root, store, runtime, request).await {
        Ok(fields) => outcome("resume", &request.name, true, fields),
        Err(error) => outcome(
            "resume",
            &request.name,
            false,
            json!({"errorType":error.kind,"error":error.message}),
        ),
    }
}
async fn resume_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    request: &ResumeRequest,
) -> Result<Value, ResumeError> {
    let name = &request.name;
    let _lock = MissionLocks::acquire(root, &[name], false)?;
    let task = active_task(store, name).await?;
    if task.get_str("name").ok() != Some(name.as_str()) {
        return Err("task name binding changed".into());
    }
    let id = task.get("_id").ok_or("task identity unavailable")?;
    if request
        .expected_task_id
        .as_ref()
        .is_some_and(|expected| task_id_text(id).as_ref() != Some(expected))
    {
        return Err("task identity changed before mutation".into());
    }
    policy::assert_observation_closed(&task)?;
    if blocked_status(&task)? {
        return Err("generation/launch ownership unresolved; resume cannot reconcile it".into());
    }
    let host = task_host(&task)?;
    runtime.assert_consumer_absent(root, &task, host).await?;
    let retry = retry_statuses(request);
    bound_task(store, name, &task).await?;
    let projection = receipt_projection();
    // Filter at the original Store/Mongo query. No payload fetch, client-side
    // whole-task status scan, row limit or alternate collection is permitted.
    let rows = store
        .find_items(
            host,
            name,
            doc! {"status":{"$in":retry.clone()}},
            Some(projection.clone()),
        )
        .await?;
    let mut changed = 0u64;
    for row in rows {
        if !policy::checked_positively_unposted(&row).map_err(policy_error)? {
            continue;
        }
        // Original lock excludes cooperative replacements; also reject visible
        // legacy control drift before each eligible row (not a platform probe).
        bound_task(store, name, &task).await?;
        let query = item_query(&row, &projection)?;
        let modified = store
            .update_item(
                host,
                name,
                query,
                doc! {"$set":{"status":"pending",
                "resumedAt":now(),"recoveryReason":"durable_no_post_proof"}},
            )
            .await?;
        changed = changed
            .checked_add(modified)
            .ok_or("resume modification count overflow")?;
    }
    bound_task(store, name, &task).await?;
    let at = now();
    let modified_count =
        i64::try_from(changed).map_err(|_| "resume audit count cannot be represented")?;
    let event = doc! {"id":bson::oid::ObjectId::new().to_hex(),"command":"resume","status":"ready","at":&at,
    "detail":{"rows":modified_count,"from":retry.clone()}};
    let matched = store
        .update_task(
            Host::Local,
            control_query(name, &task)?,
            doc! {"$set":{"status":"ready","updatedAt":at},"$push":{"events":event}},
        )
        .await?;
    if matched != 1 {
        return Err(
            "task control CAS failed; original binding changed, existing row updates retained"
                .into(),
        );
    }
    Ok(json!({"resumed":changed,"statuses":retry,"recover_processing":request.recover_processing}))
}
