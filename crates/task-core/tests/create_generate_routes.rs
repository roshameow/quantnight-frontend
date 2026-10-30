use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{
    create_generate::{
        self as core, CreateRequest, CreationMetadata, GenerateRequest, GeneratedOutput,
        GenerationCommand, GenerationErrorKind as Kind, GenerationFailure, GenerationRuntime,
    },
    mission_lock::MissionLocks,
    task_store::{CollectionSummary, Host, TaskStore},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

fn field<'a>(row: &'a Document, key: &str) -> Option<&'a Bson> {
    let mut parts = key.split('.');
    let mut value = row.get(parts.next()?)?;
    for part in parts {
        value = value.as_document()?.get(part)?;
    }
    Some(value)
}
fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(key, expected)| {
        let observed = field(row, key);
        if let Bson::Document(predicate) = expected {
            if let Ok(exists) = predicate.get_bool("$exists") {
                return observed.is_some() == exists;
            }
            if let Some(ne) = predicate.get("$ne") {
                return observed != Some(ne);
            }
        }
        if expected == &Bson::Null {
            observed.is_none() || observed == Some(&Bson::Null)
        } else {
            observed == Some(expected)
        }
    })
}
fn set(row: &mut Document, key: &str, value: Bson) -> Result<(), String> {
    if let Some((parent, child)) = key.split_once('.') {
        if !row.contains_key(parent) {
            row.insert(parent, doc! {});
        }
        return set(
            row.get_document_mut(parent)
                .map_err(|_| "invalid dotted update")?,
            child,
            value,
        );
    }
    row.insert(key, value);
    Ok(())
}
fn apply(row: &Document, update: &Document) -> Result<Document, String> {
    let mut next = row.clone();
    if let Ok(fields) = update.get_document("$set") {
        for (key, value) in fields {
            set(&mut next, key, value.clone())?;
        }
    }
    if let Ok(fields) = update.get_document("$push") {
        for (key, value) in fields {
            if !next.contains_key(key) {
                next.insert(key, Vec::<Bson>::new());
            }
            next.get_array_mut(key)
                .map_err(|_| "push to nonarray")?
                .push(value.clone());
        }
    }
    Ok(next)
}
#[derive(Default)]
struct State {
    tasks: Vec<Document>,
    items: Vec<Document>,
    exists: bool,
    calls: Vec<String>,
    writes: Vec<(Document, Document)>,
    summary_calls: usize,
    fail_summary: BTreeSet<usize>,
    fail_before: BTreeSet<String>,
    fail_after: BTreeSet<String>,
    mutate_before: BTreeMap<String, Document>,
    mutate_after: BTreeMap<String, Document>,
    insert_fail_after: bool,
    item_write_fail: bool,
}
#[derive(Default)]
struct Store(Mutex<State>);
impl Store {
    fn task(&self) -> Document {
        self.0.lock().unwrap().tasks.last().unwrap().clone()
    }
    fn seed(&self, task: Document) {
        self.0.lock().unwrap().tasks.push(task);
    }
    fn mutate(&self, fields: Document) {
        self.0
            .lock()
            .unwrap()
            .tasks
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
        assert_eq!(host, Host::Local);
        assert!(newest);
        let mut s = self.0.lock().unwrap();
        s.calls.push("find_task".into());
        Ok(s.tasks
            .iter()
            .rev()
            .find(|row| matches(row, &query))
            .cloned())
    }
    async fn insert_task(&self, host: Host, task: Document) -> Result<Bson, String> {
        assert_eq!(host, Host::Local);
        let mut s = self.0.lock().unwrap();
        s.calls.push("insert_task".into());
        let id = task.get("_id").unwrap().clone();
        s.tasks.push(task);
        if s.insert_fail_after {
            return Err("insert acknowledgment lost".into());
        }
        Ok(id)
    }
    async fn update_task(
        &self,
        host: Host,
        query: Document,
        update: Document,
    ) -> Result<u64, String> {
        assert_eq!(host, Host::Local);
        let event = update
            .get_document("$push")
            .unwrap()
            .get_document("events")
            .unwrap();
        let key = format!(
            "{}:{}",
            event.get_str("command").unwrap(),
            event.get_str("status").unwrap()
        );
        let mut s = self.0.lock().unwrap();
        s.calls.push(key.clone());
        s.writes.push((query.clone(), update.clone()));
        if let Some(change) = s.mutate_before.remove(&key) {
            s.tasks.last_mut().unwrap().extend(change);
        }
        if s.fail_before.remove(&key) {
            return Err(format!("DB before {key}"));
        }
        let position = s.tasks.iter().position(|row| matches(row, &query));
        let Some(position) = position else {
            return Ok(0);
        };
        s.tasks[position] = apply(&s.tasks[position], &update)?;
        if let Some(change) = s.mutate_after.remove(&key) {
            s.tasks[position].extend(change);
        }
        if s.fail_after.remove(&key) {
            return Err(format!("DB acknowledgment lost after {key}"));
        }
        Ok(1)
    }
    async fn update_items(
        &self,
        host: Host,
        name: &str,
        filter: Document,
        update: Document,
    ) -> Result<u64, String> {
        assert_eq!(host, Host::Local);
        assert_eq!(name, "task");
        let mut s = self.0.lock().unwrap();
        s.calls.push("update_items".into());
        if s.item_write_fail {
            return Err("runId write unconfirmed".into());
        }
        let mut modified = 0;
        for row in &mut s.items {
            if matches(row, &filter) {
                let next = apply(row, &update)?;
                if next != *row {
                    modified += 1;
                }
                *row = next;
            }
        }
        Ok(modified) // Intentionally modified, not matched: coordinator must not confuse them.
    }
    async fn summary(&self, host: Host, name: &str) -> Result<CollectionSummary, String> {
        assert_eq!(host, Host::Local);
        assert_eq!(name, "task");
        let mut s = self.0.lock().unwrap();
        s.summary_calls += 1;
        let n = s.summary_calls;
        s.calls.push("summary".into());
        if s.fail_summary.contains(&n) {
            return Err("summary unavailable".into());
        }
        let mut statuses = BTreeMap::new();
        for row in &s.items {
            *statuses
                .entry(row.get_str("status").unwrap_or("missing").to_string())
                .or_insert(0) += 1;
        }
        Ok(CollectionSummary {
            exists: s.exists,
            total: s.items.len() as u64,
            statuses,
            latest_heartbeat_at: None,
        })
    }
    async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
        panic!("not needed")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("not needed")
    }
    async fn rows(&self, _: Host, _: &str, _: Option<Document>) -> Result<Vec<Document>, String> {
        panic!("generation must not full-scan payloads")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("writer stays Python-owned")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("no rename in coordinator")
    }
    async fn ping(&self, _: Host) -> Result<(), String> {
        panic!("no extra infrastructure probe")
    }
}
struct Runtime {
    store: Arc<Store>,
    metadata: Option<i64>,
    metadata_error: bool,
    metadata_race: bool,
    rows: Vec<Document>,
    failure: Option<Kind>,
    mutation: Option<Document>,
    metadata_calls: AtomicUsize,
    generate_calls: AtomicUsize,
    commands: Mutex<Vec<GenerationCommand>>,
}
impl Runtime {
    fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            metadata: None,
            metadata_error: false,
            metadata_race: false,
            rows: vec![doc! {"_id":ObjectId::new(),"status":"pending"}],
            failure: None,
            mutation: None,
            metadata_calls: AtomicUsize::new(0),
            generate_calls: AtomicUsize::new(0),
            commands: Mutex::new(vec![]),
        }
    }
}
#[async_trait]
impl GenerationRuntime for Runtime {
    async fn creation_metadata(
        &self,
        _: &Path,
        desired: &Document,
    ) -> Result<CreationMetadata, GenerationFailure> {
        self.metadata_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(desired.get_bool("isRemote"), Ok(false));
        assert_eq!(desired.get_str("status"), Ok("pending"));
        if self.metadata_race {
            self.store.seed(task("pending"));
        }
        if self.metadata_error {
            return Err(GenerationFailure::new(
                Kind::Ordinary,
                "original binding hash mismatch",
            ));
        }
        Ok(CreationMetadata {
            recommended_max_concurrent: self.metadata,
        })
    }
    async fn generate(
        &self,
        _: &Path,
        command: &GenerationCommand,
    ) -> Result<GeneratedOutput, GenerationFailure> {
        self.generate_calls.fetch_add(1, Ordering::SeqCst);
        self.commands.lock().unwrap().push(command.clone());
        let current = self.store.task();
        assert_eq!(current.get_str("status"), Ok("generating"));
        assert_eq!(current.get_object_id("_id").unwrap(), command.task_id);
        assert_eq!(
            current
                .get_document("generation")
                .unwrap()
                .get_str("id")
                .unwrap(),
            command.generation_id
        );
        assert_eq!(
            &command.argv[..4],
            ["uv", "run", "-m", "backend.mongodb.mongo_datum"]
        );
        let mut state = self.store.0.lock().unwrap();
        state.items = self.rows.clone();
        state.exists = true;
        drop(state);
        if let Some(change) = &self.mutation {
            self.store.mutate(change.clone());
        }
        if let Some(kind) = &self.failure {
            return Err(GenerationFailure::new(
                kind.clone(),
                "original writer failure",
            ));
        }
        Ok(GeneratedOutput {
            stdout: "字".repeat(2100),
        })
    }
}
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("qn-create-generate-{}", ObjectId::new()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn task(status: &str) -> Document {
    doc! {"_id":ObjectId::new(),"name":"task","template":"","templatefile":"tpl","auth_profile":"user1",
    "taskType":"regular","status":status,"isRemote":false,"targetRemote":false,"runId":"mission:task","events":[],"updatedAt":"original"}
}
fn setup(status: Option<&str>) -> (Root, Arc<Store>, Runtime) {
    let root = Root::new();
    let store = Arc::new(Store::default());
    if let Some(status) = status {
        store.seed(task(status));
    }
    let runtime = Runtime::new(store.clone());
    (root, store, runtime)
}
fn ok(result: &quantnight_task_core::cleanup_sync::TaskOutcome) {
    assert_eq!(result.exit_code, 0, "{}", result.value);
    assert_eq!(result.value["ok"], true);
}
fn failed(result: &quantnight_task_core::cleanup_sync::TaskOutcome) {
    assert_eq!(result.exit_code, 1, "{}", result.value);
    assert_eq!(result.value["ok"], false);
}

#[tokio::test]
async fn create_new_binding_remote_intent_and_gui_config_are_preserved() {
    let (root, store, mut runtime) = setup(None);
    runtime.metadata = Some(4);
    let mut request = CreateRequest::new("task", "tpl");
    request.target_remote = true;
    let result = core::create(&root.0, store.as_ref(), &runtime, &request).await;
    ok(&result);
    let saved = store.task();
    assert_eq!(saved.get_bool("isRemote"), Ok(false));
    assert_eq!(saved.get_bool("targetRemote"), Ok(true));
    assert_eq!(saved.get_str("status"), Ok("pending"));
    assert_eq!(saved.get_str("runId"), Ok("mission:task"));
    assert_eq!(
        saved
            .get_document("mission_config")
            .unwrap()
            .get_i32("max_concurrent"),
        Ok(4)
    );
    assert_eq!(saved.get("startConfig"), saved.get("mission_config"));
    let gui = core::gui_created_task(&result).unwrap();
    assert_eq!(gui.id, saved.get_object_id("_id").unwrap().to_hex());
    assert_eq!(gui.is_remote, Some(false));
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
    let (root, store, mut runtime) = setup(None);
    runtime.metadata = Some(4);
    request.mission_config =
        Some(doc! {"max_concurrent":2,"custom":["keep",3],"nested":{"a":true}});
    ok(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(
        store.task().get_document("mission_config").unwrap(),
        request.mission_config.as_ref().unwrap()
    );
}

#[tokio::test]
async fn create_idempotent_force_never_resets_execution_or_history() {
    let (root, store, runtime) = setup(Some("generation_failed"));
    store.mutate(doc! {"isRemote":true,"generation":{"id":"closed","phase":"generation_failed"},"lastError":"retained","mission_config":{"custom":1}});
    let original = store.task();
    let mut request = CreateRequest::new("task", "tpl");
    request.expected_task_id = Some(original.get_object_id("_id").unwrap().to_hex());
    let result = core::create(&root.0, store.as_ref(), &runtime, &request).await;
    ok(&result);
    assert_eq!(result.value["created"], false);
    assert_eq!(result.value["taskRecord"]["status"], "generation_failed");
    assert_eq!(result.value["taskRecord"]["isRemote"], true);
    assert_eq!(runtime.metadata_calls.load(Ordering::SeqCst), 0);
    request.templatefile = "changed".into();
    failed(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    request.force = true;
    request.mission_config = Some(doc! {"custom":2,"full":[1,2]});
    ok(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    let saved = store.task();
    for key in [
        "_id",
        "status",
        "isRemote",
        "generation",
        "lastError",
        "runId",
    ] {
        assert_eq!(saved.get(key), original.get(key), "{key}");
    }
    assert_eq!(saved.get_str("templatefile"), Ok("changed"));
    assert_eq!(saved.get("startConfig"), saved.get("mission_config"));
}

#[tokio::test]
async fn create_expected_id_absence_collision_metadata_failure_and_uncertain_insert_are_not_success(
) {
    let (root, store, runtime) = setup(None);
    let mut request = CreateRequest::new("task", "tpl");
    request.expected_task_id = Some(ObjectId::new().to_hex());
    failed(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    assert!(store.0.lock().unwrap().tasks.is_empty());
    for (race, binding_fail) in [(true, false), (false, true)] {
        let (root, store, mut runtime) = setup(None);
        runtime.metadata_race = race;
        runtime.metadata_error = binding_fail;
        failed(
            &core::create(
                &root.0,
                store.as_ref(),
                &runtime,
                &CreateRequest::new("task", "tpl"),
            )
            .await,
        );
        assert!(!store
            .0
            .lock()
            .unwrap()
            .calls
            .contains(&"insert_task".into()));
    }
    let (root, store, runtime) = setup(None);
    store.0.lock().unwrap().insert_fail_after = true;
    failed(
        &core::create(
            &root.0,
            store.as_ref(),
            &runtime,
            &CreateRequest::new("task", "tpl"),
        )
        .await,
    );
    assert_eq!(store.0.lock().unwrap().tasks.len(), 1);
    assert_eq!(
        store
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|v| *v == "insert_task")
            .count(),
        1
    );
}

#[tokio::test]
async fn create_cas_refuses_changed_state_even_without_updated_at() {
    for change in [
        doc! {"status":"running"},
        doc! {"_id":ObjectId::new()},
        doc! {"generation":{"id":"new"}},
        doc! {"mission_config":{"late":true}},
        doc! {"acceptedObservation":{"id":"active"}},
    ] {
        let (root, store, runtime) = setup(Some("pending"));
        store
            .0
            .lock()
            .unwrap()
            .mutate_before
            .insert("create:idempotent".into(), change.clone());
        let mut request = CreateRequest::new("task", "different");
        request.force = true;
        failed(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
        let saved = store.task();
        for (k, v) in change {
            assert_eq!(saved.get(&k), Some(&v));
        }
        assert_eq!(saved.get_str("templatefile"), Ok("tpl"));
    }
}

#[tokio::test]
async fn generate_waiting_and_default_900_original_writer_then_nonempty_ready() {
    let (root, store, runtime) = setup(Some("waiting"));
    let result = core::generate(
        &root.0,
        store.as_ref(),
        &runtime,
        &GenerateRequest::new("task"),
    )
    .await;
    ok(&result);
    core::gui_generated(&result).unwrap();
    let saved = store.task();
    assert_eq!(saved.get_str("status"), Ok("ready"));
    assert_eq!(saved.get_i32("expectedCount"), Ok(1));
    assert_eq!(
        saved.get_document("generation").unwrap().get_str("phase"),
        Ok("complete")
    );
    assert_eq!(
        result.value["stdout"].as_str().unwrap().chars().count(),
        2000
    );
    assert_eq!(
        store.0.lock().unwrap().items[0].get_str("runId"),
        Ok("mission:task")
    );
    let commands = runtime.commands.lock().unwrap();
    assert_eq!(commands[0].timeout_seconds, 900);
    assert_eq!(
        commands[0].argv,
        vec![
            "uv",
            "run",
            "-m",
            "backend.mongodb.mongo_datum",
            "--template",
            "tpl",
            "--alpha_filename",
            "task",
            "--auth_profile",
            "user1"
        ]
    );
    assert_eq!(commands[0].generation_id.len(), 32);
    assert_eq!(runtime.metadata_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn super_replace_and_already_equal_runid_are_not_confused_with_failed_write() {
    let (root, store, mut runtime) = setup(Some("ready"));
    store.mutate(doc! {"taskType":"super"});
    runtime.rows[0].insert("runId", "mission:task");
    let mut request = GenerateRequest::new("task");
    request.replace = true;
    request.timeout_seconds = 123;
    ok(&core::generate(&root.0, store.as_ref(), &runtime, &request).await);
    let commands = runtime.commands.lock().unwrap();
    assert_eq!(
        &commands[0].argv[10..],
        ["--alpha_type", "super", "--replace"]
    );
    assert_eq!(commands[0].timeout_seconds, 123);
}

#[tokio::test]
async fn generation_failure_classification_and_empty_exit0_match_original() {
    for (kind, rows, summary_fails, expected) in [
        (Some(Kind::Ordinary), 1, false, "generation_failed"),
        (Some(Kind::Timeout), 0, false, "generation_unknown"),
        (
            Some(Kind::DatabaseUncertain),
            0,
            false,
            "generation_unknown",
        ),
        (Some(Kind::Interrupted), 0, false, "generation_failed"),
        (Some(Kind::Interrupted), 1, false, "generation_unknown"),
        (Some(Kind::Interrupted), 0, true, "generation_unknown"),
        (None, 0, false, "generation_failed"),
    ] {
        let (root, store, mut runtime) = setup(Some("pending"));
        runtime.failure = kind;
        runtime.rows.truncate(rows);
        if summary_fails {
            store.0.lock().unwrap().fail_summary.insert(1);
        }
        let result = core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await;
        failed(&result);
        assert_eq!(store.task().get_str("status"), Ok(if expected == "generation_failed" { "pending" } else { expected }));
        assert_eq!(result.value["generationStatus"], expected);
        assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 1);
        assert!(core::gui_generated(&result).is_err());
    }
}

#[tokio::test]
async fn db_summary_and_runid_failures_reconcile_unknown_never_ready() {
    for item_write in [false, true] {
        let (root, store, runtime) = setup(Some("pending"));
        if item_write {
            store.0.lock().unwrap().item_write_fail = true;
        } else {
            store.0.lock().unwrap().fail_summary.insert(1);
        }
        failed(
            &core::generate(
                &root.0,
                store.as_ref(),
                &runtime,
                &GenerateRequest::new("task"),
            )
            .await,
        );
        assert_eq!(store.task().get_str("status"), Ok("generation_unknown"));
    }
}

#[tokio::test]
async fn begin_busy_observer_expected_id_and_flock_block_before_writer() {
    for status in [
        "running",
        "generating",
        "generation_unknown",
        "launch_unknown",
        "syncing",
        "deleting",
        "paused",
    ] {
        let (root, store, runtime) = setup(Some(status));
        failed(
            &core::generate(
                &root.0,
                store.as_ref(),
                &runtime,
                &GenerateRequest::new("task"),
            )
            .await,
        );
        assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(store.task().get_str("status"), Ok(status));
    }
    let (root, store, runtime) = setup(Some("pending"));
    store.mutate(doc! {"acceptedObservation":{"id":"open"}});
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    assert!(store.0.lock().unwrap().writes.is_empty());
    let (root, store, runtime) = setup(Some("pending"));
    let mut request = GenerateRequest::new("task");
    request.expected_task_id = Some(ObjectId::new().to_hex());
    failed(&core::generate(&root.0, store.as_ref(), &runtime, &request).await);
    assert!(store.0.lock().unwrap().writes.is_empty());
    let _guard = MissionLocks::acquire(&root.0, &["task"], false).unwrap();
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    failed(
        &core::create(
            &root.0,
            store.as_ref(),
            &runtime,
            &CreateRequest::new("task", "tpl"),
        )
        .await,
    );
}

#[tokio::test]
async fn stale_claim_before_dispatch_or_after_writer_does_not_write_new_identity() {
    for change in [
        doc! {"_id":ObjectId::new()},
        doc! {"status":"running"},
        doc! {"generation":{"id":"other","phase":"writing"}},
        doc! {"isRemote":true},
        doc! {"auth_profile":"other"},
    ] {
        let (root, store, mut runtime) = setup(Some("pending"));
        runtime.mutation = Some(change.clone());
        let result = core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await;
        failed(&result);
        let saved = store.task();
        for (k, v) in change {
            assert_eq!(saved.get(&k), Some(&v));
        }
        assert!(!store
            .0
            .lock()
            .unwrap()
            .calls
            .contains(&"update_items".into()));
    }
    let (root, store, runtime) = setup(Some("pending"));
    store.0.lock().unwrap().mutate_after.insert(
        "generation-begin:generating".into(),
        doc! {"generation":{"id":"foreign"}},
    );
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_cas_and_lost_begin_finish_ack_never_redispatch() {
    let (root, store, runtime) = setup(Some("pending"));
    store.0.lock().unwrap().mutate_before.insert(
        "generation-begin:generating".into(),
        doc! {"generation":{"id":"foreign"}},
    );
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
    let (root, store, runtime) = setup(Some("pending"));
    store
        .0
        .lock()
        .unwrap()
        .fail_after
        .insert("generation-begin:generating".into());
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    assert_eq!(store.task().get_str("status"), Ok("generating"));
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
    let (root, store, runtime) = setup(Some("pending"));
    store
        .0
        .lock()
        .unwrap()
        .fail_after
        .insert("generation-finish:ready".into());
    let result = core::generate(
        &root.0,
        store.as_ref(),
        &runtime,
        &GenerateRequest::new("task"),
    )
    .await;
    failed(&result);
    assert_eq!(store.task().get_str("status"), Ok("ready"));
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.value["generationReconciliationUnconfirmed"], true);
    assert_eq!(
        store
            .task()
            .get_document("generation")
            .unwrap()
            .get_str("phase"),
        Ok("complete")
    );
}

#[tokio::test]
async fn finish_race_and_reconciliation_failure_preserve_original_evidence() {
    let (root, store, runtime) = setup(Some("pending"));
    store.0.lock().unwrap().mutate_before.insert(
        "generation-finish:ready".into(),
        doc! {"generation":{"id":"replacement","phase":"writing"}},
    );
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    assert_eq!(
        store
            .task()
            .get_document("generation")
            .unwrap()
            .get_str("id"),
        Ok("replacement")
    );
    let (root, store, mut runtime) = setup(Some("pending"));
    runtime.failure = Some(Kind::Timeout);
    store
        .0
        .lock()
        .unwrap()
        .fail_before
        .insert("generation-finish:generation_unknown".into());
    let result = core::generate(
        &root.0,
        store.as_ref(),
        &runtime,
        &GenerateRequest::new("task"),
    )
    .await;
    failed(&result);
    assert_eq!(store.task().get_str("status"), Ok("generating"));
    assert_eq!(result.value["error"], "original writer failure");
    assert!(result.value["generationReconciliationError"]
        .as_str()
        .unwrap()
        .contains("DB before"));
}

#[tokio::test]
async fn original_create_failed_generate_idempotent_retry_history_is_retained() {
    let (root, store, mut runtime) = setup(None);
    ok(&core::create(
        &root.0,
        store.as_ref(),
        &runtime,
        &CreateRequest::new("task", "tpl"),
    )
    .await);
    runtime.failure = Some(Kind::Ordinary);
    failed(
        &core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await,
    );
    let first = store
        .task()
        .get_document("generation")
        .unwrap()
        .get_str("id")
        .unwrap()
        .to_owned();
    ok(&core::create(
        &root.0,
        store.as_ref(),
        &runtime,
        &CreateRequest::new("task", "tpl"),
    )
    .await);
    assert_eq!(store.task().get_str("status"), Ok("pending"));
    runtime.failure = None;
    ok(&core::generate(
        &root.0,
        store.as_ref(),
        &runtime,
        &GenerateRequest::new("task"),
    )
    .await);
    let saved = store.task();
    assert_ne!(
        saved
            .get_document("generation")
            .unwrap()
            .get_str("id")
            .unwrap(),
        first
    );
    assert_eq!(saved.get_str("lastError"), Ok("original writer failure"));
    let events = saved
        .get_array("events")
        .unwrap()
        .iter()
        .map(|v| {
            let e = v.as_document().unwrap();
            (e.get_str("command").unwrap(), e.get_str("status").unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            ("create", "pending"),
            ("generation-begin", "generating"),
            ("generation-finish", "generation_failed"),
            ("create", "idempotent"),
            ("generation-begin", "generating"),
            ("generation-finish", "ready")
        ]
    );
}

struct BlockingRuntime(tokio::sync::Notify);
#[async_trait]
impl GenerationRuntime for BlockingRuntime {
    async fn creation_metadata(
        &self,
        _: &Path,
        _: &Document,
    ) -> Result<CreationMetadata, GenerationFailure> {
        Ok(CreationMetadata::default())
    }
    async fn generate(
        &self,
        _: &Path,
        _: &GenerationCommand,
    ) -> Result<GeneratedOutput, GenerationFailure> {
        self.0.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn same_lock_spans_runtime_await_and_cancelled_future_keeps_claim() {
    let (root, store, _) = setup(Some("pending"));
    let runtime = Arc::new(BlockingRuntime(tokio::sync::Notify::new()));
    let future = {
        let path = root.0.clone();
        let store = store.clone();
        let runtime = runtime.clone();
        tokio::spawn(async move {
            core::generate(
                &path,
                store.as_ref(),
                runtime.as_ref(),
                &GenerateRequest::new("task"),
            )
            .await
        })
    };
    runtime.0.notified().await;
    let claimed = store.task();
    assert_eq!(claimed.get_str("status"), Ok("generating"));
    let result = core::generate(
        &root.0,
        store.as_ref(),
        runtime.as_ref(),
        &GenerateRequest::new("task"),
    )
    .await;
    failed(&result);
    assert!(result.value["error"]
        .as_str()
        .unwrap()
        .contains("another lifecycle mutator"));
    future.abort();
    assert!(future.await.unwrap_err().is_cancelled());
    let result = core::generate(
        &root.0,
        store.as_ref(),
        runtime.as_ref(),
        &GenerateRequest::new("task"),
    )
    .await;
    failed(&result);
    assert_eq!(store.task(), claimed);
}

#[tokio::test]
async fn create_super_priority_and_unbound_defaults_keep_original_task_shapes() {
    for kind in ["regular", "super", "priority"] {
        let (root, store, mut runtime) = setup(None);
        runtime.metadata = if kind == "super" { Some(3) } else { None };
        let mut request = CreateRequest::new("task", "tpl");
        request.task_type = kind.into();
        let result = core::create(&root.0, store.as_ref(), &runtime, &request).await;
        ok(&result);
        let saved = store.task();
        assert_eq!(saved.get_str("taskType"), Ok(kind));
        assert_eq!(saved.get_str("status"), Ok("pending"));
        if kind == "super" {
            let config = saved.get_document("startConfig").unwrap();
            assert_eq!(config.get_i32("max_concurrent"), Ok(3));
            assert_eq!(config.get_i32("max_multi_simulation_children"), Ok(1));
            let tag = config.get_document("tag").unwrap();
            assert!(tag.contains_key("selection") && tag.contains_key("combo"));
        } else {
            assert!(!saved.contains_key("startConfig"));
            assert!(!saved.contains_key("mission_config"));
        }
    }
}

#[tokio::test]
async fn force_running_still_keeps_execution_side_and_exact_original_id() {
    let (root, store, runtime) = setup(Some("running"));
    store.mutate(
        doc! {"isRemote": true, "startConfig": {"old": 1}, "launchAttemptId": "original-launch"},
    );
    let before = store.task();
    let mut request = CreateRequest::new("task", "new-template");
    request.force = true;
    request.expected_task_id = Some(ObjectId::new().to_hex());
    failed(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    assert_eq!(store.task(), before);
    request.expected_task_id = Some(before.get_object_id("_id").unwrap().to_hex());
    ok(&core::create(&root.0, store.as_ref(), &runtime, &request).await);
    let saved = store.task();
    for key in [
        "_id",
        "status",
        "isRemote",
        "startConfig",
        "launchAttemptId",
    ] {
        assert_eq!(saved.get(key), before.get(key));
    }
    assert_eq!(saved.get_str("templatefile"), Ok("new-template"));
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn legacy_missing_runid_uses_original_fallback_without_overwriting_explicit_null() {
    for original in [
        None,
        Some(Bson::Null),
        Some(Bson::String("original-custom-run".into())),
    ] {
        let (root, store, runtime) = setup(Some("waiting"));
        {
            let mut state = store.0.lock().unwrap();
            let task = state.tasks.last_mut().unwrap();
            task.remove("runId");
            if let Some(value) = &original {
                task.insert("runId", value.clone());
            }
        }
        ok(&core::generate(
            &root.0,
            store.as_ref(),
            &runtime,
            &GenerateRequest::new("task"),
        )
        .await);
        let state = store.0.lock().unwrap();
        let expected = original
            .clone()
            .unwrap_or_else(|| Bson::String("mission:task".into()));
        assert_eq!(state.items[0].get("runId"), Some(&expected));
        assert_eq!(state.tasks[0].get("runId"), original.as_ref());
    }
}


#[tokio::test]
async fn failed_operation_restores_only_captured_pending_waiting_and_preserves_rows() {
    for (before, after) in [("pending", "pending"), ("waiting", "waiting"),
                            ("ready", "generation_failed"), ("generation_failed", "generation_failed")] {
        let (root, store, mut runtime) = setup(Some(before));
        runtime.failure = Some(Kind::Ordinary);
        runtime.rows[0].insert("opaque", doc! {"retained": true});
        let rows = runtime.rows.clone();
        let result = core::generate(&root.0, store.as_ref(), &runtime, &GenerateRequest::new("task")).await;
        failed(&result);
        assert_eq!(result.value["generationStatus"], "generation_failed");
        let saved = store.task();
        assert_eq!(saved.get_str("status"), Ok(after));
        assert_eq!(saved.get_str("lastError"), Ok("original writer failure"));
        let generation = saved.get_document("generation").unwrap();
        assert_eq!(generation.get_str("phase"), Ok("generation_failed"));
        assert!(generation.get_str("finishedAt").is_ok());
        assert_eq!(generation.get_str("id").unwrap(), result.value["generationId"]);
        let state = store.0.lock().unwrap();
        assert_eq!(state.items, rows, "do not roll back, delete or rewrite partial rows");
        assert_eq!(state.writes.len(), 2, "one begin and one final CAS only");
        let finish = state.writes.last().unwrap();
        assert_eq!(finish.0.get_str("status"), Ok("generating"));
        assert_eq!(finish.0.get_str("generation.id"), generation.get_str("id"));
        let event = finish.1.get_document("$push").unwrap().get_document("events").unwrap();
        assert_eq!(event.get_str("status"), Ok("generation_failed"), "event retains operation outcome");
        assert_eq!(event.get_document("detail").unwrap().get_str("generationId"), generation.get_str("id"));
        assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn lost_failed_finish_ack_keeps_restored_state_without_regeneration() {
    let (root, store, mut runtime) = setup(Some("pending"));
    runtime.failure = Some(Kind::Ordinary);
    store.0.lock().unwrap().fail_after.insert("generation-finish:generation_failed".into());
    let result = core::generate(&root.0, store.as_ref(), &runtime, &GenerateRequest::new("task")).await;
    failed(&result);
    assert_eq!(result.value["generationReconciliationUnconfirmed"], true);
    assert_eq!(store.task().get_str("status"), Ok("pending"));
    assert_eq!(store.task().get_document("generation").unwrap().get_str("phase"), Ok("generation_failed"));
    assert_eq!(runtime.generate_calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.0.lock().unwrap().writes.len(), 2);
}
