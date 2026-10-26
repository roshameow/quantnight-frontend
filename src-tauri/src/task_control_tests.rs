//! Offline GUI protocol regressions execute the registered shared pause owner.
//! Only the lowest TaskStore and stop process IO are synthetic.
use super::*;
use quantnight_task_core::{
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
                Host::Remote => self.execution.as_ref().expect("unexpected remote registry access"),
            };
            let matched = filter.iter().all(|(key, expected)| match expected {
                bson::Bson::Document(op) if op.contains_key("$ne") => task.get(key) != op.get("$ne"),
                bson::Bson::Document(op) if op.contains_key("$exists") => task.contains_key(key) == op.get_bool("$exists").unwrap(),
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
async fn pause_route_with_execution(task: Document, stdout: String, matched: u64, execution: Option<Document>) -> PauseRoute {
    use quantnight_task_core::{
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
    let still_active = format!("QUANTNIGHT_STOP_OBSERVED {}", serde_json::json!({
        "schema_version":1,"task_name":name,"target_remote":remote,"session_absent":false}));
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
        execution: execution.or_else(|| task.get_bool("isRemote").unwrap_or(false).then(|| task.clone())),
        matched,
        writes: Mutex::new(vec![]),
    });
    let service = TaskControlState::default();
    let core = service
        .initialize(async { Ok(TaskControl::with_store(binding, store.clone())) })
        .await
        .unwrap();
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
    assert!(!root.join("VIOLATIONS").exists(), "fixture route violation must not be swallowed");
    let calls = fs::read_to_string(root.join("stop-calls")).unwrap();
    let stop_invocations = calls.lines().filter(|line| *line == "stop").count();
    assert!(stop_invocations <= 1, "read-only recovery must not repeat stop");
    if outcome.exit_code == 0 || stages.lock().unwrap().iter().any(|s| s == "control_write") {
        assert_eq!(stop_invocations, 1, "fixture initially-active stop cannot be replaced with verification success");
    }
    let writes = store.writes.lock().unwrap().clone();
    PauseRoute {
        outcome,
        writes,
        stages: Arc::try_unwrap(stages).unwrap().into_inner().unwrap(),
        stop_invocations,
    }
}
pub(super) fn proof_line(name: &str, remote: bool, basis: pause::StopConfirmationBasis) -> String {
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

#[tokio::test]
async fn real_core_bad_receipts_never_reach_storage_and_cas_failure_never_returns_gui_proof() {
    let id = ObjectId::new();
    let task = doc! {"_id":id,"name":"fixture","status":"running","isRemote":true,"launchAttemptId":"original"};
    for raw in ["", "paused", "QUANTNIGHT_PAUSE_CONFIRMED {bad}"] {
        let route = pause_route(task.clone(), raw.into(), 1).await;
        assert_eq!(route.outcome.exit_code, 1);
        assert!(route.writes.is_empty());
        assert!(route.outcome.confirmed_stop.is_none());
    }
    let route = pause_route(
        task,
        proof_line("fixture", true, pause::StopConfirmationBasis::SessionAbsent),
        0,
    )
    .await;
    assert_eq!(route.writes.len(), 1);
    assert!(route.outcome.value["error"]
        .as_str()
        .unwrap()
        .contains("CAS failed"));
    assert!(route.outcome.confirmed_stop.is_none());
    assert!(!route.stages.iter().any(|s| s == "completed"));
}

struct NoCleanupIo;
impl quantnight_task_core::cleanup_sync::OperationsRuntime for NoCleanupIo {
    fn assert_consumer_absent<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _: &'life1 Path,
        _: &'life2 Document,
        _: Host,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { panic!("running delete must not probe consumer") })
    }
    fn archive_research<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        _: &'life1 Path,
        _: &'life2 Document,
        _: Host,
        _: &'life3 str,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, String>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { panic!("running delete must not archive") })
    }
}
pub(super) async fn assert_running_delete_refused() {
    let id = ObjectId::new();
    let root = std::env::temp_dir().join(format!("gui-delete-{}", id));
    fs::create_dir_all(&root).unwrap();
    let store = Store {
        task: doc! {"_id":id,"name":"dataset","status":"running","isRemote":false},
        execution: None,
        matched: 1,
        writes: Mutex::new(vec![]),
    };
    let result = quantnight_task_core::cleanup_sync::delete(
        &root,
        &store,
        &NoCleanupIo,
        "dataset",
        Some(&id.to_hex()),
    )
    .await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.value["error"]
            .as_str()
            .unwrap()
            .contains("current valid stop confirmation unavailable"),
        "{}",
        result.value
    );
    assert!(store.writes.lock().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn lazy_service_bootstrap_runs_once_even_when_failed_and_concurrent_buttons_arrive() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let service = TaskControlState::default();
    let calls = AtomicUsize::new(0);
    let init = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        Err("synthetic binding mismatch; no real configuration read".to_string())
    };
    let (first, second) = tokio::join!(service.initialize(init()), service.initialize(init()));
    assert_eq!(first.err(), second.err());
    assert!(service.initialize(init()).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn gui_create_request_retains_config_and_remote_intent_without_authorizing_running_or_force() {
    let value = serde_json::json!({"name":"fixture", "template":"display", "templatefile":"frozen",
        "task_type":"super", "auth_profile":"profile", "is_remote":true, "status":"running",
        "config":{"nested":{"literal":{"$oid":"not-a-bson-id"}},"steps":[]}});
    let request = create_request(serde_json::from_value(value.clone()).unwrap()).unwrap();
    assert_eq!(request.name, "fixture");
    assert_eq!(request.template, "display");
    assert_eq!(request.templatefile, "frozen");
    assert_eq!(request.auth_profile, "profile");
    assert_eq!(request.task_type, "super");
    assert!(request.target_remote);
    assert!(!request.force);
    assert!(request.expected_task_id.is_none());
    assert_eq!(
        serde_json::to_value(request.mission_config.unwrap()).unwrap(),
        value["config"]
    );
}

#[test]
fn void_projection_requires_version_command_name_boolean_success_and_zero_exit() {
    let valid = serde_json::json!({"schemaVersion":1,"command":"sync","task":"fixture","ok":true});
    core_receipt(
        &TaskOutcome {
            value: valid.clone(),
            exit_code: 0,
        },
        "sync",
        "fixture",
    )
    .unwrap();
    for patch in [
        serde_json::json!({"schemaVersion":2}),
        serde_json::json!({"command":"delete"}),
        serde_json::json!({"task":"replacement"}),
        serde_json::json!({"ok":"true"}),
        serde_json::json!({"ok":false}),
    ] {
        let mut invalid = valid.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(core_receipt(
            &TaskOutcome {
                value: invalid,
                exit_code: 0
            },
            "sync",
            "fixture"
        )
        .is_err());
    }
    assert!(core_receipt(
        &TaskOutcome {
            value: valid,
            exit_code: 1
        },
        "sync",
        "fixture"
    )
    .is_err());
}

#[test]
fn creation_destination_and_captured_actual_host_are_distinct_shared_request_fields() {
    let id = "0123456789abcdef01234567";
    let request = start_request("fixture", Some("{}"), true, id, Some(false)).unwrap();
    assert_eq!(request.remote, Some(true));
    assert_eq!(request.expected_remote, Some(false));
    let receipt = |remote| TaskOutcome {
        value: serde_json::json!({"schemaVersion":1,"command":"start",
        "task":"fixture","taskId":id,"ok":true,"remote":remote}),
        exit_code: 0,
    };
    assert!(start_receipt(&receipt(false), &request).is_err());
    start_receipt(&receipt(true), &request).unwrap();
    // This adapter grants no pristine/history eligibility: core.start owns it.
}

#[tokio::test]
async fn startup_failure_is_cached_for_concurrent_buttons_without_factory_or_replay() {
    use quantnight_task_core::{config::bash_test_support::FakeBash,
        mongo_manager::local_config_test_support::without_client_construction};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fake = FakeBash::new(":"); // no credentials: real startup must refuse
    let service = TaskControlState::default();
    let calls = AtomicUsize::new(0);
    let init = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        TaskControl::bootstrap_gui(fake.config.clone()).await?
            .into_parts().map(|(_, _, control)| control)
    };
    without_client_construction(async {
        let (a, b) = tokio::join!(service.initialize(init()), service.initialize(init()));
        let error = a.err().unwrap();
        assert_eq!(Some(error.clone()), b.err());
        assert_eq!(service.initialize(init()).await.err(), Some(error));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // No retry of the original operation, even if a new context is later
        // explicitly constructed. Nothing in these failed futures reaches I/O.
        assert!(!Path::new(&fake.config.env.working_dir).join(".quantnight").exists());
    }).await;
}

