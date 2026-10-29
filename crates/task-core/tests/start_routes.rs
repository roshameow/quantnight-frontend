use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{
    mission_lock::MissionLocks,
    start::{
        self as core, LaunchOutput, NotificationTarget, StartCommand, StartRequest, StartRuntime,
    },
    task_store::{id_projection, CollectionSummary, Host, TaskStore},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn get<'a>(row: &'a Document, key: &str) -> Option<&'a Bson> {
    let mut parts = key.split('.');
    let mut v = row.get(parts.next()?)?;
    for p in parts {
        v = v.as_document()?.get(p)?;
    }
    Some(v)
}
fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(k, v)| {
        let actual = get(row, k);
        if let Bson::Document(q) = v {
            if let Ok(exists) = q.get_bool("$exists") {
                return actual.is_some() == exists;
            }
            if let Some(ne) = q.get("$ne") {
                return actual != Some(ne);
            }
        }
        if v == &Bson::Null {
            actual.is_none() || actual == Some(&Bson::Null)
        } else {
            actual == Some(v)
        }
    })
}
fn apply(row: &Document, update: &Document) -> Result<Document, String> {
    let mut next = row.clone();
    if let Ok(fields) = update.get_document("$set") {
        next.extend(fields.clone());
    }
    if let Ok(push) = update.get_document("$push") {
        for (k, v) in push {
            if !next.contains_key(k) {
                next.insert(k, Vec::<Bson>::new());
            }
            next.get_array_mut(k)
                .map_err(|_| "invalid event array")?
                .push(v.clone());
        }
    }
    Ok(next)
}
#[derive(Default)]
struct State {
    local: Vec<Document>,
    remote: Vec<Document>,
    local_rows: Vec<Document>,
    remote_rows: Vec<Document>,
    calls: Vec<String>,
    fail_before: BTreeSet<String>,
    fail_after: BTreeSet<String>,
    mutate_before: BTreeMap<String, Document>,
    mutate_after: BTreeMap<String, Document>,
    id_growth: bool,
    insert_ack_lost: bool,
    original_control_file: Option<PathBuf>,
}
#[derive(Default)]
struct Store(Mutex<State>);
impl Store {
    fn publish_original_control(state:&State) {
        if let (Some(path),Some(task))=(&state.original_control_file,state.local.last()) {
            std::fs::write(path,Bson::Document(task.clone()).into_relaxed_extjson().to_string()).unwrap();
        }
    }
    fn task(&self) -> Document {
        self.0.lock().unwrap().local.last().unwrap().clone()
    }
    fn mutate(&self, fields: Document) {
        self.0
            .lock()
            .unwrap()
            .local
            .last_mut()
            .unwrap()
            .extend(fields);
    }
}
#[async_trait]
impl TaskStore for Store {
    async fn find_task(
        &self,
        host: Host,
        query: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(format!("find:{host:?}"));
        let rows = if host == Host::Local {
            &s.local
        } else {
            &s.remote
        };
        Ok(if newest {
            rows.iter().rev().find(|r| matches(r, &query))
        } else {
            rows.iter().find(|r| matches(r, &query))
        }
        .cloned())
    }
    async fn find_tasks(&self, host: Host, query: Document) -> Result<Vec<Document>, String> {
        assert_eq!(host, Host::Local);
        let mut s = self.0.lock().unwrap();
        s.calls.push("find_tasks(limit2)".into());
        Ok(s.local
            .iter()
            .filter(|r| matches(r, &query))
            .take(2)
            .cloned()
            .collect())
    }
    async fn insert_task(&self, host: Host, row: Document) -> Result<Bson, String> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(if host == Host::Local { "control-create" } else { "runtime-insert" }.into());
        let id = row.get("_id").unwrap().clone();
        if host == Host::Local { s.local.push(row); } else { s.remote.push(row); }
        Self::publish_original_control(&s);
        if s.insert_ack_lost {
            return Err("runtime insert response lost".into());
        }
        Ok(id)
    }
    async fn update_task(
        &self,
        host: Host,
        query: Document,
        update: Document,
    ) -> Result<u64, String> {
        let label = if let Ok(events) = update
            .get_document("$push")
            .and_then(|p| p.get_document("events"))
        {
            format!(
                "{}:{}",
                events.get_str("command").unwrap(),
                events.get_str("status").unwrap()
            )
        } else {
            "runtime-register".into()
        };
        let mut s = self.0.lock().unwrap();
        s.calls.push(label.clone());
        if let Some(change) = s.mutate_before.remove(&label) {
            let rows = if host == Host::Local {
                &mut s.local
            } else {
                &mut s.remote
            };
            rows.last_mut().unwrap().extend(change);
        }
        if s.fail_before.remove(&label) {
            return Err(format!("write before {label}"));
        }
        let rows = if host == Host::Local {
            &mut s.local
        } else {
            &mut s.remote
        };
        let Some(i) = rows.iter().position(|r| matches(r, &query)) else {
            return Ok(0);
        };
        rows[i] = apply(&rows[i], &update)?;
        if let Some(change)=s.mutate_after.remove(&label){
            let rows=if host==Host::Local{&mut s.local}else{&mut s.remote};
            rows.last_mut().unwrap().extend(change);
        }
        Self::publish_original_control(&s);
        if s.fail_after.remove(&label) {
            return Err(format!("response lost after {label}"));
        }
        Ok(1)
    }
    async fn summary(&self, host: Host, _: &str) -> Result<CollectionSummary, String> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(format!("summary:{host:?}"));
        let rows = if host == Host::Local {
            &s.local_rows
        } else {
            &s.remote_rows
        };
        let mut statuses = BTreeMap::new();
        for row in rows {
            *statuses
                .entry(row.get_str("status").unwrap_or("pending").into())
                .or_insert(0) += 1;
        }
        Ok(CollectionSummary {
            exists: !rows.is_empty(),
            total: rows.len() as u64,
            statuses,
            latest_heartbeat_at: None,
        })
    }
    async fn rows(
        &self,
        host: Host,
        _: &str,
        projection: Option<Document>,
    ) -> Result<Vec<Document>, String> {
        assert_eq!(
            projection,
            Some(id_projection()),
            "Rust must not read/reimplement research payload proof"
        );
        let mut s = self.0.lock().unwrap();
        s.calls.push(format!("ids:{host:?}"));
        let rows = if host == Host::Local {
            &s.local_rows
        } else {
            &s.remote_rows
        };
        let mut out: Vec<_> = rows
            .iter()
            .map(|r| doc! {"_id":r.get("_id").unwrap()})
            .collect();
        if s.id_growth {
            out.push(doc! {"_id":ObjectId::new()});
        }
        Ok(out)
    }
    async fn update_items(
        &self,
        _: Host,
        _: &str,
        _: Document,
        _: Document,
    ) -> Result<u64, String> {
        panic!("start must never reset receipts/items")
    }
    async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
        panic!("not used")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("not used")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("start never syncs or rePOSTs")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("not used")
    }
    async fn ping(&self, _: Host) -> Result<(), String> {
        panic!("not used")
    }
}
struct Runtime {
    store: Arc<Store>,
    calls: Mutex<Vec<String>>,
    absence_hosts: Mutex<Vec<bool>>,
    fail: Option<&'static str>,
    mutation: Option<(&'static str, Document)>,
    launch_mode: &'static str,
    watch_mode: &'static str,
    live: AtomicBool,
    commands: Mutex<Vec<StartCommand>>,
    inputs: Mutex<Vec<Option<Document>>>,
    proof_bad: bool,
    continuation: Option<Document>,
    fast_super: bool,
    block_launch: Option<Arc<tokio::sync::Notify>>,
}
impl Runtime {
    fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            calls: Mutex::new(vec![]),
            absence_hosts: Mutex::new(vec![]),
            fail: None,
            mutation: None,
            launch_mode: "ok",
            watch_mode: "ok",
            live: AtomicBool::new(false),
            commands: Mutex::new(vec![]),
            inputs: Mutex::new(vec![]),
            proof_bad: false,
            continuation: None,
            fast_super: false,
            block_launch: None,
        }
    }
    fn step(&self, name: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(name.into());
        if let Some((stage, fields)) = &self.mutation {
            if *stage == name {
                self.store.mutate(fields.clone());
            }
        }
        if self.fail == Some(name) {
            Err(format!("original owner rejected {name}"))
        } else {
            Ok(())
        }
    }
    fn launches(&self) -> usize {
        self.commands.lock().unwrap().len()
    }
}
#[async_trait]
impl StartRuntime for Runtime {
    async fn priority_config(
        &self,
        _: &Path,
        task: &Document,
        selected: Option<Document>,
    ) -> Result<Document, String> {
        self.step("priority")?;
        self.inputs.lock().unwrap().push(selected.clone());
        let mut config = selected.unwrap_or_else(|| doc! {"ownerDefault":true,"interval":600});
        config.insert(
            "auth_profile",
            task.get_str("auth_profile").unwrap_or("user1"),
        );
        Ok(config)
    }
    async fn validate_super_config(&self, _: &Path, _: &Document) -> Result<(), String> {
        self.step("super")
    }
    async fn bind_regular_source(
        &self,
        _: &Path,
        task: &Document,
        config: &Document,
    ) -> Result<Document, String> {
        self.step("source")?;
        let id = task.get_object_id("_id").unwrap().to_hex();
        if config
            .get("source_task_id")
            .is_some_and(|v| v != &Bson::String(id.clone()))
            || config
                .get("source_remote")
                .is_some_and(|v| v != &Bson::Boolean(true))
        {
            return Err("original source identity conflict".into());
        }
        let mut config = config.clone();
        config.extend(doc! {"source_task_id":id,"source_remote":true});
        Ok(config)
    }
    async fn verify_priority_extension(
        &self,
        _: &Path,
        task: &Document,
        local: u64,
        remote: u64,
    ) -> Result<Document, String> {
        self.step("extension")?;
        Ok(
            doc! {"reason":"verified_priority_extension","sourceTaskId":if self.proof_bad{"wrong".into()}else{task.get_object_id("_id").unwrap().to_hex()},"sourceRemote":true,
            "localBaseCount":local as i64,"remoteBaseCount":local as i64,"remoteTotalCount":remote as i64,"remotePriorityExtensionCount":(remote-local) as i64},
        )
    }
    async fn continuation(
        &self,
        _: &Path,
        _: &Document,
        _: bool,
    ) -> Result<Option<Document>, String> {
        self.step("continuation")?;
        Ok(self.continuation.clone())
    }
    async fn preflight_notification(&self, _: &Path, spec: &Document) -> Result<(), String> {
        self.step("notify-preflight")?;
        assert!(spec.get_document("target").is_ok());
        Ok(())
    }
    async fn assert_consumer_absent(&self, _: &Path, _: &Document, remote: bool) -> Result<(), String> {
        self.absence_hosts.lock().unwrap().push(remote);
        self.step("absence")?;
        if self.live.load(Ordering::SeqCst) {
            Err("task already running; use watch".into())
        } else {
            Ok(())
        }
    }
    async fn validate_execution_binding(
        &self,
        _: &Path,
        binding: &Document,
        _: bool,
    ) -> Result<(), String> {
        if let Ok(context) = binding.get_document("tmuxContext") {
            self.step("context")?;
            if context.get_str("backend") != Ok("tmux")
                || context.get_str("socketName") != Ok("default")
                || !context.get_str("binary").unwrap_or("").starts_with('/')
            {
                return Err("original execution_binding rejected context".into());
            }
        } else {
            self.step("binding")?;
        }
        Ok(())
    }
    async fn launch(&self, _: &Path, command: &StartCommand) -> Result<LaunchOutput, String> {
        self.step("launch")?;
        self.commands.lock().unwrap().push(command.clone());
        if let Some(started) = &self.block_launch {
            started.notify_one();
            return std::future::pending().await;
        }
        let task = self.store.task();
        assert_eq!(task.get_str("status"), Ok("launch_unknown"));
        for key in [
            "stopConfirmedAt",
            "stopConfirmedRemote",
            "stopConfirmedTaskId",
            "stopConfirmationBasis",
            "tmuxContext",
        ] {
            assert_eq!(task.get(key), Some(&Bson::Null));
        }
        assert!(command.timeout_seconds > 0 && command.timeout_seconds <= 120, "child launch must stay within120 and remaining original outer budget");
        assert_eq!(command.argv[0], "bash");
        self.live.store(true, Ordering::SeqCst);
        let remote = command.binding.get_bool("remote").unwrap();
        if task.get_str("taskType") == Ok("super") {
            let mut s = self.store.0.lock().unwrap();
            let rows = if remote { &mut s.remote } else { &mut s.local };
            let registry = rows
                .iter_mut()
                .find(|r| r.get("_id") == task.get("_id"))
                .unwrap();
            assert_eq!(
                registry
                    .get_document("simulatorRuntime")
                    .unwrap()
                    .get_document("startupRecovery")
                    .unwrap()
                    .get_str("phase"),
                Ok("pending")
            );
            if self.fast_super {
                registry.insert("simulatorRuntime",doc!{"startupRecovery":{"phase":"complete","launchAttemptId":task.get("launchAttemptId").unwrap()}});
            }
        }
        if self.launch_mode == "lost" {
            return Err("launcher response lost; exit unconfirmed".into());
        }
        let mut context = doc! {"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/usr/bin/tmux","sessionId":"$7","sessionCreatedAt":123456,
        "taskId":command.binding.get("taskId").unwrap(),"launchAttemptId":command.binding.get("launchAttemptId").unwrap(),"remote":remote};
        match self.launch_mode {
            "wrong-id" => {
                context.insert("taskId", "foreign");
            }
            "wrong-host" => {
                context.insert("remote", !remote);
            }
            "wrong-launch" => {
                context.insert("launchAttemptId", "foreign");
            }
            "bad-context" => {
                context.insert("socketName", "rmux");
            }
            _ => (),
        }
        let contexts = match self.launch_mode {
            "none" => vec![],
            "double" => vec![context.clone(), context],
            _ => vec![context],
        };
        Ok(LaunchOutput {
            stdout: "original launcher output".into(),
            stderr: String::new(),
            observed_contexts: contexts,
        })
    }
    async fn attach_watcher(
        &self,
        _: &Path,
        task: &Document,
        remote: bool,
        spec: &Document,
    ) -> Result<Document, String> {
        self.step("watch")?;
        assert_eq!(self.store.task().get_str("status"), Ok("running"));
        if self.watch_mode == "failure" {
            return Err("original watcher handshake failed".into());
        }
        let mut receipt = spec.clone();
        receipt.extend(doc!{"missionId":task.get_object_id("_id").unwrap().to_hex(),"launchId":task.get("launchAttemptId").unwrap(),"remote":remote,
            "producerPid":321,"producerMarkerPath":"/fixture/watcher.lock","checkpointPath":"/fixture/watcher.json","requiresLease":spec.get_document("target").unwrap().contains_key("taskId")});
        if self.watch_mode == "wrong-id" {
            receipt.insert("missionId", "foreign");
        }
        Ok(receipt)
    }
}
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("qn-start-{}", ObjectId::new()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn setup(kind: &str, remote: bool) -> (Root, Arc<Store>, Runtime, StartRequest) {
    let root = Root::new();
    let store = Arc::new(Store::default());
    let task = doc! {"_id":ObjectId::new(),"name":"task","taskType":kind,"auth_profile":"user1","status":"paused","isRemote":remote,"targetRemote":remote,"updatedAt":"old-revision","events":[],
    "launchAttemptId":"old-launch","stopConfirmedAt":bson::DateTime::from_millis(1000),"stopConfirmedTaskId":"old-id","stopConfirmedRemote":remote,"tmuxContext":{"old":"not-new-authority"}};
    let row = doc! {"_id":ObjectId::new(),"status":"processing","acceptance":"ACCEPTED","progressUrl":"fixture-original-location","submissionAttempts":[{"accepted":true}]};
    {
        let mut s = store.0.lock().unwrap();
        s.local.push(task);
        s.local_rows.push(row.clone());
        s.remote_rows.push(row);
    }
    let runtime = Runtime::new(store.clone());
    (root, store, runtime, StartRequest::new("task"))
}
fn success(o: &quantnight_task_core::cleanup_sync::TaskOutcome) {
    assert_eq!(o.exit_code, 0, "{}", o.value);
    assert_eq!(o.value["ok"], true);
}
fn failed(o: &quantnight_task_core::cleanup_sync::TaskOutcome) {
    assert_eq!(o.exit_code, 1, "{}", o.value);
    assert_eq!(o.value["ok"], false);
}
fn request_notice(r: &mut StartRequest) {
    r.notification.target = Some(NotificationTarget::Worker("worker-exact".into()));
}

#[tokio::test]
async fn regular_local_preserves_full_config_and_original_rows() {
    let (root, store, runtime, mut request) = setup("regular", false);
    let config = doc! {"max_concurrent":4,"max_multi_simulation_children":8,"custom":{"keep":[true,"x",null]},"tag":{"name":"user","tags":["not-source"]}};
    store.mutate(doc! {"startConfig":&config,"mission_config":{"stale":true}});
    request.expected_task_id = Some(store.task().get_object_id("_id").unwrap().to_hex());
    request.expected_launch_attempt_id = Some("old-launch".into());
    let rows = store.0.lock().unwrap().local_rows.clone();
    success(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    let saved = store.task();
    assert_eq!(saved.get_str("status"), Ok("running"));
    assert_eq!(saved.get_document("startConfig"), Ok(&config));
    assert_eq!(saved.get("mission_config"), saved.get("startConfig"));
    assert_eq!(rows, store.0.lock().unwrap().local_rows);
    assert_ne!(saved.get_str("launchAttemptId"), Ok("old-launch"));
    assert_eq!(runtime.launches(), 1);
    let calls = runtime.calls.lock().unwrap();
    assert!(
        calls.iter().position(|x| x == "continuation").unwrap()
            < calls.iter().position(|x| x == "launch").unwrap()
    );
}

#[tokio::test]
async fn all_three_launchers_and_priority_without_own_collection() {
    for kind in ["regular", "super", "priority"] {
        let (root, store, runtime, request) = setup(kind, false);
        if kind == "priority" {
            let mut s = store.0.lock().unwrap();
            s.local_rows.clear();
            s.remote_rows.clear();
            let task = s.local.last_mut().unwrap();
            for key in ["launchAttemptId", "stopConfirmedAt", "stopConfirmedTaskId", "stopConfirmedRemote", "tmuxContext"] { task.remove(key); }
            let created = chrono::Utc::now().to_rfc3339();
            task.extend(doc! {"status":"pending","targetRemote":true,"runId":"mission:task","createdAt":&created,
                "events":[{"id":"create-proof","command":"create","status":"pending","at":&created,"detail":{}}],
                "config":{"ignoredLegacy":true}});
        }
        let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
        success(&result);
        let commands = runtime.commands.lock().unwrap();
        assert_eq!(
            commands[0].argv[1],
            format!(
                "scripts/{}",
                match kind {
                    "super" => "start_super_task.sh",
                    "priority" => "start_priority_task.sh",
                    _ => "start_task.sh",
                }
            )
        );
        if kind == "priority" {
            assert!(result.value["counts"].is_null());
            assert_eq!(commands[0].argv[4], "--remote");
            assert_eq!(store.task().get_bool("targetRemote"), Ok(true));
            assert!(!store
                .0
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|x| x.starts_with("summary:")));
            assert!(runtime.inputs.lock().unwrap()[0].is_none());
        }
    }
}

#[tokio::test]
async fn explicit_priority_config_and_source_scope_are_not_merged_or_replaced() {
    let (root, store, runtime, mut request) = setup("priority", false);
    store.mutate(doc! {"mission_config":{"saved":true},"startConfig":{"older":true}});
    request.config_json = Some(
        r#"{"source_tasks":["exact"],"refine_templates":[],"custom":{"keep":null},"interval":42}"#
            .into(),
    );
    request.remote = Some(false);
    success(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    let saved = store.task();
    let c = saved.get_document("startConfig").unwrap();
    assert_eq!(c.get_array("refine_templates").unwrap().len(), 0);
    assert!(!c.contains_key("saved"));
    assert_eq!(
        c.get_array("source_tasks").unwrap(),
        &vec![Bson::String("exact".into())]
    );
}

#[tokio::test]
async fn remote_exact_sync_wrong_ids_and_growth_are_checked_without_writes() {
    for fault in ["none", "wrong-id", "growth", "empty"] {
        let (root, store, runtime, request) = setup("regular", true);
        {
            let mut s = store.0.lock().unwrap();
            match fault {
                "wrong-id" => {
                    s.remote_rows[0].insert("_id", ObjectId::new());
                }
                "growth" => s.id_growth = true,
                "empty" => s.remote_rows.clear(),
                _ => (),
            }
        }
        let before = store.task();
        let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
        if fault == "none" {
            success(&result);
            assert_eq!(runtime.launches(), 1);
        } else {
            failed(&result);
            assert_eq!(store.task(), before);
            assert_eq!(runtime.launches(), 0);
        }
    }
}

#[tokio::test]
async fn extension_only_for_bound_remote_regular_and_uses_original_owner() {
    for kind in ["regular", "super"] {
        for stored_remote in [true, false] {
            let (root, store, runtime, mut request) = setup(kind, stored_remote);
            request.remote = Some(true);
            store
                .0
                .lock()
                .unwrap()
                .remote_rows
                .push(doc! {"_id":ObjectId::new(),"status":"pending"});
            let before = store.0.lock().unwrap().remote_rows.clone();
            let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
            if kind == "regular" && stored_remote {
                success(&result);
                assert_eq!(
                    runtime
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|x| *x == "source")
                        .count(),
                    2
                );
                assert_eq!(
                    result.value["preflight"]["reason"],
                    "verified_priority_extension"
                );
            } else {
                failed(&result);
                assert_eq!(runtime.launches(), 0);
            }
            assert_eq!(before, store.0.lock().unwrap().remote_rows);
        }
    }
}

#[tokio::test]
async fn extension_ambiguous_source_bad_proof_or_validator_error_stops_before_claim() {
    for fault in ["source-conflict", "proof", "owner-error"] {
        let (root, store, mut runtime, request) = setup("regular", true);
        store
            .0
            .lock()
            .unwrap()
            .remote_rows
            .push(doc! {"_id":ObjectId::new()});
        match fault {
            "source-conflict" => {
                let mut other = store.task();
                other.insert("_id", ObjectId::new());
                store.0.lock().unwrap().local.push(other);
            }
            "proof" => runtime.proof_bad = true,
            _ => runtime.fail = Some("extension"),
        }
        let before = store.task();
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
        assert_eq!(store.task(), before);
    }
}

#[tokio::test]
async fn preflight_failures_never_invalidate_old_stop_or_launch() {
    for stage in [
        "continuation",
        "absence",
        "notify-preflight",
        "super",
        "priority",
    ] {
        let kind = if stage == "super" {
            "super"
        } else if stage == "priority" {
            "priority"
        } else {
            "regular"
        };
        let (root, store, mut runtime, mut request) = setup(kind, false);
        runtime.fail = Some(stage);
        if stage == "notify-preflight" {
            request_notice(&mut request);
        }
        let before = store.task();
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(store.task(), before);
        assert_eq!(runtime.launches(), 0);
    }
}

#[tokio::test]
async fn invalid_configs_busy_observation_and_expected_id_fail_without_launcher() {
    for fault in [
        "nan",
        "bson-nan",
        "nonjson",
        "null-saved",
        "busy",
        "observation",
        "wrong-id",
        "wrong-launch",
    ] {
        let (root, store, runtime, mut request) = setup("regular", false);
        match fault {
            "nan" => request.config_json = Some("{\"max_concurrent\":NaN}".into()),
            "bson-nan" => store.mutate(doc! {"startConfig":{"bad":f64::NAN}}),
            "nonjson" => store.mutate(doc! {"startConfig":{"bad":ObjectId::new()}}),
            "null-saved" => {
                store.mutate(doc! {"startConfig":Bson::Null,"mission_config":{"valid":true}})
            }
            "busy" => store.mutate(doc! {"status":"launch_unknown"}),
            "observation" => store.mutate(doc! {"acceptedObservation":{"id":"open"}}),
            "wrong-id" => request.expected_task_id = Some(ObjectId::new().to_hex()),
            _ => request.expected_launch_attempt_id = Some("stale".into()),
        }
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
        assert!(!store
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|x| x == "start:launch_unknown"));
    }
}

