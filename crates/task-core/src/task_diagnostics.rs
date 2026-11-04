//! Original bounded GUI runtime/dispatch/business diagnostics, not CLI status.
use crate::{
    config::{
        run_bash_script_observed_with_env_policy, AppConfig, BashScriptEvent, BashScriptObserver,
    },
    runtime::{RuntimeEvent, RuntimeObserver, TaskRuntimeContext},
};
use bson::{doc, oid::ObjectId, Document};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::timeout;

const DISPATCH_MARKER: &str = "QUANTNIGHT_DISPATCH_STATE ";
const DISPATCH_MAX_AGE_MS: i64 = 120_000;
const LEGACY_RECONCILIATION_REASON_PREFIX: &str =
    "请求结果待核验；暂停新请求，已接受任务继续采集。";

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
        _ => dispatch_integer(doc, key)
            .filter(|n| (0..=9_007_199_254_740_991).contains(n))
            .map(Some),
    }
}

fn dispatch_reason(doc: &Document, key: &str) -> Option<Option<String>> {
    match doc.get(key)? {
        bson::Bson::Null => Some(None),
        bson::Bson::String(s)
            if !s.trim().is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control) =>
        {
            Some(Some(s.clone()))
        }
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

fn validated_dispatch_state(
    cap: &Document,
    task: &Document,
    id: ObjectId,
    name: &str,
    remote: bool,
    now: i64,
) -> Option<DispatchState> {
    let profile = task.get_str("auth_profile").ok()?;
    if profile.is_empty()
        || profile.len() > 100
        || !profile
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || task.get_object_id("_id").ok()? != id
        || task.get_str("name").ok()? != name
        || task.get_str("status").ok()? != "running"
        || task.get_bool("isRemote").ok()? != remote
        || task.get_str("taskType").ok()? != "regular"
        || cap.get_str("_id").ok()? != format!("{profile}:{name}")
        || cap.get_str("task").ok()? != name
        || cap.get_str("source_collection").ok()? != name
        || cap.get_str("source_task_id").ok()? != id.to_hex()
        || cap.get_bool("source_remote").ok()? != remote
        || cap.get_str("auth_profile").ok()? != profile
        || cap.get_str("taskType").ok()? != "regular"
        || cap.get_str("dist").ok()? != (if remote { "remote" } else { "local" })
        || dispatch_integer(cap, "protocol_version")? < 3
        || cap.get_str("heartbeat_clock").ok()? != "utc"
    {
        return None;
    }
    let heartbeat = cap.get_datetime("heartbeatAt").ok()?.timestamp_millis();
    if heartbeat <= 0 || heartbeat > now + 5_000 || now - heartbeat >= DISPATCH_MAX_AGE_MS {
        return None;
    }
    let pid = u32::try_from(dispatch_integer(cap, "pid")?)
        .ok()
        .filter(|n| *n > 1)?;
    let run = cap.get_str("consumer_run_id").ok()?;
    if run.len() != 32 || !run.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let state = cap.get_str("submissionState").ok()?;
    if !["ready", "waiting_reconciliation", "waiting_quota_reset"].contains(&state) {
        return None;
    }
    let supplied_reason = dispatch_reason(cap, "submissionReason")?;
    if state != "ready" && supplied_reason.is_none() {
        return None;
    }
    let quota = cap.get_document("quota").ok()?;
    let reason = dispatch_reason(quota, "reason")?;
    if quota.get_str("dispatch_state").ok()? != state {
        return None;
    }
    // One exact historical writer format, not arbitrary display text. Keep the
    // quota's raw reason authoritative and normalize the DTO to that same raw
    // value. No trimming, partial matches, or compatibility for other states.
    let exact_legacy_reason = state == "waiting_reconciliation"
        && match (supplied_reason.as_deref(), reason.as_deref()) {
            (Some(supplied), Some(raw)) => {
                supplied == format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{raw}")
            }
            _ => false,
        };
    if supplied_reason != reason && !exact_legacy_reason {
        return None;
    }
    let stopped = quota.get_bool("stopped").ok()?;
    if stopped != (state == "waiting_quota_reset") {
        return None;
    }
    let limit = dispatch_counter(quota, "limit")?;
    let remaining = dispatch_counter(quota, "remaining")?;
    let floor = dispatch_counter(quota, "stop_floor")?;
    if limit == Some(0)
        || remaining.zip(limit).map_or(false, |(r, l)| r > l)
        || floor.zip(limit).map_or(false, |(f, l)| f > l)
        || (state == "waiting_quota_reset"
            && (limit.is_none() || remaining.is_none() || floor.is_none()))
    {
        return None;
    }
    let reset_at = dispatch_reset(cap)?;
    if dispatch_reset(quota)? != reset_at {
        return None;
    }
    Some(DispatchState {
        schema_version: 1,
        task_id: id.to_hex(),
        name: name.into(),
        is_remote: remote,
        auth_profile: profile.into(),
        submission_state: state.into(),
        submission_reason: reason,
        heartbeat_at: heartbeat,
        consumer_run_id: run.into(),
        pid,
        quota: DispatchQuota {
            limit,
            remaining,
            stop_floor: floor,
            budget_confirmed: quota.get_bool("budget_confirmed").ok()?,
        },
        reset_at,
    })
}

// Diagnostic metadata shares the original capability/runtime read. It never
// changes task state, receipt authority or the supervisor's retry schedule.
const RUNTIME_HISTORY_MARKER: &str = "QUANTNIGHT_RUNTIME_HISTORY ";
const BUSINESS_DIAGNOSTIC_MARKER: &str = "QUANTNIGHT_BUSINESS_DIAGNOSTIC ";
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BusinessDiagnostic {
    schema_version: u8,
    task_id: String,
    name: String,
    is_remote: bool,
    auth_profile: String,
    source: String,
    source_detail: Option<String>,
    consumer_run_id: String,
    launch_attempt_id: Option<String>,
    launch_matched: bool,
    heartbeat_at: i64,
    occurred_at: i64,
    error_class: String,
    reason: String,
    phase: String,
    scope: String,
    timeout_seconds: Option<f64>,
    recovery_state: String,
    next_retry_at: Option<i64>,
}

pub fn safe_diagnostic_text(value: &str) -> String {
    // Remove complete capability URLs (including webhook paths), not only query
    // strings. Defense in depth even when the original writer already sanitizes.
    let urls = regex::Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://[^\s<>"']+"#).unwrap();
    let secrets = regex::Regex::new(r"(?i)\b(authorization|cookie|password|passwd|token|secret|api[_-]?key)\s*[:=]\s*[^,;]+|\bBearer\s+\S+").unwrap();
    let text = urls.replace_all(value, "[redacted-uri]");
    secrets
        .replace_all(&text, "[redacted-credential]")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(1024)
        .collect()
}

fn diagnostic_time(doc: &Document, key: &str) -> Option<i64> {
    match doc.get(key)? {
        bson::Bson::DateTime(t) => Some(t.timestamp_millis()),
        bson::Bson::String(s) => {
            let t = chrono::DateTime::parse_from_rfc3339(s).ok()?;
            (t.offset().local_minus_utc() == 0).then_some(t.timestamp_millis())
        }
        _ => None,
    }
}

fn diagnostic_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn validated_business_diagnostic(
    runtime: &Document,
    task: &Document,
    id: ObjectId,
    name: &str,
    remote: bool,
    now: i64,
) -> Option<BusinessDiagnostic> {
    let profile = task.get_str("auth_profile").ok()?;
    let kind = task.get_str("taskType").ok()?;
    if !diagnostic_id(profile)
        || task.get_object_id("_id").ok()? != id
        || task.get_str("name").ok()? != name
        || task.get_bool("isRemote").ok()? != remote
        || task.get_str("status").ok()? == "deactive"
        || runtime.get_str("auth_profile").ok()? != profile
        || runtime.get_str("taskType").ok()? != kind
        || runtime.get_str("dist").ok()? != if remote { "remote" } else { "local" }
    {
        return None;
    }
    if kind == "regular" {
        if runtime.get_str("_id").ok()? != format!("{profile}:{name}")
            || runtime.get_str("task").ok()? != name
            || runtime.get_str("source_collection").ok()? != name
            || runtime.get_str("source_task_id").ok()? != id.to_hex()
            || runtime.get_bool("source_remote").ok()? != remote
            || runtime.get_str("heartbeat_clock").ok()? != "utc"
            || dispatch_integer(runtime, "protocol_version")? < 3
        {
            return None;
        }
    } else if kind != "super" {
        return None;
    }
    u32::try_from(dispatch_integer(runtime, "pid")?)
        .ok()
        .filter(|pid| *pid > 1)?;
    let d = runtime.get_document("runtimeDiagnostic").ok()?;
    if dispatch_integer(d, "schemaVersion")? != 1 {
        return None;
    }
    let run = runtime.get_str("consumer_run_id").ok()?;
    if run.len() != 32
        || !run.bytes().all(|b| b.is_ascii_hexdigit())
        || d.get_str("consumerRunId").ok()? != run
    {
        return None;
    }
    let heartbeat = diagnostic_time(runtime, "heartbeatAt")?;
    let occurred = diagnostic_time(d, "occurredAt")?;
    if occurred <= 0 || occurred > now || heartbeat < occurred || heartbeat > now + 5000 {
        return None;
    }
    // A newer normal start fences old evidence, even if an old consumer wrote
    // a recently timestamped heartbeat. Missing legacy launch proof is history.
    let launch = d
        .get_str("launchAttemptId")
        .ok()
        .filter(|v| diagnostic_id(v));
    let task_launch = task.get_str("launchAttemptId").ok();
    if launch.zip(task_launch).map_or(false, |(a, b)| a != b) {
        return None;
    }
    let mut launch_matched = launch.zip(task_launch).map_or(false, |(a, b)| a == b);
    if kind == "super" {
        let binding = task.get_document("startupBinding").ok()?;
        if binding.get_str("taskId").ok()? != id.to_hex()
            || binding.get_str("name").ok()? != name
            || binding.get_str("auth_profile").ok()? != profile
            || binding.get_bool("remote").ok()? != remote
            || binding.get_str("consumer_run_id").ok()? != run
        {
            return None;
        }
        launch_matched = launch_matched && binding.get_str("launchAttemptId").ok() == launch;
        if !launch_matched {
            return None;
        }
    }
    for key in ["launchAttemptAt", "startedAt"] {
        if diagnostic_time(task, key).map_or(false, |at| occurred < at) {
            return None;
        }
        if task.contains_key(key) && diagnostic_time(task, key).is_none() {
            launch_matched = false;
        }
    }
    let class = d.get_str("errorClass").ok()?;
    let phase = d.get_str("phase").ok()?;
    let scope = d.get_str("scope").ok()?;
    if !diagnostic_id(class)
        || !diagnostic_id(phase)
        || !["item", "accepted_receipts", "new_dispatch", "consumer"].contains(&scope)
    {
        return None;
    }
    let reason = d.get_str("reason").ok()?;
    let recovery = d.get_str("recoveryState").ok()?;
    if reason.trim().is_empty()
        || reason.len() > 8192
        || recovery.trim().is_empty()
        || recovery.len() > 1024
    {
        return None;
    }
    let budget = match d.get("timeoutSeconds") {
        None | Some(bson::Bson::Null) => None,
        Some(bson::Bson::Double(n)) if n.is_finite() && *n > 0.0 && *n <= 86400.0 => Some(*n),
        _ => Some(dispatch_integer(d, "timeoutSeconds").filter(|n| *n > 0 && *n <= 86400)? as f64),
    };
    let retry = match d.get("nextRetryAt") {
        None | Some(bson::Bson::Null) => None,
        _ => Some(diagnostic_time(d, "nextRetryAt")?),
    };
    if retry.map_or(false, |t| t < occurred || t > 8_640_000_000_000_000) {
        return None;
    }
    Some(BusinessDiagnostic {
        schema_version: 1,
        task_id: id.to_hex(),
        name: name.into(),
        is_remote: remote,
        auth_profile: profile.into(),
        source: if kind == "super" {
            "simulatorRuntime"
        } else {
            "priority_capability"
        }
        .into(),
        source_detail: None,
        consumer_run_id: run.into(),
        launch_attempt_id: launch.map(str::to_owned),
        launch_matched,
        heartbeat_at: heartbeat,
        occurred_at: occurred,
        error_class: class.into(),
        reason: safe_diagnostic_text(reason),
        phase: phase.into(),
        scope: scope.into(),
        timeout_seconds: budget,
        recovery_state: safe_diagnostic_text(recovery),
        next_retry_at: retry,
    })
}

fn validated_history_diagnostic(
    stdout: &str,
    task: &Document,
    id: ObjectId,
    name: &str,
    remote: bool,
    now: i64,
) -> Option<BusinessDiagnostic> {
    // Only the trusted original checker emits this envelope; captured pane text
    // itself is never relayed as a status/identity marker. History is not proof
    // of a task exit cause and never becomes a current dispatch/recovery state.
    let markers: Vec<_> = stdout
        .lines()
        .filter(|l| l.contains("QUANTNIGHT_RUNTIME_HISTORY"))
        .collect();
    if markers.len() != 1
        || !markers[0].starts_with(RUNTIME_HISTORY_MARKER)
        || markers[0].len() > 8192
    {
        return None;
    }
    let raw: JsonValue =
        serde_json::from_str(markers[0].strip_prefix(RUNTIME_HISTORY_MARKER)?).ok()?;
    let d = raw.as_object()?;
    if d.len() != 10
        || d.get("schemaVersion")?.as_u64()? != 1
        || d.get("name")?.as_str()? != name
        || d.get("isRemote")?.as_bool()? != remote
        || operation_task_identity(task, name, remote).ok()? != id
    {
        return None;
    }
    let process = runtime_process_output(stdout, "").ok()?;
    let negative = regex::Regex::new(r"^❌ Pane [0-9]+ \(pid=[1-9][0-9]*\) (is running: (bash|sh|zsh|sleep)|exited \(retained tmux pane\))$").ok()?;
    if process.lines().count() != 1 || !negative.is_match(process.trim()) {
        return None;
    }
    let sid = d.get("sessionId")?.as_str()?;
    if !regex::Regex::new(r"^\$[0-9]+$").ok()?.is_match(sid) {
        return None;
    }
    let created = d.get("sessionCreatedAt")?.as_i64()?;
    let occurred = d.get("occurredAt")?.as_i64()?;
    if created <= 0 || occurred < created || occurred > now {
        return None;
    }
    // A retained pane from before task creation/start cannot supply this task's
    // history. Legacy launches with no timestamp remain explicitly unconfirmed.
    for key in ["createdAt", "startedAt", "launchAttemptAt"] {
        if diagnostic_time(task, key).map_or(false, |at| created + 1000 < at || occurred < at) {
            return None;
        }
    }
    let class = d.get("errorClass")?.as_str()?;
    let phase = d.get("phase")?.as_str()?;
    let reason = d.get("reason")?.as_str()?;
    if !class.starts_with("simulation_")
        || !diagnostic_id(class)
        || !["simulation_recovery_phase", "startup_recovery"].contains(&phase)
        || reason.trim().is_empty()
        || reason.len() > 4096
    {
        return None;
    }
    let budget = if d.get("timeoutSeconds")?.is_null() {
        None
    } else {
        Some(
            d.get("timeoutSeconds")?
                .as_i64()
                .filter(|v| *v > 0 && *v <= 86400)?,
        )
    };
    Some(BusinessDiagnostic { schema_version: 1, task_id: id.to_hex(), name: name.into(), is_remote: remote,
        auth_profile: task.get_str("auth_profile").ok()?.into(), source: "tmux_log".into(),
        source_detail: Some(format!("exact session {sid}; createdAt={created}; bounded fixed logger record; launch unconfirmed")),
        consumer_run_id: "unavailable".into(), launch_attempt_id: None, launch_matched: false,
        heartbeat_at: occurred, occurred_at: occurred, error_class: class.into(), reason: safe_diagnostic_text(reason),
        phase: phase.into(), scope: "accepted_receipts".into(), timeout_seconds: budget.map(|n| n as f64),
        recovery_state: "历史启动恢复错误；当前恢复状态与退出因果未核验".into(), next_retry_at: None })
}

async fn read_runtime_evidence_with<F, Fut>(
    client: F,
    task_id: &str,
    name: &str,
    remote: bool,
) -> (Option<DispatchState>, Option<BusinessDiagnostic>)
where
    F: Fn(bool) -> Fut,
    Fut: std::future::Future<Output = Result<Arc<mongodb::Client>, String>>,
{
    let read = async {
        let id = ObjectId::parse_str(task_id).ok()?;
        let filter = doc! { "_id": id, "name": name, "status": { "$ne": "deactive" } };
        let local_client = client(false).await.ok()?;
        let local = local_client
            .database("simulation_mission")
            .collection::<Document>("tasks");
        let task = match local.find_one(filter.clone(), None).await.ok()? {
            Some(task) => task,
            None if remote => client(true)
                .await
                .ok()?
                .database("simulation_mission")
                .collection::<Document>("tasks")
                .find_one(filter.clone(), None)
                .await
                .ok()??,
            None => return None,
        };
        let profile = task.get_str("auth_profile").ok()?;
        if task.get_bool("isRemote").ok()? != remote {
            return None;
        }
        let client = client(remote).await.ok()?;
        let runtime = if task.get_str("taskType").ok()? == "super" {
            let execution = if remote {
                client
                    .database("simulation_mission")
                    .collection::<Document>("tasks")
                    .find_one(filter, None)
                    .await
                    .ok()??
            } else {
                task.clone()
            };
            // Execution-side proof must be exactly the original control launch.
            for key in [
                "auth_profile",
                "isRemote",
                "startupBinding",
                "launchAttemptId",
            ] {
                if execution.get(key) != task.get(key) {
                    return None;
                }
            }
            execution.get_document("simulatorRuntime").ok()?.clone()
        } else {
            client.database("simulation_mission").collection::<Document>("priority_consumer_capabilities")
                .find_one(doc! { "_id": format!("{profile}:{name}"), "source_task_id": id.to_hex(), "source_remote": remote }, None)
                .await.ok()??
        };
        let now = bson::DateTime::now().timestamp_millis();
        Some((
            validated_dispatch_state(&runtime, &task, id, name, remote, now),
            validated_business_diagnostic(&runtime, &task, id, name, remote, now),
        ))
    };
    timeout(Duration::from_secs(2), read)
        .await
        .ok()
        .flatten()
        .unwrap_or((None, None))
}

async fn read_runtime_evidence(
    context: &TaskRuntimeContext,
    task_id: &str,
    name: &str,
    remote: bool,
) -> (Option<DispatchState>, Option<BusinessDiagnostic>) {
    read_runtime_evidence_with(|remote| context.client(remote), task_id, name, remote).await
}

fn business_diagnostic_output(stdout: &str, diagnostic: Option<BusinessDiagnostic>) -> String {
    let mut raw = stdout
        .lines()
        .filter(|line| !line.contains("QUANTNIGHT_BUSINESS_DIAGNOSTIC"))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(diagnostic) = diagnostic {
        if let Ok(json) = serde_json::to_string(&diagnostic) {
            raw.push_str(&format!("\n{BUSINESS_DIAGNOSTIC_MARKER}{json}"));
        }
    }
    raw
}

fn dispatch_status_output(stdout: &str, dispatch: Option<DispatchState>) -> String {
    // Strip ALL untrusted marker-bearing lines, not just the first/last one.
    // Keep non-marker diagnostics so an error cannot become a positive probe.
    let mut raw = stdout
        .lines()
        .filter(|line| !line.contains("QUANTNIGHT_DISPATCH_STATE"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if let Some(dispatch) = dispatch {
        if let Ok(json) = serde_json::to_string(&dispatch) {
            raw.push_str(&format!("\n{DISPATCH_MARKER}{json}"));
        }
    }
    raw
}

const TASK_STAGE_PREFIX: &str = "QUANTNIGHT_TASK_STAGE ";
pub(crate) const RUNTIME_BUDGET: Duration = Duration::from_secs(12);
tokio::task_local! {static CALL_REMAINING_BUDGET:Duration;}
pub(crate) async fn with_call_budget<T>(budget:Option<Duration>,future:impl std::future::Future<Output=T>)->T{if let Some(budget)=budget{CALL_REMAINING_BUDGET.scope(budget.min(RUNTIME_BUDGET),future).await}else{future.await}}
#[cfg(test)]
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
pub struct TaskOperationStage {
    pub operation: String,
    pub task_id: String,
    pub name: String,
    pub is_remote: bool,
    pub request_id: String,
    pub stage: String,
    pub message: String,
}

#[derive(Clone)]
pub struct TaskOperationProgress {
    pub operation: &'static str,
    pub task_id: String,
    pub name: String,
    pub remote: bool,
    pub request_id: String,
    pub tmux_binding: Option<String>,
    pub last: Arc<Mutex<(String, String)>>,
    pub sink: Arc<dyn Fn(TaskOperationStage) + Send + Sync>,
}

impl TaskOperationProgress {
    pub fn new(
        sink: Arc<dyn Fn(TaskOperationStage) + Send + Sync>,
        operation: &'static str,
        task_id: Option<&str>,
        name: &str,
        remote: bool,
        request_id: Option<String>,
    ) -> Self {
        Self {
            operation,
            task_id: task_id.unwrap_or_default().into(),
            name: name.into(),
            remote,
            request_id: request_id.unwrap_or_default(),
            tmux_binding: None,
            last: Arc::new(Mutex::new(("initial".into(), "原生桥已接收请求".into()))),
            sink,
        }
    }

    pub fn stage(&self, stage: &str, message: &str) {
        // Serialize updates and emission, so no stale callback can overtake a
        // receipt/control-write phase. stdout cannot supply native identity.
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let message = safe_diagnostic_text(message);
        *last = (stage.into(), message.clone());
        // Legacy callers have no correlation token. Keep internal diagnostics
        // and business behavior, but do not emit an unrouteable UI event.
        if self.request_id.is_empty() {
            return;
        }
        (self.sink)(TaskOperationStage {
            operation: self.operation.into(),
            task_id: self.task_id.clone(),
            name: self.name.clone(),
            is_remote: self.remote,
            request_id: self.request_id.clone(),
            stage: stage.into(),
            message,
        });
    }

    pub fn failure(&self, cause: impl std::fmt::Display) -> String {
        let (stage, message) = self.last.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let cause=safe_diagnostic_text(&cause.to_string());
        if cause.starts_with(&format!("{} stage=",self.operation)) {self.stage(&stage,&cause);return cause;}
        let already_prefixed = format!("{} stage={}（", self.operation, stage);
        let error = if message.starts_with(&already_prefixed) {
            safe_diagnostic_text(&format!("{} stage={}：{}", self.operation, stage, cause))
        } else {
            safe_diagnostic_text(&format!(
                "{} stage={}（{}）：{}",
                self.operation, stage, message, cause
            ))
        };
        // Keep the actual failed step, never collapse it into generic unknown.
        self.stage(&stage, &error);
        error
    }

    pub fn script_stage(&self, line: &str) {
        // Parse only a bounded, exact line/schema and operation-specific fixed
        // IDs. Markers are diagnostic hints, NEVER stop/dispatch authority.
        if line.len() > 4096 {
            return;
        }
        let Some(json) = line.strip_prefix(TASK_STAGE_PREFIX) else {
            return;
        };
        let Ok(event) = serde_json::from_str::<ScriptTaskStage>(json) else {
            return;
        };
        if event.operation != self.operation
            || event.task_name != self.name
            || event.target_remote != self.remote
            || event.message.trim().is_empty()
            || event.message.len() > 1024
            || event.message.chars().any(char::is_control)
        {
            return;
        }
        let allowed: &[&str] = match self.operation {
            "runtime" => &[
                "config",
                "ssh",
                "remote_environment",
                "tmux",
                "process",
                "receipt",
                "session_absent",
                "complete",
            ],
            "stop" => &[
                "config",
                "ssh",
                "remote_environment",
                "tmux",
                "process",
                "interrupt",
                "session_exit",
                "process_exit",
                "receipt",
                "session_absent",
                "complete",
            ],
            _ => &[],
        };
        if allowed.contains(&event.stage.as_str()) {
            self.stage(&event.stage, &event.message);
        }
    }

    pub fn observer(&self) -> BashScriptObserver {
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
async fn execute_task_script(
    progress: &TaskOperationProgress,
    config: &AppConfig,
    budget: Duration,
) -> Result<(String, String), String> {
    let (key, diagnostic) = match progress.operation {
        "runtime" => ("check_task_status", true),
        #[cfg(test)] // Original shell regression only; production pause uses TaskControl.
        "stop" => ("pause_task", false),
        _ => return Err(progress.failure("原生任务操作类型无效")),
    };
    // Reuse the shared checked task-local operational context, never the full
    // parent environment or a new caller/config whitelist. These are child-only
    // overrides; original task binding and backend PYTHONPATH remain authoritative.
    let operational = crate::operations_runtime::current_environment();
    let mut environment: Vec<(&str, &str)> = operational
        .as_ref()
        .map(|environment| environment.entries().collect())
        .unwrap_or_default();
    environment.push((
        "QUANTNIGHT_TMUX_BINDING",
        progress.tmux_binding.as_deref().unwrap_or("{}"),
    ));
    environment.push(("PYTHONPATH", config.env.working_dir.as_str()));
    // Scope Some represents both present and absent whitelist values. Clear
    // those child keys first so absence cannot inherit a launchd-host value.
    // Scope None keeps the original legacy inheritance behavior.
    let remove_environment: &[&str] = if operational.is_some() {
        crate::operations_runtime::OPERATION_ENV_KEYS
    } else {
        &[]
    };
    run_bash_script_observed_with_env_policy(
        key,
        &[("task_name", &progress.name)],
        progress.remote,
        config,
        budget,
        diagnostic,
        Some(progress.observer()),
        &environment,
        remove_environment,
    )
    .await
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

pub fn operation_task_identity(
    task: &Document,
    name: &str,
    remote: bool,
) -> Result<ObjectId, String> {
    if task.get_str("name").ok() != Some(name) || task.get_str("status").ok() == Some("deactive") {
        return Err("原任务名称或活动登记已变化".into());
    }
    if task.get_bool("isRemote").unwrap_or(false) != remote {
        return Err("执行目标已经变更；尚未完成任务登记与执行目标匹配".into());
    }
    task.get_object_id("_id")
        .map_err(|e| format!("任务ID无效：{e}"))
}

fn operation_task_filter(task_id: Option<&str>, name: &str) -> Result<Document, String> {
    let mut filter = doc! { "name": name, "status": { "$ne": "deactive" } };
    if let Some(id) = task_id {
        filter.insert(
            "_id",
            ObjectId::parse_str(id).map_err(|e| format!("任务ID无效：{e}"))?,
        );
    }
    Ok(filter)
}

async fn operation_task(
    context: &TaskRuntimeContext,
    task_id: Option<&str>,
    name: &str,
    budget: Duration,
) -> Result<Document, String> {
    let filter = operation_task_filter(task_id, name)?;
    timeout(budget, async {
        let client = context.client(false).await?;
        let tasks = client
            .database("simulation_mission")
            .collection::<Document>("tasks");
        tasks
            .find_one(filter, None)
            .await
            .map_err(|e| format!("本地控制面任务记录读取失败：{e}"))
    })
    .await
    .map_err(|_| {
        format!(
            "本地控制面任务登记读取链超时（{} ms）；尚未完成任务登记与执行目标匹配",
            budget.as_millis()
        )
    })??
    .ok_or_else(|| "本地控制面缺少原任务记录；尚未完成任务登记与执行目标匹配".into())
}

// Keep legacy pane text intact while removing diagnostic protocol lines before
// the existing strict process/dispatch parser sees the response.
fn runtime_process_output(stdout: &str, stderr: &str) -> Result<String, String> {
    let stdout = stdout
        .lines()
        .filter(|line| {
            !line.starts_with(TASK_STAGE_PREFIX)
                && !line.starts_with("QUANTNIGHT_TMUX_CONTEXT ")
                && !line.contains("QUANTNIGHT_RUNTIME_HISTORY")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if stdout.trim().is_empty() {
        return Err(format!(
            "运行时脚本未返回 Pane 检查结果（stdout 为空或仅有阶段信息）；stderr: {stderr}"
        ));
    }
    Ok((if stderr.trim().is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    })
    .lines()
    .map(safe_diagnostic_text)
    .collect::<Vec<_>>()
    .join("\n"))
}

/// Shared dispatcher entry. An absent legacy id retains the original diagnostic-only
/// name lookup; Some(empty/invalid) is still rejected, never converted to absence.
/// Mutations still require exact ObjectId on the local control plane.
pub async fn inspect(
    context: &TaskRuntimeContext,
    task_name: &str,
    task_id: Option<&str>,
    is_remote: bool,
    observer: Option<RuntimeObserver>,
) -> Result<String, String> {
    let budget_limit=CALL_REMAINING_BUDGET.try_with(|budget|*budget).unwrap_or(RUNTIME_BUDGET).min(RUNTIME_BUDGET);
    if budget_limit.is_zero(){return Err("运行检查原12秒剩余预算已耗尽（含连接与任务记录读取）；本次未执行检查脚本".into());}
    let started = Instant::now();
    let mut progress = TaskOperationProgress::new(
        Arc::new(move |stage| {
            if let Some(observer) = &observer {
                if let Ok(payload) = serde_json::to_value(stage) {
                    observer(RuntimeEvent::GuiProgress {
                        name: "task-operation-stage".into(),
                        payload,
                    });
                }
            }
        }),
        "runtime",
        task_id,
        task_name,
        is_remote,
        Some("runtime".into()),
    );
    progress.stage("initial", "运行检查已接收；总预算12秒");
    // Final aggregate guard includes identity, script readers/exit/cleanup and
    // dispatch. Dropping an in-flight diagnostic also cleans its private group.
    timeout(budget_limit, async {
        context
            .verify_current()
            .await
            .map_err(|e| progress.failure(e))?;
        progress.stage("identity", "核对本地任务登记ID、名称与执行目标");
        let task = operation_task(context, task_id, task_name, Duration::from_secs(2))
            .await
            .map_err(|e| progress.failure(e))?;
        let id = operation_task_identity(&task, task_name, is_remote)
            .map_err(|e| progress.failure(e))?;
        progress.task_id = id.to_hex();
        progress.tmux_binding =
            Some(task_script_binding(&task, is_remote).map_err(|e| progress.failure(e))?);
        // Reserve two seconds for dispatch evidence and cleanup. This probe must
        // finish before the UI's 15s deadline; it never inherits the generic 300s.
        let budget = budget_limit
            .saturating_sub(started.elapsed())
            .saturating_sub(Duration::from_secs(2))
            .min(Duration::from_secs(8));
        let (stdout, stderr) = execute_task_script(&progress, context.config(), budget).await?;
        progress.stage("receipt_validate", "检查运行时脚本输出；保留原 Pane 文本");
        let process_output =
            runtime_process_output(&stdout, &stderr).map_err(|e| progress.failure(e))?;
        progress.stage("dispatch_read", "有界读取实际执行库的派发证据");
        let remaining = budget_limit.saturating_sub(started.elapsed());
        let (dispatch, diagnostic) = timeout(
            remaining,
            read_runtime_evidence(context, &id.to_hex(), task_name, is_remote),
        )
        .await
        .unwrap_or((None, None));
        let diagnostic = diagnostic.or_else(|| {
            if stderr.trim().is_empty() {
                validated_history_diagnostic(
                    &stdout,
                    &task,
                    id,
                    task_name,
                    is_remote,
                    bson::DateTime::now().timestamp_millis(),
                )
            } else {
                None
            }
        });
        let output = business_diagnostic_output(
            &dispatch_status_output(&process_output, dispatch),
            diagnostic,
        );
        progress.stage(
            "completed",
            "运行时检查已返回；状态仍由原 Pane/原生派发证据解析",
        );
        Ok(output)
    })
    .await
    .map_err(|_| {
        progress
            .failure("运行检查总预算12秒耗尽（包含任务记录读取、脚本stdout/stderr、退出与派发证据）")
    })?
}

#[cfg(test)]
use crate::gui_task_queries::Task;
#[cfg(test)]
use crate::pause::{verified_stop_proof, PauseOptions, StopBudget, StopConfirmationBasis};
#[cfg(test)]
use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(test)]
mod task_control_tests {
    //! Offline GUI protocol regressions execute the registered shared pause owner.
    //! Only the lowest TaskStore and stop process IO are synthetic.
    use super::*;
    use crate::{
        pause::{self},
        task_store::{CollectionSummary, Host, TaskStore},
    };
    use std::{future::Future, pin::Pin};
    // Explicit boxed futures match the shared async-trait ABI without adding a GUI manifest dependency.

    pub(super) struct PauseRoute {
        pub outcome: pause::PauseOutcome,
        pub writes: Vec<(Document, Document)>,
        pub stages: Vec<String>,
        pub stop_invocations: usize,
    }
    struct Store {
        task: Document,
        execution: Option<Document>,
        matched: u64,
        writes: Mutex<Vec<(Document, Document)>>,
    }
    impl TaskStore for Store {
        fn find_task<'life0, 'async_trait>(
            &'life0 self,
            host: Host,
            filter: Document,
            _newest: bool,
        ) -> Pin<Box<dyn Future<Output = Result<Option<Document>, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let task = match host {
                    Host::Local => &self.task,
                    Host::Remote => self
                        .execution
                        .as_ref()
                        .expect("unexpected remote registry access"),
                };
                let matched = filter.iter().all(|(key, expected)| match expected {
                    bson::Bson::Document(op) if op.contains_key("$ne") => {
                        task.get(key) != op.get("$ne")
                    }
                    bson::Bson::Document(op) if op.contains_key("$exists") => {
                        task.contains_key(key) == op.get_bool("$exists").unwrap()
                    }
                    bson::Bson::Null => matches!(task.get(key), None | Some(bson::Bson::Null)),
                    _ => task.get(key) == Some(expected),
                });
                Ok(matched.then(|| task.clone()))
            })
        }
        fn update_task<'life0, 'async_trait>(
            &'life0 self,
            host: Host,
            filter: Document,
            update: Document,
        ) -> Pin<Box<dyn Future<Output = Result<u64, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                assert_eq!(host, Host::Local);
                self.writes.lock().unwrap().push((filter, update));
                Ok(self.matched)
            })
        }
        fn collection_names<'life0, 'async_trait>(
            &'life0 self,
            _: Host,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected collection IO") })
        }
        fn summary<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _: Host,
            _: &'life1 str,
        ) -> Pin<Box<dyn Future<Output = Result<CollectionSummary, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected item IO") })
        }
        fn has_rows<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _: Host,
            _: &'life1 str,
        ) -> Pin<Box<dyn Future<Output = Result<bool, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected item IO") })
        }
        fn rows<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _: Host,
            _: &'life1 str,
            _: Option<Document>,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Document>, String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected item IO") })
        }
        fn insert_rows<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _: Host,
            _: &'life1 str,
            _: Vec<Document>,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected insert") })
        }
        fn rename_collection<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _: Host,
            _: &'life1 str,
            _: &'life2 str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected rename") })
        }
        fn ping<'life0, 'async_trait>(
            &'life0 self,
            _: Host,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { panic!("unexpected ping") })
        }
    }
    pub(super) async fn pause_route(task: Document, stdout: String, matched: u64) -> PauseRoute {
        pause_route_with_execution(task, stdout, matched, None).await
    }
    async fn pause_route_with_execution(
        task: Document,
        stdout: String,
        matched: u64,
        execution: Option<Document>,
    ) -> PauseRoute {
        use crate::{
            backend_binding::validate_backend_binding, config::bash_test_support::FakeBash,
        };
        use std::os::unix::fs::PermissionsExt;
        let fake = FakeBash::new(":");
        let root = Path::new(&fake.config.env.working_dir);
        fs::create_dir_all(root.join(".venv/bin")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("credentials.yaml"),
            "synthetic config, never user credentials",
        )
        .unwrap();
        let python = root.join(".venv/bin/python");
        fs::write(
            &python,
            r#"#!/bin/sh
[ "$1 $2 $3 $4" = '-B -m backend.mongodb.mongo_config --validate-task-control-binding' ] || exit 97
echo bootstrap >> bootstrap-calls
printf '%s\n' '{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}'
"#,
        )
        .unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o700)).unwrap();
        let name = task.get_str("name").unwrap();
        let remote = task.get_bool("isRemote").unwrap_or(false);
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
        let still_active = format!(
            "QUANTNIGHT_STOP_OBSERVED {}",
            serde_json::json!({
        "schema_version":1,"task_name":name,"target_remote":remote,"session_absent":false})
        );
        let stopped = proof_line(name, remote, pause::StopConfirmationBasis::SessionAbsent);
        let raw_is_valid = pause::verified_stop_proof(&stdout, name, remote).is_ok();
        fs::write(root.join("scripts/pause_task.sh"), format!(
        "set -eu\n[ \"$1\" = {name} ] && [ \"$2\" = {host} ] || {{ echo wrong-stop-route >> VIOLATIONS; exit 97; }}\nprintf '%s' \"$QUANTNIGHT_TMUX_BINDING\" > binding.json\ncase \"${{3-}}\" in\n--verify-stopped) echo verify >> stop-calls; if [ -e stopped ]; then printf '%s\\n' {stopped}; else printf '%s\\n' {active}; fi ;;\n'') echo stop >> stop-calls; {transition}; printf '%s\\n' {raw} ;;\n*) echo unknown-stop-mode >> VIOLATIONS; exit 97 ;;\nesac\n",
        name=quote(name),host=quote(if remote {"--remote"} else {"false"}),
        stopped=quote(&stopped),active=quote(&still_active),raw=quote(&stdout),
        transition=if raw_is_valid {"touch stopped"} else {":"})).unwrap();
        let active = serde_json::json!({"schemaVersion":1,"taskName":name,"remote":remote,
        "taskId":task.get_object_id("_id").unwrap().to_hex(),"launchAttemptId":task.get_str("launchAttemptId").ok(),
        "state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"explicitPauseVerified":true});
        let absent = serde_json::json!({"schemaVersion":1,"taskName":name,"remote":remote,
        "state":"session_absent","consumerRunning":false,"identityVerified":false,"ownedResidue":false});
        fs::write(root.join("scripts/check_tmux_python.sh"), format!(
        "set -eu\n[ \"$1\" = {name} ] && [ \"$2\" = {host} ] && [ \"$3\" = --inspect-execution ] && [ \"$4\" = --binding ] || {{ echo wrong-inspect-route >> VIOLATIONS; exit 97; }}\necho inspect >> stop-calls\nif [ -e stopped ]; then printf '%s\\n' {absent}; else printf '%s\\n' {active}; fi\n",
        name=quote(name),host=quote(if remote {"true"} else {"false"}),
        active=quote(&format!("QUANTNIGHT_EXECUTION_OBSERVED {active}")),
        absent=quote(&format!("QUANTNIGHT_EXECUTION_OBSERVED {absent}")))).unwrap();
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let store = Arc::new(Store {
            task: task.clone(),
            execution: execution.or_else(|| {
                task.get_bool("isRemote")
                    .unwrap_or(false)
                    .then(|| task.clone())
            }),
            matched,
            writes: Mutex::new(vec![]),
        });
        let core = crate::task_control::TaskControl::with_store(binding, store.clone());
        let original_id = task.get_object_id("_id").unwrap().to_hex();
        assert_eq!(core.task_by_id(&original_id).await.unwrap(), task);
        assert!(core
            .task_by_id(&ObjectId::new().to_hex())
            .await
            .unwrap_err()
            .contains("not registered in this local control plane"));
        assert!(store.writes.lock().unwrap().is_empty());
        let stages = Arc::new(Mutex::new(vec![]));
        let sink = stages.clone();
        let name = task.get_str("name").unwrap();
        let id = task.get_object_id("_id").unwrap().to_hex();
        let remote = task.get_bool("isRemote").unwrap_or(false);
        let outcome = core
            .pause(
                name,
                PauseOptions {
                    expected_task_id: Some(&id),
                    expected_remote: Some(remote),
                    observer: Some(Arc::new(move |event| {
                        sink.lock().unwrap().push(event.stage.to_string())
                    })),
                    ..Default::default()
                },
                StopBudget::Gui,
                None,
            )
            .await;
        let forwarded: JsonValue =
            serde_json::from_str(&fs::read_to_string(root.join("binding.json")).unwrap()).unwrap();
        assert_eq!(forwarded["taskId"], id);
        assert_eq!(forwarded["remote"], remote);
        if let Ok(launch) = task.get_str("launchAttemptId") {
            assert_eq!(forwarded["launchAttemptId"], launch);
        }
        assert_eq!(
            fs::read_to_string(root.join("bootstrap-calls")).unwrap(),
            "bootstrap\n"
        );
        assert!(
            !root.join("VIOLATIONS").exists(),
            "fixture route violation must not be swallowed"
        );
        let calls = fs::read_to_string(root.join("stop-calls")).unwrap();
        let stop_invocations = calls.lines().filter(|line| *line == "stop").count();
        assert!(
            stop_invocations <= 1,
            "read-only recovery must not repeat stop"
        );
        if outcome.exit_code == 0 || stages.lock().unwrap().iter().any(|s| s == "control_write") {
            assert_eq!(
                stop_invocations, 1,
                "fixture initially-active stop cannot be replaced with verification success"
            );
        }
        let writes = store.writes.lock().unwrap().clone();
        PauseRoute {
            outcome,
            writes,
            stages: Arc::try_unwrap(stages).unwrap().into_inner().unwrap(),
            stop_invocations,
        }
    }
    pub(super) fn proof_line(
        name: &str,
        remote: bool,
        basis: pause::StopConfirmationBasis,
    ) -> String {
        let value = match basis {
            pause::StopConfirmationBasis::RunnersExited => {
                serde_json::json!({"schema_version":1,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":true})
            }
            pause::StopConfirmationBasis::SessionAbsent => {
                serde_json::json!({"schema_version":2,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":null,"confirmation_basis":"session_absent"})
            }
        };
        format!("QUANTNIGHT_PAUSE_CONFIRMED {value}")
    }
}
#[cfg(test)]
use task_control_tests::{pause_route, proof_line};
#[cfg(test)]
fn validate_stop_output(
    progress: &TaskOperationProgress,
    stdout: &str,
    stderr: &str,
) -> Result<StopConfirmationBasis, String> {
    progress.stage("receipt_validate", "验证共享核心唯一最终V1/V2回执");
    verified_stop_proof(stdout, &progress.name, progress.remote)
        .map_err(|e| progress.failure(format!("{e}；stderr: {stderr}；stdout: {stdout}")))
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

    fn captured_reconciliation_fixture(
        remote: bool,
        legacy: bool,
    ) -> (ObjectId, Document, Document) {
        let (id, mut task, mut cap) = fixture(remote, "waiting_reconciliation");
        task.insert("auth_profile", "fixture-profile");
        cap.insert("_id", "fixture-profile:source");
        cap.insert("auth_profile", "fixture-profile");
        cap.insert("pid", 42);
        cap.insert("consumer_run_id", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        cap.insert("heartbeatAt", bson::DateTime::from_millis(NOW - 7_821));
        cap.insert(
            "submissionReason",
            if legacy {
                format!("{LEGACY_RECONCILIATION_REASON_PREFIX}{CAPTURED_RAW_REASON}")
            } else {
                CAPTURED_RAW_REASON.to_string()
            },
        );
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
                cap.insert(
                    "submissionDisplayReason",
                    "untrusted display-only text: ready",
                );
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
            let mut invalid = cap.clone();
            invalid
                .get_document_mut("quota")
                .unwrap()
                .insert("reason", raw.clone());
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(),
                "{raw:?}"
            );
        }
        let mut missing = cap.clone();
        missing.get_document_mut("quota").unwrap().remove("reason");
        assert!(validated_dispatch_state(&missing, &task, id, "source", true, NOW).is_none());
        for state in ["ready", "waiting_quota_reset"] {
            let (id, task, mut other) = fixture(true, state);
            let raw = other
                .get_document("quota")
                .unwrap()
                .get("reason")
                .unwrap()
                .clone();
            other.insert(
                "submissionReason",
                format!(
                    "{LEGACY_RECONCILIATION_REASON_PREFIX}{}",
                    raw.as_str().unwrap_or("")
                ),
            );
            assert!(
                validated_dispatch_state(&other, &task, id, "source", true, NOW).is_none(),
                "{state}"
            );
        }
    }

    #[test]
    fn legacy_reason_compatibility_keeps_identity_clock_and_quota_fences() {
        let (id, task, cap) = captured_reconciliation_fixture(true, true);
        for changes in [
            doc! { "source_task_id": ObjectId::new().to_hex() },
            doc! { "source_remote": false },
            doc! { "task": "other" },
            doc! { "source_collection": "other" },
            doc! { "auth_profile": "other" },
            doc! { "_id": "other:source" },
            doc! { "dist": "local" },
            doc! { "taskType": "super" },
            doc! { "protocol_version": 2 },
            doc! { "heartbeat_clock": "local" },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW - DISPATCH_MAX_AGE_MS) },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW + 5001) },
            doc! { "pid": 0 },
            doc! { "consumer_run_id": "bad" },
            doc! { "submissionState": "ready" },
        ] {
            let mut invalid = cap.clone();
            invalid.extend(changes.clone());
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(),
                "{changes:?}"
            );
        }
        for changes in [
            doc! { "dispatch_state": "ready" },
            doc! { "stopped": true },
            doc! { "remaining": -1 },
            doc! { "budget_confirmed": "false" },
            doc! { "reset_at": "bad" },
        ] {
            let mut invalid = cap.clone();
            invalid
                .get_document_mut("quota")
                .unwrap()
                .extend(changes.clone());
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(),
                "{changes:?}"
            );
        }
        assert!(
            validated_dispatch_state(&cap, &task, ObjectId::new(), "source", true, NOW).is_none()
        );
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_none());
        let mut paused = task.clone();
        paused.insert("status", "paused");
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
                assert_eq!(
                    wire["quota"]["budgetConfirmed"],
                    state == "waiting_quota_reset"
                );
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
        for field in ["limit", "remaining", "stop_floor", "reset_at"] {
            quota.insert(field, bson::Bson::Null);
        }
        let d = validated_dispatch_state(&cap, &task, id, "source", false, NOW).unwrap();
        assert_eq!(d.submission_state, "ready");
        assert!(d.reset_at.is_none());
        assert!(d.quota.remaining.is_none());
        let (id, task, mut cap) = fixture(false, "waiting_reconciliation");
        cap.insert("reset_at", bson::Bson::Null);
        cap.get_document_mut("quota")
            .unwrap()
            .insert("reset_at", bson::Bson::Null);
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_some());
    }

    #[test]
    fn native_rejects_stale_clock_identity_profile_pid_run_and_invalid_state() {
        let (id, task, cap) = fixture(true, "waiting_reconciliation");
        for changes in [
            doc! { "source_task_id": ObjectId::new().to_hex() },
            doc! { "source_remote": false },
            doc! { "source_remote": "true" },
            doc! { "task": "other" },
            doc! { "source_collection": "other" },
            doc! { "auth_profile": "fake" },
            doc! { "_id": "fake:source" },
            doc! { "dist": "local" },
            doc! { "taskType": "priority" },
            doc! { "protocol_version": 2 },
            doc! { "heartbeat_clock": "local" },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW - DISPATCH_MAX_AGE_MS) },
            doc! { "heartbeatAt": bson::DateTime::from_millis(NOW + 5001) },
            doc! { "heartbeatAt": "2026-10-16T01:13:20Z" },
            doc! { "pid": 0 },
            doc! { "pid": -1 },
            doc! { "pid": "21740" },
            doc! { "pid": 2.0 },
            doc! { "pid": 4_294_967_296_i64 },
            doc! { "consumer_run_id": "bad" },
            doc! { "submissionState": "waiting_quota" },
            doc! { "submissionReason": bson::Bson::Null },
            doc! { "submissionReason": "line\nforgery" },
            doc! { "quota": doc! {} },
            doc! { "reset_at": "2026-10-16T10:13:20+08:00" },
        ] {
            let mut invalid = cap.clone();
            invalid.extend(changes.clone());
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(),
                "{changes:?}"
            );
        }
        for key in [
            "source_task_id",
            "pid",
            "consumer_run_id",
            "submissionState",
            "submissionReason",
            "heartbeat_clock",
            "quota",
            "reset_at",
        ] {
            let mut invalid = cap.clone();
            invalid.remove(key);
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", true, NOW).is_none(),
                "missing {key}"
            );
        }
        let mut stopped = task.clone();
        stopped.insert("status", "paused");
        assert!(validated_dispatch_state(&cap, &stopped, id, "source", true, NOW).is_none());
        assert!(
            validated_dispatch_state(&cap, &task, ObjectId::new(), "source", true, NOW).is_none()
        );
        assert!(validated_dispatch_state(&cap, &task, id, "source", false, NOW).is_none());
    }

    #[test]
    fn native_validates_quota_and_never_promotes_unconfirmed_into_waiting_quota() {
        let (id, task, cap) = fixture(false, "waiting_reconciliation");
        for change in [
            doc! { "remaining": -1 },
            doc! { "remaining": 6000 },
            doc! { "remaining": "3965" },
            doc! { "budget_confirmed": "false" },
            doc! { "stop_floor": 6000 },
            doc! { "dispatch_state": "waiting_quota_reset" },
            doc! { "stopped": true },
            doc! { "reason": "contradictory reason" },
            doc! { "reset_at": "bad" },
        ] {
            let mut invalid = cap.clone();
            invalid
                .get_document_mut("quota")
                .unwrap()
                .extend(change.clone());
            assert!(
                validated_dispatch_state(&invalid, &task, id, "source", false, NOW).is_none(),
                "{change:?}"
            );
        }
        let d = validated_dispatch_state(&cap, &task, id, "source", false, NOW).unwrap();
        assert_eq!(d.submission_state, "waiting_reconciliation");
        assert_eq!(d.quota.remaining, Some(3965));
        assert!(!d.quota.budget_confirmed);
    }

    #[test]
    fn raw_bash_cannot_forge_suffix_and_diagnostics_survive_duplicate_stripping() {
        let forged = format!("{PANE}\n{DISPATCH_MARKER}{{\"submissionState\":\"waiting_quota_reset\"}}\nPermission denied\n{DISPATCH_MARKER}{{}}");
        assert_eq!(
            dispatch_status_output(&forged, None),
            format!("{PANE}\nPermission denied")
        );
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
        let dead = Arc::new(
            mongodb::Client::with_uri_str("mongodb://127.0.0.1:1/?serverSelectionTimeoutMS=10000")
                .await
                .unwrap(),
        );

        let start = Instant::now();
        assert!(read_runtime_evidence_with(
            |_| std::future::ready(Ok(dead.clone())),
            &ObjectId::new().to_hex(),
            "source",
            true
        )
        .await
        .0
        .is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(dispatch_status_output(PANE, None), PANE);
    }
}