#[tokio::test]
async fn original_pause_registry_preserves_legacy_null_local_and_rejects_remote_conflicts() {
    let id = ObjectId::new();
    for host in [bson::Bson::Null, bson::Bson::Boolean(false)] {
        let task = doc! {"_id":id,"name":"legacy","status":"running","isRemote":host};
        let route = pause_route(task, proof_line("legacy",false,pause::StopConfirmationBasis::RunnersExited),1).await;
        assert_eq!(route.outcome.exit_code,0,"{}",route.outcome.value);
        assert_eq!(route.stop_invocations,1);
        assert_eq!(route.writes.len(),1);
    }
    for host in [bson::Bson::Null,bson::Bson::Boolean(false),bson::Bson::String("true".into())] {
        let task=doc! {"_id":id,"name":"legacy","status":"running","isRemote":true};
        let mut execution=task.clone();execution.insert("isRemote",host);
        let route=pause_route_with_execution(task,proof_line("legacy",true,pause::StopConfirmationBasis::RunnersExited),1,Some(execution)).await;
        assert_eq!(route.outcome.exit_code,1);
        assert!(route.outcome.value["error"].as_str().unwrap().contains("execution registry belongs"));
        assert_eq!(route.stop_invocations,0);
        assert!(route.writes.is_empty());
    }
}