#[tokio::test]
async fn changed_source_or_claim_cas_never_launches() {
    for stage in ["continuation", "binding"] {
        let (root, store, mut runtime, request) = setup("regular", false);
        runtime.mutation = Some((
            stage,
            doc! {"launchAttemptId":"foreign","updatedAt":"unchanged-elsewhere"},
        ));
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
        assert_eq!(store.task().get_str("launchAttemptId"), Ok("foreign"));
    }
    let (root, store, runtime, request) = setup("regular", false);
    store
        .0
        .lock()
        .unwrap()
        .mutate_before
        .insert("start:launch_unknown".into(), doc! {"_id":ObjectId::new()});
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(runtime.launches(), 0);
}

#[tokio::test]
async fn lost_claim_response_and_unverified_launch_never_restore_pause_or_restart() {
    let (root, store, runtime, request) = setup("regular", false);
    store
        .0
        .lock()
        .unwrap()
        .fail_after
        .insert("start:launch_unknown".into());
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(runtime.launches(), 0);
    assert_eq!(store.task().get_str("status"), Ok("launch_unknown"));
    for mode in [
        "lost",
        "none",
        "double",
        "wrong-id",
        "wrong-host",
        "wrong-launch",
        "bad-context",
    ] {
        let (root, store, mut runtime, request) = setup("regular", false);
        runtime.launch_mode = mode;
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        let saved = store.task();
        assert_eq!(saved.get_str("status"), Ok("launch_unknown"));
        assert_eq!(saved.get("stopConfirmedAt"), Some(&Bson::Null));
        assert_eq!(runtime.launches(), 1);
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 1);
    }
}

