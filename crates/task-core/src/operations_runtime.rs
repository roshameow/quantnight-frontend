//! Adapters to the original shell and research owners. No taskctl recursion.
use std::{path::Path, time::Duration};
use async_trait::async_trait;
use bson::{Bson, Document};
use serde_json::{json, Value};
use tokio::process::Command;
use crate::{backend_binding::BackendBinding, cleanup_sync::OperationsRuntime,
    config::{run_process_observed, BashScriptObserver}, lifecycle_policy::validate_name,
    task_store::Host};

pub struct OriginalOperationsRuntime<'a> {
    pub binding: &'a BackendBinding,
    pub observer: Option<BashScriptObserver>,
}
impl OriginalOperationsRuntime<'_> {
    async fn verify_root(&self, root: &Path) -> Result<(), String> {
        if root.canonicalize().map_err(|_| "task root unavailable")? != self.binding.root() {
            return Err("task operation belongs to a different backend root".into());
        }
        self.binding.refresh_if_changed().await
    }
    async fn owner_result(&self, root: &Path, module: &str, flag: &str,
        expected_command: &str, request: Value, budget: Duration) -> Result<Value, String> {
        self.verify_root(root).await?;
        let mut command = self.process(root.join(".venv/bin/python"));
        command.arg("-B");
        if module.ends_with(".py") { command.arg(root.join(module)); }
        else { command.args(["-m", module]); }
        command.arg(flag).arg(request.to_string());
        let (out, _) = run_process_observed(command, budget, false, self.observer.clone()).await?;
        let receipt: Value = serde_json::from_str(&out)
            .map_err(|_| format!("{expected_command}: original owner returned no valid JSON receipt"))?;
        if receipt.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || receipt.get("command").and_then(Value::as_str) != Some(expected_command)
            || receipt.get("ok").and_then(Value::as_bool) != Some(true)
            || receipt.get("result").is_none() {
            return Err(format!("{expected_command}: original owner did not confirm its result"));
        }
        Ok(receipt["result"].clone())
    }
    async fn attach_watcher_owner(&self, root: &Path, task: &Document, remote: bool, spec: &Document, replace: bool) -> Result<Document, String> {
        let result = self.owner_result(root, ".agents/skills/manage-quantnight-frontend/scripts/watch_quant_task.py", "--native-control",
            "native-mission-watcher", json!({"action":"attach", "task":start_task_metadata(task), "remote":remote,
                "spec":crate::start::strict_json(&Bson::Document(spec.clone()))?, "replace":replace,
                "configurationBinding":self.binding.request()}), Duration::from_secs(30)).await?;
        crate::start::config_document(result)
    }
    async fn execution_owner(&self, root: &Path, store: &dyn crate::task_store::TaskStore,
        task: &Document, remote: bool, requested: Option<&Document>, allowance: Duration) -> Result<Value, String> {
        let deadline = tokio::time::Instant::now() + allowance.min(Duration::from_secs(12));
        tokio::time::timeout_at(deadline,async {
        self.verify_root(root).await?;
        let name = task.get_str("name").map_err(|_| "missing task name")?;
        let profile = task.get_str("auth_profile").unwrap_or("user1");
        let id = task.get_object_id("_id").map_err(|_| "missing original task ID")?;
        let side = Host::from_remote(remote);
        let registry = {
            let row = store.find_task(side, bson::doc! {"name":name,"status":{"$ne":"deactive"}}, true).await?;
            if row.as_ref().is_some_and(|r| r.get("_id") != task.get("_id") || (r.contains_key("auth_profile") && task.contains_key("auth_profile") && r.get("auth_profile") != task.get("auth_profile"))
                || r.get("isRemote").is_some_and(|value| match value { Bson::Null => remote, Bson::Boolean(value) => *value != remote, _ => true })
                || (r.contains_key("taskType") && r.get("taskType") != task.get("taskType"))
                || (r.contains_key("launchAttemptId") && r.get("launchAttemptId") != task.get("launchAttemptId"))) { return Err("execution registry belongs to another task/profile/host/launch".into()); }
            row
        };
        if registry.as_ref().is_some_and(|r|r.get("simulatorRuntime").is_some_and(|v| !matches!(v,Bson::Null|Bson::Document(_)))) {
            return Err("original runtime registry evidence malformed; not missing legacy metadata".into());
        }
        let mut evidence = registry.as_ref().and_then(|r| r.get_document("simulatorRuntime").ok()).cloned().unwrap_or_default();
        evidence=evidence.iter().filter(|(key,_)| ["pid","pidCreatedAt","heartbeatAt","consumer_run_id","configDigestVersion","effectiveConfigDigest","launchAttemptId","startupRecovery","task","auth_profile","dist","source_task_id","source_remote"].contains(&key.as_str())).map(|(key,value)|(key.clone(),value.clone())).collect();
        if !evidence.is_empty() { evidence.insert("registryTaskId", id.to_hex()); }
        if task.get_str("taskType").ok() == Some("regular") && !profile.is_empty() {
            if let Some(capability) = store.consumer_runtime(side, name, profile).await? { evidence = capability; }
        }
        // Do not lose independently known runs when capability replaces registry
        // evidence. The original checker must compare every available source.
        let mut known_runs=Vec::new();
        for row in [Some(task),registry.as_ref()].into_iter().flatten() {
            for key in ["startupBinding","simulatorRuntime"] {
                if let Ok(value)=row.get_document(key) {
                    if let Some(run)=value.get("consumer_run_id") {known_runs.push(run.clone().into_relaxed_extjson());}
                }
            }
            if let Some(run)=row.get("consumer_run_id") {known_runs.push(run.clone().into_relaxed_extjson());}
        }
        let binding = json!({"knownConsumerRuns":known_runs,
            "taskCreatedAt":task.get("createdAt").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
            "launchAttemptAt":task.get("launchAttemptAt").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
            "taskId":id.to_hex(),"remote":remote,"taskType":task.get_str("taskType").unwrap_or("regular"),
            "auth_profile":profile,"launchAttemptId":task.get("launchAttemptId").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
            "consumer_run_id":task.get_document("startupBinding").ok().and_then(|b| b.get_str("consumer_run_id").ok())
                .or_else(||task.get_document("simulatorRuntime").ok().and_then(|b|b.get_str("consumer_run_id").ok()))
                .or_else(||task.get_str("consumer_run_id").ok()),
            "tmuxContext":task.get("tmuxContext").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
            "runtimeEvidence":Bson::Document(evidence).into_relaxed_extjson(),
            "knownProcesses":task.get_document("observedExecutionBeforeStop").ok().and_then(|v|v.get_array("processes").ok())
                .map(|v|Bson::Array(v.clone()).into_relaxed_extjson()),
            "requestedConfig":requested.map(|c| crate::start::strict_json(&Bson::Document(c.clone()))).transpose()?});
        let remaining=deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {return Err("original execution observation deadline exhausted".into());}
        let mut command=self.process("/bin/bash");
        command.arg("scripts/check_tmux_python.sh").arg(name).arg(if remote {"true"} else {"false"})
            .args(["--inspect-execution","--binding"]).arg(binding.to_string());
        let (out,_)=run_process_observed(command,remaining,true,self.observer.clone()).await?;
        let markers:Vec<_>=out.lines().filter_map(|line| line.strip_prefix("QUANTNIGHT_EXECUTION_OBSERVED ")).collect();
        if markers.len()!=1 {return Err("original execution owner returned no unique structured observation".into());}
        let observed:Value=serde_json::from_str(markers[0]).map_err(|_| "invalid original execution observation")?;
        if observed["schemaVersion"]!=1 || observed["taskName"]!=name || observed["remote"]!=remote
            || observed["identityVerified"].as_bool().is_none() || observed["consumerRunning"].as_bool().is_none()
            || !matches!(observed["state"].as_str(),Some("live"|"owned_residue"|"session_absent"|"unverified")) {
            return Err("original execution observation identity/schema mismatch".into());
        }
        let coherent=match observed["state"].as_str(){
            Some("session_absent")=>observed["consumerRunning"]==false&&observed["identityVerified"]==false&&observed["ownedResidue"]==false&&observed.get("tmuxContext").map_or(true,Value::is_null),
            Some("live")=>observed["consumerRunning"]==true&&observed["identityVerified"]==true&&observed["ownedResidue"]==false
                &&observed["pid"].as_i64().is_some_and(|p|p>1)&&observed["pidCreatedAt"].as_f64().is_some_and(|v|v.is_finite()&&v>0.0),
            Some("owned_residue")=>observed["consumerRunning"]==false&&observed["identityVerified"]==true&&observed["ownedResidue"]==true,
            Some("unverified")=>observed["identityVerified"]==false&&observed["ownedResidue"]==false,
            _=>false,
        };
        if !coherent{return Err("original execution observation contains contradictory facts".into());}
        // Scoped legacy evidence is solely for idempotent START, never a
        // launch/run attestation or owned-residue authority.
        if observed["consumerScopeVerified"]==true && (observed["state"]!="unverified"
            || observed["consumerRunning"]!=true || observed["explicitPauseVerified"]!=true
            || observed["identityVerified"]!=false || observed["ownedResidue"]!=false
            || observed["taskId"]!=id.to_hex()
            || !observed["pid"].as_i64().is_some_and(|p|p>1)
            || !observed["pidCreatedAt"].as_f64().is_some_and(|v|v.is_finite()&&v>0.0)) {
            return Err("original consumer scope observation contains contradictory facts".into());
        }
        if observed["identityVerified"]==true && (observed["taskId"]!=id.to_hex()
            || observed["launchAttemptId"]!=binding["launchAttemptId"]) {
            return Err("independent execution identity disagrees with captured control".into());
        }
        Ok(observed)
        }).await.map_err(|_|"original execution observation deadline exhausted".to_string())?
    }
    async fn pause_owner(&self, root: &Path, task: &Document, mode: Option<&str>,
        allowance: Duration) -> Result<crate::pause::StopOutput, String> {
        let deadline = tokio::time::Instant::now() + allowance;
        self.verify_root(root).await?;
        let name = task.get_str("name").map_err(|_| "missing exact task name")?;
        validate_name(name)?;
        let id = task.get_object_id("_id").map_err(|_| "missing original task ObjectId")?;
        let remote = match task.get("isRemote") {
            None | Some(Bson::Null | Bson::Boolean(false)) => false,
            Some(Bson::Boolean(true)) => true, _ => return Err("invalid original task execution host".into()),
        };
        let binding = json!({"taskId":id.to_hex(), "remote":remote,
            "launchAttemptId":task.get("launchAttemptId").filter(|v| **v!=Bson::Null).cloned()
                .or_else(||task.get_document("tmuxContext").ok().and_then(|c|c.get("launchAttemptId")).cloned()).unwrap_or(Bson::Null).into_relaxed_extjson(),
            "tmuxContext":task.get("tmuxContext").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
            "executionObservation":task.get("observedExecutionBeforeStop").cloned().unwrap_or(Bson::Null).into_relaxed_extjson()});
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { return Err("original stop allowance exhausted before process invocation".into()); }
        let mut command = self.process("/bin/bash");
        command.arg("scripts/pause_task.sh").arg(name).arg(if remote {"--remote"} else {"false"})
            .env("QUANTNIGHT_TMUX_BINDING", binding.to_string())
            .env("QUANTNIGHT_STOP_REMAINING_SECONDS", remaining.as_secs_f64().to_string())
            .env("QUANTNIGHT_STOP_DEADLINE_EPOCH", (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).map_err(|_|"invalid caller clock")?.as_secs_f64()
                + remaining.as_secs_f64()).to_string());
        if let Some(mode) = mode { command.arg(mode); }
        let (stdout, stderr) = run_process_observed(command, remaining, false, self.observer.clone()).await?;
        Ok(crate::pause::StopOutput { stdout, stderr })
    }
    async fn verify_stop_owner(&self, root: &Path, task: &Document, allowance: Duration)
        -> Result<Option<crate::pause::StopOutput>, String> {
        let out = self.pause_owner(root, task, Some("--verify-stopped"), allowance.min(Duration::from_secs(12))).await?;
        let name = task.get_str("name").map_err(|_| "missing task name")?;
        let remote = task.get_bool("isRemote").unwrap_or(false);
        if crate::pause::verified_stop_observation(&out.stdout, name, remote)? { Ok(Some(out)) } else { Ok(None) }
    }
    fn process(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(self.binding.root()).env("PYTHONPATH", self.binding.root())
            .env_remove("TMUX").env_remove("TMUX_PANE").env_remove("TMUX_TMPDIR")
            .env_remove("QUANTNIGHT_TMUX_BINDING");
        command
    }
}

