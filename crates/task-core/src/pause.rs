//! Shared original pause coordination; Runtime is only the original stop-script
//! IO seam. No tmux resolver, alternate stop mechanism or new client lives here.
use crate::{
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    task_store::{Host, TaskStore},
};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path, sync::Arc, time::Duration};

#[derive(Debug)]
pub struct StopOutput {
    pub stdout: String,
    pub stderr: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBudget {
    Cli,
    Gui,
}
impl StopBudget {
    pub fn seconds(self) -> u64 {
        match self {
            Self::Cli => 60,
            Self::Gui => 75,
        }
    }
    pub fn duration(self) -> Duration {
        Duration::from_secs(self.seconds())
    }
}
#[async_trait]
pub trait PauseRuntime: Send + Sync {
    fn script_budget(&self) -> StopBudget {
        StopBudget::Cli
    }
    /// Invoke the original script once, carrying its persisted task/tmux binding.
    /// Implementations must use script_budget() for the existing process runner.
    /// Nonzero exit, SSH/protocol/timeout errors return Err, never a fake receipt.
    async fn stop(
        &self,
        root: &Path,
        task: &Document,
        verify_never_started: bool,
    ) -> Result<StopOutput, String>;
    /// Original professional verify-only owner. None means no stopped proof,
    /// never authoritative absence. Legacy injected ports retain original stop.
    async fn verify_stopped(&self, _root: &Path, _task: &Document,
        _remaining: Duration) -> Result<Option<StopOutput>, String> { Ok(None) }
    async fn inspect_before_stop(&self,_root:&Path,_store:&dyn TaskStore,_task:&Document,_remaining:Duration)
        -> Result<Option<Value>,String>{Ok(None)}
    async fn confirm_stopped_scope(&self,_root:&Path,_store:&dyn TaskStore,_task:&Document,_remaining:Duration)
        -> Result<(),String>{Ok(())}
    async fn stop_bounded(&self, root: &Path, task: &Document, never: bool,
        remaining: Duration) -> Result<StopOutput, String> {
        tokio::time::timeout(remaining, self.stop(root, task, never)).await
            .map_err(|_| "original stop allowance exhausted; not retried".to_string())?
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct PauseStage {
    pub stage: &'static str,
    pub message: String,
}
pub type PauseObserver = Arc<dyn Fn(PauseStage) + Send + Sync>;
#[derive(Default)]
pub struct PauseOptions<'a> {
    pub expected_task_id: Option<&'a str>,
    pub expected_launch_attempt_id: Option<&'a str>,
    pub expected_launch_absent: bool,
    pub remaining: Option<Duration>,
    pub expected_remote: Option<bool>,
    pub observer: Option<PauseObserver>,
}
#[derive(Debug)]
pub struct PauseOutcome {
    pub value: Value,
    pub exit_code: i32,
    pub confirmed_stop: Option<ConfirmedTaskStop>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTaskStop {
    pub confirmed: bool,
    pub task_id: String,
    pub task_name: String,
    pub is_remote: bool,
    pub stop_confirmed_at: i64,
    pub confirmation_basis: StopConfirmationBasis,
}

// Ported directly from the native command parser. Do not deserialize through
// serde_json::Value: doing so would silently discard duplicate object keys.
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
pub enum StopConfirmationBasis {
    RunnersExited,
    SessionAbsent,
}
impl StopConfirmationBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunnersExited => "runners_exited",
            Self::SessionAbsent => "session_absent",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopObservation { schema_version: u32, task_name: String, target_remote: bool, session_absent: bool }
/// Verify the original script's unique final read-only reply; no text inference.
pub fn verified_stop_observation(stdout: &str, name: &str, remote: bool) -> Result<bool, String> {
    if stdout.lines().any(|line| line.starts_with("QUANTNIGHT_PAUSE_CONFIRMED ")) {
        if verified_stop_proof(stdout, name, remote)? != StopConfirmationBasis::SessionAbsent {
            return Err("verify-only cannot manufacture V1 historical runner exit".into());
        }
        return Ok(true);
    }
    let prefix = "QUANTNIGHT_STOP_OBSERVED ";
    let markers: Vec<_> = stdout.lines().filter(|line| line.starts_with(prefix)).collect();
    if markers.len() != 1 || stdout.lines().last() != Some(markers[0]) {
        return Err("original verify-only response lacks unique final observation".into());
    }
    let proof: StopObservation = serde_json::from_str(&markers[0][prefix.len()..]).map_err(|e| e.to_string())?;
    if proof.schema_version != 1 || proof.task_name != name || proof.target_remote != remote || proof.session_absent {
        return Err("original verify-only observation identity/schema mismatch".into());
    }
    Ok(false)
}
pub fn verified_stop_proof(
    stdout: &str,
    name: &str,
    remote: bool,
) -> Result<StopConfirmationBasis, String> {
    let receipts: Vec<_> = stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED "))
        .collect();
    if receipts.len() != 1
        || stdout
            .lines()
            .last()
            .and_then(|line| line.trim().strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED "))
            != receipts.first().copied()
    {
        return Err(match receipts.len() {
            0 => "暂停回执校验失败：缺少结构化暂停回执".into(),
            1 => "暂停回执校验失败：结构化暂停回执后仍有输出，最终行应为暂停回执".into(),
            count => format!("暂停回执校验失败：收到{count}条结构化暂停回执，要求唯一回执"),
        });
    }
    if receipts[0].len() > 8192 {
        return Err("暂停回执校验失败：最终回执超出8192字节上限".into());
    }
    let proof: ExecutionStopProof =
        serde_json::from_str(receipts[0]).map_err(|e| format!("暂停回执解析失败：{e}"))?;
    match proof {
        ExecutionStopProof::Exited(ExecutionStopProofV1 {schema_version: 1, task_name,
            target_remote, session_absent: true, runners_exited: true})
            if task_name == name && target_remote == remote => Ok(StopConfirmationBasis::RunnersExited),
        ExecutionStopProof::Absent(SessionAbsentStopProofV2 {schema_version: 2, task_name,
            target_remote, session_absent: true, runners_exited: (), confirmation_basis: StopConfirmationBasis::SessionAbsent})
            if task_name == name && target_remote == remote => Ok(StopConfirmationBasis::SessionAbsent),
        proof => Err(format!("暂停回执校验失败：任务身份、执行主机或停止依据存在字段差异；期望 task_name={name:?}, target_remote={remote}, session_absent=true，V1 runners_exited=true 或 V2 runners_exited=null/confirmation_basis=session_absent；收到 {proof:?}")),
    }
}

fn null(value: Option<&Bson>) -> bool {
    matches!(value, None | Some(Bson::Null | Bson::Undefined))
}
fn text(value: Option<&Bson>) -> Option<&str> {
    match value {
        Some(Bson::String(v) | Bson::Symbol(v)) => Some(v),
        _ => None,
    }
}
fn string_instance(value: Option<&Bson>) -> Option<&str> {
    match value {
        Some(Bson::String(v) | Bson::Symbol(v) | Bson::JavaScriptCode(v)) => Some(v),
        Some(Bson::JavaScriptCodeWithScope(v)) => Some(&v.code),
        _ => None,
    }
}
// PyMongo decodes BSON Int64 as bson.int64.Int64, not exact builtin int.
// The original producerPid/expectedCount gates explicitly use type(x) is int.
fn integer(value: Option<&Bson>) -> Option<i64> {
    match value {
        Some(Bson::Int32(v)) => Some(*v as i64),
        _ => None,
    }
}
fn member(value: Option<&Bson>, choices: &[&str]) -> Result<bool, String> {
    // Original history's initial set lookups occur OUTSIDE its try/except.
    match value {
        Some(Bson::Array(_)) => Err("unhashable type: 'list'".into()),
        Some(Bson::Document(_)) => Err("unhashable type: 'dict'".into()),
        Some(Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_)) => {
            Err("unhashable type: 'Code'".into())
        }
        Some(Bson::RegularExpression(_)) => Err("unhashable type: 'Regex'".into()),
        Some(Bson::Decimal128(_)) => Err("unhashable type: 'Decimal128'".into()),
        _ => Ok(text(value).is_some_and(|v| choices.contains(&v))),
    }
}
fn iso(value: Option<&Bson>) -> Option<DateTime<FixedOffset>> {
    let s = string_instance(value)?;
    // Python rejects lowercase z and leap seconds even though RFC3339 accepts
    // them. Preserve its aware ISO producer forms (including basic date/time).
    if s.ends_with('z') {
        return None;
    }
    let parsed = DateTime::parse_from_rfc3339(s).ok().or_else(|| {
        [
            "%Y-%m-%d %H:%M:%S%.f%:z",
            "%Y%m%dT%H%M%S%.f%z",
            "%Y-%m-%dT%H:%M%:z",
        ]
        .iter()
        .find_map(|format| DateTime::parse_from_str(s, format).ok())
    })?;
    (parsed.timestamp_subsec_nanos() < 1_000_000_000).then_some(parsed)
}
fn id_text(value: Option<&Bson>) -> Option<String> {
    match value {
        Some(Bson::ObjectId(v)) => Some(v.to_hex()),
        Some(Bson::String(v)) => Some(v.clone()),
        _ => None,
    }
}

/// Original positive create/generate/sync/watch replay, never READY inference.
/// Err models an original initial set/key failure; malformed records inside the
/// original try/except return false (normal captured-process stop, not no-launch).
pub fn never_started_history(task: &Document) -> Result<bool, String> {
    if !member(
        task.get("status"),
        &["pending", "ready", "generation_failed"],
    )? || !member(task.get("taskType"), &["regular", "super"])?
    {
        return Ok(false);
    }
    let (Ok(remote), Ok(target)) = (task.get_bool("isRemote"), task.get_bool("targetRemote"))
    else {
        return Ok(false);
    };
    if (remote && !target)
        || string_instance(task.get("auth_profile")).map_or(true, str::is_empty)
        || string_instance(task.get("templatefile")).map_or(true, str::is_empty)
    {
        return Ok(false);
    }
    let name = task.get_str("name").map_err(|_| "'name'".to_string())?;
    if text(task.get("runId")) != Some(format!("mission:{name}").as_str()) {
        return Ok(false);
    }
    for key in [
        "launchAttemptId",
        "launchAttemptAt",
        "launchSettledAttemptId",
        "startedAt",
        "completedAt",
        "pausedAt",
        "consumer_run_id",
        "heartbeatAt",
        "startPreflight",
        "stopConfirmedAt",
        "launchHistory",
        "launchAttempts",
        "launchReceipt",
        "startReceipt",
        "consumerPid",
    ] {
        if !null(task.get(key)) {
            return Ok(false);
        }
    }
    if !remote && (!null(task.get("syncedAt")) || !null(task.get("syncedDigest"))) {
        return Ok(false);
    }
    Ok(replay_history(task, remote).unwrap_or(false))
}
fn replay_history(task: &Document, remote: bool) -> Option<bool> {
    let created = iso(task.get("createdAt"))?;
    let events = task.get_array("events").ok()?;
    let first = events.first()?.as_document()?;
    if text(first.get("command"))? != "create"
        || text(first.get("status"))? != "pending"
        || !first.get_document("detail").ok()?.is_empty()
        || string_instance(first.get("id"))?.is_empty()
    {
        return Some(false);
    }
    let (mut state, mut replay_remote) = ("pending", false);
    let mut active: Option<(String, String)> = None;
    let mut finished: Option<(String, &'static str)> = None;
    let mut last_begin: Option<&Document> = None;
    let mut last_finish = None;
    let mut previous = created;
    let mut seen = HashSet::new();
    let now = Utc::now();
    for (index, event) in events.iter().enumerate() {
        let row = event.as_document()?;
        let stamp = iso(row.get("at"))?;
        if stamp < previous || stamp > now {
            return Some(false);
        }
        let previous_stamp = previous;
        previous = stamp;
        let command = text(row.get("command"))?;
        let status = text(row.get("status"))?;
        if index == 0 {
            continue;
        }
        let detail = row.get_document("detail").ok();
        if command == "create"
            && status == "idempotent"
            && detail.is_some_and(Document::is_empty)
            && ["pending", "ready", "generating", "generation_failed"].contains(&state)
        {
            continue;
        }
        if command == "generation-begin"
            && status == "generating"
            && ["pending", "ready", "generation_failed"].contains(&state)
        {
            let detail = detail?;
            let token = text(detail.get("id"))?;
            let phase = text(detail.get("phase"))?;
            let began = iso(detail.get("startedAt"))?;
            if token.is_empty()
                || seen.contains(token)
                || !["preparing", "writing"].contains(&phase)
                || integer(detail.get("producerPid"))? <= 1
                || began < previous_stamp
                || began > stamp
            {
                return Some(false);
            }
            seen.insert(token.to_string());
            active = Some((token.into(), phase.into()));
            state = "generating";
            last_begin = Some(detail);
        } else if command == "generation-finish" && state == "generating" && active.is_some() {
            let detail = detail?;
            let (token, phase) = active.as_ref()?;
            if text(detail.get("generationId")) != Some(token.as_str())
                || ![
                    ("pending", "preparing"),
                    ("ready", "writing"),
                    ("generation_failed", "writing"),
                ]
                .contains(&(status, phase.as_str()))
            {
                return Some(false);
            }
            state = status;
            finished = Some((
                token.clone(),
                if status == "pending" {
                    "prepared"
                } else if status == "generation_failed" {
                    "generation_failed"
                } else {
                    "complete"
                },
            ));
            active = None;
            last_finish = Some(stamp);
        } else if command == "watch-attach" && status == "ok" {
            let detail = detail?;
            if text(detail.get("missionId")) != Some(id_text(task.get("_id"))?.as_str())
                || detail.get_bool("remote").ok()? != replay_remote
                || text(detail.get("launchId")) != Some("legacy")
            {
                return Some(false);
            }
        } else if command == "sync-begin"
            && status == "syncing"
            && state == "ready"
            && !replay_remote
        {
            state = "syncing";
        } else if command == "sync" && status == "ready" && state == "syncing" {
            let digest = string_instance(task.get("syncedDigest"))?;
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
                || !truthy(task.get("syncedAt"))
            {
                return Some(false);
            }
            state = "ready";
            replay_remote = true;
        } else {
            return Some(false);
        }
    }
    if Some(state) != text(task.get("status")) || replay_remote != remote || active.is_some() {
        return Some(false);
    }
    let Some((token, phase)) = finished else {
        return Some(null(task.get("generation")) && state == "pending" && !remote);
    };
    let generation = task.get_document("generation").ok()?;
    let begin = last_begin?;
    let began = iso(begin.get("startedAt"))?;
    let ended = iso(generation.get("finishedAt"))?;
    Some(
        text(generation.get("id")) == Some(token.as_str())
            && text(generation.get("phase")) == Some(phase)
            && generation.get("startedAt") == begin.get("startedAt")
            && integer(generation.get("producerPid")) == integer(begin.get("producerPid"))
            && integer(generation.get("producerPid"))? > 1
            && created <= began
            && began <= ended
            && ended <= last_finish?,
    )
}
fn truthy(value: Option<&Bson>) -> bool {
    match value {
        None | Some(Bson::Null | Bson::Undefined) => false,
        Some(Bson::Boolean(v)) => *v,
        Some(Bson::Int32(v)) => *v != 0,
        Some(Bson::Int64(v)) => *v != 0,
        Some(Bson::Double(v)) => *v != 0.0,
        Some(Bson::String(v) | Bson::Symbol(v) | Bson::JavaScriptCode(v)) => !v.is_empty(),
        Some(Bson::JavaScriptCodeWithScope(v)) => !v.code.is_empty(),
        Some(Bson::Binary(v)) => !v.bytes.is_empty(),
        Some(Bson::Array(v)) => !v.is_empty(),
        Some(Bson::Document(v)) => !v.is_empty(),
        _ => true,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Inventory {
    pub host: Host,
    pub exists: bool,
    pub count: usize,
    pub digest: String,
}
fn canonical_bson(value: &Bson) -> Bson {
    match value {
        Bson::Document(doc) => Bson::Document(canonical_document(doc)),
        Bson::Array(values) => Bson::Array(values.iter().map(canonical_bson).collect()),
        Bson::JavaScriptCodeWithScope(code) => {
            Bson::JavaScriptCodeWithScope(bson::JavaScriptCodeWithScope {
                code: code.code.clone(),
                scope: canonical_document(&code.scope),
            })
        }
        other => other.clone(),
    }
}
fn canonical_document(row: &Document) -> Document {
    let mut entries: Vec<_> = row.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(key, value)| (key.clone(), canonical_bson(value)))
        .collect()
}
async fn full_digest(rows: &mut [Document]) -> Result<String, String> {
    // IDs have already been checked below, but never let an incomplete sort
    // accidentally manufacture equal evidence when a future caller skips it.
    for row in rows.iter() {
        row.get_object_id("_id")
            .map_err(|_| "item ObjectId required for inventory digest".to_string())?;
    }
    rows.sort_by_key(|row| row.get_object_id("_id").unwrap());
    let mut digest = Sha256::new();
    for row in rows {
        let bytes = bson::to_vec(&canonical_document(row)).map_err(|e| e.to_string())?;
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
        // Cooperate with the existing inventory deadline/UI executor between
        // complete rows; never truncate the candidate set or spawn a collector.
        tokio::task::yield_now().await;
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn empty_history_value(value: Option<&Bson>) -> bool {
    match value {
        None | Some(Bson::Null | Bson::Undefined) => true,
        Some(Bson::String(v) | Bson::Symbol(v)) => v.is_empty(),
        Some(Bson::Array(v)) => v.is_empty(),
        Some(Bson::Document(v)) => v.is_empty(),
        _ => false,
    }
}
fn initial_item(row: &Document, task: &Document) -> Result<(), String> {
    let id = row
        .get_object_id("_id")
        .map_err(|_| "item ObjectId unavailable; never-started proof refused".to_string())?;
    if text(row.get("status")) != Some("pending")
        || text(row.get("runId")) != text(task.get("runId"))
        || policy::checked_receipt_evidence(row)?
        || !(null(row.get("acceptance")) || text(row.get("acceptance")) == Some("NOT_POSTED"))
        || !null(row.get("terminalStatus"))
        || !(null(row.get("_not_posted_")) || row.get_bool("_not_posted_").ok() == Some(true))
    {
        return Err(format!(
            "item {id} has dispatch/consumer evidence or illegal initial state"
        ));
    }
    for key in [
        "heartbeatAt",
        "finishedAt",
        "simulationCompletedAt",
        "startedAt",
        "attemptCounter",
        "batchBackendIds",
        "batchChildCount",
        "alpha_id",
        "alpha_ids",
        "simulationId",
        "submissionId",
        "simulation_id",
        "platformSimulationId",
        "responseCode",
        "retryAfter",
        "firstResponse",
        "responseJson",
        "responseBody",
        "errorClass",
        "error",
        "errorMessage",
        "result",
        "refine_result",
        "receipt",
        "receipts",
        "submission",
        "simulation",
        "requestId",
        "request_id",
        "terminal_status",
        "alpha",
        "simulated_at",
        "simulatedAt",
        "submissionReceipt",
        "refine_campaign_id",
        "borrowed",
        "continuationSources",
    ] {
        if !empty_history_value(row.get(key)) {
            return Err(format!("item {id} has dispatch/consumer evidence in {key}"));
        }
    }
    Ok(())
}
pub async fn never_started_inventory(
    store: &dyn TaskStore,
    task: &Document,
) -> Result<Vec<Inventory>, String> {
    if !never_started_history(task)? {
        return Err("complete original never-started lifecycle evidence unavailable".into());
    }
    let name = task.get_str("name").map_err(|e| e.to_string())?;
    let remote = task.get_bool("isRemote").map_err(|e| e.to_string())?;
    let hosts = if remote {
        vec![Host::Local, Host::Remote]
    } else {
        vec![Host::Local]
    };
    let mut inventory = Vec::new();
    for host in hosts {
        let entry = tokio::time::timeout(Duration::from_secs(20), async {
            store.ping(host).await?;
            let controls = store
                .find_tasks(host, doc! {"name": name, "status": {"$ne": "deactive"}})
                .await?;
            if host == Host::Remote {
                if !controls.is_empty()
                    && (controls.len() != 1
                        || controls[0].get("_id") != task.get("_id")
                        || !never_started_history(&controls[0])?)
                {
                    return Err("remote task identity/history uncertain".into());
                }
            } else if controls != [task.clone()] {
                return Err("original task identity/history changed or ambiguous".into());
            }
            if store.consumer_capability_exists(host, name).await? {
                return Err(
                    "consumer capability history exists; never-started proof refused".into(),
                );
            }
            let exists = store
                .collection_names(host)
                .await?
                .iter()
                .any(|v| v == name);
            // Intentionally full rows: this positive no-launch proof compares
            // arbitrary item fields across OS verification, not just status/IDs.
            let mut rows = store.rows(host, name, None).await?;
            for row in &rows {
                initial_item(row, task)?;
            }
            let count = rows.len();
            if text(task.get("status")) == Some("ready")
                && (!exists
                    || count == 0
                    || integer(task.get("expectedCount")) != Some(count as i64))
            {
                return Err("ready generation count is not positively verified".into());
            }
            if text(task.get("status")) == Some("generation_failed") && count != 0 {
                return Err(
                    "failed generation has items; pre-registration no-launch proof unavailable"
                        .into(),
                );
            }
            Ok::<Inventory, String>(Inventory {
                host,
                exists,
                count,
                digest: full_digest(&mut rows).await?,
            })
        })
        .await
        .map_err(|_| {
            "never-started inventory timeout (original per-host budget20s); proof unavailable"
                .to_string()
        })??;
        inventory.push(entry);
    }
    if remote
        && (inventory[0].count != inventory[1].count || inventory[0].digest != inventory[1].digest)
    {
        return Err("never-started local/remote inventory mismatch".into());
    }
    Ok(inventory)
}

fn stage(observer: &Option<PauseObserver>, id: &'static str, message: impl Into<String>) {
    if let Some(observer) = observer {
        observer(PauseStage {
            stage: id,
            message: message.into(),
        });
    }
}
fn tail(value: &str) -> String {
    value
        .chars()
        .rev()
        .take(1000)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
fn envelope(name: &str, ok: bool) -> Value {
    json!({"schemaVersion":1,"command":"pause","task":name,"ok":ok,"at":Utc::now().to_rfc3339()})
}
async fn active_task(store: &dyn TaskStore, name: &str) -> Result<Document, String> {
    store
        .find_task(
            Host::Local,
            doc! {"name":name,"status":{"$ne":"deactive"}},
            true,
        )
        .await?
        .ok_or_else(|| "task not found".into())
}
fn pause_filter(name: &str, task: &Document) -> Result<Document, String> {
    let mut filter = policy::task_binding_query(name, task, "pause")?;
    for key in [
        "status",
        "isRemote",
        "updatedAt",
        "launchAttemptId",
        "launchAttemptAt",
        "launchSettledAttemptId",
        "startedAt",
        "generation",
        "startupBinding",
        "tmuxContext",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "stopConfirmationBasis",
        "acceptedObservation",
    ] {
        filter.insert(
            key,
            task.get(key)
                .cloned()
                .unwrap_or_else(|| Bson::Document(doc! {"$exists":false})),
        );
    }
    Ok(filter)
}
fn remaining(deadline: tokio::time::Instant) -> Result<Duration, String> {
    let value = deadline.saturating_duration_since(tokio::time::Instant::now());
    if value.is_zero() { Err("original operation deadline exhausted; no additional attempt".into()) }
    else { Ok(value) }
}
#[derive(Clone, Copy)]
pub(crate) enum StopIntent { StopAllowed, VerifyOnly }
fn early_failure(name: &str, observer: &Option<PauseObserver>, error: String) -> PauseOutcome {
    stage(observer, "failed", error.clone());
    let mut value = envelope(name, false);
    value["reason"] = json!("pause_unconfirmed"); value["stopConfirmation"] = json!("unknown");
    value["error"] = json!(error);
    PauseOutcome { value, exit_code: 1, confirmed_stop: None }
}
pub async fn pause(root: &Path, store: &dyn TaskStore, runtime: &dyn PauseRuntime,
    name: &str, options: PauseOptions<'_>) -> PauseOutcome {
    let allowance=options.remaining.unwrap_or(runtime.script_budget().duration()).min(runtime.script_budget().duration());
    pause_with_deadline(root,store,runtime,name,options,tokio::time::Instant::now()+allowance).await
}
pub(crate) async fn pause_with_deadline(root:&Path,store:&dyn TaskStore,runtime:&dyn PauseRuntime,
    name:&str,options:PauseOptions<'_>,deadline:tokio::time::Instant)->PauseOutcome {
    let budget = runtime.script_budget();
    let deadline=deadline.min(tokio::time::Instant::now()+budget.duration());
    stage(&options.observer, "initial", format!(
        "收到请求，尚未取得生命周期锁/尚未执行停止; original stop budget{}s; one attempt", budget.seconds()));
    if let Err(error) = policy::validate_name(name) { return early_failure(name, &options.observer, error); }
    let guard = match MissionLocks::acquire(root, &[name], false) {
        Ok(guard) => guard, Err(error) => return early_failure(name, &options.observer, error),
    };
    pause_locked(root, store, runtime, name, options, &guard, deadline, StopIntent::StopAllowed).await
}
/// Private composition requires an already-owned guard, not an external skip-lock flag.
pub(crate) async fn pause_locked(
    root: &Path, store: &dyn TaskStore, runtime: &dyn PauseRuntime, name: &str,
    options: PauseOptions<'_>, _guard: &MissionLocks, deadline: tokio::time::Instant,
    intent: StopIntent,
) -> PauseOutcome {
    if let Err(error)=remaining(deadline){return early_failure(name,&options.observer,error);}
    let mut output: Option<StopOutput> = None;
    let observer = &options.observer;
    let mut reconciled_reason: Option<String> = None;
    let result: Result<(ConfirmedTaskStop,String),String> = tokio::time::timeout_at(deadline, async {
        policy::validate_name(name)?;
        stage(observer,"identity","verify original control task and launch binding");
        let task = active_task(store,name).await?;
        let id = task.get_object_id("_id").map_err(|_| "original task ObjectId unavailable".to_string())?;
        if task.get_str("name").ok() != Some(name) { return Err("task name binding changed".into()); }
        if options.expected_task_id.is_some_and(|v| v != id.to_hex()) { return Err("task identity changed before mutation".into()); }
        if options.expected_launch_attempt_id.is_some_and(|v|v.trim().is_empty()) ||
            (options.expected_launch_absent && options.expected_launch_attempt_id.is_some()) {
            return Err("exact launch must be nonempty and mutually exclusive with absent expectation".into());
        }
        match task.get("launchAttemptId") {
            None|Some(Bson::Null) if options.expected_launch_attempt_id.is_none()=>(),
            Some(Bson::String(actual)) if !actual.trim().is_empty() && !options.expected_launch_absent &&
                options.expected_launch_attempt_id.map_or(true,|expected|expected==actual)=>(),
            _=>return Err("launch identity changed or malformed before mutation".into()),
        }
        policy::assert_observation_closed(&task)?;
        let remote = match task.get("isRemote") { None | Some(Bson::Null | Bson::Boolean(false)) => false, Some(Bson::Boolean(true)) => true, _ => return Err("invalid task execution host".into()) };
        if options.expected_remote.is_some_and(|expected| expected != remote) { return Err("task execution host changed before pause".into()); }
        if matches!(text(task.get("status")),Some("generating"|"generation_unknown"|"syncing")) { return Err("candidate generation is not the simulation consumer; reconcile it before using pause".into()); }
        let inventory = if never_started_history(&task)? {
            stage(observer,"inventory","verify complete original never-started history and all actual item evidence");
            Some(never_started_inventory(store,&task).await?)
        } else { None };
        stage(observer,"stop","read original stop facts; signal at most once within remaining allowance");
        let observed = if inventory.is_some() && matches!(intent, StopIntent::StopAllowed) { None } else {
            runtime.verify_stopped(root, &task, remaining(deadline)?.min(Duration::from_secs(12))).await?
        };
        let mut verification_task=task.clone();
        verification_task.remove("observedExecutionBeforeStop");
        output = if let Some(proof) = observed { Some(proof) } else {
            if matches!(intent, StopIntent::VerifyOnly) {
                return Err("original verify-only did not prove stopped; explicit stop intent required".into());
            }
            if inventory.is_none() {
                if let Some(observation)=runtime.inspect_before_stop(root,store,&task,
                        remaining(deadline)?.min(Duration::from_secs(12))).await? {
                    let observed=crate::start::config_document(observation)?;
                    if observed.get_bool("identityVerified").ok()==Some(true) || observed.get_bool("explicitPauseVerified").ok()==Some(true){
                        if let Ok(context)=observed.get_document("tmuxContext"){verification_task.insert("tmuxContext",context.clone());}
                    }
                    verification_task.insert("observedExecutionBeforeStop",observed);
                }
            }
            let attempt = runtime.stop_bounded(root, &verification_task, inventory.is_some(), remaining(deadline)?).await;
            let attempt = attempt.and_then(|out| {
                verified_stop_proof(&out.stdout, name, remote).map(|_| out)
            });
            match attempt {
                Ok(out) => Some(out),
                Err(first) => {
                    // Explicit counter-evidence from the original captured tree
                    // cannot be erased by a weaker session-only observation.
                    if first.contains("known_original_runners_still_live") { return Err(first); }
                    // Exactly one read-only recovery, never another stop signal.
                    let proof = runtime.verify_stopped(root, &verification_task,
                        remaining(deadline)?.min(Duration::from_secs(12))).await
                        .map_err(|next| format!("{first}; original read-only recovery: {next}"))?
                        .ok_or_else(|| format!("{first}; original read-only recovery did not confirm stop"))?;
                    reconciled_reason = Some(first);
                    Some(proof)
                }
            }
        };
        let result = output.as_ref().unwrap();
        stage(observer,"receipt_validate","validate unique final V1/V2 receipt and exact task/host");
        let basis = verified_stop_proof(&result.stdout,name,remote)?;
        // A fresh contradictory process observation cannot be overridden by
        // either receipt version; never send a second stop to "fix" it.
        runtime.confirm_stopped_scope(root,store,&verification_task,
            remaining(deadline)?.min(Duration::from_secs(12))).await?;
        if let Some(before) = inventory {
            stage(observer,"inventory_recheck","re-read original task and all items after process verification");
            let current = active_task(store,name).await?;
            if current != task || never_started_inventory(store,&current).await? != before {
                return Err("never-started task/token/history/items changed during process verification".into());
            }
        }
        let at = bson::DateTime::now(); let now = Utc::now().to_rfc3339();
        let proof = match basis {
            StopConfirmationBasis::RunnersExited => doc! {"schema_version":1,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":true},
            StopConfirmationBasis::SessionAbsent => doc! {"schema_version":2,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":Bson::Null,"confirmation_basis":"session_absent"},
        };
        let mut fields = doc! {"status":"paused","pausedAt":&now,"updatedAt":&now,"stopConfirmedAt":at,
            "stopConfirmedRemote":remote,"stopConfirmedTaskId":id.to_hex(),"stopConfirmationBasis":basis.as_str()};
        if task.get_str("status").ok()==Some("launch_unknown") && !task.contains_key("launchSettledAttemptId") {
            // V1/V2 stops do not settle an old launcher. Preserve the captured
            // legacy unknown fact in this same task/launch CAS before changing
            // its status; do not turn every marker-less legacy task into a hold.
            fields.insert("launchSettledAttemptId",Bson::Null);
        }
        let event = doc! {"id":ObjectId::new().to_hex(),"command":"pause","status":"paused","at":&now,"detail":proof};
        stage(observer,"control_write","CAS original task/launch/stop/observer snapshot; never name-only update");
        let filter = pause_filter(name,&task)?;
        let write = store.update_task(Host::Local, filter.clone(),
            doc! {"$set":fields.clone(),"$push":{"events":event.clone()}}).await;
        if !matches!(write, Ok(1)) {
            let first = write.err().unwrap_or_else(|| "pause control CAS failed: original task/host/launch/revision changed".into());
            // Same generated event + exact committed fields; never re-issue CAS.
            let mut committed = filter;
            committed.extend(fields.clone()); committed.remove("events");
            let row = store.find_task(Host::Local, committed, false).await?;
            if !row.as_ref().and_then(|r| r.get_array("events").ok())
                .is_some_and(|events| events.contains(&Bson::Document(event))) {
                return Err(first);
            }
            reconciled_reason = Some(first);
        }
        stage(observer,"completed","verified stop evidence and control CAS confirmed; not platform cancellation");
        Ok((ConfirmedTaskStop {confirmed:true,task_id:id.to_hex(),task_name:name.into(),is_remote:remote,
            stop_confirmed_at:at.timestamp_millis(),confirmation_basis:basis},now))
    }).await.unwrap_or_else(|_| Err("original pause total deadline exhausted; stop not repeated".into()));
    let mut value = envelope(name, result.is_ok());
    if let Some(reason) = reconciled_reason {
        value["reconciledFromObservation"] = json!(true);
        value["originalError"] = json!(reason);
    }
    let script_returned_success = output.is_some();
    if let Some(output) = output {
        value["stdout"] = json!(tail(&output.stdout));
        value["stderr"] = json!(tail(&output.stderr));
    }
    match result {
        Ok((stop,control_revision)) => {
            value["controlRevision"] = json!(control_revision);
            value["taskStatus"] = json!("paused");
            value["stopConfirmation"] = json!("confirmed");
            value["confirmationBasis"] = json!(stop.confirmation_basis.as_str());
            value["taskId"] = json!(stop.task_id);
            value["remote"] = json!(stop.is_remote);
            PauseOutcome {
                value,
                exit_code: 0,
                confirmed_stop: Some(stop),
            }
        }
        Err(error) => {
            stage(observer, "failed", error.clone());
            value["reason"] = json!("pause_unconfirmed");
            value["stopConfirmation"] = json!("unknown");
            value["error"] = json!(error);
            if script_returned_success {
                value["returncode"] = json!(0);
            }
            PauseOutcome {
                value,
                exit_code: 1,
                confirmed_stop: None,
            }
        }
    }
}