#[tokio::test]
async fn running_cas_refuses_new_identity_after_confirmed_consumer() {
    let (root, store, runtime, request) = setup("regular", false);
    let replacement = ObjectId::new();
    store.0.lock().unwrap().mutate_before.insert(
        "start:running".into(),
        doc! {"_id":replacement,"status":"pending","launchAttemptId":"foreign"},
    );
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(store.task().get_object_id("_id"), Ok(replacement));
    assert_eq!(store.task().get_str("status"), Ok("pending"));
    assert_eq!(runtime.launches(), 1);
}

#[tokio::test]
async fn super_registration_refuses_foreign_identity_profile_side_and_lost_ack() {
    for fault in ["id", "profile", "side", "deactive", "insert-lost", "cas"] {
        let (root, store, runtime, request) = setup("super", true);
        let task = store.task();
        {
            let mut s = store.0.lock().unwrap();
            if fault == "insert-lost" {
                s.insert_ack_lost = true;
            } else {
                let mut registry = doc! {"_id":task.get("_id").unwrap(),"name":"task","taskType":"super","auth_profile":"user1","isRemote":true};
                match fault {
                    "id" => {
                        registry.insert("_id", ObjectId::new());
                    }
                    "profile" => {
                        registry.insert("auth_profile", "foreign");
                    }
                    "side" => {
                        registry.insert("isRemote", false);
                    }
                    "deactive" => {
                        registry.insert("status", "deactive");
                    }
                    "cas" => {
                        s.mutate_before.insert(
                            "runtime-register".into(),
                            doc! {"simulatorRuntime":{"foreign":true}},
                        );
                    }
                    _ => (),
                };
                s.remote.push(registry);
            }
        }
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
        assert_eq!(store.task().get_str("status"), Ok("launch_unknown"));
    }
}

#[tokio::test]
async fn super_startup_binding_is_new_and_fast_sweep_fact_is_not_overwritten() {
    for remote in [false, true] {
        let (root, store, mut runtime, request) = setup("super", remote);
        runtime.fast_super = true;
        store.mutate(
            doc! {"startConfig":{"startup_binding":{"old":"must-not-authorize"},"custom":[]}},
        );
        success(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        let saved = store.task();
        let binding = saved.get_document("startupBinding").unwrap();
        assert_eq!(
            saved
                .get_document("startConfig")
                .unwrap()
                .get_document("startup_binding"),
            Ok(binding)
        );
        assert!(!binding.contains_key("old"));
        let s = store.0.lock().unwrap();
        let registry = if remote { &s.remote[0] } else { &s.local[0] };
        assert_eq!(
            registry
                .get_document("simulatorRuntime")
                .unwrap()
                .get_document("startupRecovery")
                .unwrap()
                .get_str("phase"),
            Ok("complete")
        );
    }
}

#[tokio::test]
async fn watcher_failure_reports_consumer_started_and_only_watch_repair() {
    for fault in ["failure", "wrong-id", "metadata"] {
        let (root, store, mut runtime, mut request) = setup("regular", false);
        request_notice(&mut request);
        runtime.watch_mode = fault;
        if fault == "metadata" {
            store
                .0
                .lock()
                .unwrap()
                .fail_before
                .insert("watch-attach:ok".into());
        }
        let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
        failed(&result);
        assert_eq!(result.value["consumerStarted"], true);
        assert_eq!(result.value["notificationAttached"], false);
        assert!(result.value["nextAction"]
            .as_str()
            .unwrap()
            .contains("do not restart"));
        assert_eq!(store.task().get_str("status"), Ok("running"));
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 1);
    }
}

#[tokio::test]
async fn notification_success_saved_target_and_no_notify_preserve_exact_routes() {
    let (root, store, runtime, request) = setup("regular", false);
    store.mutate(doc!{"agentNotification":{"target":{"sessionId":"main-exact"},"reviewSeconds":120,"timeoutSeconds":600}});
    let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
    success(&result);
    assert_eq!(
        result.value["notification"]["target"]["sessionId"],
        "main-exact"
    );
    assert_eq!(result.value["notification"]["requiresLease"], false);
    let (root, store, runtime, mut request) = setup("regular", false);
    request.notification.no_notify = true;
    store.mutate(doc! {"agentNotification":{"target":{"taskId":"old-worker"}}});
    success(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert!(!runtime
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|x| x == "watch" || x == "notify-preflight"));
}

