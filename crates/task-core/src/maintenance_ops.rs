//! Shared watch / paused GET-only observation / Priority prune coordination.
//! Original watcher, finite observer and research cleanup remain Runtime owners.
use crate::start::NotificationOptions;
use async_trait::async_trait;
use bson::Document;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct WatchRequest {
    pub name: String,
    pub notification: NotificationOptions,
    pub replace_watcher: bool,
}
#[derive(Clone, Debug)]
pub struct ObserveAcceptedRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PriorityPruneRequest {
    pub name: String,
    pub confirm: String,
    pub sharpe: f64,
    pub fitness: f64,
    pub dry_run: bool,
}
#[derive(Clone, Debug)]
pub struct ObservationCommand {
    pub argv: Vec<String>,
    pub timeout_seconds: u64,
}
#[derive(Clone, Debug)]
pub struct ObservationOutput {
    pub returncode: i32,
    pub stdout: String,
    pub stderr: String,
}
#[derive(Clone, Debug)]
pub struct PriorityPruneCommand {
    /// Relative to the bound root on the actual execution host.
    pub checkpoint_relative: PathBuf,
    pub name: String,
    pub profile: String,
    pub remote: bool,
    pub sharpe: f64,
    pub fitness: f64,
    pub dry_run: bool,
    pub binding: Document,
    pub timeout_seconds: u64,
}
#[async_trait]
pub trait MaintenanceRuntime: Send + Sync {
    /// One original watcher --native-control preflight: validate exact recipient
    /// and compute launch fallback/generation from this task snapshot in the
    /// SAME call. Extract result.expectedBinding from the original preflight
    /// response, never from the later attachment receipt; no second helper
    /// process is needed.
    async fn prepare_watch(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
    ) -> Result<Document, String>;
    /// Original attachment owner verifies PID birth/cmd/cwd, mission/launch/
    /// generation/host; replace stops only its authenticated old producer.
    async fn attach_watcher(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
        replace: bool,
    ) -> Result<Document, String>;
    /// Original process inspector; uncertainty is Err, never absence.
    async fn assert_consumer_absent(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
    ) -> Result<(), String>;
    /// One original start_super_task.sh --observe-accepted invocation, 540s.
    /// A timeout/lost response is Err; never retry or clear the persisted claim.
    async fn observe_accepted(
        &self,
        root: &Path,
        command: &ObservationCommand,
    ) -> Result<ObservationOutput, String>;
    /// Original scripts.priority_cleanup.prune owner, including original pool
    /// flock/checkpoint validation and never-claimed reservation deletion CAS.
    /// Remote path uses original remote_exec/host/workdir/stdin, no deployment.
    /// Must check quiescence within the original pool lock, never write tasks.
    /// Preserve the old local quiescence callback's fresh PAUSED Priority /
    /// ObjectId / profile / effective-host control recheck, as well as the
    /// original bound consumer check; a stale snapshot alone is not quiescence.
    async fn priority_prune(
        &self,
        root: &Path,
        task: &Document,
        command: &PriorityPruneCommand,
    ) -> Result<Document, String>;
}