fn require_exact_absence(stdout: &str, name: &str, remote: bool) -> Result<(), String> {
    let exact = format!("❌ Session '{name}' 不存在 ({})", if remote { "remote" } else { "local" });
    if stdout.lines().filter(|line| *line == exact).count() != 1
        || stdout.lines().any(|line| line.starts_with("QUANTNIGHT_TMUX_CONTEXT ")
            || line.starts_with("✅ Pane ") || line.starts_with("❌ Pane ")) {
        return Err("execution_context_unverified: original checker did not prove exact session absence".into());
    }
    Ok(())
}

#[async_trait]
impl OperationsRuntime for OriginalOperationsRuntime<'_> {
    async fn assert_consumer_absent(&self, root: &Path, task: &Document, host: Host) -> Result<(), String> {
        self.verify_root(root).await?;
        let name = task.get_str("name").map_err(|_| "missing exact task name")?;
        validate_name(name)?;
        let id = task.get_object_id("_id").map_err(|_| "missing original task ObjectId")?;
        // An initial sync also checks the other host. Its absence check must
        // not reinterpret a local session context as a remote launch identity.
        let binding = if task.get_bool("isRemote").unwrap_or(false) == host.is_remote() {
            json!({"taskId": id.to_hex(), "remote": host.is_remote(),
                "launchAttemptId": task.get("launchAttemptId").cloned().unwrap_or(Bson::Null).into_relaxed_extjson(),
                "tmuxContext": task.get("tmuxContext").cloned().unwrap_or(Bson::Null).into_relaxed_extjson()})
        } else { json!({}) };
        let mut command = self.process("/bin/bash");
        command.arg("scripts/check_tmux_python.sh").arg(name)
            .arg(if host.is_remote() { "true" } else { "false" })
            .arg("--require-absent").arg("--binding").arg(binding.to_string());
        let (out, _) = run_process_observed(command, Duration::from_secs(20), true, self.observer.clone()).await?;
        require_exact_absence(&out, name, host.is_remote())
    }

    async fn verify_stopped(&self, root: &Path, task: &Document, remaining: Duration)
        -> Result<Option<crate::pause::StopOutput>, String> {
        self.verify_stop_owner(root, task, remaining).await
    }
    async fn confirm_stopped_scope(&self,root:&Path,store:&dyn crate::task_store::TaskStore,task:&Document,remaining:Duration)->Result<(),String>{
        let observed=self.execution_owner(root,store,task,task.get_bool("isRemote").unwrap_or(false),None,remaining).await?;
        if observed["state"]!="session_absent"{return Err("original execution still live/unconfirmed after session-only stop proof".into());}
        Ok(())
    }
    async fn archive_research(&self, root: &Path, task: &Document, host: Host,
        evidence_collection: &str) -> Result<Value, String> {
        self.verify_root(root).await?;
        // Only metadata consumed by templates.catalog/archive_statistics, not
        // the entire potentially large event history or simulation payload.
        let mut metadata = Document::new();
        for key in ["_id", "name", "templatefile", "taskType", "auth_profile", "isRemote"] {
            if let Some(value) = task.get(key) { metadata.insert(key, value.clone()); }
        }
        let request = json!({"task": Bson::Document(metadata).into_relaxed_extjson(),
            "remote": host.is_remote(), "evidenceCollection": evidence_collection,
            "configurationBinding": self.binding.request()});
        let mut command = self.process(root.join(".venv/bin/python"));
        command.args(["-B", "-m", "util.dataset_mission", "--native-archive-statistics"])
            .arg(request.to_string());
        let (out, _) = run_process_observed(command, Duration::from_secs(90), false, self.observer.clone()).await?;
        let receipt: Value = serde_json::from_str(&out)
            .map_err(|_| "original research archive returned no valid JSON receipt")?;
        if receipt.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || receipt.get("command").and_then(Value::as_str) != Some("native-archive-statistics")
            || receipt.get("ok").and_then(Value::as_bool) != Some(true)
            || receipt.get("result").is_none() {
            return Err("original research archive did not confirm retained evidence".into());
        }
        Ok(receipt["result"].clone())
    }
}