#[tokio::test]
async fn legal_get_only_continuation_report_does_not_reset_receipts_or_block_all_work() {
    let (root, store, mut runtime, request) = setup("regular", false);
    runtime.continuation = Some(
        doc! {"status":"error","ownReceiptRecovery":true,"admissionBlocked":true,"unresolved":1},
    );
    let rows = store.0.lock().unwrap().local_rows.clone();
    let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
    success(&result);
    assert_eq!(
        result.value["preflight"]["datasetContinuation"]["ownReceiptRecovery"],
        true
    );
    assert_eq!(rows, store.0.lock().unwrap().local_rows);
    let (root, store, runtime, request) = setup("regular", false);
    let _guard = MissionLocks::acquire(&root.0, &["task"], false).unwrap();
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert!(runtime.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn missing_original_url_owner_rejection_and_priority_nonfinite_stay_before_launch() {
    let (root, store, mut runtime, request) = setup("regular", false);
    store.0.lock().unwrap().local_rows[0].remove("progressUrl");
    let before_rows = store.0.lock().unwrap().local_rows.clone();
    let before_task = store.task();
    runtime.fail = Some("continuation"); // original no-trusted-own-receipt rejection
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(runtime.launches(), 0);
    assert_eq!(store.task(), before_task);
    assert_eq!(store.0.lock().unwrap().local_rows, before_rows);
    for config in [
        doc! {"unknownExtension": f64::INFINITY},
        doc! {"interval": f64::NAN},
    ] {
        let (root, store, runtime, request) = setup("priority", false);
        store.mutate(doc! {"mission_config": config});
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
        assert_eq!(store.task().get_str("status"), Ok("paused"));
        assert!(!runtime
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "priority"));
    }
}

#[tokio::test]
async fn original_mutator_lock_spans_launcher_await_and_abort_retains_unknown_claim() {
    let (root, store, mut runtime, request) = setup("regular", false);
    let started = Arc::new(tokio::sync::Notify::new());
    runtime.block_launch = Some(started.clone());
    let runtime = Arc::new(runtime);
    let handle = {
        let path = root.0.clone();
        let store = store.clone();
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(
            async move { core::start(&path, store.as_ref(), runtime.as_ref(), &request).await },
        )
    };
    started.notified().await;
    let claimed = store.task();
    assert_eq!(claimed.get_str("status"), Ok("launch_unknown"));
    failed(&core::start(&root.0, store.as_ref(), runtime.as_ref(), &request).await);
    assert_eq!(runtime.launches(), 1);
    handle.abort();
    assert!(handle.await.unwrap_err().is_cancelled());
    failed(&core::start(&root.0, store.as_ref(), runtime.as_ref(), &request).await);
    assert_eq!(store.task(), claimed);
    assert_eq!(runtime.launches(), 1);
}

#[tokio::test]
async fn start_cannot_change_an_existing_execution_host_or_bypass_captured_host() {
    for kind in ["regular", "super", "priority"] {
        for actual in [false, true] {
            let (root, store, runtime, mut request) = setup(kind, actual);
            request.remote = Some(!actual);
            let before = store.task();
            failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
            assert_eq!(runtime.launches(), 0);
            assert_eq!(store.task(), before);
        }
    }
    let (root, store, runtime, mut request) = setup("regular", false);
    request.expected_remote = Some(true);
    failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(runtime.launches(), 0);
}

#[tokio::test]
async fn a_priority_pending_label_or_erased_launch_is_not_pristine_destination_authority() {
    for change in [doc! {"status":"pending","targetRemote":true},
        doc! {"status":"pending","targetRemote":true,"launchAttemptId":Bson::Null,"tmuxContext":Bson::Null,"stopConfirmedAt":Bson::Null}] {
        let (root, store, runtime, request) = setup("priority", false);
        store.mutate(change);
        failed(&core::start(&root.0, store.as_ref(), &runtime, &request).await);
        assert_eq!(runtime.launches(), 0);
    }
}

#[async_trait]
impl quantnight_task_core::create_generate::GenerationRuntime for Runtime {
    async fn creation_metadata(&self, _: &Path, desired: &Document)
        -> Result<quantnight_task_core::create_generate::CreationMetadata, quantnight_task_core::create_generate::GenerationFailure> {
        assert_eq!(desired.get_str("taskType"), Ok("priority"));
        Ok(quantnight_task_core::create_generate::CreationMetadata::default())
    }
    async fn generate(&self, _: &Path, _: &quantnight_task_core::create_generate::GenerationCommand)
        -> Result<quantnight_task_core::create_generate::GeneratedOutput, quantnight_task_core::create_generate::GenerationFailure> {
        panic!("Priority first start must not generate or dispatch simulation items")
    }
}

#[tokio::test]
async fn actual_shared_create_then_first_priority_remote_start_preserves_intent_without_migration() {
    use quantnight_task_core::create_generate::{self, CreateRequest};
    let root = Root::new();
    let store = Arc::new(Store::default());
    let runtime = Runtime::new(store.clone());
    let mut create = CreateRequest::new("task", "pipeline");
    create.task_type = "priority".into();
    create.target_remote = true;
    let config = doc! {"interval":600,"priority":1,"source_tasks":["original-source"],"custom":{"keep":7}};
    create.mission_config = Some(config.clone());
    success(&create_generate::create(&root.0, store.as_ref(), &runtime, &create).await);
    let created = store.task();
    assert_eq!(created.get_str("status"), Ok("pending"));
    assert_eq!(created.get_bool("isRemote"), Ok(false));
    assert_eq!(created.get_bool("targetRemote"), Ok(true));
    let mut start = StartRequest::new("task");
    start.remote = Some(true);
    start.expected_remote = Some(false);
    start.expected_task_id = Some(created.get_object_id("_id").unwrap().to_hex());
    start.notification.no_notify = true;
    success(&core::start(&root.0, store.as_ref(), &runtime, &start).await);
    assert_eq!(*runtime.absence_hosts.lock().unwrap(), vec![false, true]);
    assert_eq!(runtime.launches(), 1);
    assert_eq!(store.task().get_bool("isRemote"), Ok(true));
    assert_eq!(store.task().get_bool("targetRemote"), Ok(true));
    assert_eq!(runtime.commands.lock().unwrap()[0].argv[1], "scripts/start_priority_task.sh");
    let saved = store.task();
    let actual = saved.get_document("mission_config").unwrap();
    for (key, value) in &config { assert_eq!(actual.get(key), Some(value)); }
    assert!(store.0.lock().unwrap().local_rows.is_empty());
    assert!(store.0.lock().unwrap().remote_rows.is_empty());
}

#[path = "support/original_professional_runtime.rs"]
mod original_professional_runtime;

#[tokio::test]
async fn public_start_original_checker_recognizes_same_run_without_claim_launch_or_config_lie() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::OriginalOperationsRuntime};
    use serde_json::json;
    for remote in [false,true] {
        for case in ["different","unverified","ensure","equal","legacy_inline","legacy_absent","caller_echo_only"] {
            let fixture=original_professional_runtime::original_professional_fixture(false);
            let root=Path::new(&fixture.config.env.working_dir);
            std::fs::write(root.join("state"),"active").unwrap();
            let (_old_root,store,_fake,mut req)=setup("super",remote);
            req.name="fixture".into();req.notification.no_notify=true;
            let id=store.task().get_object_id("_id").unwrap().to_hex();
            let input=json!({"max_concurrent":1,"opaque":[1,2]});
            let loaded=json!({"max_concurrent":1,"opaque":[1,2],"startup_binding":{
                "taskId":id,"name":"fixture","remote":remote,"launchAttemptId":"old-launch","consumer_run_id":"same-run","auth_profile":"user1"}});
            let config=if matches!(case,"unverified"|"legacy_absent"|"caller_echo_only"){"/mutable/old.json".into()}else{loaded.to_string()};
            let argv=vec!["python".to_string(),"-m".into(),"scripts.super_alpha_simulate_param".into(),"--task".into(),"fixture".into(),
                "--dist".into(),if remote{"remote".into()}else{"local".into()},"--auth_profile".into(),"user1".into(),"--config".into(),config];
            std::fs::write(root.join("psutil.py"),format!(r#"STATUS_ZOMBIE='zombie'
ARGV={argv}
import os
class NoSuchProcess(Exception): pass
class Process:
 def __init__(self,pid):
  self.pid=pid
  if pid not in (1001,1002): raise NoSuchProcess(pid)
 def create_time(self): return 1700000010.0 if self.pid==1002 else 1700000000.0
 def cwd(self): return os.getcwd()
 def cmdline(self): return ARGV if self.pid==1002 else ['bash']
 def is_running(self): return True
 def status(self): return 'running'
"#,argv=serde_json::to_string(&argv).unwrap())).unwrap();
            {let mut d=store.0.lock().unwrap();let t=d.local.last_mut().unwrap();
             t.insert("name","fixture");t.remove("tmuxContext");
             t.insert("status",if case=="legacy_inline"{"launch_unknown"}else if case=="ensure"{"completed_with_errors"}else{"running"});
             t.insert("startConfig",doc!{"mutable_saved_value":"not_loaded_proof"});
             if !matches!(case,"legacy_inline"|"caller_echo_only") {
                 t.insert("simulatorRuntime",doc!{"pid":1002,"pidCreatedAt":1700000010.0,"consumer_run_id":"same-run",
                    "startupRecovery":{"taskId":&id,"remote":remote,"consumer_run_id":"same-run","launchAttemptId":"old-launch"}});
             }
             if case=="legacy_absent" {
                 t.remove("launchAttemptId");
                 t.get_document_mut("simulatorRuntime").unwrap().get_document_mut("startupRecovery").unwrap().remove("launchAttemptId");
             }
             if remote {let task=t.clone();d.remote.push(task);}
            }
            req.config_json=if matches!(case,"ensure"|"legacy_inline"|"legacy_absent"|"caller_echo_only"){None}
                else if case=="different"{Some(json!({"max_concurrent":2,"opaque":[1,2]}).to_string())}else{Some(input.to_string())};
            let before=store.task();let binding=validate_backend_binding(&fixture.config).await.unwrap();
            let runtime=OriginalOperationsRuntime{binding:&binding,observer:None};
            let out=core::start(root,store.as_ref(),&runtime,&req).await;
            // User contract now permits non-destructive exact legacy scope. The
            // caller echo still never becomes independent identity evidence.
            let satisfied=true;
            assert_eq!(out.exit_code,if satisfied{0}else{1},"{remote}/{case}: {}",out.value);
            assert_eq!(out.value["startedNew"],false);
            assert_eq!(out.value["configApplied"],false);
            if satisfied {
                assert_eq!(out.value["outcome"],"already_running");assert_eq!(out.value["consumerRunning"],true);
                assert_eq!(out.value["operationSettled"],true);assert_eq!(out.value["taskType"],"super");assert_eq!(out.value["authProfile"],"user1");
                assert!(out.value.get("launchAttemptId").is_some());
                if case=="legacy_absent" {assert!(out.value["launchAttemptId"].is_null());}
                assert_eq!(out.value["configComparison"],"not_compared");
                assert_eq!(out.value["requestSatisfied"],true);
                assert_eq!(out.value["executionObservation"]["configComparison"],"not_requested", "original checker must not receive a config comparison request");
            }else{assert_eq!(out.value["outcome"],"refused");}
            if case=="caller_echo_only" {
                assert_eq!(out.value["executionObservation"]["identityVerified"],false);
                assert_eq!(out.value["executionObservation"]["consumerScopeVerified"],true);
            }
            if case=="legacy_inline" {
                assert_eq!(out.value["controlReconciled"],true);
                let saved=store.task();assert_eq!(saved.get_str("status"),Ok("running"));
                assert_eq!(saved.get("launchSettledAttemptId"),saved.get("launchAttemptId"));
                for key in ["_id","launchAttemptId","simulatorRuntime","startConfig","mission_config","tmuxContext"] {
                    assert_eq!(saved.get(key),before.get(key),"never overwrite original launch/runtime/config");
                }
            }else{assert_eq!(store.task(),before,"never overwrite original launch/runtime/config");}
            let d=store.0.lock().unwrap();assert!(!d.calls.iter().any(|c|c.contains("runtime-register")||c.starts_with("start:")));
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();
            assert!(!trace.contains("new-session")&&!trace.contains("if-shell"));
        }
    }
}

#[tokio::test]
async fn owned_residue_uses_original_stop_under_same_lock_then_one_original_launch() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::OriginalOperationsRuntime};
    use serde_json::Value;
    struct Professional<'a>{ original:OriginalOperationsRuntime<'a>, config_fixture:Runtime }
    #[async_trait]
    impl StartRuntime for Professional<'_> {
        async fn inspect_execution(&self,r:&Path,s:&dyn TaskStore,t:&Document,h:bool,c:Option<&Document>,d:std::time::Duration)->Result<Option<Value>,String>{StartRuntime::inspect_execution(&self.original,r,s,t,h,c,d).await}
        async fn verify_stopped(&self,r:&Path,t:&Document,d:std::time::Duration)->Result<Option<quantnight_task_core::pause::StopOutput>,String>{StartRuntime::verify_stopped(&self.original,r,t,d).await}
        async fn stop_owned_residue(&self,r:&Path,t:&Document,d:std::time::Duration)->Result<quantnight_task_core::pause::StopOutput,String>{self.original.stop_owned_residue(r,t,d).await}
        async fn priority_config(&self,r:&Path,t:&Document,c:Option<Document>)->Result<Document,String>{self.config_fixture.priority_config(r,t,c).await}
        async fn validate_super_config(&self,r:&Path,c:&Document)->Result<(),String>{self.config_fixture.validate_super_config(r,c).await}
        async fn bind_regular_source(&self,r:&Path,t:&Document,c:&Document)->Result<Document,String>{self.config_fixture.bind_regular_source(r,t,c).await}
        async fn verify_priority_extension(&self,r:&Path,t:&Document,l:u64,n:u64)->Result<Document,String>{self.config_fixture.verify_priority_extension(r,t,l,n).await}
        async fn continuation(&self,r:&Path,t:&Document,h:bool)->Result<Option<Document>,String>{self.config_fixture.continuation(r,t,h).await}
        async fn preflight_notification(&self,r:&Path,s:&Document)->Result<(),String>{self.config_fixture.preflight_notification(r,s).await}
        async fn assert_consumer_absent(&self,r:&Path,t:&Document,h:bool)->Result<(),String>{StartRuntime::assert_consumer_absent(&self.original,r,t,h).await}
        async fn validate_execution_binding(&self,r:&Path,b:&Document,h:bool)->Result<(),String>{self.original.validate_execution_binding(r,b,h).await}
        async fn launch(&self,r:&Path,c:&StartCommand)->Result<LaunchOutput,String>{self.original.launch(r,c).await}
        async fn attach_watcher(&self,r:&Path,t:&Document,h:bool,s:&Document)->Result<Document,String>{StartRuntime::attach_watcher(&self.original,r,t,h,s).await}
    }
    for remote in [false,true] { for dead in [false,true] { for successor in [false,true] {
        if dead && successor {continue;}
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);
        std::fs::write(root.join("state"),if dead{"dead-residue"}else{"residue"}).unwrap();
        let native=root.join("native-tmux");let source=std::fs::read_to_string(&native).unwrap();
        let source=source.lines().map(|line|if line.starts_with("elif a[0]=='list-panes':") {
            "elif a[0]=='list-panes':\n dead=state.read_text()=='dead-residue'\n if a[-1]=='#{session_id}|#{pane_pid}|#{pane_dead}': print('$4|1001|'+str(int(dead)))\n elif a[-1]=='#{session_id}|#{pane_pid}': print('$4|1001')\n else: print('$4|0|0|1001|bash|'+str(int(dead)))"
        }else{line}).collect::<Vec<_>>().join("\n");std::fs::write(&native,source).unwrap();
        let ps=root.join("bin/ps");let source=std::fs::read_to_string(&ps).unwrap()
            .replace("if [[ -f \"$STATE\" ]]; then", "if [[ -f \"$STATE\" && $(cat \"$STATE\") != dead-residue ]]; then")
            .replace("echo \"1002 1001 python\"", "if [[ $(cat \"$STATE\") == active ]]; then echo '1002 1001 python'; else echo '1002 1001 sleep'; fi");
        std::fs::write(ps,source).unwrap();
        std::fs::write(root.join("psutil.py"),r#"import os
from pathlib import Path
STATUS_ZOMBIE='zombie'
class NoSuchProcess(Exception): pass
class Process:
 def __init__(self,pid):
  self.pid=pid
  if not Path(os.environ['STATE']).exists(): raise NoSuchProcess(pid)
 def create_time(self): return 1700000001.0
 def cmdline(self): return ['bash'] if self.pid==1001 else ['sleep','9999999']
 def is_running(self): return Path(os.environ['STATE']).exists()
 def status(self): return 'running'
"#).unwrap();
        let (_unused,store,config_fixture,mut req)=setup("super",remote);
        req.name="fixture".into();req.notification.no_notify=true;
        let id=store.task().get_object_id("_id").unwrap().to_hex();
        store.mutate(doc!{"name":"fixture","status":"completed_with_errors","startConfig":{"max_concurrent":1},
            "tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":native.canonicalize().unwrap().to_string_lossy().as_ref(),
                "sessionId":"$4","sessionCreatedAt":1700000000000_i64,"taskId":&id,"launchAttemptId":"old-launch","remote":remote}});
        if remote {let t=store.task();store.0.lock().unwrap().remote.push(t);}
        if successor {store.0.lock().unwrap().mutate_after.insert("pause:paused".into(),doc!{"runId":"successor-mission"});}
        let rows=store.0.lock().unwrap().local_rows.clone();
        let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let runtime=Professional{original:OriginalOperationsRuntime{binding:&binding,observer:None},config_fixture};
        let out=core::start(root,store.as_ref(),&runtime,&req).await;
        if successor {
            assert_eq!(out.exit_code,1,"{}",out.value);
            assert_eq!(store.task().get_str("runId").unwrap(),"successor-mission");
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();
            assert!(!trace.contains("new-session"));
            assert_eq!(trace.lines().filter(|l|l.contains("if-shell")).count(),1);
            assert!(!store.0.lock().unwrap().calls.iter().any(|c|c=="start:launch_unknown"));
            continue;
        }
        assert_eq!(out.exit_code,0,"remote={remote} dead={dead}: {}",out.value);
        assert_eq!(out.value["startedNew"],true);
        assert_ne!(store.task().get_str("launchAttemptId").unwrap(),"old-launch");
        assert_eq!(store.0.lock().unwrap().local_rows,rows,"no accepted receipt reset/rePOST permission");
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),1);
        assert_eq!(trace.lines().filter(|l|l.contains("if-shell")).count(),1);
        if dead {assert!(!trace.contains("send-keys"));}
        let d=store.0.lock().unwrap();
        assert_eq!(d.calls.iter().filter(|c|*c=="pause:paused").count(),1);
        assert_eq!(d.calls.iter().filter(|c|*c=="start:launch_unknown").count(),1);
    }}}
}