use crate::{
    cleanup_sync::{failure, outcome, wire_document, TaskOutcome},
    lifecycle_policy as policy,
    mission_lock::MissionLocks,
    start,
    task_store::{Host, TaskStore},
};
use bson::{doc, oid::ObjectId, Bson};
use serde_json::{json, Value};

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}
fn nonce() -> String {
    bson::Uuid::new().to_string().replace('-', "")
}
fn task_id(task: &Document) -> Result<ObjectId, String> {
    task.get_object_id("_id")
        .map_err(|_| "task must retain its original ObjectId".into())
}
fn host(task: &Document, key: &str) -> Result<bool, String> {
    match task.get(key) {
        None => Ok(false),
        Some(Bson::Boolean(v)) => Ok(*v),
        _ => Err(format!("invalid execution host binding: {key}")),
    }
}
async fn active(store: &dyn TaskStore, name: &str) -> Result<Document, String> {
    let task = store
        .find_task(
            Host::Local,
            doc! {"name":name,"status":{"$ne":"deactive"}},
            true,
        )
        .await?
        .ok_or("task not found")?;
    task_id(&task)?;
    if task.get_str("name") != Ok(name) {
        return Err("task name binding changed".into());
    }
    Ok(task)
}
/// Preserve missing vs null and original policy fences; additionally pin fields
/// used across awaited specialist calls even for legacy writers without updatedAt.
fn binding(task: &Document, command: &str) -> Result<Document, String> {
    let name = task.get_str("name").map_err(|_| "missing task name")?;
    let mut query = policy::task_binding_query(name, task, command)?;
    for key in [
        "acceptedObservation",
        "taskType",
        "auth_profile",
        "targetRemote",
        "generation",
        "startupBinding",
        "startedAt",
        "launchAttemptAt",
        "runId",
        "startConfig",
        "mission_config",
        "config",
        "stopConfirmedAt",
        "stopConfirmedRemote",
        "stopConfirmedTaskId",
        "stopConfirmationBasis",
        "agentNotification",
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
async fn unchanged(
    store: &dyn TaskStore,
    task: &Document,
    command: &str,
) -> Result<Document, String> {
    let current = active(
        store,
        task.get_str("name").map_err(|_| "missing task name")?,
    )
    .await?;
    if binding(task, command)?
        .keys()
        .any(|k| current.get(k) != task.get(k))
    {
        return Err(
            "task identity/state/claim changed before mutation; retain original evidence".into(),
        );
    }
    Ok(current)
}
async fn update(
    store: &dyn TaskStore,
    task: &Document,
    command: &str,
    mut fields: Document,
    detail: Document,
) -> Result<Document, String> {
    let query = binding(task, command)?;
    fields.insert("updatedAt", now());
    let event = doc! {"id":nonce(),"command":command,
    "status":fields.get_str("status").unwrap_or("ok"),"at":now(),"detail":detail};
    let mut saved = task.clone();
    saved.extend(fields.clone());
    // Validate before the write, not after it has succeeded.
    match saved.get_mut("events") {
        Some(Bson::Array(events)) => events.push(Bson::Document(event.clone())),
        None => {
            saved.insert("events", vec![Bson::Document(event.clone())]);
        }
        _ => return Err("invalid task event history".into()),
    }
    if store
        .update_task(
            Host::Local,
            query,
            doc! {"$set":fields,"$push":{"events":event}},
        )
        .await?
        != 1
    {
        return Err(format!(
            "{command} task CAS not confirmed; original claim/evidence retained"
        ));
    }
    Ok(saved)
}
fn lock(root: &Path, name: &str) -> Result<MissionLocks, String> {
    policy::validate_name(name)?;
    MissionLocks::acquire(root, &[name], false)
}

pub async fn watch(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &WatchRequest,
) -> TaskOutcome {
    let _lock = match lock(root, &request.name) {
        Ok(lock) => lock,
        Err(e) => return failure("watch", &request.name, e),
    };
    match watch_locked(root, store, runtime, request).await {
        Ok(v) => v,
        Err(e) => failure("watch", &request.name, e),
    }
}
async fn watch_locked(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &WatchRequest,
) -> Result<TaskOutcome, String> {
    let task = active(store, &request.name).await?;
    policy::assert_observation_closed(&task)?;
    // Original legacy watch treats an absent/null execution side as local.
    // Prune and observation retain their stricter boolean host contracts.
    let remote = match task.get("isRemote") {
        None | Some(Bson::Null) => false,
        Some(Bson::Boolean(v)) => *v,
        _ => return Err("invalid execution host binding: isRemote".into()),
    };
    let spec = start::notification_spec(&task, &request.notification)?
        .ok_or("provide an exact --notify-session-id or --notify-task-id")?;
    let expected = runtime.prepare_watch(root, &task, remote, &spec).await?;
    unchanged(store, &task, "watch-attach").await?;
    // No consumer inspection/start: generating, legacy and already running
    // tasks retain the original watcher owner's complete attachment semantics.
    let receipt = runtime
        .attach_watcher(root, &task, remote, &spec, request.replace_watcher)
        .await?;
    let committed = async {
        start::notification_receipt_bound(&receipt, &task, remote, &spec, &expected)?;
        update(
            store,
            &task,
            "watch-attach",
            doc! {"agentNotification":&receipt},
            receipt.clone(),
        )
        .await?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(e) = committed {
        // The original producer may already exist. Return its evidence even if
        // metadata CAS failed; never terminate an unverified PID or spawn twice.
        return Ok(outcome(
            "watch",
            &request.name,
            false,
            json!({
            "simulationStarted":false,"notification":wire_document(&receipt),
            "notificationMetadataConfirmed":false,"error":e,
            "nextAction":"reconcile original producer/identity; do not restart simulation or duplicate watcher"}),
        ));
    }
    Ok(outcome(
        "watch",
        &request.name,
        true,
        json!({"notification":wire_document(&receipt),"simulationStarted":false}),
    ))
}

pub async fn observe_accepted(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &ObserveAcceptedRequest,
) -> TaskOutcome {
    let _lock = match lock(root, &request.name) {
        Ok(lock) => lock,
        Err(e) => return failure("observe-accepted", &request.name, e),
    };
    match observe_locked(root, store, runtime, request).await {
        Ok(v) => v,
        Err(e) => failure("observe-accepted", &request.name, e),
    }
}
async fn observe_locked(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &ObserveAcceptedRequest,
) -> Result<TaskOutcome, String> {
    let task = active(store, &request.name).await?;
    let id = task_id(&task)?.to_hex();
    if task.get_str("taskType") != Ok("super") || task.get_str("status") != Ok("paused") {
        return Err(
            "observe-accepted requires the original paused SUPER task and execution side".into(),
        );
    }
    let remote = task
        .get_bool("isRemote")
        .map_err(|_| "observation execution side must be boolean")?;
    if request
        .expected_task_id
        .as_ref()
        .is_some_and(|expected| expected != &id)
    {
        return Err("observation task ID mismatch".into());
    }
    policy::assert_observation_closed(&task)?;
    if !policy::stop_confirmation_valid(&task, remote) {
        return Err(
            "observe-accepted requires confirmed stopped execution for this task/side".into(),
        );
    }
    runtime.assert_consumer_absent(root, &task, remote).await?;
    let profile = task
        .get_str("auth_profile")
        .map_err(|_| "original authentication profile unavailable")?;
    policy::validate_name(profile).map_err(|_| "original authentication profile unavailable")?;
    let mut config = start::saved_start_config(&task, None)?;
    let tag = match config.get("tag") {
        None | Some(Bson::Null) | Some(Bson::Boolean(false)) => None,
        Some(Bson::Document(tag)) => Some(tag),
        Some(Bson::Int32(0) | Bson::Int64(0)) => None,
        Some(Bson::Double(v)) if *v == 0.0 => None,
        Some(Bson::String(v)) if v.is_empty() => None,
        Some(Bson::Array(v)) if v.is_empty() => None,
        _ => return Err("invalid saved observation tag".into()),
    };
    let mut runs = Vec::<String>::new();
    for candidate in [
        task.get_str("runId").ok(),
        tag.and_then(|v| v.get_str("runId").ok()),
        Some(request.name.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if !candidate.is_empty() && !runs.iter().any(|v| v == candidate) {
            runs.push(candidate.into());
        }
    }
    let observation = doc! {"id":nonce(),"taskId":&id,"name":&request.name,
    "auth_profile":profile,"remote":remote,"runIds":runs};
    config.extend(
        doc! {"mission_tag":&request.name,"dist":if remote {"remote"} else {"local"},
        "auth_profile":profile,"max_concurrent":1,"max_multi_simulation_children":1,
        "breakpoint":true,"tag":{},"accepted_observation":&observation},
    );
    let config_json = start::strict_json(&Bson::Document(config))?.to_string();
    let mut claim = observation.clone();
    claim.extend(doc! {"startedAt":now(),"finishedAt":Bson::Null});
    update(
        store,
        &task,
        "observe-accepted-begin",
        doc! {"acceptedObservation":&claim},
        observation.clone(),
    )
    .await?;
    // Re-read the durable claim instead of trusting an acknowledged local copy.
    let claimed = active(store, &request.name).await?;
    let mut expected = task.clone();
    expected.insert("acceptedObservation", claim.clone());
    for key in binding(&expected, "observe-accepted-begin")?
        .keys()
        .filter(|key| *key != "updatedAt")
    {
        if claimed.get(key) != expected.get(key) {
            return Err(
                "observation claim/identity changed before execution; claim retained".into(),
            );
        }
    }
    let command = ObservationCommand {
        argv: vec![
            "bash".into(),
            "scripts/start_super_task.sh".into(),
            request.name.clone(),
            config_json,
            if remote { "--remote" } else { "false" }.into(),
            profile.into(),
            "--observe-accepted".into(),
        ],
        timeout_seconds: 540,
    };
    let output = runtime.observe_accepted(root, &command).await
        .map_err(|e| format!("observation response unconfirmed: {e}; claim retained, no automatic start or retry"))?;
    let receipt = observation_receipt(&output, &observation)?;
    let recovery = receipt
        .get_document("recovery")
        .map_err(|_| "observation recovery missing")?
        .clone();
    claim.extend(doc! {"finishedAt":now(),"recovery":&recovery});
    update(
        store,
        &claimed,
        "observe-accepted-finish",
        doc! {"acceptedObservation":&claim},
        receipt,
    )
    .await?;
    let ok = integer(recovery.get("unknown")) == Some(0);
    Ok(outcome(
        "observe-accepted",
        &request.name,
        ok,
        json!({"taskId":id,"taskStatus":"paused",
        "remote":remote,"simulationStarted":false,"observation":wire_document(&claim)}),
    ))
}
fn integer(value: Option<&Bson>) -> Option<i64> {
    match value {
        Some(Bson::Int32(v)) => Some(i64::from(*v)),
        Some(Bson::Int64(v)) => Some(*v),
        _ => None,
    }
}
fn observation_receipt(
    output: &ObservationOutput,
    expected: &Document,
) -> Result<Document, String> {
    let receipts: Vec<_> = output
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("SUPER_ACCEPTED_OBSERVATION "))
        .collect();
    if output.returncode != 0 || receipts.len() != 1 {
        return Err(format!("observation exit/receipt unconfirmed (exit {}, receipts {}); claim retained, no automatic start or retry", output.returncode, receipts.len()));
    }
    let receipt = start::config_document(
        serde_json::from_str::<Value>(receipts[0])
            .map_err(|e| format!("invalid observation receipt: {e}; claim retained"))?,
    )?;
    if receipt.get_document("binding").ok() != Some(expected)
        || receipt.get_bool("closed") != Ok(true)
    {
        return Err("observation completion binding mismatch; claim retained".into());
    }
    let recovery = receipt
        .get_document("recovery")
        .map_err(|_| "observation recovery unavailable; claim retained")?;
    if ["observed", "released", "unknown"]
        .iter()
        .any(|key| integer(recovery.get(*key)).is_none_or(|n| n < 0))
        || integer(recovery.get("released")) != Some(0)
    {
        return Err("invalid observation counters/release evidence; claim retained".into());
    }
    Ok(receipt)
}

pub async fn priority_prune(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &PriorityPruneRequest,
) -> TaskOutcome {
    let _lock = match lock(root, &request.name) {
        Ok(lock) => lock,
        Err(e) => return failure("priority-prune", &request.name, e),
    };
    match prune_locked(root, store, runtime, request).await {
        Ok(v) => v,
        Err(e) => failure("priority-prune", &request.name, e),
    }
}
async fn prune_locked(
    root: &Path,
    store: &dyn TaskStore,
    runtime: &dyn MaintenanceRuntime,
    request: &PriorityPruneRequest,
) -> Result<TaskOutcome, String> {
    if request.confirm != request.name {
        return Err("priority-prune requires exact --confirm TASK_NAME".into());
    }
    if [request.sharpe, request.fitness]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err("priority-prune thresholds must be finite and nonnegative".into());
    }
    let task = active(store, &request.name).await?;
    policy::assert_observation_closed(&task)?;
    if task.get_str("taskType") != Ok("priority") || task.get_str("status") != Ok("paused") {
        return Err("priority-prune requires a PAUSED Priority task".into());
    }
    // Validate both fields before OR (short-circuit must not hide malformed host).
    let side = host(&task, "isRemote")?;
    let target = host(&task, "targetRemote")?;
    let remote = side || target;
    // Original prune eligibility is PAUSED + original quiescence, not a saved
    // stop-proof requirement. Legacy tasks without stop fields remain valid;
    // do not rewrite isRemote or manufacture a proof for the effective host.
    let profile = match task.get("auth_profile") {
        None => "user1",
        Some(Bson::String(s)) => s.as_str(),
        _ => return Err("invalid priority auth_profile".into()),
    };
    if profile.is_empty()
        || !profile
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("invalid priority auth_profile".into());
    }
    runtime.assert_consumer_absent(root, &task, remote).await?;
    unchanged(store, &task, "priority-prune").await?;
    let command = PriorityPruneCommand {
        checkpoint_relative: PathBuf::from("artifacts/alpha_optimization/priority_roots")
            .join(&request.name)
            .join(profile)
            .join("pool.json"),
        name: request.name.clone(),
        profile: profile.into(),
        remote,
        sharpe: request.sharpe,
        fitness: request.fitness,
        dry_run: request.dry_run,
        timeout_seconds: 180,
        binding: doc! {"taskId":task_id(&task)?.to_hex(),"remote":remote,
        "launchAttemptId":task.get("launchAttemptId").cloned().unwrap_or(Bson::Null),
        "tmuxContext":task.get("tmuxContext").cloned().unwrap_or(Bson::Null)},
    };
    let report = runtime.priority_prune(root, &task, &command).await?;
    let mut wire = start::strict_json(&Bson::Document(report.clone()))?;
    if wire.to_string().len() + 1 > 16 * 1024 * 1024 {
        return Err("cleanup report exceeds 16 MiB wire limit".into());
    }
    if report.get_bool("dryRun") != Ok(request.dry_run) {
        return Err("cleanup dry-run receipt mismatch".into());
    }
    if !request.dry_run {
        // The original algorithm may have already retired roots. Failed audit
        // CAS is a failure, not permission to rerun cleanup or switch tasks.
        if let Err(error) = update(
            store,
            &task,
            "priority-prune",
            Document::new(),
            report.clone(),
        )
        .await
        {
            return Ok(outcome(
                "priority-prune",
                &request.name,
                false,
                json!({"remote":remote,
                "error":error,"cleanupReport":wire,"auditConfirmed":false}),
            ));
        }
    }
    wire["remote"] = json!(remote);
    Ok(outcome("priority-prune", &request.name, true, wire))
}