fn start_task_metadata(task: &Document) -> Value {
    let mut metadata = Document::new();
    for key in ["_id", "name", "taskType", "auth_profile", "templatefile", "status", "isRemote",
        "targetRemote", "runId", "launchAttemptId", "startedAt", "generation", "tmuxContext",
        "launchAttemptAt", "stopConfirmedAt", "stopConfirmedRemote", "stopConfirmedTaskId", "stopConfirmationBasis"] {
        if let Some(value) = task.get(key) { metadata.insert(key, value.clone()); }
    }
    Bson::Document(metadata).into_relaxed_extjson()
}

#[async_trait]
impl crate::start::StartRuntime for OriginalOperationsRuntime<'_> {
    async fn inspect_execution(&self, root: &Path, store: &dyn crate::task_store::TaskStore, task: &Document,
        remote: bool, requested: Option<&Document>, remaining: Duration) -> Result<Option<Value>, String> {
        self.execution_owner(root, store, task, remote, requested, remaining).await.map(Some)
    }
    async fn verify_stopped(&self, root: &Path, task: &Document, remaining: Duration)
        -> Result<Option<crate::pause::StopOutput>, String> { self.verify_stop_owner(root,task,remaining).await }
    async fn stop_owned_residue(&self, root:&Path, task:&Document, remaining:Duration)
        -> Result<crate::pause::StopOutput,String> {
        self.pause_owner(root,task,Some("--stop-owned-residue"),remaining.min(Duration::from_secs(60))).await
    }
    async fn priority_config(&self, root: &Path, task: &Document, selected: Option<Document>) -> Result<Document, String> {
        let selected = selected.map(|value| crate::start::strict_json(&Bson::Document(value))).transpose()?;
        let result = self.owner_result(root, "scripts.priority_task_config", "--native-config-preflight",
            "native-config-preflight", json!({"task":start_task_metadata(task),"selected":selected}),
            Duration::from_secs(30)).await?;
        crate::start::config_document(result)
    }
    async fn validate_super_config(&self, root: &Path, config: &Document) -> Result<(), String> {
        self.owner_result(root, "util.super_capacity", "--native-config-preflight", "native-config-preflight",
            json!({"config":crate::start::strict_json(&Bson::Document(config.clone()))?}),
            Duration::from_secs(30)).await?;
        Ok(())
    }
    async fn bind_regular_source(&self, root: &Path, task: &Document, config: &Document) -> Result<Document, String> {
        let result = self.owner_result(root, "scripts.source_task_registration", "--native-source-preflight",
            "native-source-preflight", json!({"action":"bind-source","task":start_task_metadata(task),
                "config":crate::start::strict_json(&Bson::Document(config.clone()))?}), Duration::from_secs(30)).await?;
        crate::start::config_document(result)
    }
    async fn verify_priority_extension(&self, root: &Path, task: &Document,
        local_count: u64, remote_count: u64) -> Result<Document, String> {
        let result = self.owner_result(root, "scripts.source_task_registration", "--native-source-preflight",
            "native-source-preflight", json!({"action":"verify-extension","task":start_task_metadata(task),
                "localCount":local_count,"remoteCount":remote_count,"configurationBinding":self.binding.request()}),
            Duration::from_secs(60)).await?;
        crate::start::config_document(result)
    }
    async fn continuation(&self, root: &Path, task: &Document, remote: bool) -> Result<Option<Document>, String> {
        let result = self.owner_result(root, "util.mission_recovery", "--native-continuation-preflight",
            "native-continuation-preflight", json!({"task":start_task_metadata(task),"remote":remote,
                "configurationBinding":self.binding.request()}), Duration::from_secs(150)).await?;
        if result.is_null() { return Ok(None); }
        // This owner returns typed BSON audit evidence, NOT business config.
        // Decode EJSON only here; Priority/source config stays literal JSON.
        match Bson::try_from(result).map_err(|e| format!("invalid continuation BSON audit: {e}"))? {
            Bson::Document(audit) => Ok(Some(audit)),
            _ => Err("continuation audit must be a BSON document or null".into()),
        }
    }
    async fn preflight_notification(&self, root: &Path, spec: &Document) -> Result<(), String> {
        self.owner_result(root, ".agents/skills/manage-quantnight-frontend/scripts/watch_quant_task.py", "--native-control",
            "native-mission-watcher", json!({"action":"preflight", "target":crate::start::strict_json(spec.get("target").ok_or("notification target missing")?)?,
                "itemKey":spec.get_str("itemKey").map_err(|_| "notification itemKey missing")?}), Duration::from_secs(20)).await?;
        Ok(())
    }
    async fn assert_consumer_absent(&self, root: &Path, task: &Document, remote: bool) -> Result<(), String> {
        self.verify_root(root).await?;
        let name = task.get_str("name").map_err(|_| "missing task name")?;
        validate_name(name)?;
        // Original start presence check does NOT adopt an older launch context.
        let mut command = self.process("/bin/bash");
        command.arg("scripts/check_tmux_python.sh").arg(name)
            .arg(if remote { "true" } else { "false" }).args(["--require-absent", "--binding", "{}"]);
        let (out, _) = run_process_observed(command, Duration::from_secs(20), true, self.observer.clone()).await?;
        require_exact_absence(&out, name, remote)
    }
    async fn validate_execution_binding(&self, root: &Path, binding: &Document, remote: bool) -> Result<(), String> {
        self.verify_root(root).await?;
        let mut command = self.process(root.join(".venv/bin/python"));
        command.args(["-B", "-m", "scripts.script_config", "--validate-execution-binding"])
            .arg(Bson::Document(binding.clone()).into_relaxed_extjson().to_string())
            .args(["--remote", if remote { "true" } else { "false" }]);
        let (out, _) = run_process_observed(command, Duration::from_secs(30), false, self.observer.clone()).await?;
        let result: Value = serde_json::from_str(&out).map_err(|_| "execution binding owner returned invalid JSON")?;
        if result.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || result.get("command").and_then(Value::as_str) != Some("validate-execution-binding")
            || result.get("ok").and_then(Value::as_bool) != Some(true) || result.get("context").is_none() {
            return Err("original execution binding owner did not confirm supplied identity".into());
        }
        Ok(())
    }
    async fn launch(&self, root: &Path, specification: &crate::start::StartCommand) -> Result<crate::start::LaunchOutput, String> {
        let deadline=tokio::time::Instant::now()+Duration::from_secs(specification.timeout_seconds.min(120));
        self.verify_root(root).await?;
        if specification.timeout_seconds == 0 || specification.timeout_seconds > 120 || specification.argv.first().map(String::as_str) != Some("bash")
            || !matches!(specification.argv.get(1).map(String::as_str), Some("scripts/start_task.sh" | "scripts/start_super_task.sh" | "scripts/start_priority_task.sh")) {
            return Err("start must use one of the original launchers and its 120s budget".into());
        }
        let mut command = self.process("/bin/bash");
        command.args(&specification.argv[1..]).env("QUANTNIGHT_TMUX_BINDING", Bson::Document(specification.binding.clone()).into_relaxed_extjson().to_string());
        let remaining=deadline.saturating_duration_since(tokio::time::Instant::now());
        let (stdout, stderr) = run_process_observed(command, remaining, false, self.observer.clone()).await?;
        // This only extracts the original inspector's structured output; the
        // original execution_binding owner validates it in the coordinator.
        let observed_contexts = stdout.lines().filter_map(|line| line.strip_prefix("QUANTNIGHT_TMUX_CONTEXT "))
            .map(|line| serde_json::from_str::<Value>(line).map_err(|_| "invalid observed context JSON".to_string())
                .and_then(crate::start::config_document)).collect::<Result<Vec<_>,_>>()?;
        Ok(crate::start::LaunchOutput { stdout, stderr, observed_contexts })
    }
    async fn attach_watcher(&self, root: &Path, task: &Document, remote: bool, spec: &Document) -> Result<Document, String> {
        self.attach_watcher_owner(root, task, remote, spec, false).await
    }
}