fn completion_original_super_preflight(root:&Path) {
    // Lowest process fixture for the unrelated config-owner response; lifecycle,
    // binding, launch/check shell, and all task/launch CAS remain original.
    let p=root.join(".venv/bin/python");let source=std::fs::read_to_string(&p).unwrap();
    std::fs::write(p,source.replace("set -euo pipefail",r#"set -euo pipefail
if [[ "${3:-}" == util.super_capacity ]]; then
 echo '{"schemaVersion":1,"command":"native-config-preflight","ok":true,"result":{}}'; exit 0
fi"#)).unwrap();
}

#[tokio::test]
async fn completion_public_facade_unknown_launch_blocks_late_original_ssh_helper() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    use std::os::unix::net::UnixDatagram;
    let fixture=original_professional_runtime::original_professional_fixture(false);
    let root=Path::new(&fixture.config.env.working_dir);
    completion_original_super_preflight(root);
    let notify=UnixDatagram::bind(root.join("done.sock")).unwrap();
    notify.set_read_timeout(Some(std::time::Duration::from_secs(12))).unwrap();
    let ssh=root.join("bin/ssh");let old=std::fs::read_to_string(&ssh).unwrap();
    let delayed=r#"
if sys.argv[-2:]==['bash','-se']:
 from pathlib import Path
 root=Path(os.environ['STATE']).parent
 payload=sys.stdin.buffer.read()
 (root/'late-payload').write_bytes(payload)
 os.mkfifo(root/'release')
 child=r'''import os,sys,selectors,subprocess,socket
from pathlib import Path
root=Path(sys.argv[1]);fd=os.open(root/'release',os.O_RDONLY|os.O_NONBLOCK)
s=selectors.DefaultSelector();s.register(fd,selectors.EVENT_READ)
print('ready',flush=True)
ready=s.select(8)
if ready:
 os.read(fd,1)
 result=subprocess.run(['/bin/bash','-se'],input=(root/'late-payload').read_bytes(),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=6).returncode
else: result=98
sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.sendto(str(result).encode(),str(root/'done.sock'))
'''
 p=subprocess.Popen([sys.executable,'-B','-c',child,str(root)],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True)
 assert p.stdout.readline()==b'ready\n'
 (root/'late-helper.pid').write_text(str(p.pid))
 sys.exit(255)
"#;
    std::fs::write(&ssh,old.replace("if os.environ.get(\"SSH_FAIL\")",&format!("{delayed}\nif os.environ.get(\"SSH_FAIL\")"))).unwrap();
    let (_unused,store,_runtime,mut request)=setup("super",true);
    request.name="fixture".into();request.notification.no_notify=true;
    store.mutate(doc!{"name":"fixture"});
    let binding=validate_backend_binding(&fixture.config).await.unwrap();
    let control=TaskControl::with_store(binding,store.clone());
    let first=control.start(&request,None).await;failed(&first);
    let before=store.task();assert_eq!(before.get_str("status"),Ok("launch_unknown"));
    assert!(root.join("late-helper.pid").exists(),"{}",first.value);
    assert!(!root.join("state").exists());
    let second=control.start(&request,None).await;failed(&second);
    assert!(second.value.to_string().contains("settlement"),"{}",second.value);
    assert_eq!(store.task(),before,"unresolved old attempt must not be overwritten");
    assert_eq!(store.0.lock().unwrap().calls.iter().filter(|c|*c=="start:launch_unknown").count(),1);
    // Release the ORIGINAL SSH child only. It may still launch; this test proves
    // that no successor was admitted while its outcome was unknown.
    use std::io::Write;
    std::fs::OpenOptions::new().write(true).open(root.join("release")).unwrap().write_all(b"x").unwrap();
    let mut msg=[0;16];let count=notify.recv(&mut msg).unwrap();assert_eq!(&msg[..count],b"0");
    assert_eq!(store.task(),before);
    let trace=std::fs::read_to_string(root.join("trace")).unwrap();
    assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),1);
    assert!(!trace.contains("if-shell"));
}

