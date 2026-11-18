//! Shared original pause coordination; Runtime is only the original stop-script
//! IO seam. No tmux resolver, alternate stop mechanism or new client lives here.
use crate::{
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    task_store::{Host, TaskStore},
};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};

#[derive(Debug)]
pub struct StopOutput {
    pub stdout: String,
    pub stderr: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    async fn verify_launcher_settled(&self,_root:&Path,_task:&Document,_remaining:Duration)
        -> Result<bool,String>{Ok(false)}
    async fn inspect_generation(
        &self,
        _root: &Path,
        _task: &Document,
    ) -> Result<crate::create_generate::GenerationProcesses, String> {
        Err("original generation process inspection unavailable".into())
    }
    async fn stop_bounded(&self, root: &Path, task: &Document, never: bool,
        remaining: Duration) -> Result<StopOutput, String> {
        tokio::time::timeout(remaining, self.stop(root, task, never)).await
            .map_err(|_| "original stop allowance exhausted; not retried".to_string())?
    }
}
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct PauseStage {
    pub stage: String,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn stage(observer: &Option<PauseObserver>, id: &'static str, message: impl Into<String>) {
    if let Some(observer) = observer {
        observer(PauseStage {
            stage: id.into(),
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
    policy::task_binding_query(name, task, "pause-control")
}
fn remaining(deadline: tokio::time::Instant) -> Result<Duration, String> {
    let value = deadline.saturating_duration_since(tokio::time::Instant::now());
    if value.is_zero() { Err("original operation deadline exhausted; no additional attempt".into()) }
    else { Ok(value) }
}
#[derive(Clone, Copy)]
pub(crate) enum StopIntent { StopAllowed }
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
/// Stop the selected task's current activity without writing its control row.
/// Callers keep the same exclusive mission lock through their own pause CAS
/// or collection rename, and bound the whole operation by `deadline`.
/// `observe-accepted` takes this lock too; its saved claim is not a liveness
/// test. Both callers use the runtime's current process/writer observations.
pub(crate) async fn stop_activity_locked(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn PauseRuntime,
    task: &Document,
    _guard: &MissionLocks,
    deadline: tokio::time::Instant,
    observer: &Option<PauseObserver>,
    output: &mut Option<StopOutput>,
    reconciled_reason: &mut Option<String>,
) -> Result<StopConfirmationBasis, String> {
    let name = task.get_str("name").map_err(|_| "missing exact task name")?;
    policy::validate_name(name)?;
    let remote = crate::cleanup_sync::task_host(task)?.is_remote();
    let filter = pause_filter(name, task)?;
    let mut verification_task = task.clone();
    verification_task.remove("observedExecutionBeforeStop");
    stage(observer, "stop", "inspect the current execution scope; signal at most once");
    let observation = runtime.inspect_before_stop(
        root, store, task, remaining(deadline)?.min(Duration::from_secs(12)),
    ).await?;
    if let Some(observation) = observation {
        let observed = crate::start::config_document(observation)?;
        if observed.get_bool("identityVerified") == Ok(true) || observed.get_bool("explicitPauseVerified") == Ok(true) {
            if let Ok(context) = observed.get_document("tmuxContext") {
                verification_task.insert("tmuxContext", context.clone());
            }
        }
        verification_task.insert("observedExecutionBeforeStop", observed);
    }
    let current = active_task(store, name).await?;
    if filter.keys().any(|key| task.get(key) != current.get(key)) {
        return Err("current task/host/operation changed before stop; no signal sent".into());
    }
    let attempt = runtime.stop_bounded(root, &verification_task, false, remaining(deadline)?).await
        .and_then(|out| verified_stop_proof(&out.stdout, name, remote).map(|basis| (out, basis)));
    let (response, basis, used_recovery) = match attempt {
        Ok((out, basis)) => (out, basis, false),
        Err(first) => {
            // An unconfirmed reply never authorizes another signal. Do
            // not erase explicit evidence of a live or replaced scope.
            if first.contains("known_original_runners_still_live") {
                return Err(first);
            }
            let proof = runtime.verify_stopped(root, &verification_task, remaining(deadline)?.min(Duration::from_secs(12))).await
                .map_err(|next| format!("{first}; original read-only recovery: {next}"))?
                .ok_or_else(|| format!("{first}; original read-only recovery did not confirm stop"))?;
            let basis = verified_stop_proof(&proof.stdout, name, remote)?;
            *reconciled_reason = Some(first);
            (proof, basis, true)
        }
    };
    *output = Some(response);
    if used_recovery {
        stage(observer, "receipt_validate", "check current execution scope after stop");
        runtime.confirm_stopped_scope(root, store, &verification_task, remaining(deadline)?.min(Duration::from_secs(12))).await?;
    }
    Ok(basis)
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
    let mut execution_confirmed = false;
    let result: Result<(ConfirmedTaskStop,String,String),String> = tokio::time::timeout_at(deadline, async {
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
        let launch = task.get("launchAttemptId");
        if options.expected_launch_attempt_id.is_some_and(|expected| launch != Some(&Bson::String(expected.into())))
            || (options.expected_launch_absent && !matches!(launch, None | Some(Bson::Null))) {
            return Err("launch identity changed before mutation".into());
        }
        let remote = match task.get("isRemote") { None | Some(Bson::Null | Bson::Boolean(false)) => false, Some(Bson::Boolean(true)) => true, _ => return Err("invalid task execution host".into()) };
        if options.expected_remote.is_some_and(|expected| expected != remote) { return Err("task execution host changed before pause".into()); }
        let filter = pause_filter(name, &task)?;
        let StopIntent::StopAllowed = intent;
        let basis = stop_activity_locked(root, store, runtime, &task, _guard, deadline,
            observer, &mut output, &mut reconciled_reason).await?;
        execution_confirmed = true;
        let at = bson::DateTime::now(); let now = Utc::now().to_rfc3339();
        let proof = match basis {
            StopConfirmationBasis::RunnersExited => doc! {"schema_version":1,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":true},
            StopConfirmationBasis::SessionAbsent => doc! {"schema_version":2,"task_name":name,"target_remote":remote,"session_absent":true,"runners_exited":Bson::Null,"confirmation_basis":"session_absent"},
        };
        // Normalize interrupted generation back to pending (recording failed
        // generation phase); all other stopped states converge to paused.
        let is_gen_interrupted = matches!(
            task.get_str("status").ok(),
            Some("generating" | "generation_unknown")
        );
        let task_status = if is_gen_interrupted {
            "pending"
        } else {
            "paused"
        };
        let mut fields = doc! {"status":task_status,"pausedAt":&now,"updatedAt":&now,"stopConfirmedAt":at,
            "stopConfirmedRemote":remote,"stopConfirmedTaskId":id.to_hex(),"stopConfirmationBasis":basis.as_str()};
        if is_gen_interrupted {
            let mut gen_doc = task.get_document("generation").cloned().unwrap_or_default();
            gen_doc.insert("phase", "generation_failed");
            fields.insert("generation", gen_doc);
        }
        // Compatibility projection only: current scoped stop completed. No
        // persisted launch settlement is consulted as an admission condition.
        if task.contains_key("launchSettledAttemptId") || task.get_str("status") == Ok("launching") {
            if let Ok(launch) = task.get_str("launchAttemptId") {
                fields.insert("launchSettledAttemptId", launch);
            }
        }
        let event = doc! {"id":ObjectId::new().to_hex(),"command":"pause","status":"paused","at":&now,"detail":proof};
        stage(observer,"control_write","update original task and operation; never a name-only write");
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
            stop_confirmed_at:at.timestamp_millis(),confirmation_basis:basis},now,task_status.to_string()))
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
        Ok((stop,control_revision,task_status)) => {
            value["controlRevision"] = json!(control_revision);
            value["taskStatus"] = json!(task_status);
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
            value["executionStopConfirmed"] = json!(execution_confirmed);
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