#[async_trait]
impl crate::inspection::InspectionRuntime for OriginalOperationsRuntime<'_> {
    async fn inspect_execution(&self, root: &Path, store: &dyn crate::task_store::TaskStore, task: &Document, host: Host)
        -> Result<Option<Value>, String> { self.execution_owner(root,store,task,host.is_remote(),None,Duration::from_secs(12)).await.map(Some) }
    async fn canonical_namespace(&self, root: &Path, profile: &str) -> Result<crate::task_store::CanonicalNamespace, String> {
        let result = self.owner_result(root, "backend.mongodb.mongo_config", "--resolve-task-result-namespace",
            "resolve-task-result-namespace", json!({"authProfile":profile,"configurationBinding":self.binding.request()}),
            Duration::from_secs(8)).await?;
        let database = result.get("database").and_then(Value::as_str).ok_or("original result database mapping missing")?;
        let collection = result.get("collection").and_then(Value::as_str).ok_or("original result collection mapping missing")?;
        crate::task_store::CanonicalNamespace::new(database.into(), collection.into())
    }
    async fn priority_snapshot(&self, root: &Path, task: &Document, host: Host, inspect_items: bool) -> Result<Value, String> {
        let mut metadata = start_task_metadata(task);
        // The original snapshot owner selects/validates these saved values.
        // It restores metadata BSON separately from opaque business config.
        for key in ["mission_config", "startConfig", "config"] {
            if let Some(value) = task.get(key) { metadata[key] = value.clone().into_relaxed_extjson(); }
        }
        self.owner_result(root, "scripts.priority_task_config", "--native-status-snapshot", "native-status-snapshot",
            json!({"task":metadata,"remote":host.is_remote(),"inspectItems":inspect_items,
                "configurationBinding":self.binding.request()}), Duration::from_secs(90)).await
    }
    async fn priority_default_config(&self, root: &Path, task: &Document) -> Result<Document, String> {
        let result = self.owner_result(root, "scripts.priority_task_config", "--native-default-config", "native-default-config",
            json!({"task":start_task_metadata(task)}), Duration::from_secs(30)).await?;
        crate::start::config_document(result)
    }
    async fn verify_registration(&self, root: &Path, task: &Document) -> Result<Value, String> {
        self.owner_result(root, "util.dataset_mission", "--native-verify-registration", "native-verify-registration",
            json!({"task":start_task_metadata(task),"configurationBinding":self.binding.request()}),
            Duration::from_secs(90)).await
    }
}