#[tokio::test]
async fn completion_public_facade_settled_original_launcher_can_recover_absence() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    for remote in [false,true] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);completion_original_super_preflight(root);
        let (_unused,store,_runtime,mut request)=setup("super",remote);
        request.name="fixture".into();request.notification.no_notify=true;store.mutate(doc!{"name":"fixture"});
        store.0.lock().unwrap().fail_before.insert("start:running".into());
        let binding=validate_backend_binding(&fixture.config).await.unwrap();let control=TaskControl::with_store(binding,store.clone());
        let first=control.start(&request,None).await;failed(&first);
        let task=store.task();assert_eq!(task.get_str("status"),Ok("launch_unknown"));
        assert_eq!(task.get("launchSettledAttemptId"),task.get("launchAttemptId"),"{}",first.value);
        std::fs::remove_file(root.join("state")).unwrap();
        store.0.lock().unwrap().fail_before.remove("start:running");
        let recovered=control.start(&request,None).await;success(&recovered);
        for key in ["task","taskId","remote","taskType","authProfile","launchAttemptId","operationSettled"] {
            assert!(recovered.value.get(key).is_some(),"missing {key}: {}",recovered.value);
        }
        assert_eq!(recovered.value["operationSettled"],true);
        assert_eq!(recovered.value["taskType"],"super");assert_eq!(recovered.value["authProfile"],"user1");
        assert_ne!(store.task().get("launchAttemptId"),task.get("launchAttemptId"));
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),2);
    }
}

#[tokio::test]
async fn completion_original_start_preserves_launcher120_without_total120() {
    let (root,store,runtime,request)=setup("regular",false);
    let out=core::start(&root.0,store.as_ref(),&runtime,&request).await;success(&out);
    assert_eq!(runtime.commands.lock().unwrap()[0].timeout_seconds,120);
    assert_eq!(out.value["operationSettled"],true);assert_eq!(out.value["taskType"],"regular");
    assert_eq!(out.value["authProfile"],"user1");
    assert_eq!(out.value["launchAttemptId"].as_str(),store.task().get_str("launchAttemptId").ok());
}

#[tokio::test]
async fn completion_legacy_unknown_pause_v2_preserves_pending_launcher_fence() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl,pause::{PauseOptions,StopBudget}};
    for remote in [false,true] {for case in ["legacy_unknown","legacy_normal","settled_unknown"] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);completion_original_super_preflight(root);
        let (_unused,store,_runtime,mut request)=setup("super",remote);
        request.name="fixture".into();request.notification.no_notify=true;
        {let mut d=store.0.lock().unwrap();let task=d.local.last_mut().unwrap();
         task.insert("name","fixture");task.remove("tmuxContext");
         task.insert("status",if case=="legacy_normal"{"running"}else{"launch_unknown"});
         if case=="settled_unknown" {task.insert("launchSettledAttemptId","old-launch");}}
        let binding=validate_backend_binding(&fixture.config).await.unwrap();let control=TaskControl::with_store(binding,store.clone());
        let out=control.pause("fixture",PauseOptions::default(),StopBudget::Cli,None).await;
        assert_eq!(out.exit_code,0,"{remote}/{case}: {}",out.value);
        assert_eq!(out.value["confirmationBasis"],"session_absent");
        let paused=store.task();assert_eq!(paused.get_str("status"),Ok("paused"));
        if case=="legacy_unknown" {assert_eq!(paused.get("launchSettledAttemptId"),Some(&Bson::Null));}
        if case=="legacy_normal" {assert!(!paused.contains_key("launchSettledAttemptId"));}
        let result=control.start(&request,None).await;
        if case=="legacy_unknown" {
            failed(&result);assert_eq!(store.task(),paused);
            assert!(!store.0.lock().unwrap().calls.iter().any(|c|c=="start:launch_unknown"));
            let archive=control.delete("fixture",None,None,None).await;failed(&archive);
            assert!(archive.value.to_string().contains("launcher remains unresolved"),"{}",archive.value);
            assert_eq!(store.task(),paused);
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("new-session")&&!trace.contains("if-shell"));
        } else {success(&result);assert_eq!(result.value["startedNew"],true);}
    }}
}

#[tokio::test]
async fn completion_original_facade_claimed_but_not_dispatched_can_settle_without_retry() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    for remote in [false,true] {for case in ["persisted","persist_failure","replacement"] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);completion_original_super_preflight(root);
        let refuse=root.join("refuse-validation");std::fs::write(&refuse,"true").unwrap();
        let p=root.join(".venv/bin/python");let source=std::fs::read_to_string(&p).unwrap();
        std::fs::write(&p,source.replace("set -euo pipefail",&format!(r#"set -euo pipefail
if [[ "${{3:-}}" == scripts.script_config && -f '{refuse}' ]]; then
 echo '{{"schemaVersion":1,"command":"validate-execution-binding","ok":false}}'; exit 0
fi"#,refuse=refuse.display()))).unwrap();
        let (_unused,store,_runtime,mut request)=setup("super",remote);
        request.name="fixture".into();request.notification.no_notify=true;store.mutate(doc!{"name":"fixture"});
        request.expected_task_id=Some(store.task().get_object_id("_id").unwrap().to_hex());
        if case=="persist_failure" {store.0.lock().unwrap().fail_before.insert("start-not-dispatched:ok".into());}
        if case=="replacement" {store.0.lock().unwrap().mutate_before.insert("start-not-dispatched:ok".into(),doc!{"_id":ObjectId::new(),"launchAttemptId":"successor"});}
        let binding=validate_backend_binding(&fixture.config).await.unwrap();let control=TaskControl::with_store(binding,store.clone());
        let first=control.start(&request,None).await;failed(&first);
        assert_ne!(first.value["operationSettled"],true,"failed response must not fabricate join proof");
        let saved=store.task();assert_eq!(saved.get_str("status"),Ok("launch_unknown"));
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("new-session")&&!trace.contains("if-shell"));
        assert_eq!(store.0.lock().unwrap().calls.iter().filter(|c|*c=="start:launch_unknown").count(),1);
        if case=="persisted" {
            assert_eq!(saved.get("launchSettledAttemptId"),saved.get("launchAttemptId"));
            assert!(saved.get_array("events").unwrap().iter().any(|e|e.as_document().is_some_and(|d|d.get_str("command")==Ok("start-not-dispatched") && d.get_document("detail").unwrap().get_bool("launcherInvoked")==Ok(false))));
        } else {assert_eq!(saved.get("launchSettledAttemptId"),Some(&Bson::Null));}
        std::fs::remove_file(&refuse).unwrap();
        store.0.lock().unwrap().fail_before.remove("start-not-dispatched:ok");
        let next=control.start(&request,None).await;
        if case=="persisted" {success(&next);assert_eq!(next.value["startedNew"],true);assert_ne!(store.task().get("launchAttemptId"),saved.get("launchAttemptId"));}
        else {failed(&next);assert_eq!(store.task(),saved);}
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),usize::from(case=="persisted"));
    }}
}

#[tokio::test]
async fn paused_restart_uses_explicit_new_config_without_comparing_to_previous_run() {
    for kind in ["regular", "super", "priority"] {
        for remote in [false, true] {
            let (root, store, runtime, mut request) = setup(kind, remote);
            store.mutate(doc! {"startConfig":{"old_only":true,"max_concurrent":1},
                "mission_config":{"old_only":true,"max_concurrent":1}});
            let mut input = doc! {"max_concurrent":3,"max_multi_simulation_children":1,
                "custom":{"user_choice":[true,"new",null]}};
            if kind == "priority" {
                input.insert("mission_tag", "pipeline");
                input.insert("source_remote", !remote); // original root scope, not CP host
            }
            request.config_json = Some(serde_json::to_string(&input).unwrap());
            request.remote = Some(remote);
            let before_local = store.0.lock().unwrap().local_rows.clone();
            let before_remote = store.0.lock().unwrap().remote_rows.clone();
            let result = core::start(&root.0, store.as_ref(), &runtime, &request).await;
            success(&result);
            assert_eq!(runtime.launches(),1);
            let task=store.task();let actual=task.get_document("startConfig").unwrap();
            for (key,value) in &input { assert_eq!(actual.get(key),Some(value),"{kind}/{remote}/{key}"); }
            assert!(!actual.contains_key("old_only"));
            assert_eq!(store.0.lock().unwrap().local_rows,before_local);
            assert_eq!(store.0.lock().unwrap().remote_rows,before_remote);
        }
    }
}

