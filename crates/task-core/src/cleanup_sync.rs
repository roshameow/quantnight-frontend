//! Original taskctl sync/delete coordination. No transport, private client, drop,
//! tmux resolver, research policy, or alternate ledger lives in this module.
use crate::{
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    task_store::{id_projection, receipt_projection, CollectionSummary, Host, TaskStore},
};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct TaskOutcome {
    pub value: Value,
    pub exit_code: i32,
}

#[async_trait]
pub trait OperationsRuntime: Send + Sync {
    /// Only authoritative absence of this exact task's consumer is Ok.
    async fn assert_consumer_absent(
        &self,
        root: &Path,
        task: &Document,
        host: Host,
    ) -> Result<(), String>;
    /// Original stop owner's read-only reconciliation; never sends stop signals.
    async fn verify_stopped(&self, _root: &Path, _task: &Document,
        _remaining: std::time::Duration) -> Result<Option<crate::pause::StopOutput>, String> { Ok(None) }
    async fn confirm_stopped_scope(&self,_root:&Path,_store:&dyn TaskStore,_task:&Document,
        _remaining:std::time::Duration)->Result<(),String>{Ok(())}
    /// Invoke the original archive_statistics owner using the exact retained
    /// execution namespace. A null result denotes an unbound research task.
    async fn archive_research(
        &self,
        root: &Path,
        task: &Document,
        host: Host,
        evidence_collection: &str,
    ) -> Result<Value, String>;
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub(crate) fn outcome(command: &str, name: &str, ok: bool, fields: Value) -> TaskOutcome {
    let mut value =
        json!({"schemaVersion": 1, "command": command, "task": name, "ok": ok, "at": now()});
    if let Some(fields) = fields.as_object() {
        for (key, field) in fields {
            if !["schemaVersion", "command", "task", "ok", "at"].contains(&key.as_str()) {
                value[key] = field.clone();
            }
        }
    }
    TaskOutcome {
        value,
        exit_code: if ok { 0 } else { 1 },
    }
}
pub(crate) fn failure(command: &str, name: &str, error: impl ToString) -> TaskOutcome {
    outcome(
        command,
        name,
        false,
        json!({"errorType": "RuntimeError", "error": error.to_string()}),
    )
}
fn wire(value: &Bson) -> Value {
    match value {
        Bson::ObjectId(id) => json!(id.to_hex()),
        Bson::DateTime(at) => {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(at.timestamp_millis())
                .map(|value| json!(value.to_rfc3339()))
                .unwrap_or_else(|| json!(at.to_string()))
        }
        Bson::Document(row) => {
            Value::Object(row.iter().map(|(k, v)| (k.clone(), wire(v))).collect())
        }
        Bson::Array(values) => Value::Array(values.iter().map(wire).collect()),
        other => other.clone().into_relaxed_extjson(),
    }
}
pub(crate) fn wire_document(row: &Document) -> Value {
    wire(&Bson::Document(row.clone()))
}
fn task_id(task: &Document) -> Result<ObjectId, String> {
    task.get_object_id("_id")
        .map_err(|_| "task must retain its original ObjectId".into())
}
fn task_host(task: &Document) -> Result<Host, String> {
    match task.get("isRemote") {
        None | Some(Bson::Null) | Some(Bson::Boolean(false)) => Ok(Host::Local),
        Some(Bson::Boolean(true)) => Ok(Host::Remote),
        _ => Err("malformed task isRemote; refusing to infer execution host".into()),
    }
}
fn validate_task(task: &Document, name: &str, expected: Option<&str>) -> Result<(), String> {
    let id = task_id(task)?;
    if task.get_str("name").ok() != Some(name) {
        return Err("task name binding changed".into());
    }
    task.get_str("status")
        .map_err(|_| "malformed task status".to_string())?;
    task_host(task)?;
    if let Some(expected) = expected {
        let expected = ObjectId::parse_str(expected)
            .map_err(|_| "invalid expected task ObjectId".to_string())?;
        if id != expected {
            return Err("task identity changed before mutation".into());
        }
    }
    policy::assert_observation_closed(task)
}
async fn task(
    store: &dyn TaskStore,
    name: &str,
    include_deactive: bool,
) -> Result<Option<Document>, String> {
    let active = store
        .find_task(
            Host::Local,
            doc! {"name": name, "status": {"$ne": "deactive"}},
            true,
        )
        .await?;
    if active.is_some() || !include_deactive {
        return Ok(active);
    }
    store
        .find_task(Host::Local, doc! {"name": name}, true)
        .await
}
// Additional snapshot fences protect malformed/legacy writers that forget to
// advance updatedAt. They only reject stale mutations; no new state is invented.
fn binding(name: &str, expected: &Document, command: &str) -> Result<Document, String> {
    let mut query = policy::task_binding_query(name, expected, command)?;
    for field in [
        "acceptedObservation",
        "targetRemote",
        "generation",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "stopConfirmationBasis",
        "launchAttemptAt",
        "launchSettledAttemptId",
    ] {
        query.insert(
            field,
            expected
                .get(field)
                .cloned()
                .unwrap_or_else(|| Bson::Document(doc! {"$exists": false})),
        );
    }
    Ok(query)
}
async fn bound(store: &dyn TaskStore, name: &str, expected: &Document) -> Result<(), String> {
    let current = task(store, name, true)
        .await?
        .ok_or("task disappeared before mutation")?;
    for field in [
        "_id",
        "status",
        "isRemote",
        "updatedAt",
        "tmuxContext",
        "launchAttemptId",
        "acceptedObservation",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "launchAttemptAt",
        "launchSettledAttemptId",
    ] {
        if current.get(field) != expected.get(field) {
            return Err("task identity/state changed before mutation".into());
        }
    }
    policy::assert_observation_closed(&current)
}
fn validate_claim(original: &Document, claimed: &Document, status: &str) -> Result<(), String> {
    if claimed.get_str("status").ok() != Some(status) {
        return Err("task claim status changed before mutation".into());
    }
    for field in original.keys().chain(claimed.keys()) {
        if !["status", "updatedAt", "events"].contains(&field.as_str())
            && original.get(field) != claimed.get(field)
        {
            return Err(format!(
                "task claim binding changed before mutation: {field}"
            ));
        }
    }
    policy::assert_observation_closed(claimed)
}
fn event(command: &str, status: &str, detail: &Value) -> Result<Document, String> {
    Ok(
        doc! {"id": ObjectId::new().to_hex(), "command": command, "status": status,
        "at": now(), "detail": bson::to_bson(detail).map_err(|e| e.to_string())?},
    )
}
async fn update(
    store: &dyn TaskStore,
    name: &str,
    expected: &Document,
    command: &str,
    mut fields: Document,
    detail: Value,
) -> Result<(), String> {
    fields.insert("updatedAt", now());
    let status = fields.get_str("status").unwrap_or("ok");
    let audit = event(command, status, &detail)?;
    let count = store
        .update_task(
            Host::Local,
            binding(name, expected, command)?,
            doc! {"$set": fields, "$push": {"events": audit}},
        )
        .await?;
    if count != 1 {
        return Err(format!("task control record changed; {command} CAS failed"));
    }
    Ok(())
}

fn id_text(row: &Document) -> Result<String, String> {
    match row.get("_id") {
        Some(Bson::ObjectId(id)) => Ok(id.to_hex()),
        Some(Bson::String(id)) if !id.is_empty() => Ok(id.clone()),
        Some(Bson::Int32(id)) => Ok(id.to_string()),
        Some(Bson::Int64(id)) => Ok(id.to_string()),
        _ => Err("unsupported/missing task item ID; cannot verify original digest".into()),
    }
}
pub(crate) fn digest(rows: &[Document]) -> Result<String, String> {
    let mut ids = rows.iter().map(id_text).collect::<Result<Vec<_>, _>>()?;
    ids.sort();
    Ok(format!("{:x}", Sha256::digest(ids.join("\n").as_bytes())))
}
pub(crate) fn summary_value(summary: &CollectionSummary) -> Value {
    let mut value =
        json!({"exists": summary.exists, "total": summary.total, "statuses": summary.statuses});
    if summary.exists {
        value["latestHeartbeatAt"] = summary
            .latest_heartbeat_at
            .as_ref()
            .map(wire)
            .unwrap_or(Value::Null);
    }
    value
}
async fn remote_identity(
    store: &dyn TaskStore,
    name: &str,
    expected: &Document,
    reject_running: bool,
) -> Result<Option<Document>, String> {
    let other = store
        .find_task(
            Host::Remote,
            doc! {"name": name, "status": {"$ne": "deactive"}},
            true,
        )
        .await?;
    if let Some(other) = &other {
        if other.get("_id") != expected.get("_id") {
            return Err("same-name remote task has different identity; refusing mutation".into());
        }
        other
            .get_str("status")
            .map_err(|_| "malformed remote task status".to_string())?;
        if reject_running && other.get_str("status").ok() == Some("running") {
            return Err("remote task state conflicts with initial sync".into());
        }
        policy::assert_observation_closed(other)?;
    }
    Ok(other)
}

pub async fn sync(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    name: &str,
    expected_task_id: Option<&str>,
) -> TaskOutcome {
    sync_with_expected_host(root, store, runtime, name, expected_task_id, None).await
}

pub async fn sync_with_expected_host(
    root: &Path, store: &dyn TaskStore, runtime: &dyn OperationsRuntime,
    name: &str, expected_task_id: Option<&str>, expected_remote: Option<bool>,
) -> TaskOutcome {
    if let Err(error) = policy::validate_name(name) {
        return outcome(
            "sync",
            name,
            false,
            json!({"errorType": "ValueError", "error": error}),
        );
    }
    match sync_inner(root, store, runtime, name, expected_task_id, expected_remote).await {
        Ok(fields) => outcome("sync", name, true, fields),
        Err(error) => failure("sync", name, error),
    }
}
async fn sync_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    name: &str,
    expected_task_id: Option<&str>,
    expected_remote: Option<bool>,
) -> Result<Value, String> {
    policy::validate_name(name)?;
    let _lock = MissionLocks::acquire(root, &[name], false)?;
    let original = task(store, name, false)
        .await?
        .ok_or("sync requires a ready/paused task")?;
    validate_task(&original, name, expected_task_id)?;
    if expected_remote.is_some_and(|expected| Some(expected) != original.get_bool("isRemote").ok()) {
        return Err("original execution host changed before locked sync".into());
    }
    if !matches!(original.get_str("status").ok(), Some("ready" | "paused")) {
        return Err(
            "sync requires a ready/paused task; reconcile generation or launch first".into(),
        );
    }
    if original.get_bool("isRemote").ok() != Some(false)
        || original.get_bool("targetRemote").ok() != Some(true)
    {
        return Err("sync requires an initial task-bound remote destination; resume executed tasks in place".into());
    }
    runtime
        .assert_consumer_absent(root, &original, Host::Local)
        .await?;
    runtime
        .assert_consumer_absent(root, &original, Host::Remote)
        .await?;
    let local_summary = store.summary(Host::Local, name).await?;
    if !local_summary.exists || local_summary.total == 0 {
        return Err("local simulation collection missing or empty".into());
    }
    let local = summary_value(&local_summary);
    store.ping(Host::Remote).await?;
    remote_identity(store, name, &original, true).await?;
    if store.has_rows(Host::Remote, name).await? {
        return Err("execution destination is not empty; resume in place, never overwrite".into());
    }
    // The only full-payload read: these exact rows are the actual copy source.
    // Existing TaskStore Vec API is retained, with no candidate limit/pagination.
    let source = store.rows(Host::Local, name, None).await?;
    if source.len() as u64 != local_summary.total {
        return Err("local source count changed before initial sync".into());
    }
    for row in &source {
        let pending = row.get("status").is_none() || row.get_str("status").ok() == Some("pending");
        if !pending || policy::checked_receipt_evidence(row)? {
            return Err("initial sync source contains execution evidence".into());
        }
    }
    let source_digest = digest(&source)?;
    bound(store, name, &original).await?;
    update(
        store,
        name,
        &original,
        "sync-begin",
        doc! {"status": "syncing"},
        json!({}),
    )
    .await?;
    let claimed = task(store, name, false)
        .await?
        .ok_or("sync claim missing before copy")?;
    validate_claim(&original, &claimed, "syncing")?;
    validate_task(&claimed, name, expected_task_id)?;
    for batch in source.chunks(100) {
        bound(store, name, &claimed).await?;
        remote_identity(store, name, &claimed, true).await?;
        store
            .insert_rows(Host::Remote, name, batch.to_vec())
            .await?;
    }
    let remote_summary = store.summary(Host::Remote, name).await?;
    let remote = summary_value(&remote_summary);
    let destination_ids = store
        .rows(Host::Remote, name, Some(id_projection()))
        .await?;
    let current_source_ids = store.rows(Host::Local, name, Some(id_projection())).await?;
    let remote_digest = digest(&destination_ids)?;
    if !remote_summary.exists
        || remote_summary.total != local_summary.total
        || destination_ids.len() as u64 != remote_summary.total
        || current_source_ids.len() != source.len()
        || digest(&current_source_ids)? != source_digest
        || remote_digest != source_digest
    {
        return Err(format!(
            "remote verification failed: local={local} remote={remote}"
        ));
    }
    remote_identity(store, name, &claimed, true).await?;
    update(
        store,
        name,
        &claimed,
        "sync",
        doc! {"status": "ready", "isRemote": true,
        "syncedAt": now(), "syncedDigest": &remote_digest},
        remote.clone(),
    )
    .await?;
    Ok(json!({"local": local, "remote": remote, "digest": remote_digest}))
}