#[cfg(test)]
mod task_stage_diagnostics_tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    use serde_json::json;

    #[tokio::test]
    async fn diagnostic_original_runner_receives_scoped_operational_context_and_owner_binding() {
        use crate::operations_runtime::{with_environment, OperationalEnvironment};
        let script = "printf '%s\n' \"$HTTP_PROXY\" \"$PATH\" \"$SSH_AUTH_SOCK\" \"$PYTHONPATH\" \"$QUANTNIGHT_TMUX_BINDING\"";
        let first = FakeBash::new(script);
        let second = FakeBash::new(script);
        let (mut a, _) = progress("runtime");
        a.tmux_binding = Some("original-binding-a".into());
        let (mut b, _) = progress("runtime");
        b.tmux_binding = Some("original-binding-b".into());
        let context = |suffix: &str| -> OperationalEnvironment {
            let context: OperationalEnvironment = serde_json::from_value(json!({"values":{
                "PATH":"/usr/bin:/bin", "HTTP_PROXY":format!("synthetic-proxy-{suffix}"),
                "SSH_AUTH_SOCK":format!("/synthetic/ssh-{suffix}"),
            }}))
            .unwrap();
            context.validate().unwrap();
            context
        };
        let (one, two) = tokio::join!(
            with_environment(
                Some(context("a")),
                execute_task_script(&a, &first.config, Duration::from_secs(2))
            ),
            with_environment(
                Some(context("b")),
                execute_task_script(&b, &second.config, Duration::from_secs(2))
            ),
        );
        let (one, one_error) = one.unwrap();
        let (two, two_error) = two.unwrap();
        assert!(one_error.is_empty() && two_error.is_empty());
        assert_eq!(
            one.lines().collect::<Vec<_>>(),
            vec![
                "synthetic-proxy-a",
                "/usr/bin:/bin",
                "/synthetic/ssh-a",
                first.config.env.working_dir.as_str(),
                "original-binding-a"
            ]
        );
        assert_eq!(
            two.lines().collect::<Vec<_>>(),
            vec![
                "synthetic-proxy-b",
                "/usr/bin:/bin",
                "/synthetic/ssh-b",
                second.config.env.working_dir.as_str(),
                "original-binding-b"
            ]
        );
    }

    #[test]
    fn diagnostic_optional_id_preserves_absence_and_rejects_present_invalid_id() {
        let id = ObjectId::new();
        let absent = operation_task_filter(None, "exact__priority").unwrap();
        assert_eq!(
            absent,
            doc! {"name":"exact__priority","status":{"$ne":"deactive"}}
        );
        let exact = operation_task_filter(Some(&id.to_hex()), "exact__priority").unwrap();
        assert_eq!(exact.get_object_id("_id").unwrap(), id);
        assert_eq!(exact.get_str("name").unwrap(), "exact__priority");
        for supplied in ["", " ", "not-an-object-id"] {
            assert!(operation_task_filter(Some(supplied), "exact__priority")
                .unwrap_err()
                .starts_with("任务ID无效："));
        }
        // Legacy diagnostics never change the binding rules for start/pause.
        let task = doc! {"_id":id,"name":"exact__priority","status":"running","isRemote":true};
        assert!(operation_task_identity(&task, "exact__priority", false).is_err());
    }

    fn progress(
        operation: &'static str,
    ) -> (TaskOperationProgress, Arc<Mutex<Vec<TaskOperationStage>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        (
            TaskOperationProgress {
                operation,
                task_id: "0123456789abcdef01234567".into(),
                name: "exact__priority".into(),
                remote: true,
                request_id: "request-native-1".into(),
                tmux_binding: None,
                last: Arc::new(Mutex::new(("initial".into(), "原生请求".into()))),
                sink: Arc::new(move |event| recorded.lock().unwrap().push(event)),
            },
            events,
        )
    }

    fn stage(operation: &str, phase: &str) -> JsonValue {
        json!({"operation": operation, "task_name": "exact__priority", "target_remote": true,
            "stage": phase, "message": "正在核对精确任务的执行步骤"})
    }

    fn shell_line(value: &JsonValue) -> String {
        format!("printf '%s\\n' '{}{}'\n", TASK_STAGE_PREFIX, value)
    }

    fn stop_receipt() -> String {
        format!(
            "QUANTNIGHT_PAUSE_CONFIRMED {}",
            json!({"schema_version":1,
            "task_name":"exact__priority", "target_remote":true,
            "session_absent":true, "runners_exited":true})
        )
    }

    #[test]
    fn bounded_exact_schema_identity_cannot_forge_scope_or_control_authority() {
        let (p, events) = progress("runtime");
        p.script_stage(&format!("{TASK_STAGE_PREFIX}{}", stage("runtime", "ssh")));
        for change in [
            json!({"operation":"stop"}),
            json!({"task_name":"exact"}),
            json!({"target_remote":false}),
            json!({"target_remote":"true"}),
            json!({"stage":"control_write"}),
            json!({"stage":"paused"}),
            json!({"stage":"interrupt"}),
            json!({"message":""}),
            json!({"message":"\n"}),
            json!({"message":"x".repeat(1025)}),
            json!({"requestId":"spoof"}),
            json!({"taskId":"spoof"}),
            json!({"confirmed":true}),
        ] {
            let mut value = stage("runtime", "ssh");
            for (key, val) in change.as_object().unwrap() {
                value[key] = val.clone();
            }
            p.script_stage(&format!("{TASK_STAGE_PREFIX}{value}"));
        }
        for line in [
            "QUANTNIGHT_TASK_STAGE {broken}".to_string(),
            format!(" {}{}", TASK_STAGE_PREFIX, stage("runtime", "ssh")),
            format!("{}{}", TASK_STAGE_PREFIX, "x".repeat(4097)),
            "QUANTNIGHT_PAUSE_CONFIRMED {\"confirmed\":true}".into(),
            format!("{TASK_STAGE_PREFIX}{{\"operation\":\"runtime\",\"operation\":\"stop\"}}"),
            format!("{TASK_STAGE_PREFIX}{}", "[".repeat(300)),
        ] {
            p.script_stage(&line);
        }
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap(),
            json!({"operation":"runtime",
            "taskId":"0123456789abcdef01234567", "name":"exact__priority", "isRemote":true,
            "requestId":"request-native-1", "stage":"ssh", "message":"正在核对精确任务的执行步骤"})
        );
        drop(events);
        let (p, events) = progress("stop");
        for phase in [
            "config",
            "ssh",
            "tmux",
            "process",
            "interrupt",
            "session_exit",
            "process_exit",
            "receipt",
            "complete",
        ] {
            p.script_stage(&format!("{TASK_STAGE_PREFIX}{}", stage("stop", phase)));
        }
        assert_eq!(events.lock().unwrap().len(), 9);
    }

    #[tokio::test]
    async fn native_operation_real_runner_reports_stages_and_preserves_legacy_pane() {
        let pane = "✅ Pane 0 (pid=123) is running: python";
        let script = shell_line(&stage("runtime", "ssh"))
            + &shell_line(&stage("runtime", "process"))
            + &format!("printf '%s\\n' '{pane}'\n");
        let fake = FakeBash::new(&script);
        let (p, events) = progress("runtime");
        let (stdout, stderr) = execute_task_script(&p, &fake.config, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(runtime_process_output(&stdout, &stderr).unwrap(), pane);
        let events = events.lock().unwrap();
        assert_eq!(
            events.iter().map(|e| e.stage.as_str()).collect::<Vec<_>>(),
            ["config", "launch", "wait", "ssh", "process"]
        );
        assert!(events
            .iter()
            .all(|e| e.task_id == p.task_id && e.request_id == p.request_id && e.is_remote));
    }

    #[tokio::test]
    async fn native_nonzero_timeout_keep_last_script_stage_and_stderr() {
        for (tail, cause, budget) in [
            (
                "echo exact-ssh-denied >&2; exit 7",
                "exact-ssh-denied",
                Duration::from_secs(2),
            ),
            (
                "echo exact-ssh-stall >&2; exec /bin/sleep 30",
                "exact-ssh-stall",
                Duration::from_millis(100),
            ),
        ] {
            let fake = FakeBash::new(&(shell_line(&stage("runtime", "ssh")) + tail));
            let (p, events) = progress("runtime");
            let error = execute_task_script(&p, &fake.config, budget)
                .await
                .unwrap_err();
            assert!(
                error.contains("stage=ssh") && error.contains(cause),
                "{error}"
            );
            let events = events.lock().unwrap();
            assert_eq!(events.last().unwrap().stage, "ssh");
            assert!(events.last().unwrap().message.contains(cause));
            assert!(!events
                .iter()
                .any(|e| ["control_write", "completed"].contains(&e.stage.as_str())));
        }
    }

    #[tokio::test]
    async fn native_configuration_launch_errors_emit_specific_failed_step() {
        let mut fake = FakeBash::new(":");
        fake.config.bash.scripts.remove("check_task_status");
        let (p, events) = progress("runtime");
        let error = execute_task_script(&p, &fake.config, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.contains("stage=config") && error.contains("未找到 check_task_status"));
        assert_eq!(events.lock().unwrap().last().unwrap().stage, "config");
        let mut fake = FakeBash::new(":");
        fake.config
            .bash
            .scripts
            .get_mut("check_task_status")
            .unwrap()
            .command = Some("/not/a/command".into());
        let error = execute_task_script(&p, &fake.config, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.contains("stage=launch") && error.contains("启动命令失败"));
        assert_eq!(events.lock().unwrap().last().unwrap().stage, "launch");
    }

    #[tokio::test]
    async fn empty_runtime_fake_stop_success_never_confirmed() {
        for source in [
            ":",
            "echo 'QUANTNIGHT_TASK_STAGE {}'",
            "echo 'QUANTNIGHT_TMUX_CONTEXT {}'",
        ] {
            let fake = FakeBash::new(source);
            let (p, _) = progress("runtime");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1))
                .await
                .unwrap();
            p.stage("receipt_validate", "核验运行时输出");
            let error = runtime_process_output(&out, &err)
                .map_err(|e| p.failure(e))
                .unwrap_err();
            assert!(
                error.contains("stage=receipt_validate") && error.contains("为空或仅有阶段信息")
            );
        }
        for source in [
            ":",
            "echo 已暂停",
            "echo 'QUANTNIGHT_PAUSE_CONFIRMED {broken}'",
            "echo 'QUANTNIGHT_PAUSE_CONFIRMED {\"schema_version\":1,\"confirmed\":true}'",
        ] {
            let fake = FakeBash::new(source);
            let (p, events) = progress("stop");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1))
                .await
                .unwrap();
            let error = validate_stop_output(&p, &out, &err).unwrap_err();
            assert!(error.contains("stage=receipt_validate"), "{error}");
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.stage == "control_write"));
        }
    }

    #[tokio::test]
    async fn stop_receipt_final_and_control_write_builder_retains_cas() {
        let receipt = stop_receipt();
        let script =
            shell_line(&stage("stop", "process_exit")) + &format!("printf '%s\n' '{receipt}'");
        let fake = FakeBash::new(&script);
        let (p, _) = progress("stop");
        let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1))
            .await
            .unwrap();
        validate_stop_output(&p, &out, &err).unwrap();
        assert_eq!(out.lines().last(), Some(receipt.as_str()));
        let id = ObjectId::parse_str(&p.task_id).unwrap();
        for host in [bson::Bson::Boolean(true), bson::Bson::Null] {
            let remote = host == bson::Bson::Boolean(true);
            let original =
                doc! {"_id":id,"name":&p.name,"isRemote":host.clone(),"status":"running"};
            let route = pause_route(
                original,
                proof_line(&p.name, remote, StopConfirmationBasis::RunnersExited),
                0,
            )
            .await;
            let (filter, update) = &route.writes[0];
            assert_eq!(filter.get_object_id("_id").unwrap(), id);
            assert_eq!(filter.get_str("name").unwrap(), "exact__priority");
            assert_eq!(filter.get_str("status").unwrap(), "running");
            assert_eq!(filter.get("isRemote"), Some(&host));
            assert_eq!(
                filter.get_document("updatedAt").unwrap(),
                &doc! {"$exists":false}
            );
            assert_eq!(
                filter.get_document("acceptedObservation").unwrap(),
                &doc! {"$exists":false}
            );
            assert_eq!(
                update
                    .get_document("$set")
                    .unwrap()
                    .get_str("status")
                    .unwrap(),
                "paused"
            );
            assert!(route.outcome.value["error"]
                .as_str()
                .unwrap()
                .contains("CAS failed"));
            assert!(route.outcome.confirmed_stop.is_none());
            assert!(route.stages.iter().any(|s| s == "control_write"));
            assert!(!route.stages.iter().any(|s| s == "completed"));
        }
        assert!(verified_stop_proof(
            &(out + "\n" + TASK_STAGE_PREFIX + &stage("stop", "complete").to_string()),
            &p.name,
            true
        )
        .is_err());
    }

    #[tokio::test]
    async fn legacy_missing_request_id_suppresses_only_events_not_operation_or_diagnostics() {
        for operation in ["runtime", "stop"] {
            let result_line = if operation == "stop" {
                stop_receipt()
            } else {
                "✅ Pane 0 (pid=123) is running: python".into()
            };
            let fake = FakeBash::new(
                &(shell_line(&stage(operation, "process"))
                    + &format!("printf '%s\\n' '{result_line}'")),
            );
            let (mut p, events) = progress(operation);
            p.request_id.clear();
            p.stage("initial", "legacy调用不发送缺少requestId的事件");
            let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(p.last.lock().unwrap().0, "process");
            assert!(p.failure("synthetic diagnostic").contains("stage=process"));
            if operation == "stop" {
                validate_stop_output(&p, &out, &err).unwrap();
            } else {
                assert_eq!(runtime_process_output(&out, &err).unwrap(), result_line);
            }
            assert!(events.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn native_binding_rejects_changed_host_identity_and_budgets_fit_ui() {
        let id = ObjectId::new();
        let mut task =
            doc! {"_id":id, "name":"exact__priority", "isRemote":true, "status":"running"};
        assert_eq!(
            operation_task_identity(&task, "exact__priority", true).unwrap(),
            id
        );
        assert!(operation_task_identity(&task, "exact__priority", false)
            .unwrap_err()
            .contains("执行目标已经变更"));
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

    fn progress(
        operation: &'static str,
        remote: bool,
    ) -> (TaskOperationProgress, Arc<Mutex<Vec<TaskOperationStage>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        (
            TaskOperationProgress {
                operation,
                task_id: "0123456789abcdef01234567".into(),
                name: "exact__priority".into(),
                remote,
                request_id: "session-absent-v2-request".into(),
                tmux_binding: None,
                last: Arc::new(Mutex::new(("initial".into(), "请求已接收".into()))),
                sink: Arc::new(move |event| sink_events.lock().unwrap().push(event)),
            },
            events,
        )
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
    fn receipt(proof: &JsonValue) -> String {
        format!("QUANTNIGHT_PAUSE_CONFIRMED {proof}")
    }
    fn print_line(line: &str) -> String {
        let escaped = line.replace('\'', "'\"'\"'");
        format!("printf '%s\\n' '{escaped}'\n")
    }

    #[tokio::test]
    async fn real_runner_absent_runtime_is_success_and_stage_never_writes_control() {
        for remote in [false, true] {
            let legacy = format!(
                "❌ Session 'exact__priority' 不存在 ({})",
                if remote { "remote" } else { "local" }
            );
            let script = print_line(&format!(
                "{TASK_STAGE_PREFIX}{}",
                stage("runtime", remote, "session_absent")
            )) + &print_line(&legacy)
                + "exit 0\n";
            let fake = FakeBash::new(&script);
            let (progress, events) = progress("runtime", remote);
            let (out, err) = execute_task_script(&progress, &fake.config, Duration::from_secs(2))
                .await
                .unwrap();
            assert_eq!(runtime_process_output(&out, &err).unwrap(), legacy);
            assert_eq!(
                dispatch_status_output(&runtime_process_output(&out, &err).unwrap(), None),
                legacy
            );
            assert_eq!(progress.last.lock().unwrap().0, "session_absent");
            assert!(events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event.stage == "session_absent"
                    && event.message
                        == stage("runtime", remote, "session_absent")["message"]
                            .as_str()
                            .unwrap()));
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|event| ["control_write", "paused"].contains(&event.stage.as_str())));
            // Legacy absence output is not itself a structured pause receipt.
            assert!(verified_stop_proof(&out, &progress.name, remote).is_err());
        }
    }

    #[tokio::test]
    async fn real_runner_v2_and_v1_preserve_distinct_basis_through_metadata_and_cas_gate() {
        for remote in [false, true] {
            for (version, expected) in [
                (1, StopConfirmationBasis::RunnersExited),
                (2, StopConfirmationBasis::SessionAbsent),
            ] {
                let final_line = receipt(&proof(version, remote));
                let script = print_line(&format!(
                    "{TASK_STAGE_PREFIX}{}",
                    stage(
                        "stop",
                        remote,
                        if version == 2 {
                            "session_absent"
                        } else {
                            "receipt"
                        }
                    )
                )) + &print_line(&final_line);
                let fake = FakeBash::new(&script);
                let (progress, events) = progress("stop", remote);
                let (out, err) =
                    execute_task_script(&progress, &fake.config, Duration::from_secs(2))
                        .await
                        .unwrap();
                let basis = validate_stop_output(&progress, &out, &err).unwrap();
                assert_eq!(basis, expected);
                assert_eq!(out.lines().last(), Some(final_line.as_str()));
                assert!(!events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|e| e.stage == "control_write"));
                let id = ObjectId::parse_str(&progress.task_id).unwrap();
                let original = doc! {"_id":id,"name":&progress.name,"isRemote":remote,"status":"running", "updatedAt":"original-control-revision","launchAttemptId":"original-launch"};
                for matched in [0, 1] {
                    let route = pause_route(original.clone(), out.clone(), matched).await;
                    let (filter, update) = &route.writes[0];
                    assert_eq!(
                        filter.get_str("updatedAt").unwrap(),
                        "original-control-revision"
                    );
                    assert_eq!(
                        filter.get_str("launchAttemptId").unwrap(),
                        "original-launch"
                    );
                    assert_eq!(filter.get_bool("isRemote").unwrap(), remote);
                    let set = update.get_document("$set").unwrap();
                    assert_eq!(
                        set.get_str("stopConfirmationBasis").unwrap(),
                        basis.as_str()
                    );
                    assert!(
                        !set.contains_key("runners_exited")
                            && !set.contains_key("platformCancelled")
                    );
                    let mut persisted = original.clone();
                    persisted.extend(set.clone());
                    let mapped: Task = bson::from_document(persisted).unwrap();
                    assert_eq!(
                        serde_json::to_value(mapped).unwrap()["stopConfirmationBasis"],
                        basis.as_str()
                    );
                    if matched == 0 {
                        assert!(route.outcome.value["error"]
                            .as_str()
                            .unwrap()
                            .contains("CAS failed"));
                        assert!(route.outcome.confirmed_stop.is_none());
                        assert!(!route.stages.iter().any(|s| s == "completed"));
                    } else {
                        let wire =
                            serde_json::to_value(route.outcome.confirmed_stop.unwrap()).unwrap();
                        assert_eq!(wire["confirmed"], true);
                        assert_eq!(wire["taskId"], id.to_hex());
                        assert_eq!(wire["isRemote"], remote);
                        assert_eq!(wire["confirmationBasis"], basis.as_str());
                        assert_eq!(route.stages.last().unwrap(), "completed");
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn real_runner_strict_v2_rejects_forged_missing_wrong_type_duplicate_and_nonfinal_proofs()
    {
        let valid = proof(2, true);
        let mut bad = Vec::new();
        for change in [
            json!({"schema_version":true}),
            json!({"schema_version":2.0}),
            json!({"schema_version":3}),
            json!({"task_name":"exact"}),
            json!({"target_remote":false}),
            json!({"target_remote":"true"}),
            json!({"session_absent":false}),
            json!({"session_absent":"true"}),
            json!({"runners_exited":true}),
            json!({"runners_exited":false}),
            json!({"runners_exited":"null"}),
            json!({"confirmation_basis":"runners_exited"}),
            json!({"confirmation_basis":null}),
            json!({"confirmation_basis":false}),
            json!({"extra":"forged"}),
        ] {
            let mut invalid = valid.clone();
            for (key, value) in change.as_object().unwrap() {
                invalid[key] = value.clone();
            }
            bad.push(receipt(&invalid));
        }
        for missing in [
            "schema_version",
            "task_name",
            "target_remote",
            "session_absent",
            "runners_exited",
            "confirmation_basis",
        ] {
            let mut invalid = valid.clone();
            invalid.as_object_mut().unwrap().remove(missing);
            bad.push(receipt(&invalid));
        }
        bad.push(format!(
            "QUANTNIGHT_PAUSE_CONFIRMED {}",
            valid.to_string().replacen("{", "{\"schema_version\":2,", 1)
        ));
        bad.push(receipt(&valid) + "\n" + &receipt(&valid));
        bad.push(
            receipt(&valid)
                + "\n"
                + TASK_STAGE_PREFIX
                + &stage("stop", true, "complete").to_string(),
        );
        bad.push(format!(
            "{TASK_STAGE_PREFIX}{}",
            stage("stop", true, "session_absent")
        ));
        for stdout in bad {
            let script = stdout.lines().map(print_line).collect::<String>();
            let fake = FakeBash::new(&script);
            let (progress, events) = progress("stop", true);
            let (out, err) = execute_task_script(&progress, &fake.config, Duration::from_secs(2))
                .await
                .unwrap();
            let error = validate_stop_output(&progress, &out, &err).unwrap_err();
            assert!(
                error.contains("stage=receipt_validate"),
                "{stdout}: {error}"
            );
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|e| ["control_write", "completed"].contains(&e.stage.as_str())));
        }
        // V1 remains exact: adding the V2 basis or changing true->null is invalid.
        for change in [
            json!({"confirmation_basis":"session_absent"}),
            json!({"confirmation_basis":null}),
            json!({"runners_exited":null}),
        ] {
            let mut invalid = proof(1, true);
            for (key, value) in change.as_object().unwrap() {
                invalid[key] = value.clone();
            }
            assert!(verified_stop_proof(&receipt(&invalid), "exact__priority", true).is_err());
        }
    }

    #[tokio::test]
    async fn real_runner_progress_cannot_fake_control_write_or_confirmation() {
        let (progress, events) = progress("runtime", true);
        let script = print_line(&format!(
            "{TASK_STAGE_PREFIX}{}",
            stage("runtime", true, "session_absent")
        )) + &print_line(&format!(
            "{TASK_STAGE_PREFIX}{}",
            stage("runtime", true, "control_write")
        )) + &print_line(&format!(
            "{TASK_STAGE_PREFIX}{}",
            stage("stop", true, "session_absent")
        )) + &print_line(&receipt(&proof(2, true)))
            + &print_line("❌ Session 'exact__priority' 不存在 (remote)");
        let fake = FakeBash::new(&script);
        let (out, _) = execute_task_script(&progress, &fake.config, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(progress.last.lock().unwrap().0, "session_absent");
        assert!(!events
            .lock()
            .unwrap()
            .iter()
            .any(|e| ["control_write", "paused", "completed"].contains(&e.stage.as_str())));
        assert!(verified_stop_proof(&out, &progress.name, true).is_err());
    }

    #[tokio::test]
    async fn control_cas_retains_original_status_revision_host_launch_and_absent_fields() {
        let id = ObjectId::new();
        let original = doc! {"_id":id,"name":"exact__priority","status":"running","isRemote":true,"updatedAt":17_i64,
        "launchAttemptId":"launch-v1","generation":{"id":"generation-v1"},"stopConfirmedAt":bson::Bson::Null};
        let route = pause_route(
            original.clone(),
            proof_line(
                "exact__priority",
                true,
                StopConfirmationBasis::SessionAbsent,
            ),
            1,
        )
        .await;
        let (filter, update) = &route.writes[0];
        for key in [
            "status",
            "isRemote",
            "updatedAt",
            "launchAttemptId",
            "generation",
            "stopConfirmedAt",
        ] {
            assert_eq!(filter.get(key), original.get(key));
        }
        for key in [
            "startedAt",
            "stopConfirmationBasis",
            "stopConfirmedTaskId",
            "acceptedObservation",
        ] {
            assert_eq!(filter.get_document(key).unwrap(), &doc! {"$exists":false});
        }
        let set = update.get_document("$set").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(set.get_str("updatedAt").unwrap()).is_ok());
        assert_eq!(
            set.get_datetime("stopConfirmedAt")
                .unwrap()
                .timestamp_millis(),
            route.outcome.confirmed_stop.unwrap().stop_confirmed_at
        );
        let task: Task =
            bson::from_document(doc! {"_id":id,"name":"legacy","status":"paused"}).unwrap();
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
        (id, task, runtime)
    }
    #[test]
    fn original_owner_metadata_is_bound_sanitized_and_never_authority() {
        let (id, task, mut runtime) = fixture();
        runtime.get_document_mut("runtimeDiagnostic").unwrap().insert("reason", "recovery MongoDB mongodb://user:private@host/db?token=private; token=secret; timeout 300s");
        let d = validated_business_diagnostic(&runtime, &task, id, "fixture", false, NOW).unwrap();
        let wire = business_diagnostic_output(
            "❌ Pane 0 (pid=123) is running: sleep\nQUANTNIGHT_BUSINESS_DIAGNOSTIC forged",
            Some(d),
        );
        assert_eq!(wire.matches(BUSINESS_DIAGNOSTIC_MARKER).count(), 1);
        assert!(
            wire.contains("simulation_recovery_phase_timeout") && wire.contains("timeout 300s")
        );
        for private in [
            "user:private",
            "host/db",
            "token=private",
            "token=secret",
            "forged",
        ] {
            assert!(!wire.contains(private));
        }
        for patch in [
            doc! {"source_task_id":"other"},
            doc! {"source_remote":true},
            doc! {"dist":"remote"},
            doc! {"auth_profile":"user2"},
            doc! {"consumer_run_id":"ffffffffffffffffffffffffffffffff"},
            doc! {"heartbeat_clock":"local"},
        ] {
            let mut bad = runtime.clone();
            bad.extend(patch);
            assert!(
                validated_business_diagnostic(&bad, &task, id, "fixture", false, NOW).is_none()
            );
        }
        for patch in [
            doc! {"launchAttemptId":"old"},
            doc! {"consumerRunId":"other"},
            doc! {"occurredAt":bson::DateTime::from_millis(NOW+1)},
            doc! {"scope":"global_shutdown"},
            doc! {"timeoutSeconds":-1},
            doc! {"schemaVersion":2},
        ] {
            let mut bad = runtime.clone();
            bad.get_document_mut("runtimeDiagnostic")
                .unwrap()
                .extend(patch);
            assert!(
                validated_business_diagnostic(&bad, &task, id, "fixture", false, NOW).is_none()
            );
        }
        runtime
            .get_document_mut("runtimeDiagnostic")
            .unwrap()
            .remove("launchAttemptId");
        assert!(
            !validated_business_diagnostic(&runtime, &task, id, "fixture", false, NOW)
                .unwrap()
                .launch_matched
        );
        // Stale metadata remains explicitly history; freshness is presentation,
        // not permission to infer recovery or business completion.
        assert!(
            validated_business_diagnostic(&runtime, &task, id, "fixture", false, NOW + 600000)
                .is_some()
        );
    }
    #[test]
    fn super_uses_original_launch_and_explicit_utc_not_local_naive_clock() {
        let (id, mut task, mut runtime) = fixture();
        task.insert("taskType", "super");
        runtime.insert("taskType", "super");
        task.insert(
            "startupBinding",
            doc! {"taskId":id.to_hex(),"name":"fixture","auth_profile":"user1","remote":false,
            "consumer_run_id":"0123456789abcdef0123456789abcdef","launchAttemptId":"launch1"},
        );
        let stamp = chrono::DateTime::from_timestamp_millis(NOW)
            .unwrap()
            .to_rfc3339();
        runtime.insert("heartbeatAt", stamp);
        assert!(
            validated_business_diagnostic(&runtime, &task, id, "fixture", false, NOW).is_some()
        );
        for key in [
            "taskId",
            "name",
            "auth_profile",
            "consumer_run_id",
            "launchAttemptId",
        ] {
            let mut bad = task.clone();
            bad.get_document_mut("startupBinding")
                .unwrap()
                .insert(key, "other");
            assert!(
                validated_business_diagnostic(&runtime, &bad, id, "fixture", false, NOW).is_none()
            );
        }
        runtime.insert("heartbeatAt", "2026-09-22T00:00:00");
        assert!(
            validated_business_diagnostic(&runtime, &task, id, "fixture", false, NOW).is_none()
        );
    }
    #[test]
    fn history_envelope_cannot_supply_current_state_or_rewrite_identity() {
        let (id, task, _) = fixture();
        let record = serde_json::json!({"schemaVersion":1,"name":"fixture","isRemote":false,"sessionId":"$5",
            "sessionCreatedAt":NOW-300000,"occurredAt":NOW-1000,"errorClass":"simulation_recovery_phase_timeout",
            "reason":"timeout 300s; original receipts retained","phase":"simulation_recovery_phase","timeoutSeconds":300});
        let pane = "❌ Pane 0 (pid=123) is running: sleep";
        let wire = format!("{pane}\n{RUNTIME_HISTORY_MARKER}{record}");
        let d = validated_history_diagnostic(&wire, &task, id, "fixture", false, NOW).unwrap();
        assert!(!d.launch_matched && d.next_retry_at.is_none());
        assert_eq!(runtime_process_output(&wire, "").unwrap(), pane);
        for change in [
            serde_json::json!({"name":"other"}),
            serde_json::json!({"isRemote":true}),
            serde_json::json!({"occurredAt":NOW-400000}),
            serde_json::json!({"occurredAt":NOW+1}),
            serde_json::json!({"confirmed":true}),
            serde_json::json!({"sessionId":"$3147|old"}),
        ] {
            let mut bad = record.clone();
            bad.as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
            assert!(validated_history_diagnostic(
                &format!("{pane}\n{RUNTIME_HISTORY_MARKER}{bad}"),
                &task,
                id,
                "fixture",
                false,
                NOW
            )
            .is_none());
        }
        for bad in [
            wire.replace("❌", "✅"),
            format!("{wire}\n{RUNTIME_HISTORY_MARKER}{record}"),
            wire.replace(pane, ""),
            format!("{wire}\nPermission denied"),
        ] {
            assert!(validated_history_diagnostic(&bad, &task, id, "fixture", false, NOW).is_none());
        }
    }

    // Cross-repo original shell + native transport, with only filesystem/process
    // IO faked. No real tmux, SSH, DB, platform or service operations.
    #[tokio::test]
    #[ignore = "requires QUANTNIGHT_RUNTIME_SOURCE pointing to original shell"]
    async fn original_shell_fake_os_native_runner_history_route() {
        use std::os::unix::fs::PermissionsExt;
        let source = PathBuf::from(
            std::env::var("QUANTNIGHT_RUNTIME_SOURCE").expect("source path required"),
        );
        let python =
            std::env::var("QUANTNIGHT_TEST_PYTHON").expect("existing Python interpreter required");
        let polluted="export PATH=\"$PWD/bin:/usr/bin:/bin\"; export TMUX=pi-polluted TMUX_PANE=%999 TMUX_TMPDIR=/pi-polluted; ";
        let fake = FakeBash::new(
            &(polluted.to_string() + "exec /bin/bash ./check_tmux_python.sh fixture"),
        );
        let directory = Path::new(&fake.config.env.working_dir)
            .canonicalize()
            .unwrap();
        let root = directory.as_path();
        fs::copy(&source, root.join("check_tmux_python.sh")).unwrap();
        fs::copy(
            source.parent().unwrap().join("pause_task.sh"),
            root.join("pause_task.sh"),
        )
        .unwrap();
        fs::write(
            root.join("script_config.sh"),
            format!("SCRIPT_CONFIG_PYTHON='{python}'\nget_local_workdir() {{ pwd; }}\n"),
        )
        .unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir(root.join("native")).unwrap();
        let native = root.join("native/tmux");
        // Execute the actual backend resolver; substitute only its host binary
        // candidates with our executable fixture, never a frontend resolver.
        let owner = fs::read_to_string(source.parent().unwrap().join("script_config.py")).unwrap();
        let support = owner
            .split("# BEGIN NATIVE TMUX SUPPORT\n")
            .nth(1)
            .unwrap()
            .split("# END NATIVE TMUX SUPPORT")
            .next()
            .unwrap();
        let mut lines: Vec<String> = support.lines().map(str::to_owned).collect();
        let candidates = lines
            .iter_mut()
            .find(|line| line.starts_with("NATIVE_TMUX_CANDIDATES ="))
            .unwrap();
        *candidates = format!(
            "NATIVE_TMUX_CANDIDATES = ({},)",
            serde_json::to_string(&native).unwrap()
        );
        let support = lines.join("\n");
        fs::write(
            root.join("script_config.py"),
            format!(
                "{support}\n\ndef tmux_support_source():\n    return {}\n",
                serde_json::to_string(&support).unwrap()
            ),
        )
        .unwrap();
        let os_fake = format!(
            r##"#!{python}
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
"##
        );
        let created = (bson::DateTime::now().timestamp_millis() / 1000) - 600;
        let os_fake = os_fake.replace("FIXED_SESSION_CREATED", &created.to_string());
        for path in [&native, &root.join("bin/ps")] {
            fs::write(path, &os_fake).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let shim = root.join("bin/tmux");
        fs::write(
            &shim,
            "#!/bin/sh\necho BAD_PI_SHIM >> shim-called; exit 97\n",
        )
        .unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = root.join("bin/uv");
        fs::write(&uv,format!("#!/bin/sh\n[ \"$1 $2 $3\" = 'run --no-sync python' ] || exit 98\nshift 3\nexec '{python}' \"$@\"\n")).unwrap();
        fs::set_permissions(&uv, fs::Permissions::from_mode(0o755)).unwrap();
        let (id, mut task, _) = fixture();
        task.insert(
            "tmuxContext",
            doc! {"schemaVersion":1,"backend":"tmux","socketName":"default",
            "binary":native.to_str().unwrap(),"sessionId":"$5","sessionCreatedAt":created*1000,
            "taskId":id.to_hex(),"launchAttemptId":"launch1","remote":false},
        );
        let events = Arc::new(Mutex::new(Vec::<TaskOperationStage>::new()));
        let copy = events.clone();
        let mut p = TaskOperationProgress {
            operation: "runtime",
            task_id: id.to_hex(),
            name: "fixture".into(),
            remote: false,
            request_id: "test-request".into(),
            tmux_binding: Some(task_script_binding(&task, false).unwrap()),
            last: Arc::new(Mutex::new(("initial".into(), "original runner".into()))),
            sink: Arc::new(move |e| copy.lock().unwrap().push(e)),
        };
        let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(3))
            .await
            .unwrap();
        assert!(err.is_empty());
        let observed_at = bson::DateTime::now().timestamp_millis();
        let d =
            validated_history_diagnostic(&out, &task, id, "fixture", false, observed_at).unwrap();
        let wire =
            business_diagnostic_output(&runtime_process_output(&out, &err).unwrap(), Some(d));
        assert!(wire.contains("simulation_recovery_phase_timeout") && wire.contains("300"));
        assert!(
            !wire.contains("private")
                && !wire.contains("EOFError")
                && !wire.contains("fake paused")
        );
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .all(|e| e.task_id == id.to_hex() && e.request_id == "test-request"));
        // Same persisted binding, real pause bootstrap and original V2 proof.
        fs::write(root.join("absent"), "").unwrap();
        fs::write(
            root.join("fake.sh"),
            polluted.to_string() + "exec /bin/bash ./pause_task.sh fixture false",
        )
        .unwrap();
        p.operation = "stop";
        let (out, err) = execute_task_script(&p, &fake.config, Duration::from_secs(5))
            .await
            .unwrap();
        let basis = validate_stop_output(&p, &out, &err).unwrap();
        assert!(matches!(basis, StopConfirmationBasis::SessionAbsent));
        let route = pause_route(task.clone(), out, 1).await;
        assert_eq!(route.outcome.exit_code, 0);
        let filter = &route.writes[0].0;
        assert_eq!(
            filter.get_document("tmuxContext").unwrap(),
            task.get_document("tmuxContext").unwrap()
        );
        assert!(
            !root.join("shim-called").exists(),
            "Pi PATH shim must never be invoked"
        );
        assert!(!fs::read_to_string(root.join("native-calls"))
            .unwrap()
            .contains("kill-session"));
        // Exact output of the original script/native route above; the Node
        // SFC/Pinia test consumes this only after this native test has passed.
        println!(
            "\nNATIVE_DIAGNOSTIC_TEST_WIRE {}",
            serde_json::json!({
                "taskId":id.to_hex(), "name":"fixture", "isRemote":false,
                "authProfile":task.get_str("auth_profile").unwrap_or("user1"),
                "launchAttemptId":task.get_str("launchAttemptId").unwrap(),
                "observedAt":observed_at, "output":wire
            })
        );
    }
}

/// Lowest Mongo wire/process IO only; all runtime/query/diagnostic orchestration
/// above remains production code. No live database, launchd or platform access.
#[cfg(all(test, target_os = "macos"))]
mod runtime_projection_tests {
    use super::*;
    use crate::{
        config::bash_test_support::FakeBash,
        runtime::{self, codec, identity, protocol::CallContext, server, RuntimeClient},
        task_control::TaskControl,
        task_request::TaskRequest,
    };
    use std::{
        io::Cursor,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream, UnixStream},
    };

    struct MongoIo {
        uri: String,
        finds: Arc<AtomicUsize>,
        violations: Arc<Mutex<Vec<String>>>,
        worker: Option<tokio::task::JoinHandle<()>>,
    }
    impl Drop for MongoIo {
        fn drop(&mut self) {
            if let Some(worker) = &self.worker {
                worker.abort();
            }
        }
    }
    impl MongoIo {
        async fn new(rows: Vec<Document>) -> Self {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let finds = Arc::new(AtomicUsize::new(0));
            let violations = Arc::new(Mutex::new(Vec::new()));
            let count = finds.clone();
            let errors = violations.clone();
            let worker = tokio::spawn(async move {
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    let rows = rows.clone();
                    let count = count.clone();
                    let errors = errors.clone();
                    connections.spawn(async move {
                        let mut socket = socket;
                        // Record the violation before dropping the socket: a
                        // driver EOF must not hide the lowest-IO oracle.
                        if let Err(error) = mongo_connection(&mut socket, rows, count).await {
                            errors.lock().unwrap().push(error);
                        }
                    });
                }
            });
            Self { uri: format!("mongodb://{address}/?directConnection=true&serverSelectionTimeoutMS=1000&connectTimeoutMS=1000"), finds, violations, worker: Some(worker) }
        }
        fn assert_clean(&self) {
            let errors = self.violations.lock().unwrap();
            assert!(errors.is_empty(), "unexpected lowest Mongo IO: {errors:?}");
        }
        async fn shutdown(mut self) {
            if let Some(worker) = self.worker.take() {
                worker.abort();
                let _ = worker.await;
            }
        }
    }

    // The driver's real Mongo IO seam, not the task IPC codec. Support only
    // handshake, exact task reads and the original bounded capability read.
    async fn mongo_connection(
        socket: &mut TcpStream,
        rows: Vec<Document>,
        count: Arc<AtomicUsize>,
    ) -> Result<(), String> {
        loop {
            let mut header = [0_u8; 16];
            match socket.read_exact(&mut header).await {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error.to_string()),
            }
            let length = i32::from_le_bytes(header[0..4].try_into().unwrap());
            if !(16..=1_048_576).contains(&length) {
                return Err("invalid synthetic Mongo message length".into());
            }
            let request_id = i32::from_le_bytes(header[4..8].try_into().unwrap());
            let opcode = i32::from_le_bytes(header[12..16].try_into().unwrap());
            let mut body = vec![0; length as usize - 16];
            socket
                .read_exact(&mut body)
                .await
                .map_err(|e| e.to_string())?;
            let offset = match opcode {
                2004 => {
                    // OP_QUERY initial driver handshake
                    let end = body
                        .get(4..)
                        .and_then(|b| b.iter().position(|v| *v == 0))
                        .ok_or("invalid Mongo handshake namespace")?
                        + 4;
                    end + 1 + 8
                }
                2013 if body.get(4) == Some(&0) => 5, // OP_MSG body section
                _ => return Err(format!("unexpected synthetic Mongo opcode {opcode}")),
            };
            let command = Document::from_reader(&mut Cursor::new(
                body.get(offset..).ok_or("truncated Mongo body")?,
            ))
            .map_err(|e| e.to_string())?;
            let name = command
                .keys()
                .next()
                .map(String::as_str)
                .ok_or("empty Mongo command")?;
            let response = match name.to_ascii_lowercase().as_str() {
                "hello" | "ismaster" => doc! {"ok":1.0,"ismaster":true,"isWritablePrimary":true,
                "minWireVersion":0,"maxWireVersion":13,"logicalSessionTimeoutMinutes":30,
                "maxBsonObjectSize":16_777_216,"maxMessageSizeBytes":48_000_000,"maxWriteBatchSize":100_000,"connectionId":1},
                "find" => {
                    count.fetch_add(1, Ordering::SeqCst);
                    if command.get_str("$db").unwrap_or("simulation_mission")
                        != "simulation_mission"
                    {
                        return Err("projection changed its original mission database".into());
                    }
                    let collection = command.get_str("find").map_err(|e| e.to_string())?;
                    let batch = match collection {
                        "tasks" => {
                            // mongodb2.8 Find::build omits filter for the
                            // original collection.find(None,None) task list.
                            // Omission means an empty predicate, not bad IO;
                            // present non-document filters remain violations.
                            let empty = Document::new();
                            let filter = match command.get("filter") {
                                None => &empty,
                                Some(bson::Bson::Document(filter)) => filter,
                                Some(_) => return Err("task find has a non-document filter".into()),
                            };
                            rows.iter()
                                .filter(|row| {
                                    filter.iter().all(|(key, value)| match value {
                                        bson::Bson::Document(operator)
                                            if operator.contains_key("$ne") =>
                                        {
                                            row.get(key) != operator.get("$ne")
                                        }
                                        _ => row.get(key) == Some(value),
                                    })
                                })
                                .cloned()
                                .collect::<Vec<_>>()
                        }
                        "priority_consumer_capabilities" => Vec::new(),
                        _ => return Err(format!("unexpected projection collection {collection}")),
                    };
                    doc! {"ok":1.0,"cursor":{"id":0_i64,"ns":format!("simulation_mission.{collection}"),"firstBatch":batch}}
                }
                "endsessions" => doc! {"ok":1.0},
                _ => return Err(format!("unexpected projection Mongo command {name}")),
            };
            let encoded = bson::to_vec(&response).map_err(|e| e.to_string())?;
            let mut reply = Vec::new();
            if opcode == 2004 {
                reply.extend_from_slice(&0_i32.to_le_bytes());
                reply.extend_from_slice(&0_i64.to_le_bytes());
                reply.extend_from_slice(&0_i32.to_le_bytes());
                reply.extend_from_slice(&1_i32.to_le_bytes());
            } else {
                reply.extend_from_slice(&0_i32.to_le_bytes());
                reply.push(0);
            }
            reply.extend_from_slice(&encoded);
            let mut frame = Vec::new();
            frame.extend_from_slice(&((16 + reply.len()) as i32).to_le_bytes());
            frame.extend_from_slice(&1_i32.to_le_bytes());
            frame.extend_from_slice(&request_id.to_le_bytes());
            frame.extend_from_slice(&(if opcode == 2004 { 1_i32 } else { 2013_i32 }).to_le_bytes());
            frame.extend_from_slice(&reply);
            socket.write_all(&frame).await.map_err(|e| e.to_string())?;
        }
    }

    async fn runtime_fixture(
        local: &MongoIo,
        remote: &MongoIo,
    ) -> (FakeBash, Arc<server::RuntimeState>) {
        let mut fake = FakeBash::new(
            "echo probe >> diagnostic-calls; printf '%s\\n' '❌ Pane 0 (pid=12) is running: bash'",
        );
        fake.config.mongodb.local_uri = local.uri.clone();
        fake.config.mongodb.remote_uri = remote.uri.clone();
        fake.config.mongodb.databases.mission = "simulation_mission".into();
        fake.config.mongodb.databases.simulation = "simulation_db".into();
        let root = Path::new(&fake.config.env.working_dir);
        fs::write(root.join("credentials.yaml"), "synthetic binding only\n").unwrap();
        fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let python = root.join(".venv/bin/python");
        fs::write(&python,"#!/bin/sh\nprintf '%s\\n' '{\"schemaVersion\":1,\"command\":\"validate-task-control-binding\",\"ok\":true}'\n").unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o700)).unwrap();
        let control = Arc::new(TaskControl::connect(&fake.config).await.unwrap());
        let selection = identity::RuntimeSelection {
            binding_path: root.join("binding.json"),
            backend_root: root.into(),
            frontend_root: root.into(),
            executable: std::env::current_exe().unwrap(),
            configuration: identity::ConfigurationSelector {
                kind: "backend_root".into(),
                path: root.into(),
            },
            protocol_version: 1,
            binding_revision: "isolated-projection".into(),
            target: crate::DEV_TARGET.into(),
            profile: crate::DEV_PROFILE.into(),
        };
        let host = identity::HostIdentity {
            selection,
            uid: identity::uid(),
            owner: identity::process_identity(std::process::id()).unwrap(),
            generation: "isolated-projection".into(),
            image: crate::native_identity::loaded().unwrap(),
            core_source: crate::DEV_SOURCE_ID.into(),
            cli_source: "isolated-test".into(),
            mission_database: "simulation_mission".into(),
            simulation_database: "simulation_db".into(),
            task_settings: crate::backend_binding::task_settings_fingerprint(&fake.config).unwrap(),
            ready: true,
        };
        (fake, server::RuntimeState::new(control, host))
    }
    async fn call(
        state: Arc<server::RuntimeState>,
        request: TaskRequest,
        observer: Option<RuntimeObserver>,
    ) -> Result<runtime::CallResult, runtime::TransportError> {
        call_with_context(state,request,observer,CallContext::current().unwrap()).await
    }
    async fn call_with_context(state:Arc<server::RuntimeState>,request:TaskRequest,observer:Option<RuntimeObserver>,context:CallContext)->Result<runtime::CallResult,runtime::TransportError>{
        let (client, mut host) = UnixStream::pair().unwrap();
        identity::peer_uid(&client).unwrap();
        identity::peer_uid(&host).unwrap();
        let host = tokio::spawn(async move {
            let message = codec::read(&mut host).await.unwrap();
            server::execute_message(host, state, message).await.unwrap();
        });
        let result =
            RuntimeClient::exchange(client, context, request, observer)
                .await;
        host.await.unwrap();
        result
    }

    #[tokio::test]
    async fn real_common_client_dispatch_preserves_gui_remote_cache_and_local_precedence() {
        let id = ObjectId::new();
        let local = MongoIo::new(vec![
            doc! {"_id":id,"name":"exact","status":"deactive","isRemote":false},
        ])
        .await;
        let remote = MongoIo::new(vec![
            doc! {"_id":ObjectId::new(),"name":"exact","status":"ready"},
            doc! {"_id":ObjectId::new(),"name":"remote-only","status":"ready"},
        ])
        .await;
        let (_files, state) = runtime_fixture(&local, &remote).await;
        for _ in 0..2 {
            let result = call(state.clone(), TaskRequest::GuiTasks, None).await;
            local.assert_clean();
            remote.assert_clean();
            let result = result.unwrap();
            let tasks = crate::gui_task_queries::decode_list(result.value).unwrap();
            assert_eq!(tasks.len(), 2);
            assert_eq!(tasks[0].id, id);
            assert_eq!(tasks[0].status, "deactive");
            assert_eq!(
                tasks[0].control_source,
                Some(crate::gui_task_queries::TaskControlSource::Local)
            );
            assert_eq!(tasks[1].name, "remote-only");
            assert_eq!(tasks[1].is_remote, Some(true));
            assert_eq!(
                tasks[1].control_source,
                Some(crate::gui_task_queries::TaskControlSource::RemoteOnly)
            );
        }
        assert_eq!(state.status().accepted_by_kind["GuiTasks"],2);
        assert_eq!(state.status().accepted_by_kind["Status"],0);
        assert_eq!(state.status().pools.local.initialization_attempts,1);
        assert_eq!(state.status().pools.remote.initialization_attempts,1);
        assert_eq!(local.finds.load(Ordering::SeqCst), 2);
        assert_eq!(
            remote.finds.load(Ordering::SeqCst),
            1,
            "second real dispatch must use original10s remote cache"
        );
        local.assert_clean();
        remote.assert_clean();
        drop(state);
        local.shutdown().await;
        remote.shutdown().await;
    }

    #[tokio::test]
    async fn real_common_client_dispatch_runs_original_diagnostic_script_and_fences_optional_id() {
        let id = ObjectId::new();
        let local = MongoIo::new(vec![
            doc! {"_id":id,"name":"exact","status":"running","isRemote":false,
            "taskType":"regular","auth_profile":"user1"},
        ])
        .await;
        let (_files, state) = runtime_fixture(&local, &local).await;
        let root = Path::new(&_files.config.env.working_dir);
        for task_id in [Some(id.to_hex()), None] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let recorded = events.clone();
            let observer: RuntimeObserver =
                Arc::new(move |event| recorded.lock().unwrap().push(event));
            let result = call(
                state.clone(),
                TaskRequest::TaskDiagnostics {
                    name: "exact".into(),
                    task_id,
                    remote: false,
                },
                Some(observer),
            )
            .await;
            local.assert_clean();
            let result = result.unwrap();
            assert_eq!(result.value, "❌ Pane 0 (pid=12) is running: bash");
            let emitted = events.lock().unwrap();
            let stages: Vec<_> = emitted
                .iter()
                .filter_map(|event| match event {
                    RuntimeEvent::GuiProgress { name, payload }
                        if name == "task-operation-stage" =>
                    {
                        payload["stage"].as_str()
                    }
                    _ => None,
                })
                .collect();
            for expected in [
                "initial",
                "identity",
                "config",
                "launch",
                "wait",
                "receipt_validate",
                "dispatch_read",
                "completed",
            ] {
                assert!(
                    stages.contains(&expected),
                    "shared transport lost original native stage {expected}: {stages:?}"
                );
            }
            assert_eq!(stages.last(), Some(&"completed"));
        }
        assert_eq!(
            fs::read_to_string(root.join("diagnostic-calls")).unwrap(),
            "probe\nprobe\n"
        );
        let finds = local.finds.load(Ordering::SeqCst);
        let error = call(
            state.clone(),
            TaskRequest::TaskDiagnostics {
                name: "exact".into(),
                task_id: Some(String::new()),
                remote: false,
            },
            None,
        )
        .await;
        local.assert_clean();
        let error = error.unwrap_err();
        assert!(error.safe_message.contains("任务ID无效"));
        assert_eq!(
            local.finds.load(Ordering::SeqCst),
            finds,
            "present invalid id cannot become absent/name lookup"
        );
        assert_eq!(
            fs::read_to_string(root.join("diagnostic-calls")).unwrap(),
            "probe\nprobe\n"
        );
        local.assert_clean();
        drop(state);
        local.shutdown().await;
    }
    #[tokio::test]
    async fn original_diagnostic_budget_spent_before_host_admission_does_not_restart_twelve_seconds(){
        let local=MongoIo::new(vec![]).await;let remote=MongoIo::new(vec![]).await;
        let(_files,state)=runtime_fixture(&local,&remote).await;
        let mut context=CallContext::current().unwrap();context.diagnostic_deadline_ns=Some(0);
        let error=call_with_context(state,TaskRequest::TaskDiagnostics{name:"exact".into(),task_id:None,remote:false},None,context).await.unwrap_err();
        assert_eq!(error.class,"application");assert!(error.safe_message.contains("原12秒剩余预算"));
        assert_eq!(local.finds.load(Ordering::SeqCst),0);assert_eq!(remote.finds.load(Ordering::SeqCst),0);
        local.assert_clean();remote.assert_clean();local.shutdown().await;remote.shutdown().await;
    }

    #[tokio::test]
    async fn actual_server_slow_reader_burst_flushes_events_and_gap_before_completed(){
        use crate::runtime::protocol::Message;
        let id=ObjectId::new();let local=MongoIo::new(vec![doc!{"_id":id,"name":"exact","status":"running","isRemote":false,"taskType":"regular","auth_profile":"user1"}]).await;
        let(files,state)=runtime_fixture(&local,&local).await;let root=Path::new(&files.config.env.working_dir);
        let python=std::env::var("QUANTNIGHT_TEST_PYTHON").expect("original prepared interpreter required");
        fs::write(root.join("burst.py"),r#"import json
for i in range(256):
 print('QUANTNIGHT_TASK_STAGE '+json.dumps(dict(operation='runtime',task_name='exact',target_remote=False,stage='process',message='stage-%d '%i+'x'*900)),flush=True)
print('❌ Pane 0 (pid=12) is running: bash',flush=True)
"#).unwrap();
        fs::write(root.join("fake.sh"),format!("#!/bin/sh\nexec '{python}' burst.py\n")).unwrap();
        let(mut client,mut host)=UnixStream::pair().unwrap();
        // Only this disposable socket leaf gets a small send buffer, making
        // slow-consumer pressure deterministic without production thresholds.
        {use std::os::fd::AsRawFd;let size:libc::c_int=4096;assert_eq!(unsafe{libc::setsockopt(host.as_raw_fd(),libc::SOL_SOCKET,libc::SO_SNDBUF,(&size as *const libc::c_int).cast(),std::mem::size_of_val(&size) as libc::socklen_t)},0);}
        let serving=state.clone();
        let server=tokio::spawn(async move{let message=codec::read(&mut host).await.unwrap();server::execute_message(host,serving,message).await.unwrap();});
        codec::write_owned(&mut client,Message::Call{context:CallContext::current().unwrap(),request:TaskRequest::TaskDiagnostics{name:"exact".into(),task_id:Some(id.to_hex()),remote:false}}).await.unwrap();
        assert!(matches!(codec::read(&mut client).await.unwrap(),Message::Accepted{..}));
        // Deliberately do not consume process updates until the ORIGINAL core
        // future is finished. No sleep, extra business deadline or writer mock.
        state.wait_drained().await;assert_eq!(state.status().active,0);
        let(mut events,mut gaps)=(0_usize,0_u64);let result=loop{match codec::read(&mut client).await.unwrap(){
            Message::Event(RuntimeEvent::ProgressGap{dropped})=>{gaps+=dropped;events+=1;},
            Message::Event(_)=>events+=1,
            Message::Completed{result,..}=>break result,
            other=>panic!("unexpected original server response: {other:?}"),
        }};
        assert_eq!(result.exit_code,0);assert_eq!(result.value,"❌ Pane 0 (pid=12) is running: bash");assert!(events>0);assert!(gaps>0,"slow reader must produce an explicit progress gap, not silent event loss");
        server.await.unwrap();assert!(codec::read(&mut client).await.is_err(),"Completed must be the final original server message");local.assert_clean();
        assert_eq!(local.finds.load(Ordering::SeqCst),3,"original registration plus dispatch-control and capability reads only");drop(state);local.shutdown().await;
    }

}