#[async_trait]
impl crate::maintenance_ops::MaintenanceRuntime for OriginalOperationsRuntime<'_> {
    async fn prepare_watch(&self, root: &Path, task: &Document, remote: bool, spec: &Document) -> Result<Document, String> {
        let result = self.owner_result(root, ".agents/skills/manage-quantnight-frontend/scripts/watch_quant_task.py", "--native-control",
            "native-mission-watcher", json!({"action":"preflight", "task":start_task_metadata(task),
                "target":crate::start::strict_json(spec.get("target").ok_or("notification target missing")?)?,
                "itemKey":spec.get_str("itemKey").map_err(|_| "notification itemKey missing")?}), Duration::from_secs(20)).await?;
        let expected = crate::start::config_document(result.get("expectedBinding").cloned().ok_or("watcher preparation lacks independent expected binding")?)?;
        if expected.get_bool("remote").ok() != Some(remote) { return Err("watch preparation host mismatch".into()); }
        Ok(expected)
    }
    async fn attach_watcher(&self, root: &Path, task: &Document, remote: bool, spec: &Document, replace: bool) -> Result<Document, String> {
        self.attach_watcher_owner(root, task, remote, spec, replace).await
    }
    async fn assert_consumer_absent(&self, root: &Path, task: &Document, remote: bool) -> Result<(), String> {
        OperationsRuntime::assert_consumer_absent(self, root, task, Host::from_remote(remote)).await
    }
    async fn observe_accepted(&self, root: &Path, specification: &crate::maintenance_ops::ObservationCommand) -> Result<crate::maintenance_ops::ObservationOutput, String> {
        use crate::config::{run_process_observed_typed, ProcessFailureKind};
        use crate::maintenance_ops::ObservationOutput;
        self.verify_root(root).await?;
        if specification.timeout_seconds != 540
            || specification.argv.first().map(String::as_str) != Some("bash")
            || specification.argv.get(1).map(String::as_str) != Some("scripts/start_super_task.sh")
            || specification.argv.last().map(String::as_str) != Some("--observe-accepted") {
            return Err("observation must use the original finite GET-only launcher and 540s budget".into());
        }
        let mut command = self.process("/bin/bash");
        // process() removes inherited tmux authority. This is NOT a new tmux
        // launch, and no persisted binding/old environment is injected here.
        command.args(&specification.argv[1..]);
        match run_process_observed_typed(command, Duration::from_secs(540), false, self.observer.clone()).await {
            Ok((stdout, stderr)) => Ok(ObservationOutput { returncode:0, stdout, stderr }),
            Err(error) if error.kind == ProcessFailureKind::Exit && error.exit_code.is_some() =>
                Ok(ObservationOutput { returncode:error.exit_code.unwrap(), stdout:error.stdout(), stderr:error.stderr() }),
            Err(error) => Err(error.to_string()),
        }
    }
    async fn priority_prune(&self, root: &Path, task: &Document, specification: &crate::maintenance_ops::PriorityPruneCommand) -> Result<Document, String> {
        self.verify_root(root).await?;
        let relative = std::path::PathBuf::from("artifacts/alpha_optimization/priority_roots")
            .join(&specification.name).join(&specification.profile).join("pool.json");
        if specification.timeout_seconds != 180 || specification.checkpoint_relative != relative
            || task.get_str("name").ok() != Some(specification.name.as_str()) {
            return Err("Priority prune requires original checkpoint scope and 180s budget".into());
        }
        let result = self.owner_result(root, "scripts.priority_cleanup", "--native-prune", "native-prune",
            json!({"task":start_task_metadata(task),"remote":specification.remote,
                "sharpe":specification.sharpe,"fitness":specification.fitness,"dryRun":specification.dry_run,
                "configurationBinding":self.binding.request()}), Duration::from_secs(180)).await?;
        crate::start::config_document(result)
    }
}

#[async_trait]
impl crate::priority_update::PriorityUpdateRuntime for OriginalOperationsRuntime<'_> {
    async fn publish_priority_config(&self, root: &Path, task: &Document, config: &Document, remote: bool) -> Result<(), String> {
        self.verify_root(root).await?;
        let name = task.get_str("name").map_err(|_| "original Priority name missing")?;
        validate_name(name)?;
        let profile = match task.get("auth_profile") { None => "user1", Some(Bson::String(value)) => value,
            _ => return Err("original Priority profile invalid".into()) };
        let config_json = crate::start::strict_json(&Bson::Document(config.clone()))?.to_string();
        let mut command = self.process("/bin/bash");
        command.args(["scripts/update_priority_task.sh", name, &config_json,
            if remote { "--remote" } else { "false" }, profile]);
        // Preserve the original update-script budget. Exit0 proves publication,
        // not consumer liveness, configuration adoption or a new launch.
        run_process_observed(command, Duration::from_secs(300), false, self.observer.clone()).await?;
        Ok(())
    }
}