fn receipt_path(root: &Path, name: &str) -> PathBuf {
    root.join("reports/missions")
        .join(format!("{name}.cleanup.json"))
}
fn new_receipt(name: &str, task: &Document, remote: bool) -> Result<Value, String> {
    let mut config = Document::new();
    for field in [
        "templatefile",
        "taskType",
        "auth_profile",
        "runId",
        "startConfig",
        "mission_config",
        "config",
        "researchBinding",
    ] {
        if let Some(value) = task.get(field) {
            config.insert(field, value.clone());
        }
    }
    Ok(
        json!({"schemaVersion": 1, "task": name, "taskId": task_id(task)?.to_hex(), "at": now(), "remote": remote,
        "localDeleted": false, "remoteDeleted": if remote { Value::Bool(false) } else { Value::Null },
        "controlHidden": false, "canonicalResultsDeleted": false, "itemsPreserved": true,
        "localArchive": null, "remoteArchive": null, "unresolvedItemCount": 0,
        "receiptAssessmentUnknownCount": 0, "platformRequestsCancelled": false,
        "capacityReleased": false, "taskConfig": wire_document(&config)}),
    )
}
fn preserve_previous_receipt(path: &Path, receipt: &mut Value) -> Result<(), String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cleanup receipt read failed: {e}")),
    };
    let previous: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("cleanup receipt invalid; retain evidence: {e}"))?;
    if previous["taskId"] == receipt["taskId"] && previous["task"] == receipt["task"] {
        if previous["remote"] != receipt["remote"] {
            return Err("cleanup receipt execution host changed".into());
        }
        // Retain diagnostics and archive identity across failures before a fresh
        // assessment. Success flags are still re-verified, never inherited.
        for field in ["unresolvedItemCount", "receiptAssessmentUnknownCount", "localArchive", "remoteArchive"] {
            if let Some(value) = previous.get(field) {
                receipt[field] = value.clone();
            }
        }
        // Retain actual original research evidence if a partial retry's helper
        // returns null. Never reuse old success flags without re-verification.
        if !previous["researchArchive"].is_null() {
            receipt["researchArchive"] = previous["researchArchive"].clone();
        }
    }
    Ok(())
}
fn persist_receipt(path: &Path, receipt: &Value) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("invalid cleanup receipt path")?)
        .map_err(|e| format!("cleanup receipt directory: {e}"))?;
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|e| format!("cleanup receipt serialization: {e}"))?;
    // Replace the same original report atomically; never truncate a prior
    // partial-success receipt on an interrupted write. This sibling is only
    // this call's temporary file, not another persistent ledger/registry.
    let temporary = path.with_extension(format!("json.tmp-{}", ObjectId::new().to_hex()));
    let saved = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if saved.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    saved.map_err(|e| format!("cleanup receipt write failed: {e}"))
}
async fn archive_collection(
    store: &dyn TaskStore,
    host: Host,
    name: &str,
    archive: &str,
) -> Result<Option<String>, String> {
    let mut names = store.collection_names(host).await?;
    if names.iter().any(|n| n == name) {
        if names.iter().any(|n| n == archive) {
            return Err(
                "both active and archived collections exist; reconcile before cleanup".into(),
            );
        }
        store.rename_collection(host, name, archive).await?;
        names = store.collection_names(host).await?;
        if names.iter().any(|n| n == name) || !names.iter().any(|n| n == archive) {
            return Err("archive rename not verified; cleanup incomplete".into());
        }
    }
    Ok(names
        .iter()
        .any(|n| n == archive)
        .then(|| archive.to_string()))
}