#[tokio::test]
async fn shared_start_legacy_exact_consumer_reconciles_without_config_or_duplicate_launch() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl,
        pause::{PauseOptions,StopBudget}};
    for kind in ["regular","super","priority"] { for remote in [false,true] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);
        std::fs::write(root.join("state"),"active").unwrap();
        let (_unused,store,_runtime,mut request)=setup(kind,remote);
        request.name="fixture".into();request.notification.no_notify=true;
        request.config_json=Some("not even JSON: must never parse/apply on already-running".into());
        {let mut d=store.0.lock().unwrap();let task=d.local.last_mut().unwrap();
         task.insert("name","fixture");task.insert("status","launch_unknown");
         task.insert("launchSettledAttemptId",Bson::Null);task.remove("tmuxContext");
         task.insert("mission_config",doc!{"saved":"unchanged"});}
        let id=store.task().get_object_id("_id").unwrap().to_hex();
        original_professional_runtime::install_original_super_process(root,&id,"fixture",remote,"old-launch");
        let process=root.join("psutil.py");
        let source=std::fs::read_to_string(&process).unwrap();
        let module=match kind {"regular"=>"scripts.alpha_simulate_param","super"=>"scripts.super_alpha_simulate_param",_=>"scripts.refine_simulate_param"};
        let argv=serde_json::json!(["python","-m",module,"--task","fixture","--dist",if remote{"remote"}else{"local"},
            "--auth_profile","user1","--config",if kind=="priority"{"tmp/tmp_config_fixture.json"}else{"{}"}]);
        let source=source.lines().map(|line|if line.starts_with("ARGV="){format!("ARGV={argv}")}else{line.to_string()}).collect::<Vec<_>>().join("\n");
        std::fs::write(process,source).unwrap();
        let before=store.task();
        let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let control=TaskControl::with_store(binding,store.clone());
        // An existing partial publication is not itself a compatibility gate.
        store.mutate(doc!{"consumer_run_id":"legacy-expected-not-observed","simulatorRuntime":{"configDigestVersion":1}});
        if remote {let row=store.task();store.0.lock().unwrap().remote.push(row);}
        for _ in 0..2 {
            let result=control.start(&request,None).await;success(&result);
            assert_eq!(result.value["outcome"],"already_running","{kind}/{remote}: {}",result.value);
            assert_eq!(result.value["startedNew"],false);assert_eq!(result.value["configApplied"],false);
            assert_eq!(result.value["consumerRunning"],true);assert_eq!(result.value["operationSettled"],true,"this non-launching RPC is completed");
            assert_eq!(store.task().get("launchSettledAttemptId"),Some(&Bson::Null),"current RPC completion must not settle historical launch");
            assert_eq!(result.value["executionObservation"]["identityVerified"],false,"never invent independent launch identity");
            assert_eq!(result.value["executionObservation"]["configComparison"],"not_requested");
            if let Ok(directory)=std::env::var("QUANTNIGHT_TEST_START_WIRE") {
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(Path::new(&directory).join(format!("{kind}-{remote}.json")),result.value.to_string()).unwrap();
            }
        }
        let saved=store.task();assert_eq!(saved.get_str("status"),Ok("running"));
        assert_eq!(saved.get("launchAttemptId"),before.get("launchAttemptId"));
        assert_eq!(saved.get("launchSettledAttemptId"),Some(&Bson::Null),"scope must not settle a lost launch");
        assert_eq!(saved.get("mission_config"),before.get("mission_config"));
        assert_eq!(saved.get("tmuxContext"),None,"do not turn echoed context into future independent launch proof");
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("new-session")&&!trace.contains("if-shell"));
        let pause=control.pause("fixture",PauseOptions::default(),StopBudget::Cli,None).await;
        assert_eq!(pause.exit_code,0,"{}",pause.value);assert_eq!(store.task().get_str("status"),Ok("paused"));
        assert_eq!(store.task().get("launchSettledAttemptId"),store.task().get("launchAttemptId"),"only original confirmed pause closes historical launch");
    }}
}

#[tokio::test]
async fn shared_start_strong_original_consumer_recovers_lost_control_ack_through_cas() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    for remote in [false,true] {for fault in ["none","lost_ack","replacement"] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);
        std::fs::write(root.join("state"),"active").unwrap();
        let (_unused,store,_runtime,mut request)=setup("super",remote);
        request.name="fixture".into();request.notification.no_notify=true;
        {let mut d=store.0.lock().unwrap();let task=d.local.last_mut().unwrap();
         task.insert("name","fixture");task.insert("status","launch_unknown");
         task.insert("launchSettledAttemptId",Bson::Null);task.remove("tmuxContext");
         task.insert("mission_config",doc!{"saved":"unchanged"});}
        let id=store.task().get_object_id("_id").unwrap().to_hex();
        original_professional_runtime::install_original_super_process(root,&id,"fixture",remote,"old-launch");
        if fault=="lost_ack" {store.0.lock().unwrap().fail_after.insert("start-reconciled:running".into());}
        if fault=="replacement" {store.0.lock().unwrap().mutate_before.insert("start-reconciled:running".into(),doc!{"launchAttemptId":"successor"});}
        let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let control=TaskControl::with_store(binding,store.clone());
        let result=control.start(&request,None).await;
        if fault=="none" {success(&result);}else{failed(&result);assert_ne!(result.value["operationSettled"],true,"failed current control CAS is not settled");}
        if fault=="replacement" {
            assert_eq!(store.task().get_str("launchAttemptId"),Ok("successor"));
            assert_eq!(store.task().get_str("status"),Ok("launch_unknown"));
        }else{
            let repeat=control.start(&request,None).await;success(&repeat);
            assert_eq!(repeat.value["startedNew"],false);assert_eq!(repeat.value["configApplied"],false);
            assert_eq!(repeat.value["operationSettled"],true);
            let saved=store.task();assert_eq!(saved.get_str("status"),Ok("running"));
            assert_eq!(saved.get("launchSettledAttemptId"),saved.get("launchAttemptId"));
        }
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("new-session")&&!trace.contains("if-shell"));
    }}
}

#[tokio::test]
async fn shared_start_actual_priority_file_launcher_fresh_lost_ack_and_repeated_start() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl,
        create_generate::CreateRequest,pause::{PauseOptions,StopBudget}};
    struct RestorePath(Option<std::ffi::OsString>);
    impl Drop for RestorePath {
        fn drop(&mut self) { if let Some(value)=&self.0 { std::env::set_var("PATH",value); } else {std::env::remove_var("PATH");} }
    }
    // Gate executes native tests on one thread; only the actual uv OS boundary
    // is selected via PATH. No original launcher/config/checker is stubbed.
    for kind in ["regular","super","priority"] {for remote in [false,true] {
      let cases:&[&str]=if kind=="priority"{&["none","launcher","control","direct_pause","v2_only","pause_launch_race","pause_host_race","pause_ack_lost","nextafter_stop"]}else{&["launcher","direct_pause"]};
      for &lost in cases {
        let mut fixture=original_professional_runtime::original_professional_fixture(false);
        fixture.config.mongodb.databases.mission="simulation_mission".into();
        fixture.config.mongodb.databases.simulation="simulation_db".into();
        let root=Path::new(&fixture.config.env.working_dir);
        let backend=std::env::var("QUANTNIGHT_BACKEND_SOURCE").unwrap();
        let python=std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap();
        let support=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/priority_start_os.py");
        let fixture_setup=std::process::Command::new(&python).args(["-B"]).arg(support).arg(root).arg(&backend)
            .env("PYTHONPATH",&backend).output().unwrap();
        assert!(fixture_setup.status.success(),"{}",String::from_utf8_lossy(&fixture_setup.stderr));
        let _path=RestorePath(std::env::var_os("PATH"));
        std::env::set_var("PATH",format!("{}:/bin:/usr/bin",root.join("bin").display()));
        let store=if kind=="priority"{Arc::new(Store::default())}else{setup(kind,remote).1};
        if kind!="priority" {store.mutate(doc!{"name":"fixture","status":"ready","tmuxContext":Bson::Null});}
        {let mut state=store.0.lock().unwrap();state.original_control_file=Some(root.join("original-control.json"));Store::publish_original_control(&state);}
        let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let control=TaskControl::with_store(binding,store.clone());
        if kind=="priority" {
            let mut create=CreateRequest::new("fixture","pipeline");create.task_type="priority".into();create.target_remote=remote;
            create.mission_config=Some(doc!{"mission_tag":"pipeline","refine_templates":[],"rounds":{"enabled":false},"interval":37});
            let created=control.create(&create,None).await;success(&created);
        }
        let original_rows={let state=store.0.lock().unwrap();(state.local_rows.clone(),state.remote_rows.clone())};
        let original=store.task();
        if !matches!(lost,"none"|"control") {std::fs::write(root.join("lose-launch-ack"),"injected lowest-OS lost ACK").unwrap();}
        if lost=="control" {store.0.lock().unwrap().fail_after.insert("start:running".into());}
        let mut request=StartRequest::new("fixture");request.remote=Some(remote);request.notification.no_notify=true;
        request.expected_task_id=Some(original.get_object_id("_id").unwrap().to_hex());
        let first=control.start(&request,None).await;
        if lost=="none" {success(&first);assert_eq!(first.value["startedNew"],true);}
        else {failed(&first);}
        assert!(root.join("launched-argv.json").exists(),"{}",first.value);
        let claimed=store.task();assert_ne!(claimed.get("launchAttemptId"),original.get("launchAttemptId"));
        assert_eq!(claimed.get_object_id("_id"),original.get_object_id("_id"));
        let config_path=root.join("tmp/tmp_config_fixture.json");
        assert!(config_path.exists(),"real original launcher must publish its file config");
        let bytes=std::fs::read(&config_path).unwrap();
        let argv:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("launched-argv.json")).unwrap()).unwrap();
        let args=argv.as_array().unwrap();
        let option=|key:&str|args[args.iter().position(|v|v==key).unwrap()+1].as_str().unwrap();
        assert_eq!(option("--task"),"fixture");assert_eq!(option("--dist"),if remote{"remote"}else{"local"});
        // Mutable file changed AFTER launch. Neither it nor new supplied config
        // may be mistaken for loaded data or used as already-running admission.
        std::fs::write(&config_path,"not the loaded config anymore").unwrap();
        request.config_json=Some("invalid submitted JSON is not used by ensure".into());
        for _ in 0..if lost=="direct_pause"{0}else{2} {
            let repeat=control.start(&request,None).await;success(&repeat);
            assert_eq!(repeat.value["outcome"],"already_running");
            assert_eq!(repeat.value["startedNew"],false);assert_eq!(repeat.value["configApplied"],false);
            assert_eq!(repeat.value["consumerRunning"],true);
        }
        let saved=store.task();assert_eq!(saved.get_str("status"),Ok(if lost=="direct_pause"{"launch_unknown"}else{"running"}));
        if lost=="direct_pause" {assert!(!saved.get_array("events").unwrap().iter().any(|e|e.as_document().unwrap().get_str("command")==Ok("start-reconciled")));}
        assert_eq!(saved.get("launchAttemptId"),claimed.get("launchAttemptId"));
        if kind=="priority" {assert!(store.0.lock().unwrap().remote.is_empty(),"no new Priority registry");}
        {let state=store.0.lock().unwrap();assert_eq!((state.local_rows.clone(),state.remote_rows.clone()),original_rows,"accepted rows unchanged, no POST/reset");}
        assert_ne!(std::fs::read(&config_path).unwrap(),bytes,"start must not republish config");
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),1,"no duplicate launcher");
        assert!(!trace.contains("if-shell"),"START never stops existing consumer");
        if lost=="v2_only" {std::fs::remove_file(root.join("state")).unwrap();}
        if lost=="pause_launch_race" {store.0.lock().unwrap().mutate_before.insert("pause:paused".into(),doc!{"launchAttemptId":"successor"});}
        if lost=="pause_host_race" {store.0.lock().unwrap().mutate_before.insert("pause:paused".into(),doc!{"isRemote":!remote});}
        if lost=="pause_ack_lost" {store.0.lock().unwrap().fail_after.insert("pause:paused".into());}
        if lost=="nextafter_stop" {std::fs::write(root.join("nextafter-on-stop"),"next representable birth, not same process").unwrap();}
        let pause=control.pause("fixture",PauseOptions::default(),StopBudget::Cli,None).await;
        if matches!(lost,"pause_launch_race"|"pause_host_race"|"nextafter_stop") {
            assert_eq!(pause.exit_code,1,"{}",pause.value);
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();
            assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),1);
            if lost=="nextafter_stop"{assert!(!trace.contains("if-shell"),"one-ULP changed birth must reject BEFORE signal");}
            continue;
        }
        assert_eq!(pause.exit_code,0,"{}",pause.value);
        assert_eq!(pause.value["confirmationBasis"],if lost=="v2_only"{"session_absent"}else{"runners_exited"});
        if lost!="v2_only" {
            let paused=store.task();assert_eq!(paused.get("launchSettledAttemptId"),paused.get("launchAttemptId"));
            let detail=paused.get_array("events").unwrap().last().unwrap().as_document().unwrap().get_document("detail").unwrap();
            assert_eq!(detail.get_document("controlReconciliation").unwrap().get_str("basis"),Ok("observed_consumer_stopped"));
        }
        if root.join("lose-launch-ack").exists(){std::fs::remove_file(root.join("lose-launch-ack")).unwrap();}
        request.config_json=Some(if kind=="priority"{serde_json::json!({"mission_tag":"pipeline","refine_templates":[],"rounds":{"enabled":false},"interval":93})}else{serde_json::json!({"mission_tag":"fixture","max_concurrent":1,"max_multi_simulation_children":1,"interval":93})}.to_string());
        let restarted=control.start(&request,None).await;
        if lost=="v2_only" {
            failed(&restarted);
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();
            assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),1);
            continue;
        }
        success(&restarted);
        assert_eq!(restarted.value["startedNew"],true);assert_eq!(restarted.value["configApplied"],true);
        assert_ne!(store.task().get("launchAttemptId"),claimed.get("launchAttemptId"));
        assert_eq!(store.task().get_document("mission_config").unwrap().get_i32("interval"),Ok(93));
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("new-session")).count(),2);
    }}}
}