/// CLI and GUI use the same stop adapter with their original 60s/75s budgets.
pub struct OriginalPauseRuntime<'a> {
    pub common: OriginalOperationsRuntime<'a>,
    pub budget: crate::pause::StopBudget,
}
#[async_trait]
impl crate::pause::PauseRuntime for OriginalPauseRuntime<'_> {
    fn script_budget(&self) -> crate::pause::StopBudget { self.budget }
    async fn stop(&self, root: &Path, task: &Document, never: bool) -> Result<crate::pause::StopOutput, String> {
        self.stop_bounded(root, task, never, self.budget.duration()).await
    }
    async fn stop_bounded(&self, root: &Path, task: &Document, never: bool, remaining: Duration)
        -> Result<crate::pause::StopOutput, String> {
        self.common.pause_owner(root, task, if never {Some("--verify-never-started")}
            else if task.get_document("observedExecutionBeforeStop").ok().and_then(|v|v.get_str("state").ok())==Some("owned_residue"){Some("--stop-owned-residue")} else {None},
            remaining.min(self.budget.duration())).await
    }
    async fn inspect_before_stop(&self,root:&Path,store:&dyn crate::task_store::TaskStore,task:&Document,remaining:Duration)
        ->Result<Option<Value>,String>{
        let observed=self.common.execution_owner(root,store,task,task.get_bool("isRemote").unwrap_or(false),None,remaining).await?;
        if observed["state"]=="session_absent" || observed["explicitPauseVerified"]==true || (observed["identityVerified"]==true
            && matches!(observed["state"].as_str(),Some("live"|"owned_residue"))){Ok(Some(observed))}
        else{Err("original execution identity unconfirmed; refusing a stop signal to a possibly foreign consumer".into())}
    }
    async fn confirm_stopped_scope(&self,root:&Path,store:&dyn crate::task_store::TaskStore,task:&Document,remaining:Duration)->Result<(),String>{
        OperationsRuntime::confirm_stopped_scope(&self.common,root,store,task,remaining).await
    }
    async fn verify_stopped(&self, root: &Path, task: &Document, remaining: Duration)
        -> Result<Option<crate::pause::StopOutput>, String> {
        self.common.verify_stop_owner(root, task, remaining).await
    }
}

fn generation_failure(kind: crate::create_generate::GenerationErrorKind, message: impl ToString)
    -> crate::create_generate::GenerationFailure {
    crate::create_generate::GenerationFailure::new(kind, message.to_string())
}

async fn generation_child(command: Command, budget: Duration, observer: Option<BashScriptObserver>)
    -> Result<crate::create_generate::GeneratedOutput, crate::create_generate::GenerationFailure> {
    use crate::config::{run_process_observed_typed, ProcessFailureKind};
    use crate::create_generate::{GeneratedOutput, GenerationErrorKind as Kind};
    run_process_observed_typed(command, budget, false, observer).await
        .map(|(stdout, _)| GeneratedOutput { stdout })
        .map_err(|error| {
            let kind = match error.kind {
                ProcessFailureKind::Timeout => Kind::Timeout,
                ProcessFailureKind::Exit if error.exit_code.is_none() => Kind::Interrupted,
                ProcessFailureKind::Spawn | ProcessFailureKind::Exit => Kind::Ordinary,
                ProcessFailureKind::Setup | ProcessFailureKind::Wait
                    | ProcessFailureKind::Stdout | ProcessFailureKind::Stderr => Kind::DatabaseUncertain,
            };
            generation_failure(kind, error)
        })
}