pub async fn delete(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    name: &str,
    expected_task_id: Option<&str>,
) -> TaskOutcome {
    delete_with_expected_host(root, store, runtime, name, expected_task_id, None).await
}

struct ArchiveStopVerifier<'a>(&'a dyn OperationsRuntime);
#[async_trait]
impl crate::pause::PauseRuntime for ArchiveStopVerifier<'_> {
    async fn stop(&self, _: &Path, _: &Document, _: bool) -> Result<crate::pause::StopOutput, String> {
        Err("archive verify-only cannot send stop signals".into())
    }
    async fn confirm_stopped_scope(&self,root:&Path,store:&dyn TaskStore,task:&Document,remaining:std::time::Duration)->Result<(),String>{
        self.0.confirm_stopped_scope(root,store,task,remaining).await
    }
    async fn verify_stopped(&self, root: &Path, task: &Document, remaining: std::time::Duration)
        -> Result<Option<crate::pause::StopOutput>, String> {
        self.0.verify_stopped(root, task, remaining).await
    }
}

pub async fn delete_with_expected_host(
    root: &Path, store: &dyn TaskStore, runtime: &dyn OperationsRuntime,
    name: &str, expected_task_id: Option<&str>, expected_remote: Option<bool>,
) -> TaskOutcome {
    if let Err(error) = policy::validate_name(name) {
        return outcome(
            "delete",
            name,
            false,
            json!({"errorType": "ValueError", "error": error}),
        );
    }
    let _lock = match MissionLocks::acquire(root, &[name], false) {
        Ok(lock) => lock,
        Err(error) => return failure("delete", name, error),
    };
    let mut original = match task(store, name, true).await {
        Ok(Some(task)) => task,
        Ok(None) if expected_task_id.is_none() => {
            return outcome("delete", name, true, json!({"alreadyDeleted": true}))
        }
        Ok(None) => {
            return failure(
                "delete",
                name,
                "expected task not found; cannot confirm deletion",
            )
        }
        Err(error) => return failure("delete", name, error),
    };
    // Invalid identity must not overwrite a same-name task's receipt file.
    if let Err(error) = validate_task(&original, name, expected_task_id) {
        return failure("delete", name, error);
    }
    let host = match task_host(&original) {
        Ok(host) => host,
        Err(error) => return failure("delete", name, error),
    };
    if expected_remote.is_some_and(|expected| expected != host.is_remote()) {
        return failure("delete", name, "original execution host changed before locked cleanup");
    }
    if original.get_str("status").ok() != Some("deactive")
        && !policy::stop_confirmation_valid(&original, host.is_remote())
        && !matches!(original.get_str("status").ok(), Some("generating"|"generation_unknown"|"syncing"|"deleting")) {
        let stopped = crate::pause::pause_locked(root, store, &ArchiveStopVerifier(runtime), name,
            crate::pause::PauseOptions { expected_task_id, expected_remote: Some(host.is_remote()),
                expected_launch_attempt_id: original.get_str("launchAttemptId").ok(), observer: None, ..Default::default() },
            &_lock, tokio::time::Instant::now() + std::time::Duration::from_secs(12),
            crate::pause::StopIntent::VerifyOnly).await;
        if stopped.exit_code != 0 {
            return outcome("delete", name, false, json!({"errorType":"RuntimeError",
                "error":"current valid stop confirmation unavailable after original verify-only",
                "stopReconciliation":stopped.value,"requiredAction":"inspect_original_stop_result"}));
        }
        let prior=original.clone();
        original = match task(store, name, false).await {
            Ok(Some(current)) => current,
            Ok(None) => return failure("delete", name, "task disappeared after stop verification"),
            Err(error) => return failure("delete", name, error),
        };
        if ["_id","launchAttemptId","startupBinding","isRemote","taskType","auth_profile","generation","acceptedObservation","runId","startConfig","mission_config","config","templatefile"].iter()
            .any(|key|original.get(*key)!=prior.get(*key))
            || original.get_str("updatedAt").ok()!=stopped.value["controlRevision"].as_str()
            || original.get_str("status").ok()!=Some("paused") {
            return failure("delete",name,"original stop snapshot replaced before archive; successor not adopted");
        }
        if let Err(error) = validate_task(&original, name, expected_task_id) { return failure("delete", name, error); }
        if task_host(&original).ok() != Some(host) { return failure("delete", name, "host changed after stop verification"); }
    }
    let path = receipt_path(root, name);
    let mut receipt = match new_receipt(name, &original, host.is_remote()) {
        Ok(receipt) => receipt,
        Err(error) => return failure("delete", name, error),
    };
    if let Err(error) = preserve_previous_receipt(&path, &mut receipt) {
        return failure("delete", name, error);
    }
    receipt["receiptPath"] = json!(path.to_string_lossy());
    let result = delete_inner(root, store, runtime, name, &original, host, &mut receipt).await;
    match result {
        Ok(()) => receipt["ok"] = json!(true),
        Err(error) => {
            receipt["ok"] = json!(false);
            receipt["errorType"] = json!("RuntimeError");
            receipt["error"] = json!(error);
        }
    }
    if let Err(error) = persist_receipt(&path, &receipt) {
        receipt["ok"] = json!(false);
        receipt["errorType"] = json!("RuntimeError");
        if receipt.get("error").is_none() {
            receipt["error"] = json!(error);
        }
        receipt["receiptWriteError"] = json!(error);
    }
    let ok = receipt["ok"] == true;
    receipt["receiptPath"] = json!(path.to_string_lossy());
    if ok {
        outcome(
            "delete",
            name,
            true,
            json!({"receiptPath": path.to_string_lossy(), "localDeleted": true,
            "remoteDeleted": receipt["remoteDeleted"], "controlHidden": true, "canonicalResultsDeleted": false, "itemsPreserved": true,
            "taskId": receipt["taskId"], "remote": receipt["remote"],
            "localArchive": receipt["localArchive"], "remoteArchive": receipt["remoteArchive"],
            "unresolvedItemCount": receipt["unresolvedItemCount"],
            "receiptAssessmentUnknownCount": receipt["receiptAssessmentUnknownCount"],
            "platformRequestsCancelled": false, "capacityReleased": false}),
        )
    } else {
        outcome("delete", name, false, receipt)
    }
}
async fn delete_inner(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn OperationsRuntime,
    name: &str,
    original: &Document,
    host: Host,
    receipt: &mut Value,
) -> Result<(), String> {
    if matches!(
        original.get_str("status").ok(),
        Some("generating" | "generation_unknown" | "launch_unknown" | "syncing" | "running")
    ) {
        receipt["requiredAction"] = json!(if matches!(original.get_str("status").ok(),
            Some("generating" | "generation_unknown" | "syncing")) {
            "reconcile_generation_or_sync"
        } else { "pause" });
        return Err(
            "pause/reconcile generation or running/unconfirmed consumer before delete".into(),
        );
    }
    if crate::start::launch_attempt_unsettled(original) {
        receipt["requiredAction"]=json!("reconcile_original_launcher");
        return Err("original launcher remains unresolved after pause; preserve active items and attempt".into());
    }
    if !policy::stop_confirmation_valid(original, host.is_remote()) {
        receipt["requiredAction"] = json!("pause");
        return Err("current valid stop confirmation required before delete".into());
    }
    runtime.assert_consumer_absent(root, original, host).await?;
    bound(store, name, original).await?;
    receipt["localBefore"] = summary_value(&store.summary(Host::Local, name).await?);
    store.ping(host).await?;
    if host == Host::Remote {
        remote_identity(store, name, original, false).await?;
    }
    let archive = format!("{name}__archive_{}", task_id(original)?.to_hex());
    let names = store.collection_names(host).await?;
    let active_exists = names.iter().any(|n| n == name);
    let archived_exists = names.iter().any(|n| n == &archive);
    if active_exists && archived_exists {
        return Err("both active and archived collections exist; reconcile before cleanup".into());
    }
    let evidence_name = if !active_exists && archived_exists {
        archive.as_str()
    } else {
        name
    };
    let rows = store
        .rows(host, evidence_name, Some(receipt_projection()))
        .await?;
    // Archive is a namespace rename, not request resolution. Count execution
    // items (not distinct platform simulations); malformed receipt metadata is
    // retained as opaque unresolved evidence. Only this diagnostic assessment
    // tolerates classification errors: control/IO/identity gates still fail.
    let mut unresolved = 0_u64;
    let mut assessment_unknown = 0_u64;
    for row in &rows {
        match policy::checked_unresolved_execution_row(row) {
            Ok(false) => (),
            Ok(true) => unresolved += 1,
            Err(_) => {
                unresolved += 1;
                assessment_unknown += 1;
            }
        }
    }
    receipt["unresolvedItemCount"] = json!(unresolved);
    receipt["receiptAssessmentUnknownCount"] = json!(assessment_unknown);
    let research = runtime
        .archive_research(root, original, host, evidence_name)
        .await?;
    if !research.is_null() || receipt.get("researchArchive").is_none() {
        receipt["researchArchive"] = research;
    }
    update(
        store,
        name,
        original,
        "delete-begin",
        doc! {"status": "deleting"},
        json!({}),
    )
    .await?;
    let claimed = task(store, name, true)
        .await?
        .ok_or("cleanup claim missing before archive")?;
    validate_claim(original, &claimed, "deleting")?;
    if host == Host::Remote {
        let other = remote_identity(store, name, &claimed, false).await?;
        bound(store, name, &claimed).await?;
        receipt["remoteArchive"] =
            json!(archive_collection(store, Host::Remote, name, &archive).await?);
        receipt["remoteDeleted"] = json!(true);
        if let Some(other) = other {
            let matched = store
                .update_task(
                    Host::Remote,
                    binding(name, &other, "delete")?,
                    doc! {"$set": {"status": "deactive", "deletedAt": now(), "updatedAt": now()}},
                )
                .await?;
            if matched != 1 {
                return Err("remote control changed; archive retained, cleanup incomplete".into());
            }
        }
    }
    bound(store, name, &claimed).await?;
    receipt["localArchive"] = json!(archive_collection(store, Host::Local, name, &archive).await?);
    receipt["localDeleted"] = json!(true);
    let mut complete = receipt.clone();
    complete["controlHidden"] = json!(true);
    let cleanup = bson::to_bson(&complete).map_err(|e| e.to_string())?;
    update(
        store,
        name,
        &claimed,
        "delete",
        doc! {"status": "deactive", "deletedAt": now(), "cleanup": cleanup},
        complete,
    )
    .await?;
    receipt["controlHidden"] = json!(true);
    Ok(())
}