#[tokio::test]
async fn shared_start_scope_never_adopts_known_foreign_or_replaced_original_execution() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    for kind in ["regular","super","priority"] {for remote in [false,true] {
      for fault in ["task","profile","host","cwd","module_origin","recreated_task","preexisting_consumer","registry_id","registry_profile","registry_host","registry_launch","run","pid","birth","session","pane"] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);
        std::fs::write(root.join("state"),"active").unwrap();
        let (_unused,store,_runtime,mut request)=setup(kind,remote);
        request.name="fixture".into();request.notification.no_notify=true;
        {let mut d=store.0.lock().unwrap();let task=d.local.last_mut().unwrap();
         task.insert("name","fixture");task.insert("status","launch_unknown");task.remove("tmuxContext");}
        let id=store.task().get_object_id("_id").unwrap().to_hex();
        original_professional_runtime::install_original_super_process(root,&id,"fixture",remote,"old-launch");
        let module=match kind {"regular"=>"scripts.alpha_simulate_param","super"=>"scripts.super_alpha_simulate_param",_=>"scripts.refine_simulate_param"};
        let argv=serde_json::json!(["python","-m",module,"--task",if fault=="task"{"foreign"}else{"fixture"},
          "--dist",if remote != (fault=="host"){"remote"}else{"local"},"--auth_profile",if fault=="profile"{"foreign"}else{"user1"},"--config","tmp/tmp_config_fixture.json"]);
        let process=root.join("psutil.py");let source=std::fs::read_to_string(&process).unwrap();
        std::fs::write(process,source.lines().map(|line|if line.starts_with("ARGV="){format!("ARGV={argv}")}else{line.to_string()}).collect::<Vec<_>>().join("\n")).unwrap();
        match fault {
          "cwd"=>{
            let process=root.join("psutil.py");let text=std::fs::read_to_string(&process).unwrap();
            std::fs::write(process,text.replace("def cwd(self): return str(Path(os.environ['STATE']).parent)","def cwd(self): return '/foreign/checkout'")).unwrap();
          },
          "module_origin"=>{
            let process=root.join("psutil.py");let text=std::fs::read_to_string(&process).unwrap();
            let mut args=argv.as_array().unwrap().clone();args.remove(1);args[1]=serde_json::json!("/foreign/scripts/refine_simulate_param.py");
            let argv=serde_json::Value::Array(args);
            std::fs::write(process,text.lines().map(|line|if line.starts_with("ARGV="){format!("ARGV={argv}")}else{line.to_string()}).collect::<Vec<_>>().join("\n")).unwrap();
          },
          "recreated_task"=>store.mutate(doc!{"createdAt":bson::DateTime::now()}),
          "preexisting_consumer"=>store.mutate(doc!{"launchAttemptAt":bson::DateTime::now()}),
          "run"=>store.mutate(doc!{"consumer_run_id":"known-original-run","simulatorRuntime":{"consumer_run_id":"observed-foreign-run"}}),
          "pid"=>store.mutate(doc!{"simulatorRuntime":{"pid":9999,"pidCreatedAt":1700000010.0}}),
          "birth"=>store.mutate(doc!{"simulatorRuntime":{"pid":1002,"pidCreatedAt":1700000011.0}}),
          "session"=>store.mutate(doc!{"tmuxContext":{"sessionId":"$9"}}),
          "pane"=>{
            let native=root.join("native-tmux");let source=std::fs::read_to_string(&native).unwrap();
            let replacement="".to_owned()+"elif a[0]=='list-panes':\n marker=state.parent/'pane-read'\n if marker.exists(): print('$4|0|0|9001|bash|0')\n else:\n  marker.write_text('read')\n  print('$4|0|0|1001|bash|0')";
            std::fs::write(native,source.lines().map(|line|if line.starts_with("elif a[0]=='list-panes':"){replacement.as_str()}else{line}).collect::<Vec<_>>().join("\n")).unwrap();
          }, _=>(),
        }
        if fault.starts_with("registry_") {
            let mut row=store.task();
            match fault {"registry_id"=>{row.insert("_id",ObjectId::new());},"registry_profile"=>{row.insert("auth_profile","foreign");},
              "registry_host"=>{row.insert("isRemote",!remote);},_=>{row.insert("launchAttemptId","foreign");}}
            if remote {store.0.lock().unwrap().remote.push(row);} else {
                // Local lookup sees the current control record; inject the
                // authoritative known identity through runtime instead.
                let field=match fault {"registry_id"=>"source_task_id","registry_profile"=>"auth_profile","registry_host"=>"source_remote",_=>"launchAttemptId"};
                let value=if fault=="registry_host"{Bson::Boolean(!remote)}else{Bson::String("foreign".into())};
                let mut runtime=doc!{"pid":1002,"pidCreatedAt":1700000010.0};runtime.insert(field,value);
                store.mutate(doc!{"simulatorRuntime":runtime});
            }
        }else if remote && matches!(fault,"pid"|"birth"|"run") {let row=store.task();store.0.lock().unwrap().remote.push(row);}
        let before=store.task();let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let control=TaskControl::with_store(binding,store.clone());let result=control.start(&request,None).await;
        if fault=="preexisting_consumer" {
            success(&result);assert_eq!(result.value["outcome"],"already_running");
            assert_eq!(result.value["operationSettled"],true,"completed acknowledgment is not historical launch settlement");
            assert_eq!(result.value["executionObservation"]["consumerPredatesLaunch"],true);
            let pause=control.pause("fixture",quantnight_task_core::pause::PauseOptions::default(),quantnight_task_core::pause::StopBudget::Cli,None).await;
            assert_eq!(pause.exit_code,0,"{}",pause.value);
            assert_ne!(store.task().get("launchSettledAttemptId"),store.task().get("launchAttemptId"));
            failed(&control.start(&request,None).await);
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("new-session"));
            continue;
        }
        assert_eq!(result.exit_code,1,"{kind}/{remote}/{fault}: {}",result.value);
        assert_eq!(store.task(),before,"foreign execution must not reconcile control");
        let trace=std::fs::read_to_string(root.join("trace")).unwrap_or_default();
        assert!(!trace.contains("new-session")&&!trace.contains("if-shell"),"no launch/signal on foreign or replaced primary");
      }
    }}
}

#[test]
fn original_checker_pid_birth_wire_keeps_exact_f64_identity() {
    // Python decimal/bits -> actual Rust Value parse/serialize -> Python bits.
    // No epsilon, timestamp truncation, string-wire substitute or custom parser.
    let python=std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap_or_else(|_|"python3".into());
    let probe="import json,struct,sys; x=float(sys.argv[1]); print(json.dumps({'decimal':sys.argv[1],'number':x,'bits':struct.unpack('>Q',struct.pack('>d',x))[0]}))";
    let mut rows=Vec::new();
    for raw in ["1793237748.0946019","1793234992.019803","1793237748.094602","0.1","0.30000000000000004","1.7976931348623157e308"] {
        let original=std::process::Command::new(&python).args(["-B","-c",probe,raw]).output().unwrap();
        assert!(original.status.success());
        let value:serde_json::Value=serde_json::from_slice(&original.stdout).unwrap();
        let expected=value["bits"].as_u64().unwrap();
        let number=value["number"].as_f64().unwrap();
        assert_eq!(number.to_bits(),expected,"original parser changed Python PID birth {raw}");
        let serialized=value["number"].to_string();
        let returned=std::process::Command::new(&python).args(["-B","-c",probe,&serialized]).output().unwrap();
        assert!(returned.status.success());
        let returned:serde_json::Value=serde_json::from_slice(&returned.stdout).unwrap();
        assert_eq!(returned["bits"].as_u64(),Some(expected));
        rows.push(serde_json::json!({"python":value,"rustBits":number.to_bits(),"rustSerialized":serialized,"pythonReturned":returned}));
    }
    if let Ok(path)=std::env::var("QUANTNIGHT_TEST_PID_WIRE") {std::fs::write(path,serde_json::to_string_pretty(&rows).unwrap()).unwrap();}
}