#[async_trait]
impl crate::create_generate::GenerationRuntime for OriginalOperationsRuntime<'_> {
    async fn creation_metadata(&self, root: &Path, desired: &Document)
        -> Result<crate::create_generate::CreationMetadata, crate::create_generate::GenerationFailure> {
        use crate::create_generate::{CreationMetadata, GenerationErrorKind as Kind};
        self.verify_root(root).await.map_err(|e| generation_failure(Kind::Ordinary, e))?;
        let kind = desired.get_str("taskType").unwrap_or("regular");
        let name = desired.get_str("templatefile").unwrap_or("");
        // Exact original read_binding routing; no manifest interpretation here.
        if kind == "priority" || name.is_empty() { return Ok(CreationMetadata::default()); }
        if !matches!(kind, "regular" | "super") {
            return Err(generation_failure(Kind::Ordinary, "unsupported task type"));
        }
        let mut command = self.process(root.join(".venv/bin/python"));
        command.args(["-B", "-m", "templates.catalog", "binding", "--root"])
            .arg(root).args(["--kind", kind, "--name", name]);
        let (out, _) = run_process_observed(command, Duration::from_secs(30), false, self.observer.clone())
            .await.map_err(|e| generation_failure(Kind::Ordinary, e))?;
        let receipt: Value = serde_json::from_str(&out)
            .map_err(|_| generation_failure(Kind::Ordinary, "template catalog returned no valid JSON receipt"))?;
        if receipt.get("ok").and_then(Value::as_bool) != Some(true) || receipt.get("binding").is_none() {
            return Err(generation_failure(Kind::Ordinary, "original template catalog binding was not confirmed"));
        }
        let metadata = &receipt["binding"];
        if metadata.is_null() { return Ok(CreationMetadata::default()); }
        let recommended = metadata.get("recommendedMaxConcurrent").and_then(Value::as_i64)
            .ok_or_else(|| generation_failure(Kind::Ordinary, "validated template binding lacks integer recommendedMaxConcurrent"))?;
        Ok(CreationMetadata { recommended_max_concurrent: Some(recommended) })
    }
    async fn generate(&self, root: &Path, specification: &crate::create_generate::GenerationCommand)
        -> Result<crate::create_generate::GeneratedOutput, crate::create_generate::GenerationFailure> {
        use crate::create_generate::GenerationErrorKind as Kind;
        self.verify_root(root).await.map_err(|e| generation_failure(Kind::Ordinary, e))?;
        if specification.argv.iter().take(4).map(String::as_str).collect::<Vec<_>>()
            != ["uv", "run", "-m", "backend.mongodb.mongo_datum"] {
            return Err(generation_failure(Kind::Ordinary, "generation must use the original mongo_datum writer"));
        }
        let mut command = self.process("uv");
        command.args(&specification.argv[1..]);
        generation_child(command, Duration::from_secs(specification.timeout_seconds), self.observer.clone()).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{backend_binding::validate_backend_binding, config::bash_test_support::FakeBash};
    use std::os::unix::fs::PermissionsExt;
    use bson::{doc, oid::ObjectId};

    fn fixture(check: &str) -> FakeBash {
        let fake = FakeBash::new(":");
        let root = Path::new(&fake.config.env.working_dir);
        std::fs::write(root.join("credentials.yaml"), "synthetic fixture\n").unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        let python = root.join(".venv/bin/python");
        std::fs::write(&python, r#"#!/bin/sh
printf '%s\n' "$@" >> python.argv
if [ "$3" = "backend.mongodb.mongo_config" ]; then
  echo '{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}'
elif [ "$3" = "util.dataset_mission" ]; then
  echo '{"schemaVersion":1,"command":"native-archive-statistics","ok":true,"result":{"archivePath":"retained.json","platformFetched":false}}'
elif [ "$3" = "templates.catalog" ]; then
  echo '{"ok":true,"binding":{"recommendedMaxConcurrent":3}}'
else
  exit 29
fi
"#).unwrap();
        std::fs::set_permissions(python, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(root.join("scripts/check_tmux_python.sh"), check).unwrap();
        fake
    }
    fn task() -> Document {
        doc! {"_id": ObjectId::parse_str("0123456789abcdef01234567").unwrap(),
            "name": "fixture", "isRemote": false, "templatefile": "frozen", "taskType": "regular"}
    }
    #[tokio::test]
    async fn exact_absence_uses_original_script_and_nonzero_is_not_absence() {
        for (script, accepted) in [
            ("echo \"❌ Session 'fixture' 不存在 (local)\"", true),
            ("echo \"❌ Session 'other' 不存在 (local)\"", false),
            ("echo \"❌ Session 'fixture' 不存在 (remote)\"", false),
            ("echo \"❌ Session 'fixture' 不存在 (local)\"; exit 23", false),
            ("echo \"❌ Session 'fixture' 不存在 (local)\"; echo 'QUANTNIGHT_TMUX_CONTEXT {}'", false),
            (":", false),
        ] {
            let fake = fixture(script);
            let binding = validate_backend_binding(&fake.config).await.unwrap();
            let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
            assert_eq!(runtime.assert_consumer_absent(binding.root(), &task(), Host::Local).await.is_ok(), accepted);
        }
    }
    #[tokio::test]
    async fn archive_uses_original_research_owner_and_exact_retained_namespace() {
        let fake = fixture(":");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        let retained = "fixture__archive_0123456789abcdef01234567";
        let result = runtime.archive_research(binding.root(), &task(), Host::Local, retained).await.unwrap();
        assert_eq!(result["platformFetched"], false);
        let argv = std::fs::read_to_string(binding.root().join("python.argv")).unwrap();
        assert!(argv.contains("util.dataset_mission") && argv.contains(retained));
        assert!(!argv.contains("taskctl.py") && !argv.contains("mongodb://"));
        let request: Value = serde_json::from_str(argv.lines().last().unwrap()).unwrap();
        assert_eq!(request["task"]["_id"]["$oid"], "0123456789abcdef01234567");
        assert_eq!(request["remote"], false);
    }
    #[tokio::test]
    async fn start_presence_ignores_old_launch_context_and_launcher_only_extracts_original_marker() {
        use crate::start::{StartRuntime, StartCommand};
        let fake = fixture("printf '%s\\n' \"$@\" > check.argv; echo \"❌ Session 'fixture' 不存在 (local)\"");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        let mut original = task();
        original.insert("tmuxContext", doc! {"old": "do-not-adopt"});
        StartRuntime::assert_consumer_absent(&runtime, binding.root(), &original, false).await.unwrap();
        assert!(std::fs::read_to_string(binding.root().join("check.argv")).unwrap().ends_with("--binding\n{}\n"));
        let launcher = binding.root().join("scripts/start_task.sh");
        std::fs::write(&launcher, r#"echo called >> launch.count
printf '%s' "$QUANTNIGHT_TMUX_BINDING" > launch.binding
echo 'QUANTNIGHT_TMUX_CONTEXT {"taskId":"0123456789abcdef01234567","launchAttemptId":"new","remote":false}'
"#).unwrap();
        let command = StartCommand { argv: vec!["bash".into(), "scripts/start_task.sh".into(), "fixture".into(), "{}".into(), "false".into(), "user1".into()],
            timeout_seconds: 120, binding: doc! {"taskId":"0123456789abcdef01234567","launchAttemptId":"new","remote":false} };
        let observed = runtime.launch(binding.root(), &command).await.unwrap();
        assert_eq!(observed.observed_contexts.len(), 1);
        assert_eq!(observed.observed_contexts[0].get_str("launchAttemptId").unwrap(), "new");
        let sent: Value = serde_json::from_str(&std::fs::read_to_string(binding.root().join("launch.binding")).unwrap()).unwrap();
        assert_eq!(sent["launchAttemptId"], "new");
        std::fs::write(launcher, "echo called >> launch.count; echo unconfirmed; exit 23").unwrap();
        assert!(runtime.launch(binding.root(), &command).await.is_err());
        assert_eq!(std::fs::read_to_string(binding.root().join("launch.count")).unwrap(), "called\ncalled\n");
    }
    #[tokio::test]
    async fn only_continuation_decodes_bson_audit_while_config_remains_literal_json() {
        use crate::start::StartRuntime;
        let fake = fixture(":");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let python = binding.root().join(".venv/bin/python");
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        std::fs::write(&python, r#"#!/bin/sh
echo '{"schemaVersion":1,"command":"native-continuation-preflight","ok":true,"result":{"linked":{"$numberInt":"0"},"unresolved":{"$numberLong":"1"},"status":"error","ownReceiptRecovery":true,"errors":[{"itemId":{"$oid":"0123456789abcdef01234567"}}]}}'
"#).unwrap();
        let audit = runtime.continuation(binding.root(), &task(), false).await.unwrap().unwrap();
        assert_eq!(audit.get_i32("linked").unwrap(), 0);
        assert_eq!(audit.get_i64("unresolved").unwrap(), 1);
        assert_eq!(audit.get_str("status").unwrap(), "error");
        assert!(audit.get_bool("ownReceiptRecovery").unwrap());
        assert_eq!(audit.get_array("errors").unwrap()[0].as_document().unwrap().get_object_id("itemId").unwrap().to_hex(), "0123456789abcdef01234567");
        std::fs::write(python, r#"#!/bin/sh
echo '{"schemaVersion":1,"command":"native-config-preflight","ok":true,"result":{"interval":600,"custom":{"$oid":"0123456789abcdef01234567"}}}'
"#).unwrap();
        let config = runtime.priority_config(binding.root(), &task(), None).await.unwrap();
        assert_eq!(config.get_i32("interval").unwrap(), 600);
        assert_eq!(config.get_document("custom").unwrap().get_str("$oid").unwrap(), "0123456789abcdef01234567");
    }
    #[tokio::test]
    async fn source_bridge_sends_counts_not_payload_and_preserves_literal_json_keys() {
        use crate::start::StartRuntime;
        let fake = fixture(":");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        std::fs::write(binding.root().join(".venv/bin/python"), r#"#!/bin/sh
printf '%s\n' "$@" > source.argv
echo '{"schemaVersion":1,"command":"native-source-preflight","ok":true,"result":{"literal":{"$oid":"0123456789abcdef01234567"}}}'
"#).unwrap();
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        let result = runtime.bind_regular_source(binding.root(), &task(), &doc! {"literal":{"$oid":"0123456789abcdef01234567"}}).await.unwrap();
        assert_eq!(result.get_document("literal").unwrap().get_str("$oid").unwrap(), "0123456789abcdef01234567");
        runtime.verify_priority_extension(binding.root(), &task(), 2, 3).await.unwrap();
        let argv = std::fs::read_to_string(binding.root().join("source.argv")).unwrap();
        assert!(argv.contains("scripts.source_task_registration"));
        let request: Value = serde_json::from_str(argv.lines().last().unwrap()).unwrap();
        assert_eq!(request["localCount"], 2); assert_eq!(request["remoteCount"], 3);
        assert!(request.get("rows").is_none());
        assert!(request["task"].get("mission_config").is_none());
    }
    #[tokio::test]
    async fn pause_adapter_passes_original_binding_and_only_explicit_inventory_flag() {
        use crate::pause::{PauseRuntime, StopBudget, verified_stop_proof, StopConfirmationBasis};
        let fake = fixture(":");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let script = binding.root().join("scripts/pause_task.sh");
        std::fs::write(&script, r#"printf '%s\n' "$@" > stop.argv
printf '%s' "$QUANTNIGHT_TMUX_BINDING" > stop.binding
echo 'QUANTNIGHT_PAUSE_CONFIRMED {"schema_version":2,"task_name":"fixture","target_remote":false,"session_absent":true,"runners_exited":null,"confirmation_basis":"session_absent"}'
"#).unwrap();
        for budget in [StopBudget::Cli, StopBudget::Gui] {
            let runtime = OriginalPauseRuntime { common: OriginalOperationsRuntime { binding: &binding, observer: None }, budget };
            assert_eq!(runtime.script_budget(), budget);
            let output = runtime.stop(binding.root(), &task(), false).await.unwrap();
            assert_eq!(verified_stop_proof(&output.stdout, "fixture", false).unwrap(), StopConfirmationBasis::SessionAbsent);
            assert_eq!(std::fs::read_to_string(binding.root().join("stop.argv")).unwrap(), "fixture\nfalse\n");
            let supplied: Value = serde_json::from_str(&std::fs::read_to_string(binding.root().join("stop.binding")).unwrap()).unwrap();
            assert_eq!(supplied["taskId"], "0123456789abcdef01234567");
            runtime.stop(binding.root(), &task(), true).await.unwrap();
            assert!(std::fs::read_to_string(binding.root().join("stop.argv")).unwrap().ends_with("--verify-never-started\n"));
        }
        std::fs::write(script, "echo 'QUANTNIGHT_PAUSE_CONFIRMED {}'; exit 23").unwrap();
        let runtime = OriginalPauseRuntime { common: OriginalOperationsRuntime { binding: &binding, observer: None }, budget: StopBudget::Cli };
        assert!(runtime.stop(binding.root(), &task(), false).await.is_err());
    }
    #[tokio::test]
    async fn creation_metadata_uses_original_catalog_not_taskctl() {
        use crate::create_generate::GenerationRuntime;
        let fake = fixture(":");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        let metadata = runtime.creation_metadata(binding.root(), &task()).await.unwrap();
        assert_eq!(metadata.recommended_max_concurrent, Some(3));
        let path = binding.root().join("python.argv");
        let argv = std::fs::read_to_string(&path).unwrap();
        assert!(argv.contains("templates.catalog\nbinding\n--root\n"));
        assert!(!argv.contains("taskctl.py"));
        let mut priority = task(); priority.insert("taskType", "priority");
        assert!(runtime.creation_metadata(binding.root(), &priority).await.unwrap().recommended_max_concurrent.is_none());
        assert_eq!(std::fs::read_to_string(path).unwrap(), argv);
    }
    #[tokio::test]
    async fn generation_result_uses_exit_and_typed_failure_not_output_claims() {
        use crate::create_generate::GenerationErrorKind as Kind;
        for (script, expected) in [
            ("echo real-output; exit 0", None),
            ("echo 'success timeout complete'; exit 23", Some(Kind::Ordinary)),
            ("exec /bin/sleep 30", Some(Kind::Timeout)),
            ("printf '\\377\\n'", Some(Kind::DatabaseUncertain)),
            ("kill -TERM $$", Some(Kind::Interrupted)),
        ] {
            let fake = FakeBash::new(script);
            let mut command = Command::new("/bin/bash");
            command.arg("fake.sh").current_dir(&fake.config.env.working_dir);
            let result = generation_child(command, Duration::from_millis(100), None).await;
            if let Some(expected) = expected { assert_eq!(result.unwrap_err().kind, expected); }
            else { assert_eq!(result.unwrap().stdout, "real-output"); }
        }
    }
    #[tokio::test]
    async fn changed_configuration_blocks_before_any_script() {
        let fake = fixture("echo should-not-run > escaped");
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let runtime = OriginalOperationsRuntime { binding: &binding, observer: None };
        std::fs::write(binding.root().join("credentials.yaml"), "changed").unwrap();
        std::fs::write(binding.root().join(".venv/bin/python"), "#!/bin/sh\nexit 23\n").unwrap();
        assert!(runtime.assert_consumer_absent(binding.root(), &task(), Host::Local).await.is_err());
        assert!(!binding.root().join("escaped").exists());
    }
}
