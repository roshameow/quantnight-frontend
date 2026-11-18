//! Original taskctl sync/delete coordination. No transport, private client, drop,
//! tmux resolver, research policy, or alternate ledger lives in this module.
use crate::{
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    task_store::{id_projection, CollectionSummary, Host, TaskStore},
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
    /// Stop the exact task's current activity using the same owner as pause.
    /// The caller holds the mission lock through the subsequent archive.
    async fn stop_for_archive(
        &self, _root: &Path, _store: &dyn TaskStore, _task: &Document, _guard: &MissionLocks,
    ) -> Result<(), String> { Err("task stop is unavailable on this runtime".into()) }
    /// Original stop owner's read-only reconciliation; never sends stop signals.
    async fn verify_stopped(&self, _root: &Path, _task: &Document,
        _remaining: std::time::Duration) -> Result<Option<crate::pause::StopOutput>, String> { Ok(None) }
    async fn confirm_stopped_scope(&self,_root:&Path,_store:&dyn TaskStore,_task:&Document,
        _remaining:std::time::Duration)->Result<(),String>{Ok(())}
    async fn inspect_generation(
        &self,
        _root: &Path,
        _task: &Document,
    ) -> Result<crate::create_generate::GenerationProcesses, String> {
        Err("original generation process inspection unavailable".into())
    }
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
pub(crate) fn archive_namespace(name: &str, id: ObjectId) -> String {
    format!("{name}__archive_{}", id.to_hex())
}
pub(crate) fn task_host(task: &Document) -> Result<Host, String> {
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
    Ok(())
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
fn binding(name: &str, expected: &Document, command: &str) -> Result<Document, String> {
    policy::task_binding_query(name, expected, command)
}
async fn bound(store: &dyn TaskStore, name: &str, expected: &Document) -> Result<(), String> {
    let current = task(store, name, true)
        .await?
        .ok_or("task disappeared before mutation")?;
    for field in binding(name, expected, "sync")?.keys() {
        if current.get(field) != expected.get(field) {
            return Err("task identity/state changed before mutation".into());
        }
    }
    Ok(())
}
fn validate_claim(original: &Document, claimed: &Document, status: &str) -> Result<(), String> {
    if claimed.get_str("status").ok() != Some(status) {
        return Err("task claim status changed before mutation".into());
    }
    for field in ["_id", "name", "isRemote", "targetRemote", "generation", "launchAttemptId"] {
        if original.get(field) != claimed.get(field) {
            return Err(format!("task claim binding changed before mutation: {field}"));
        }
    }
    Ok(())
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
    if original.get_str("status").ok() == Some("deleting") {
        return Err("archive incomplete; repeat delete for this task ID".into());
    }
    // A committed initial sync moved this exact ID from local to remote.
    // Read back its outcome even if the original request still expects local;
    // this exception never authorizes writes on a stale host selection.
    if original.get_bool("isRemote") == Ok(true)
        && original.get_bool("targetRemote") == Ok(true)
        && original.get_str("syncedAt").is_ok_and(|stamp| !stamp.is_empty())
        && (expected_remote != Some(false) || expected_task_id.is_some())
    {
        if let Some(digest) = original.get_str("syncedDigest").ok().filter(|digest| !digest.is_empty()) {
            return Ok(json!({"alreadySynced":true, "taskId":task_id(&original)?.to_hex(), "remote":true, "digest":digest}));
        }
    }
    if expected_remote.is_some_and(|expected| Some(expected) != original.get_bool("isRemote").ok()) {
        return Err("original execution host changed before locked sync".into());
    }
    let continuing = original.get_str("status").ok() == Some("syncing");
    if !continuing && !matches!(original.get_str("status").ok(), Some("ready" | "paused")) {
        return Err("sync needs prepared local data; generate the task first".into());
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
    remote_identity(store, name, &original).await?;
    let populated = store.has_rows(Host::Remote, name).await?;
    if populated && !continuing {
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
    let mut missing = source.iter().map(|row| Ok((id_text(row)?, row)))
        .collect::<Result<std::collections::HashMap<_, _>, String>>()?;
    if missing.len() != source.len() {
        return Err("local sync source has ambiguous item IDs".into());
    }
    if populated {
        // Only an interrupted copy can reach this path. Read every already
        // copied row and compare its full value; never overwrite a row or
        // infer a match from a count. A lost insert reply is thus reentrant.
        for row in store.rows(Host::Remote, name, None).await? {
            let key = id_text(&row)?;
            if missing.remove(&key) != Some(&row) {
                return Err("remote copy contains a different item; nothing was overwritten".into());
            }
        }
    }
    let claimed = if continuing {
        original.clone()
    } else {
        bound(store, name, &original).await?;
        update(store, name, &original, "sync-begin", doc! {"status": "syncing"}, json!({})).await?;
        let claimed = task(store, name, false).await?.ok_or("sync claim missing before copy")?;
        validate_claim(&original, &claimed, "syncing")?;
        validate_task(&claimed, name, expected_task_id)?;
        claimed
    };
    let to_copy = source.iter().filter(|row| id_text(row).is_ok_and(|id| missing.contains_key(&id)))
        .cloned().collect::<Vec<_>>();
    for batch in to_copy.chunks(100) {
        bound(store, name, &claimed).await?;
        remote_identity(store, name, &claimed).await?;
        store.insert_rows(Host::Remote, name, batch.to_vec()).await?;
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
    remote_identity(store, name, &claimed).await?;
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
        "localArchive": null, "remoteArchive": null, "unresolvedItemCount": null,
        "receiptAssessmentUnknownCount": null, "platformRequestsCancelled": false,
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

fn archive_target(name: &str, record: &Document) -> Result<Document, String> {
    Ok(doc! {
        "name": name,
        "_id": task_id(record)?,
        "isRemote": record.get("isRemote").cloned().unwrap_or(Bson::Null),
        "status": {"$ne": "deactive"},
    })
}

async fn assert_archive_target(store: &dyn TaskStore, name: &str, original: &Document) -> Result<(), String> {
    let current = task(store, name, false).await?.ok_or("task disappeared before archive")?;
    if task_id(&current)? != task_id(original)? || task_host(&current)? != task_host(original)? {
        return Err("task execution host or identity changed before archive; no rename on the new target".into());
    }
    Ok(())
}

async fn archive_control_update(
    store: &dyn TaskStore, host: Host, name: &str, original: &Document,
    fields: Document, command: &str, detail: &Value,
) -> Result<(), String> {
    let status = fields.get_str("status").unwrap_or("ok");
    let audit = event(command, status, detail)?;
    let matched = store.update_task(host, archive_target(name, original)?,
        doc! {"$set": fields, "$push": {"events": audit}}).await?;
    if matched != 1 {
        return Err("task or execution host changed during archive; retry delete for the original task".into());
    }
    Ok(())
}

pub async fn delete_with_expected_host(
    root: &Path, store: &dyn TaskStore, runtime: &dyn OperationsRuntime,
    name: &str, expected_task_id: Option<&str>, expected_remote: Option<bool>,
) -> TaskOutcome {
    let result = delete_locked_entry(root, store, runtime, name, expected_task_id, expected_remote).await;
    match result {
        Ok((ok, fields)) => outcome("delete", name, ok, fields),
        Err(error) => failure("delete", name, error),
    }
}

async fn delete_locked_entry(
    root: &Path, store: &dyn TaskStore, runtime: &dyn OperationsRuntime,
    name: &str, expected_task_id: Option<&str>, expected_remote: Option<bool>,
) -> Result<(bool, Value), String> {
    policy::validate_name(name)?;
    let guard = MissionLocks::acquire(root, &[name], false)?;
    let expected_id = expected_task_id.map(ObjectId::parse_str).transpose()
        .map_err(|_| "invalid expected task ObjectId")?;
    // An explicit old identity may return its archive without targeting a new
    // live same-name task. A name-only call continues to resolve the active task.
    let original = if let Some(id) = expected_id {
        store.find_task(Host::Local, doc! {"_id": id, "name": name}, true).await?
    } else {
        task(store, name, true).await?
    };
    let original = match original {
        Some(record) => record,
        None if expected_id.is_none() => return Ok((true, json!({"alreadyDeleted": true}))),
        None => return Err("expected task not found; no task was stopped or archived".into()),
    };
    if original.get_str("name").ok() != Some(name)
        || expected_id.is_some_and(|id| task_id(&original).ok() != Some(id))
    {
        return Err("task identity changed before archive".into());
    }
    let host = task_host(&original)?;
    if expected_remote.is_some_and(|expected| expected != host.is_remote()) {
        return Err("task execution host changed before archive".into());
    }
    if original.get_str("status").ok() == Some("deactive") {
        // A committed archive is the result of a previous delete, not a
        // precondition to run pause again (possibly against a successor).
        let mut fields = original.get_document("cleanup").map(wire_document).unwrap_or_else(|_| {
            json!({"taskId":task_id(&original).map(|id| id.to_hex()).ok(), "remote":host.is_remote(), "controlHidden":true})
        });
        fields["alreadyDeleted"] = json!(true);
        return Ok((true, fields));
    }
    let path = receipt_path(root, name);
    let mut receipt = new_receipt(name, &original, host.is_remote())?;
    // A prior report is optional context, never an archive prerequisite.
    let preserve_report = match preserve_previous_receipt(&path, &mut receipt) {
        Ok(()) => true,
        Err(error) => {
            receipt["previousReceiptWarning"] = json!(error);
            false
        }
    };
    receipt["receiptPath"] = if preserve_report { json!(path.to_string_lossy()) } else { Value::Null };
    let result = delete_inner(root, store, runtime, name, &original, host, &guard, &mut receipt).await;
    let ok = result.is_ok();
    receipt["ok"] = json!(ok);
    if let Err(error) = result {
        receipt["errorType"] = json!("RuntimeError");
        receipt["error"] = json!(error);
    }
    // If the final task update succeeded, loss of a redundant file does not
    // turn a completed rename into a failure (the DB holds the same receipt).
    if preserve_report {
        if let Err(error) = persist_receipt(&path, &receipt) {
            receipt["receiptWriteError"] = json!(error);
            receipt["receiptPath"] = Value::Null;
        }
    }
    Ok((ok, receipt))
}

async fn delete_inner(
    root: &Path, store: &dyn TaskStore, runtime: &dyn OperationsRuntime,
    name: &str, original: &Document, host: Host, guard: &MissionLocks, receipt: &mut Value,
) -> Result<(), String> {
    let touch_remote = host == Host::Remote || original.get_str("status").ok() == Some("syncing");
    let archive = archive_namespace(name, task_id(original)?);
    // Guard the intended namespaces before signalling anything. An absent
    // remote registration is fine; a same-name different task is not ours.
    if touch_remote {
        remote_identity(store, name, original).await?;
    }
    runtime.stop_for_archive(root, store, original, guard).await?;
    // Keep partial cleanup intent recoverable through the original delete.
    // This is set only after stopping and doesn't depend on a historical phase.
    if original.get_str("status").ok() != Some("deleting") {
        archive_control_update(store, Host::Local, name, original,
            doc! {"status":"deleting", "updatedAt": now()}, "delete-begin", &json!({})).await?;
    }
    assert_archive_target(store, name, original).await?;
    if touch_remote {
        let current_remote = remote_identity(store, name, original).await?;
        // A remote archive in an earlier call can leave no active control row.
        // Use the current exact row, not the snapshot read before stopping.
        let archived_remote = archive_collection(store, Host::Remote, name, &archive).await?;
        let had_remote_control = current_remote.is_some();
        receipt["remoteArchive"] = json!(archived_remote);
        if let Some(other) = current_remote {
            archive_control_update(store, Host::Remote, name, &other,
                doc! {"status":"deactive", "deletedAt":now(), "updatedAt":now()}, "delete", &json!({})).await?;
        }
        if host == Host::Remote || archived_remote.is_some() || had_remote_control {
            receipt["remoteDeleted"] = json!(true);
        }
        assert_archive_target(store, name, original).await?;
    }
    receipt["localArchive"] = json!(archive_collection(store, Host::Local, name, &archive).await?);
    receipt["localDeleted"] = json!(true);
    // No full item scan or research callback on delete. The retained namespace
    // and task identity support explicit archive-research independently.
    let mut complete = receipt.clone();
    complete["controlHidden"] = json!(true);
    let cleanup = bson::to_bson(&complete).map_err(|e| e.to_string())?;
    archive_control_update(store, Host::Local, name, original,
        doc! {"status":"deactive", "deletedAt":now(), "updatedAt":now(), "cleanup":cleanup}, "delete", &complete).await?;
    receipt["controlHidden"] = json!(true);
    Ok(())
}
