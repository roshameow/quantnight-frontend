//! Actual registered Rust coordinators over a lowest-layer in-memory TaskStore.
//! Runtime fakes are explicit external-owner seams, not live process/DB adapters.
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{
    maintenance_ops::{
        self as core, MaintenanceRuntime, ObservationCommand, ObservationOutput,
        ObserveAcceptedRequest, PriorityPruneCommand, PriorityPruneRequest, WatchRequest,
    },
    mission_lock::MissionLocks,
    start::{NotificationOptions, NotificationTarget},
    task_store::{CollectionSummary, Host, TaskStore},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(k, v)| {
        if let Bson::Document(d) = v {
            if let Ok(exists) = d.get_bool("$exists") {
                return row.contains_key(k) == exists;
            }
            if let Some(ne) = d.get("$ne") {
                return row.get(k) != Some(ne);
            }
        }
        row.get(k) == Some(v) || (v == &Bson::Null && !row.contains_key(k))
    })
}
struct State {
    task: Document,
    writes: Vec<Document>,
    mutate_before: BTreeMap<String, Document>,
    mutate_after: BTreeMap<String, Document>,
    lost_ack: Option<String>,
}
struct Store(Mutex<State>);
impl Store {
    fn task(&self) -> Document {
        self.0.lock().unwrap().task.clone()
    }
    fn mutate(&self, fields: Document) {
        self.0.lock().unwrap().task.extend(fields);
    }
    fn race(&self, command: &str, fields: Document) {
        self.0
            .lock()
            .unwrap()
            .mutate_before
            .insert(command.into(), fields);
    }
}
#[async_trait]
impl TaskStore for Store {
    async fn find_task(
        &self,
        host: Host,
        filter: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        assert_eq!(host, Host::Local);
        assert!(newest);
        let row = self.task();
        Ok(matches(&row, &filter).then_some(row))
    }
    async fn update_task(
        &self,
        host: Host,
        filter: Document,
        update: Document,
    ) -> Result<u64, String> {
        assert_eq!(host, Host::Local);
        let command = update
            .get_document("$push")
            .unwrap()
            .get_document("events")
            .unwrap()
            .get_str("command")
            .unwrap();
        let mut s = self.0.lock().unwrap();
        s.writes.push(filter.clone());
        if let Some(fields) = s.mutate_before.remove(command) {
            s.task.extend(fields);
        }
        if !matches(&s.task, &filter) {
            return Ok(0);
        }
        s.task.extend(update.get_document("$set").unwrap().clone());
        let event = update
            .get_document("$push")
            .unwrap()
            .get("events")
            .unwrap()
            .clone();
        if !s.task.contains_key("events") {
            s.task.insert("events", Vec::<Bson>::new());
        }
        s.task.get_array_mut("events").unwrap().push(event);
        if let Some(fields) = s.mutate_after.remove(command) {
            s.task.extend(fields);
        }
        if s.lost_ack.as_deref() == Some(command) {
            return Err("fake write acknowledgement lost".into());
        }
        Ok(1)
    }
    async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
        panic!("no item inventory")
    }
    async fn summary(&self, _: Host, _: &str) -> Result<CollectionSummary, String> {
        panic!("no item summary")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("no item probe")
    }
    async fn rows(&self, _: Host, _: &str, _: Option<Document>) -> Result<Vec<Document>, String> {
        panic!("no item read")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("no item insertion")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("no archive")
    }
    async fn ping(&self, _: Host) -> Result<(), String> {
        panic!("no connection")
    }
}
#[derive(Default)]
struct Behavior {
    calls: Vec<String>,
    observer: String,
    watch_patch: Document,
    preflight_error: bool,
    absent_error: bool,
    attach_error: bool,
    after_observe: Document,
    after_attach: Document,
    after_prune: Document,
    prune_error: bool,
    prune_patch: Document,
}
struct Runtime {
    store: Arc<Store>,
    behavior: Mutex<Behavior>,
}
impl Runtime {
    fn calls(&self) -> Vec<String> {
        self.behavior.lock().unwrap().calls.clone()
    }
}
#[async_trait]
impl MaintenanceRuntime for Runtime {
    async fn prepare_watch(
        &self,
        _: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
    ) -> Result<Document, String> {
        let mut b = self.behavior.lock().unwrap();
        b.calls.push("preflight".into());
        assert!(spec.contains_key("target"));
        if b.preflight_error {
            return Err("fake exact target unavailable".into());
        }
        // Synthetic original-owner output, separate from receipt tampering.
        // Production must invoke the original Python owner, not this fake.
        let launch = task
            .get_str("launchAttemptId")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| task.get_str("startedAt").ok().filter(|s| !s.is_empty()))
            .unwrap_or("legacy");
        let mut expected = doc! {"launchId":launch,"missionId":task.get_object_id("_id").unwrap().to_hex(),"remote":remote};
        if matches!(
            task.get_str("status").ok(),
            Some("generating" | "generation_unknown" | "generation_failed")
        ) || (task.get_str("status") == Ok("pending")
            && task
                .get_document("generation")
                .ok()
                .and_then(|g| g.get_str("phase").ok())
                == Some("prepared"))
        {
            let g = task.get_document("generation").unwrap();
            expected.insert("generationId", g.get("id").unwrap().clone());
            expected.insert("generationPhase", g.get("phase").unwrap().clone());
        }
        Ok(expected)
    }
    async fn attach_watcher(
        &self,
        root: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
        replace: bool,
    ) -> Result<Document, String> {
        let mut b = self.behavior.lock().unwrap();
        b.calls.push(format!("attach:{replace}"));
        if b.attach_error {
            return Err("original owner rejected foreign PID/birth/launch/generation".into());
        }
        assert!(MissionLocks::acquire(root, &["fixture"], false).is_err());
        let launch = task
            .get_str("launchAttemptId")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| task.get_str("startedAt").ok().filter(|s| !s.is_empty()))
            .unwrap_or("legacy");
        let mut receipt = spec.clone();
        receipt.extend(doc!{"missionId":task.get_object_id("_id").unwrap().to_hex(),"launchId":launch,
            "remote":remote,"producerPid":42,"producerMarkerPath":"fixture.lock","checkpointPath":"fixture.json",
            "requiresLease":spec.get_document("target").unwrap().contains_key("taskId"),"reused":!replace});
        if matches!(
            task.get_str("status").ok(),
            Some("generating" | "generation_unknown" | "generation_failed")
        ) || (task.get_str("status") == Ok("pending")
            && task
                .get_document("generation")
                .ok()
                .and_then(|g| g.get_str("phase").ok())
                == Some("prepared"))
        {
            let g = task.get_document("generation").unwrap();
            receipt.insert("generationId", g.get("id").unwrap().clone());
            receipt.insert("generationPhase", g.get("phase").unwrap().clone());
        }
        if replace {
            receipt.insert("replacedProducer",doc!{"pid":41,"processCreatedAt":123.5,"target":{"taskId":"old"},"itemKey":"worker:old"});
        }
        receipt.extend(b.watch_patch.clone());
        self.store.mutate(b.after_attach.clone());
        Ok(receipt)
    }
    async fn assert_consumer_absent(
        &self,
        _: &Path,
        _: &Document,
        remote: bool,
    ) -> Result<(), String> {
        let mut b = self.behavior.lock().unwrap();
        b.calls.push(format!("absent:{remote}"));
        if b.absent_error {
            Err("fake absence unconfirmed".into())
        } else {
            Ok(())
        }
    }
    async fn observe_accepted(
        &self,
        root: &Path,
        command: &ObservationCommand,
    ) -> Result<ObservationOutput, String> {
        let mut b = self.behavior.lock().unwrap();
        b.calls.push("observe".into());
        assert_eq!(command.timeout_seconds, 540);
        assert_eq!(
            &command.argv[..3],
            &["bash", "scripts/start_super_task.sh", "fixture"]
        );
        assert_eq!(command.argv[6], "--observe-accepted");
        assert_eq!(command.argv.len(), 7);
        assert!(MissionLocks::acquire(root, &["fixture"], false).is_err());
        let config: Value = serde_json::from_str(&command.argv[3]).unwrap();
        assert_eq!(config["tag"], json!({}));
        assert_eq!(config["max_concurrent"], 1);
        assert_eq!(config["max_multi_simulation_children"], 1);
        assert_eq!(config["breakpoint"], true);
        assert_eq!(config["mission_tag"], "fixture");
        assert_eq!(config["auth_profile"], "user1");
        let task = self.store.task();
        let claim = task.get_document("acceptedObservation").unwrap();
        assert_eq!(claim.get("finishedAt"), Some(&Bson::Null));
        assert_eq!(task.get_str("status"), Ok("paused"));
        let remote = task.get_bool("isRemote").unwrap();
        assert_eq!(command.argv[4], if remote { "--remote" } else { "false" });
        assert_eq!(config["dist"], if remote { "remote" } else { "local" });
        assert_eq!(command.argv[5], "user1");
        assert_eq!(
            config["accepted_observation"]["id"],
            claim.get_str("id").unwrap()
        );
        assert_eq!(
            config["accepted_observation"]["runIds"],
            json!(["run-original", "tag-run", "fixture"])
        );
        self.store.mutate(b.after_observe.clone());
        if b.observer == "lost" {
            return Err("timeout after 540 seconds".into());
        }
        let mut receipt = json!({"binding":config["accepted_observation"],"closed":true,
            "recovery":{"observed":2,"released":0,"unknown":0}});
        match b.observer.as_str() {
            "unknown" => receipt["recovery"]["unknown"] = json!(1),
            "released" => receipt["recovery"]["released"] = json!(1),
            "bool" => receipt["recovery"]["observed"] = json!(true),
            "float" => receipt["recovery"]["observed"] = json!(1.0),
            "negative" => receipt["recovery"]["unknown"] = json!(-1),
            "missing" => {
                receipt["recovery"]
                    .as_object_mut()
                    .unwrap()
                    .remove("observed");
            }
            "foreign" => receipt["binding"]["id"] = json!("other"),
            "open" => receipt["closed"] = json!(false),
            _ => {}
        }
        let line = format!("SUPER_ACCEPTED_OBSERVATION {receipt}\n");
        let stdout = match b.observer.as_str() {
            "empty" => String::new(),
            "duplicate" => line.repeat(2),
            "malformed" => "SUPER_ACCEPTED_OBSERVATION {invalid}\n".into(),
            _ => format!("original runner diagnostic\n{line}"),
        };
        Ok(ObservationOutput {
            returncode: if b.observer == "nonzero" { 1 } else { 0 },
            stdout,
            stderr: String::new(),
        })
    }
    async fn priority_prune(
        &self,
        root: &Path,
        task: &Document,
        command: &PriorityPruneCommand,
    ) -> Result<Document, String> {
        let mut b = self.behavior.lock().unwrap();
        b.calls
            .push(format!("prune:{}:{}", command.remote, command.dry_run));
        assert!(MissionLocks::acquire(root, &["fixture"], false).is_err());
        assert_eq!(command.timeout_seconds, 180);
        assert_eq!(command.name, "fixture");
        assert_eq!(command.profile, "user1");
        assert_eq!(
            command.checkpoint_relative,
            PathBuf::from("artifacts/alpha_optimization/priority_roots/fixture/user1/pool.json")
        );
        assert_eq!(
            command.binding.get_str("taskId").unwrap(),
            task.get_object_id("_id").unwrap().to_hex()
        );
        assert_eq!(command.binding.get_bool("remote"), Ok(command.remote));
        assert_eq!(
            command.binding.get("launchAttemptId"),
            task.get("launchAttemptId")
        );
        assert_eq!(command.binding.get("tmuxContext"), task.get("tmuxContext"));
        assert_eq!(command.sharpe, 0.5);
        assert_eq!(command.fitness, 0.8);
        self.store.mutate(b.after_prune.clone());
        if b.prune_error {
            return Err("original cleanup pool lock/checkpoint/claimed reservation guard".into());
        }
        let mut report = doc! {"dryRun":command.dry_run,"checkpointPath":"original-host/pool.json",
        "neverClaimedDeleted":if command.dry_run {0}else{2},"wouldDelete":if command.dry_run {2}else{0},
        "selectedRoots":["original-root"],"keptStates":{"processing":1,"UNKNOWN":1}};
        report.extend(b.prune_patch.clone());
        Ok(report)
    }
}
struct Fixture {
    root: PathBuf,
    store: Arc<Store>,
    runtime: Runtime,
}
impl Fixture {
    fn new(kind: &str, remote: bool) -> Self {
        let root = std::env::temp_dir().join(format!("maintenance-core-{}", ObjectId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let id = ObjectId::new();
        let task = doc! {"_id":id,"name":"fixture","status":"paused","taskType":kind,"isRemote":remote,
        "targetRemote":remote,"auth_profile":"user1","runId":"run-original","updatedAt":"before",
        "launchAttemptId":"launch-original","launchAttemptAt":bson::DateTime::from_millis(1000),
        "stopConfirmedAt":bson::DateTime::from_millis(2000),"stopConfirmedTaskId":id.to_hex(),
        "stopConfirmedRemote":remote,"tmuxContext":{"original":"binding"},
        "startConfig":{"max_concurrent":8,"tag":{"runId":"tag-run"}},"events":[]};
        let store = Arc::new(Store(Mutex::new(State {
            task,
            writes: vec![],
            mutate_before: BTreeMap::new(),
            mutate_after: BTreeMap::new(),
            lost_ack: None,
        })));
        let runtime = Runtime {
            store: store.clone(),
            behavior: Mutex::new(Behavior::default()),
        };
        Self {
            root,
            store,
            runtime,
        }
    }
    fn observer(&self, mode: &str) {
        self.runtime.behavior.lock().unwrap().observer = mode.into();
    }
    async fn observe(&self) -> quantnight_task_core::cleanup_sync::TaskOutcome {
        core::observe_accepted(
            &self.root,
            self.store.as_ref(),
            &self.runtime,
            &ObserveAcceptedRequest {
                name: "fixture".into(),
                expected_task_id: Some(self.store.task().get_object_id("_id").unwrap().to_hex()),
            },
        )
        .await
    }
    fn watch_request(&self) -> WatchRequest {
        WatchRequest {
            name: "fixture".into(),
            notification: NotificationOptions {
                target: Some(NotificationTarget::Worker("worker-exact".into())),
                ..Default::default()
            },
            replace_watcher: false,
        }
    }
    async fn watch(&self) -> quantnight_task_core::cleanup_sync::TaskOutcome {
        core::watch(
            &self.root,
            self.store.as_ref(),
            &self.runtime,
            &self.watch_request(),
        )
        .await
    }
    fn prune_request(&self, dry: bool) -> PriorityPruneRequest {
        PriorityPruneRequest {
            name: "fixture".into(),
            confirm: "fixture".into(),
            sharpe: 0.5,
            fitness: 0.8,
            dry_run: dry,
        }
    }
    async fn prune(&self, dry: bool) -> quantnight_task_core::cleanup_sync::TaskOutcome {
        core::priority_prune(
            &self.root,
            self.store.as_ref(),
            &self.runtime,
            &self.prune_request(dry),
        )
        .await
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn observe_keeps_pause_exact_original_binding_local_and_remote() {
    for remote in [false, true] {
        let f = Fixture::new("super", remote);
        let before = f.store.task();
        let out = f.observe().await;
        assert_eq!(out.exit_code, 0, "{}", out.value);
        assert_eq!(out.value["simulationStarted"], false);
        let after = f.store.task();
        for key in [
            "status",
            "stopConfirmedAt",
            "stopConfirmedRemote",
            "launchAttemptId",
            "startConfig",
        ] {
            assert_eq!(before.get(key), after.get(key));
        }
        let claim = after.get_document("acceptedObservation").unwrap();
        assert!(claim.get_str("finishedAt").is_ok());
        assert_eq!(after.get_array("events").unwrap().len(), 2);
        assert_eq!(
            f.runtime.calls(),
            [format!("absent:{remote}"), "observe".into()]
        );
        let writes = &f.store.0.lock().unwrap().writes;
        for filter in writes {
            for key in [
                "_id",
                "name",
                "acceptedObservation",
                "stopConfirmedAt",
                "launchAttemptAt",
                "startConfig",
                "generation",
            ] {
                assert!(filter.contains_key(key));
            }
        }
    }
}
#[tokio::test]
async fn unknown_closes_only_observer_not_business_success() {
    let f = Fixture::new("super", false);
    f.observer("unknown");
    let out = f.observe().await;
    assert_eq!(out.exit_code, 1);
    assert_eq!(out.value["ok"], false);
    assert_eq!(out.value["observation"]["recovery"]["unknown"], 1);
    assert!(f
        .store
        .task()
        .get_document("acceptedObservation")
        .unwrap()
        .get_str("finishedAt")
        .is_ok());
}
#[tokio::test]
async fn unconfirmed_observer_keeps_claim_and_blocks_all_three_mutators() {
    for mode in [
        "lost",
        "empty",
        "duplicate",
        "malformed",
        "nonzero",
        "released",
        "bool",
        "float",
        "negative",
        "missing",
        "foreign",
        "open",
    ] {
        let f = Fixture::new("super", false);
        f.observer(mode);
        let out = f.observe().await;
        assert_eq!(out.exit_code, 1, "{mode}: {}", out.value);
        let task = f.store.task();
        assert_eq!(
            task.get_document("acceptedObservation")
                .unwrap()
                .get("finishedAt"),
            Some(&Bson::Null)
        );
        let before = f.runtime.calls();
        assert_eq!(f.observe().await.exit_code, 1);
        assert_eq!(f.watch().await.exit_code, 1);
        assert_eq!(f.prune(false).await.exit_code, 1);
        assert_eq!(f.runtime.calls(), before);
        assert_eq!(f.store.task(), task);
    }
}
#[tokio::test]
async fn observer_identity_stop_and_profile_guards_precede_execution() {
    for patch in [
        doc! {"status":"running"},
        doc! {"taskType":"regular"},
        doc! {"isRemote":"false"},
        doc! {"stopConfirmedRemote":"false"},
        doc! {"stopConfirmedTaskId":"foreign"},
        doc! {"stopConfirmedAt":bson::DateTime::from_millis(500)},
        doc! {"auth_profile":"../other"},
        doc! {"auth_profile":""},
        doc! {"acceptedObservation":{"id":"active"}},
    ] {
        let f = Fixture::new("super", false);
        f.store.mutate(patch);
        assert_eq!(f.observe().await.exit_code, 1);
        assert!(!f.runtime.calls().contains(&"observe".into()));
        assert!(f.store.0.lock().unwrap().writes.is_empty());
    }
    let f = Fixture::new("super", false);
    let out = core::observe_accepted(
        &f.root,
        f.store.as_ref(),
        &f.runtime,
        &ObserveAcceptedRequest {
            name: "fixture".into(),
            expected_task_id: Some(ObjectId::new().to_hex()),
        },
    )
    .await;
    assert_eq!(out.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
}
#[tokio::test]
async fn observer_begin_cas_and_lost_ack_never_dispatch() {
    let f = Fixture::new("super", false);
    f.store
        .race("observe-accepted-begin", doc! {"auth_profile":"successor"});
    assert_eq!(f.observe().await.exit_code, 1);
    assert!(!f.store.task().contains_key("acceptedObservation"));
    assert!(!f.runtime.calls().contains(&"observe".into()));
    let f = Fixture::new("super", false);
    f.store.0.lock().unwrap().lost_ack = Some("observe-accepted-begin".into());
    assert_eq!(f.observe().await.exit_code, 1);
    assert!(f.store.task().contains_key("acceptedObservation"));
    assert!(!f.runtime.calls().contains(&"observe".into()));
}
#[tokio::test]
async fn observer_finish_cas_cannot_overwrite_changed_stop_or_owner() {
    for patch in [
        doc! {"stopConfirmedTaskId":"successor"},
        doc! {"launchAttemptId":"next"},
        doc! {"generation":{"id":"next"}},
        doc! {"acceptedObservation":{"id":"next","finishedAt":Bson::Null}},
    ] {
        let f = Fixture::new("super", false);
        f.runtime.behavior.lock().unwrap().after_observe = patch;
        assert_eq!(f.observe().await.exit_code, 1);
        assert_eq!(
            f.store
                .task()
                .get_document("acceptedObservation")
                .unwrap()
                .get("finishedAt"),
            Some(&Bson::Null)
        );
    }
}
#[tokio::test]
async fn watch_only_attaches_exact_worker_or_main_and_can_explicitly_replace() {
    for remote in [false, true] {
        for replace in [false, true] {
            for worker in [false, true] {
                let f = Fixture::new("regular", remote);
                let mut req = f.watch_request();
                req.replace_watcher = replace;
                if !worker {
                    req.notification.target =
                        Some(NotificationTarget::Session("main-exact".into()));
                }
                let out = core::watch(&f.root, f.store.as_ref(), &f.runtime, &req).await;
                assert_eq!(out.exit_code, 0, "{}", out.value);
                assert_eq!(out.value["simulationStarted"], false);
                assert_eq!(out.value["notification"]["requiresLease"], worker);
                assert_eq!(
                    f.runtime.calls(),
                    ["preflight", &format!("attach:{replace}")]
                );
                assert_eq!(f.store.task().get_str("status"), Ok("paused"));
                if replace {
                    assert_eq!(out.value["notification"]["replacedProducer"]["pid"], 41);
                }
            }
        }
    }
}
#[tokio::test]
async fn watch_legacy_started_at_and_generation_do_not_require_new_launch() {
    for legacy in [true, false] {
        for generation in [true, false] {
            let f = Fixture::new("regular", false);
            {
                let mut s = f.store.0.lock().unwrap();
                s.task.remove("launchAttemptId");
                if !legacy {
                    s.task.insert("startedAt", "2026-10-20T01:02:03+00:00");
                }
            }
            if generation {
                f.store.mutate(
                    doc! {"status":"generating","generation":{"id":"gen-exact","phase":"writing"}},
                );
            }
            let out = f.watch().await;
            assert_eq!(out.exit_code, 0, "{}", out.value);
            assert_eq!(
                out.value["notification"]["launchId"],
                if legacy {
                    "legacy"
                } else {
                    "2026-10-20T01:02:03+00:00"
                }
            );
            if generation {
                assert_eq!(out.value["notification"]["generationId"], "gen-exact");
            }
            assert!(!f.store.task().contains_key("launchAttemptId"));
        }
    }
}
#[tokio::test]
async fn watch_rejects_receipt_identity_generation_or_route_tampering() {
    for patch in [
        doc! {"missionId":"other"},
        doc! {"launchId":"other"},
        doc! {"remote":true},
        doc! {"target":{"taskId":"other"}},
        doc! {"itemKey":"worker:other"},
        doc! {"requiresLease":false},
        doc! {"producerPid":1},
        doc! {"producerMarkerPath":""},
        doc! {"generationId":"foreign"},
        doc! {"generationPhase":"writing"},
    ] {
        let f = Fixture::new("regular", false);
        f.runtime.behavior.lock().unwrap().watch_patch = patch;
        let out = f.watch().await;
        assert_eq!(out.exit_code, 1, "{}", out.value);
        assert!(!f.store.task().contains_key("agentNotification"));
        assert!(out.value["notification"].is_object());
    }
}
#[tokio::test]
async fn watch_metadata_cas_failure_preserves_original_producer_evidence() {
    for patch in [
        doc! {"generation":{"id":"next"}},
        doc! {"startedAt":"next"},
        doc! {"agentNotification":{"target":{"taskId":"other"}}},
        doc! {"acceptedObservation":{"id":"new"}},
    ] {
        let f = Fixture::new("regular", false);
        f.runtime.behavior.lock().unwrap().after_attach = patch;
        let out = f.watch().await;
        assert_eq!(out.exit_code, 1);
        assert_eq!(out.value["notification"]["producerPid"], 42);
        assert_eq!(out.value["simulationStarted"], false);
        assert_eq!(f.runtime.calls(), ["preflight", "attach:false"]);
    }
}
#[tokio::test]
async fn watch_missing_target_bad_interval_priority_and_original_owner_failure() {
    let f = Fixture::new("regular", false);
    let mut req = f.watch_request();
    req.notification = NotificationOptions::default();
    assert_eq!(
        core::watch(&f.root, f.store.as_ref(), &f.runtime, &req)
            .await
            .exit_code,
        1
    );
    assert!(f.runtime.calls().is_empty());
    req = f.watch_request();
    req.notification.no_notify = true;
    assert_eq!(
        core::watch(&f.root, f.store.as_ref(), &f.runtime, &req)
            .await
            .exit_code,
        1
    );
    for n in [0, 59, 86401] {
        req = f.watch_request();
        req.notification.review_seconds = Some(n);
        assert_eq!(
            core::watch(&f.root, f.store.as_ref(), &f.runtime, &req)
                .await
                .exit_code,
            1
        );
    }
    let f = Fixture::new("priority", false);
    assert_eq!(f.watch().await.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
    let f = Fixture::new("regular", false);
    f.runtime.behavior.lock().unwrap().attach_error = true;
    assert_eq!(f.watch().await.exit_code, 1);
    assert!(!f.store.task().contains_key("agentNotification"));
    let f = Fixture::new("regular", false);
    f.runtime.behavior.lock().unwrap().preflight_error = true;
    assert_eq!(f.watch().await.exit_code, 1);
    assert_eq!(f.runtime.calls(), ["preflight"]);
}
#[tokio::test]
async fn prune_uses_original_host_checkpoint_and_only_real_run_appends_audit() {
    for remote in [false, true] {
        for dry in [false, true] {
            let f = Fixture::new("priority", remote);
            let before = f.store.task();
            let out = f.prune(dry).await;
            assert_eq!(out.exit_code, 0, "{}", out.value);
            assert_eq!(out.value["remote"], remote);
            assert_eq!(out.value["dryRun"], dry);
            assert_eq!(out.value["keptStates"]["UNKNOWN"], 1);
            assert_eq!(
                f.runtime.calls(),
                [format!("absent:{remote}"), format!("prune:{remote}:{dry}")]
            );
            if dry {
                assert_eq!(before, f.store.task());
                assert!(f.store.0.lock().unwrap().writes.is_empty());
            } else {
                assert_eq!(f.store.task().get_array("events").unwrap().len(), 1);
                assert_eq!(f.store.task().get_str("status"), Ok("paused"));
            }
        }
    }
}
#[tokio::test]
async fn prune_confirmation_nonfinite_host_profile_and_absence_fail_before_cleanup() {
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
        let f = Fixture::new("priority", false);
        let mut req = f.prune_request(false);
        req.sharpe = n;
        assert_eq!(
            core::priority_prune(&f.root, f.store.as_ref(), &f.runtime, &req)
                .await
                .exit_code,
            1
        );
        assert!(f.runtime.calls().is_empty());
    }
    let f = Fixture::new("priority", false);
    let mut req = f.prune_request(false);
    req.confirm = "other".into();
    assert_eq!(
        core::priority_prune(&f.root, f.store.as_ref(), &f.runtime, &req)
            .await
            .exit_code,
        1
    );
    assert!(f.runtime.calls().is_empty());
    for patch in [
        doc! {"status":"running"},
        doc! {"taskType":"regular"},
        doc! {"targetRemote":"true"},
        doc! {"isRemote":"false"},
        doc! {"auth_profile":"../escape"},
    ] {
        let f = Fixture::new("priority", false);
        f.store.mutate(patch);
        assert_eq!(f.prune(false).await.exit_code, 1);
        assert!(f.runtime.calls().is_empty());
        assert!(f.store.0.lock().unwrap().writes.is_empty());
    }
    for kind in ["super", "priority"] {
        let f = Fixture::new(kind, false);
        f.runtime.behavior.lock().unwrap().absent_error = true;
        let out = if kind == "super" {
            f.observe().await
        } else {
            f.prune(false).await
        };
        assert_eq!(out.exit_code, 1);
        assert!(f.store.0.lock().unwrap().writes.is_empty());
    }
}
#[tokio::test]
async fn prune_original_failure_and_audit_cas_never_retry_algorithm() {
    let f = Fixture::new("priority", false);
    f.runtime.behavior.lock().unwrap().prune_error = true;
    assert_eq!(f.prune(false).await.exit_code, 1);
    assert!(f.store.0.lock().unwrap().writes.is_empty());
    let f = Fixture::new("priority", false);
    f.runtime.behavior.lock().unwrap().after_prune = doc! {"launchAttemptId":"next"};
    let out = f.prune(false).await;
    assert_eq!(out.exit_code, 1);
    assert_eq!(out.value["auditConfirmed"], false);
    assert!(out.value["cleanupReport"].is_object());
    assert_eq!(f.runtime.calls(), ["absent:false", "prune:false:false"]);
    assert_eq!(f.store.task().get_array("events").unwrap().len(), 0);
}
#[tokio::test]
async fn prune_bad_report_does_not_write_audit() {
    for patch in [doc! {"dryRun":true}, doc! {"metric":f64::NAN}] {
        let f = Fixture::new("priority", false);
        f.runtime.behavior.lock().unwrap().prune_patch = patch;
        assert_eq!(f.prune(false).await.exit_code, 1);
        assert!(f.store.0.lock().unwrap().writes.is_empty());
    }
}
#[tokio::test]
async fn shared_nonblocking_mission_flock_precedes_any_runtime_call() {
    let f = Fixture::new("super", false);
    let _held = MissionLocks::acquire(&f.root, &["fixture"], false).unwrap();
    assert_eq!(f.observe().await.exit_code, 1);
    assert_eq!(f.watch().await.exit_code, 1);
    assert_eq!(f.prune(false).await.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
    assert!(f.store.0.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn original_name_root_and_object_id_guards_are_not_bypassed() {
    let f = Fixture::new("regular", false);
    for name in ["../fixture", "", "_hidden", "two/names"] {
        let mut req = f.watch_request();
        req.name = name.into();
        assert_eq!(
            core::watch(&f.root, f.store.as_ref(), &f.runtime, &req)
                .await
                .exit_code,
            1
        );
    }
    let root = f.root.join("absent-backend-root");
    assert_eq!(
        core::watch(&root, f.store.as_ref(), &f.runtime, &f.watch_request())
            .await
            .exit_code,
        1
    );
    f.store.mutate(doc! {"_id":"not-an-object-id"});
    assert_eq!(f.watch().await.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
}
#[tokio::test]
async fn watch_reuses_saved_exact_target_and_shared_default_spec() {
    let f = Fixture::new("regular", false);
    f.store.mutate(doc! {"agentNotification":{"target":{"sessionId":"saved-main"},"reviewSeconds":600,"timeoutSeconds":3600}});
    let mut req = f.watch_request();
    req.notification = NotificationOptions::default();
    let out = core::watch(&f.root, f.store.as_ref(), &f.runtime, &req).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(
        out.value["notification"]["target"]["sessionId"],
        "saved-main"
    );
    assert_eq!(out.value["notification"]["itemKey"], "mission:fixture");
    assert_eq!(out.value["notification"]["reviewSeconds"], 600);
    assert_eq!(out.value["notification"]["timeoutSeconds"], 3600);
}
#[tokio::test]
async fn remote_priority_checks_effective_host_quiescence_without_rewriting_control() {
    let f = Fixture::new("priority", false);
    f.store.mutate(doc! {"targetRemote":true});
    f.runtime.behavior.lock().unwrap().absent_error = true;
    assert_eq!(f.prune(false).await.exit_code, 1);
    assert_eq!(f.runtime.calls(), ["absent:true"]);
    assert!(f.store.0.lock().unwrap().writes.is_empty());
    f.runtime.behavior.lock().unwrap().absent_error = false;
    let out = f.prune(false).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["remote"], true);
    assert_eq!(f.store.task().get_bool("isRemote"), Ok(false));
    assert_eq!(f.store.task().get_bool("targetRemote"), Ok(true));
    assert_eq!(f.store.task().get_bool("stopConfirmedRemote"), Ok(false));
}

#[tokio::test]
async fn acknowledged_observation_claim_is_reread_before_process_side_effect() {
    for patch in [
        doc! {"runId":"foreign-run"},
        doc! {"auth_profile":"foreign-profile"},
        doc! {"acceptedObservation":{"id":"foreign-owner","finishedAt":Bson::Null}},
        doc! {"stopConfirmedAt":bson::DateTime::from_millis(3000)},
    ] {
        let f = Fixture::new("super", false);
        f.store
            .0
            .lock()
            .unwrap()
            .mutate_after
            .insert("observe-accepted-begin".into(), patch);
        let out = f.observe().await;
        assert_eq!(out.exit_code, 1, "{}", out.value);
        assert_eq!(f.runtime.calls(), ["absent:false"]);
        assert_eq!(
            f.store
                .task()
                .get_document("acceptedObservation")
                .unwrap()
                .get("finishedAt"),
            Some(&Bson::Null)
        );
    }
}

#[tokio::test]
async fn legacy_null_watch_host_remains_local_without_relaxing_strict_observer_host() {
    let f = Fixture::new("regular", false);
    f.store.mutate(doc! {"isRemote":Bson::Null});
    let out = f.watch().await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["notification"]["remote"], false);
    let f = Fixture::new("super", false);
    f.store.mutate(doc! {"isRemote":Bson::Null});
    assert_eq!(f.observe().await.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
}

#[tokio::test]
async fn legacy_paused_priority_without_stop_fields_can_prune_after_original_quiescence() {
    for remote in [false, true] {
        for dry in [false, true] {
            let f = Fixture::new("priority", remote);
            {
                let mut s = f.store.0.lock().unwrap();
                for key in [
                    "stopConfirmedAt",
                    "stopConfirmedRemote",
                    "stopConfirmedTaskId",
                    "stopConfirmationBasis",
                ] {
                    s.task.remove(key);
                }
            }
            let before = f.store.task();
            let out = f.prune(dry).await;
            assert_eq!(out.exit_code, 0, "{}", out.value);
            assert_eq!(
                f.runtime.calls(),
                [format!("absent:{remote}"), format!("prune:{remote}:{dry}")]
            );
            let after = f.store.task();
            for key in [
                "stopConfirmedAt",
                "stopConfirmedRemote",
                "stopConfirmedTaskId",
                "stopConfirmationBasis",
            ] {
                assert!(!after.contains_key(key));
            }
            assert_eq!(after.get("isRemote"), before.get("isRemote"));
            assert_eq!(after.get("targetRemote"), before.get("targetRemote"));
            if dry {
                assert_eq!(after, before);
            }
        }
    }
    // Observation still requires its original stop proof, unlike prune.
    let f = Fixture::new("super", false);
    f.store.0.lock().unwrap().task.remove("stopConfirmedAt");
    assert_eq!(f.observe().await.exit_code, 1);
    assert!(f.runtime.calls().is_empty());
}
