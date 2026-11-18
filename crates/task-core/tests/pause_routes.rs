use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, DateTime, Document};
use pause::{PauseOptions, PauseRuntime, StopBudget, StopConfirmationBasis, StopOutput};
use quantnight_task_core::{mission_lock, pause, task_store};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use task_store::{CollectionSummary, Host, TaskStore};
const NAME: &str = "fixture";
const ID: &str = "0123456789abcdef01234567";
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "qn-pause-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn at(n: u32) -> String {
    format!("2024-01-01T00:00:{n:02}+00:00")
}
fn event(command: &str, status: &str, n: u32, detail: Document) -> Document {
    doc! {"id":format!("event-{n}"),"command":command,"status":status,"at":at(n),"detail":detail}
}
fn pending() -> Document {
    doc! {"_id":ObjectId::parse_str(ID).unwrap(),"name":NAME,"status":"pending","taskType":"regular","auth_profile":"fixture-profile","templatefile":"fixture-template","isRemote":false,"targetRemote":true,"runId":"mission:fixture","createdAt":at(0),"events":[event("create","pending",0,doc!{})]}
}
fn generated(status: &str, remote: bool) -> Document {
    let mut t = pending();
    let phase = if status == "pending" {
        "preparing"
    } else {
        "writing"
    };
    let begin = doc! {"id":"gen-1","phase":phase,"startedAt":at(1),"producerPid":37};
    if let Some(Bson::Array(events)) = t.get_mut("events") {
        events.push(Bson::Document(event(
            "generation-begin",
            "generating",
            2,
            begin,
        )));
        events.push(Bson::Document(event(
            "generation-finish",
            status,
            3,
            doc! {"generationId":"gen-1"},
        )));
    }
    t.insert("status", status);
    t.insert("expectedCount", if status == "ready" { 2 } else { 0 });
    t.insert("generation",doc!{"id":"gen-1","phase":if status=="ready"{"complete"}else if status=="pending"{"prepared"}else{"generation_failed"},"startedAt":at(1),"finishedAt":at(3),"producerPid":37});
    if remote {
        t.insert("isRemote", true);
        t.insert("syncedAt", at(8));
        t.insert("syncedDigest", "a".repeat(64));
        if let Some(Bson::Array(events)) = t.get_mut("events") {
            events.push(Bson::Document(event("sync-begin", "syncing", 7, doc! {})));
            events.push(Bson::Document(event("sync", "ready", 8, doc! {})));
        }
    }
    t
}
fn normal() -> Document {
    doc! {"_id":ObjectId::parse_str(ID).unwrap(),"name":NAME,"status":"running","isRemote":false,"launchAttemptId":"launch-1","startupBinding":{"token":"old"},"tmuxContext":{"sessionId":"$1"}}
}
fn items() -> Vec<Document> {
    (1..=2).map(|i|doc!{"_id":ObjectId::parse_str(&format!("{i:024x}")).unwrap(),"status":"pending","runId":"mission:fixture","regular":"  expression  ","settings":{"nested":{"a":1,"b":2},"list":[1,2]},"custom":7i32}).collect()
}
fn proof(version: u32, remote: bool) -> String {
    format!(
        "QUANTNIGHT_PAUSE_CONFIRMED {}",
        if version == 1 {
            json!({"schema_version":1,"task_name":NAME,"target_remote":remote,"session_absent":true,"runners_exited":true})
        } else {
            json!({"schema_version":2,"task_name":NAME,"target_remote":remote,"session_absent":true,"runners_exited":null,"confirmation_basis":"session_absent"})
        }
    )
}
fn matches(row: &Document, q: &Document) -> bool {
    q.iter().all(|(k, v)| match v {
        Bson::Document(d) if d.contains_key("$ne") => row.get(k) != d.get("$ne"),
        Bson::Document(d) if d.contains_key("$exists") => {
            row.contains_key(k) == d.get_bool("$exists").unwrap()
        }
        Bson::Null => matches!(row.get(k), None | Some(Bson::Null)),
        _ => row.get(k) == Some(v),
    })
}
#[derive(Default)]
struct Data {
    tasks: HashMap<Host, Vec<Document>>,
    items: HashMap<Host, Vec<Document>>,
    caps: HashMap<Host, Vec<Document>>,
    calls: Vec<String>,
    queries: Vec<Document>,
    writes: Vec<(Document, Document)>,
    fail: Option<String>,
    cas_zero: bool,
    write_timeout: bool,
    readback_error: bool,
    control_patch: HashMap<Host, Document>,
}
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Data>>);
impl Memory {
    fn new(t: Document, rows: Option<Vec<Document>>) -> Self {
        let s = Self::default();
        {
            let mut d = s.0.lock().unwrap();
            d.tasks.insert(Host::Local, vec![t.clone()]);
            if let Some(rows) = rows {
                d.items.insert(Host::Local, rows.clone());
                if t.get_bool("isRemote").unwrap_or(false) {
                    d.items.insert(Host::Remote, rows);
                }
            }
            if t.get_bool("isRemote").unwrap_or(false) {
                d.tasks.insert(Host::Remote, vec![t]);
            }
        }
        s
    }
    fn task(&self) -> Document {
        self.0.lock().unwrap().tasks[&Host::Local][0].clone()
    }
    fn no_write(&self) {
        assert!(self.0.lock().unwrap().writes.is_empty());
    }
}
fn label(h: Host) -> &'static str {
    if h == Host::Local {
        "local"
    } else {
        "remote"
    }
}
fn call(d: &mut Data, l: String) -> Result<(), String> {
    d.calls.push(l.clone());
    if d.fail.as_deref() == Some(l.as_str()) {
        return Err(format!("injected {l}: timeout/unavailable, not absence"));
    }
    Ok(())
}
#[async_trait]
impl TaskStore for Memory {
    async fn find_task(
        &self,
        h: Host,
        q: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, format!("task:{}", label(h)))?;
        if d.readback_error && !d.writes.is_empty() { return Err("exact readback unavailable; stop not repeated".into()); }
        let mut rows = d.tasks.get(&h).cloned().unwrap_or_default();
        if newest {
            rows.sort_by_key(|v| {
                v.get_object_id("_id")
                    .map(|id| id.to_hex())
                    .unwrap_or_else(|_| format!("{:?}", v.get("_id")))
            });
            rows.reverse();
        }
        Ok(rows.into_iter().find(|r| matches(r, &q)))
    }
    async fn find_tasks(&self, h: Host, q: Document) -> Result<Vec<Document>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, format!("controls:{}", label(h)))?;
        if let Some(patch) = d.control_patch.remove(&h) {
            d.tasks.get_mut(&h).unwrap()[0].extend(patch);
        }
        let options = task_store::never_started_controls_options();
        assert_eq!(options.limit, Some(2));
        assert_eq!(options.batch_size, Some(2));
        assert!(options.projection.is_none());
        let rows: Vec<_> = d
            .tasks
            .get(&h)
            .into_iter()
            .flatten()
            .filter(|r| matches(r, &q))
            .take(2)
            .cloned()
            .collect();
        d.queries.push(doc!{"op":"controls","host":label(h),"filter":q,"limit":2,"projection":Bson::Null,"returned":rows.len() as i64});
        Ok(rows)
    }
    async fn consumer_capability_exists(&self, h: Host, n: &str) -> Result<bool, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, format!("capability:{}", label(h)))?;
        let (q, options) = task_store::consumer_capability_query(n);
        assert_eq!(options.projection, Some(doc! {"_id":1}));
        let result = d
            .caps
            .get(&h)
            .into_iter()
            .flatten()
            .find(|v| matches(v, &q))
            .map(|v| {
                v.iter()
                    .filter(|(k, _)| k.as_str() == "_id")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Document>()
            });
        d.queries.push(doc!{"op":"capability","host":label(h),"filter":q,"projection":options.projection.unwrap(),"limit":1,"result":result.clone().map(Bson::Document).unwrap_or(Bson::Null)});
        Ok(result.is_some())
    }
    async fn update_task(&self, h: Host, q: Document, u: Document) -> Result<u64, String> {
        assert_eq!(h, Host::Local);
        let mut d = self.0.lock().unwrap();
        call(&mut d, "write".into())?;
        d.writes.push((q.clone(), u.clone()));
        if d.cas_zero {
            return Ok(0);
        }
        let timeout = d.write_timeout;
        let Some(row) = d
            .tasks
            .entry(h)
            .or_default()
            .iter_mut()
            .find(|row| matches(row, &q))
        else {
            return Ok(0);
        };
        row.extend(u.get_document("$set").unwrap().clone());
        if let Ok(push) = u.get_document("$push") {
            if !row.contains_key("events") {
                row.insert("events", Bson::Array(vec![]));
            }
            if let Some(Bson::Array(events)) = row.get_mut("events") {
                events.push(push.get("events").unwrap().clone());
            }
        }
        if timeout {
            return Err("write timeout after possible application; not retried".into());
        }
        Ok(1)
    }
    async fn collection_names(&self, h: Host) -> Result<Vec<String>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, format!("names:{}", label(h)))?;
        Ok(if d.items.contains_key(&h) {
            vec![NAME.into()]
        } else {
            vec![]
        })
    }
    async fn rows(&self, h: Host, n: &str, p: Option<Document>) -> Result<Vec<Document>, String> {
        assert_eq!(n, NAME);
        assert!(p.is_none(), "positive proof must retain ALL item fields");
        let mut d = self.0.lock().unwrap();
        call(&mut d, format!("items:{}", label(h)))?;
        let rows = d.items.get(&h).cloned().unwrap_or_default();
        d.queries.push(doc!{"op":"items","host":label(h),"projection":Bson::Null,"limit":Bson::Null,"returned":rows.len() as i64});
        Ok(rows)
    }
    async fn ping(&self, h: Host) -> Result<(), String> {
        call(&mut self.0.lock().unwrap(), format!("ping:{}", label(h)))
    }
    async fn summary(&self, _: Host, _: &str) -> Result<CollectionSummary, String> {
        panic!("pause cannot infer no-launch from summary")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("pause requires complete inventory, not limit1 item probe")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("pause cannot insert items")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("pause cannot rename")
    }
}
struct Runtime {
    store: Memory,
    stdout: String,
    stderr: String,
    fail: bool,
    mutate: Option<&'static str>,
    calls: Mutex<Vec<bool>>,
    budget: StopBudget,
}
impl Runtime {
    fn new(store: &Memory, remote: bool) -> Self {
        Self {
            store: store.clone(),
            stdout: proof(2, remote),
            stderr: "original stderr".into(),
            fail: false,
            mutate: None,
            calls: Mutex::new(vec![]),
            budget: StopBudget::Cli,
        }
    }
}
#[async_trait]
impl PauseRuntime for Runtime {
    fn script_budget(&self) -> StopBudget {
        self.budget
    }
    async fn stop(&self, root: &Path, task: &Document, verify: bool) -> Result<StopOutput, String> {
        assert!(
            mission_lock::MissionLocks::acquire(root, &[NAME], false).is_err(),
            "real original lock spans script"
        );
        assert_eq!(task.get_object_id("_id").unwrap().to_hex(), ID);
        self.calls.lock().unwrap().push(verify);
        if self.fail {
            return Err("original pause script nonzero/timeout: precise stderr".into());
        }
        if let Some(change) = self.mutate {
            let mut d = self.store.0.lock().unwrap();
            match change {
                "task-id" => {
                    d.tasks.get_mut(&Host::Local).unwrap()[0].insert("_id", ObjectId::new());
                }
                "launch" => {
                    d.tasks.get_mut(&Host::Local).unwrap()[0]
                        .insert("launchAttemptId", "new-launch");
                }
                "startup" => {
                    d.tasks.get_mut(&Host::Local).unwrap()[0]
                        .insert("startupBinding", doc! {"token":"changed"});
                }
                "observer" => {
                    d.tasks.get_mut(&Host::Local).unwrap()[0].insert(
                        "acceptedObservation",
                        doc! {"id":"new","finishedAt":Bson::Null},
                    );
                }
                "token" => {
                    if let Some(Bson::Document(g)) =
                        d.tasks.get_mut(&Host::Local).unwrap()[0].get_mut("generation")
                    {
                        g.insert("id", "new-token");
                    }
                }
                "old-event" => {
                    if let Some(Bson::Array(e)) =
                        d.tasks.get_mut(&Host::Local).unwrap()[0].get_mut("events")
                    {
                        e[0].as_document_mut()
                            .unwrap()
                            .insert("id", "changed-old-event");
                    }
                }
                "capability" => {
                    d.caps.insert(Host::Local,vec![doc!{"_id":"old-consumer","task":NAME,"heartbeatAt":DateTime::from_millis(1)}]);
                }
                "ambiguous" => {
                    let mut t = d.tasks[&Host::Local][0].clone();
                    t.insert(
                        "_id",
                        ObjectId::parse_str("000000000000000000000000").unwrap(),
                    );
                    d.tasks.get_mut(&Host::Local).unwrap().push(t);
                }
                "item-add" => {
                    let mut item = items()[0].clone();
                    item.insert("_id", ObjectId::new());
                    d.items.get_mut(&Host::Local).unwrap().push(item);
                }
                "item-remove" => {
                    d.items.get_mut(&Host::Local).unwrap().pop();
                }
                "receipt" => {
                    d.items.get_mut(&Host::Local).unwrap()[0]
                        .insert("progressUrl", "accepted-progress");
                }
                "payload" => {
                    d.items.get_mut(&Host::Local).unwrap()[0].insert("regular", "  expression   ");
                }
                "remote-payload" => {
                    d.items.get_mut(&Host::Remote).unwrap()[0].insert("regular", "changed");
                }
                "type" => {
                    d.items.get_mut(&Host::Local).unwrap()[0].insert("custom", 7i64);
                }
                "empty-type" => {
                    d.items.get_mut(&Host::Local).unwrap()[0].insert("responseBody", doc! {});
                }
                "nested" => {
                    d.items.get_mut(&Host::Local).unwrap()[0]
                        .insert("settings", doc! {"nested":{"a":1,"b":3},"list":[1,2]});
                }
                "array" => {
                    d.items.get_mut(&Host::Local).unwrap()[0]
                        .insert("settings", doc! {"nested":{"a":1,"b":2},"list":[2,1]});
                }
                "reorder" => {
                    let rows = d.items.get_mut(&Host::Local).unwrap();
                    rows.reverse();
                    for row in rows.iter_mut() {
                        let mut entries: Vec<_> =
                            row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                        entries.reverse();
                        *row = entries.into_iter().collect();
                        row.insert("settings", doc! {"list":[1,2],"nested":{"b":2,"a":1}});
                    }
                }
                "items-error" => d.fail = Some("items:local".into()),
                _ => panic!("unknown fixture mutation"),
            }
        }
        Ok(StopOutput {
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        })
    }
}
async fn run(s: &Memory, r: &Runtime, root: &Root) -> pause::PauseOutcome {
    pause::pause(
        &root.0,
        s,
        r,
        NAME,
        PauseOptions {
            expected_task_id: Some(ID),
            ..Default::default()
        },
    )
    .await
}
fn failed(out: pause::PauseOutcome, part: &str) {
    assert_eq!(out.exit_code, 1, "{}", out.value);
    assert_eq!(out.value["ok"], false);
    assert_eq!(out.value["stopConfirmation"], "unknown");
    assert!(out.confirmed_stop.is_none());
    assert!(
        out.value.to_string().contains(part),
        "{} lacks {part}",
        out.value
    );
}

#[test]
fn native_receipt_port_is_strict_for_versions_duplicates_extra_fields_host_and_final_line() {
    for remote in [false, true] {
        for version in [1, 2] {
            assert_eq!(
                pause::verified_stop_proof(&proof(version, remote), NAME, remote).unwrap(),
                if version == 1 {
                    StopConfirmationBasis::RunnersExited
                } else {
                    StopConfirmationBasis::SessionAbsent
                }
            );
        }
    }
    let good = proof(2, false);
    let parsed: Value =
        serde_json::from_str(good.strip_prefix("QUANTNIGHT_PAUSE_CONFIRMED ").unwrap()).unwrap();
    let mut bad = vec![
        String::new(),
        "Pane absent".into(),
        "QUANTNIGHT_TASK_STAGE {\"stage\":\"complete\"}".into(),
        format!("{good}\n{good}"),
        format!("{good}\nafter"),
        format!("{good}\n\n"),
        good.replacen(
            "\"schema_version\":2",
            "\"schema_version\":2,\"schema_version\":2",
            1,
        ),
        format!("QUANTNIGHT_PAUSE_CONFIRMED {}", "x".repeat(8193)),
    ];
    for (key, value) in [
        ("schema_version", json!(3)),
        ("task_name", json!("other")),
        ("target_remote", json!(true)),
        ("session_absent", json!(false)),
        ("runners_exited", json!(true)),
        ("confirmation_basis", json!("runners_exited")),
        ("extra", json!(true)),
    ] {
        let mut v = parsed.clone();
        v[key] = value;
        bad.push(format!("QUANTNIGHT_PAUSE_CONFIRMED {v}"));
    }
    for field in ["runners_exited", "confirmation_basis", "session_absent"] {
        let mut v = parsed.clone();
        v.as_object_mut().unwrap().remove(field);
        bad.push(format!("QUANTNIGHT_PAUSE_CONFIRMED {v}"));
    }
    for raw in bad {
        assert!(
            pause::verified_stop_proof(&raw, NAME, false).is_err(),
            "accepted {raw}"
        );
    }
}

#[tokio::test]
async fn normal_stop_preserves_cli_gui_fields_stage_hook_and_native_cas_fences() {
    for (version, budget) in [(1, StopBudget::Cli), (2, StopBudget::Gui)] {
        let root = Root::new();
        let store = Memory::new(
            normal(),
            Some(vec![doc! {"status":"processing","acceptance":"UNKNOWN"}]),
        );
        let mut runtime = Runtime::new(&store, false);
        runtime.stdout = proof(version, false);
        runtime.budget = budget;
        let stages = Arc::new(Mutex::new(vec![]));
        let recorded = stages.clone();
        let out = pause::pause(
            &root.0,
            &store,
            &runtime,
            NAME,
            PauseOptions {
                expected_task_id: Some(ID),
                expected_launch_attempt_id: Some("launch-1"),
                expected_remote: Some(false),
                observer: Some(Arc::new(move |e| recorded.lock().unwrap().push(e.stage))),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(out.exit_code, 0, "{}", out.value);
        assert_eq!(out.value["schemaVersion"], 1);
        assert_eq!(out.value["stopConfirmation"], "confirmed");
        assert_eq!(out.value["taskStatus"], "paused");
        assert_eq!(*runtime.calls.lock().unwrap(), vec![false]);
        let stop = out.confirmed_stop.unwrap();
        assert_eq!(stop.task_id, ID);
        assert!(stop.stop_confirmed_at > 0);
        assert_eq!(
            serde_json::to_value(&stop).unwrap()["confirmationBasis"],
            if version == 1 {
                "runners_exited"
            } else {
                "session_absent"
            }
        );
        let d = store.0.lock().unwrap();
        assert_eq!(d.writes.len(), 1);
        let (q, _) = &d.writes[0];
        for field in ["_id", "name", "status", "isRemote", "updatedAt"] {
            assert!(q.contains_key(field), "{field}");
        }
        assert!(
            d.queries.is_empty(),
            "normal pause must not fetch inventory or infer noPOST"
        );
        assert_eq!(
            d.items[&Host::Local][0].get_str("acceptance").unwrap(),
            "UNKNOWN"
        );
        let observed_stages = stages.lock().unwrap();
        assert_eq!(observed_stages.last().map(String::as_str), Some("completed"));
        assert!(!observed_stages.iter().any(|stage| stage == "inventory"));
        assert_eq!(budget.seconds(), if version == 1 { 60 } else { 75 });
    }
}

#[tokio::test]
async fn ready_without_full_history_never_claims_never_started() {
    let mut task = normal();
    task.insert("status", "ready");
    let store = Memory::new(task, None);
    let runtime = Runtime::new(&store, false);
    let out = run(&store, &runtime, &Root::new()).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(*runtime.calls.lock().unwrap(), vec![false]);
    assert!(store.0.lock().unwrap().queries.is_empty());
}

#[tokio::test]
async fn guard_failures_prevent_stop_and_original_flock_is_shared() {
    for field in ["id", "launch", "host"] {
        let s = Memory::new(normal(), None);
        let r = Runtime::new(&s, false);
        let root = Root::new();
        let out = pause::pause(
            &root.0,
            &s,
            &r,
            NAME,
            PauseOptions {
                expected_task_id: Some(if field == "id" { "other" } else { ID }),
                expected_launch_attempt_id: if field == "launch" {
                    Some("other")
                } else {
                    None
                },
                expected_remote: if field == "host" { Some(true) } else { None },
                ..Default::default()
            },
        )
        .await;
        assert_eq!(out.exit_code, 1);
        assert!(r.calls.lock().unwrap().is_empty());
        s.no_write();
    }
    let root = Root::new();
    let s = Memory::new(normal(), None);
    let r = Runtime::new(&s, false);
    let _held = mission_lock::MissionLocks::acquire(&root.0, &[NAME], false).unwrap();
    failed(run(&s, &r, &root).await, "another lifecycle mutator");
    assert!(s.0.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn script_receipt_and_cas_errors_never_confirm_or_retry() {
    for fault in [
        "script",
        "receipt",
        "cas",
        "write-timeout",
        "task-id",
    ] {
        let s = Memory::new(normal(), None);
        let mut r = Runtime::new(&s, false);
        match fault {
            "script" => r.fail = true,
            "receipt" => r.stdout = "QUANTNIGHT_TASK_STAGE {\"stage\":\"complete\"}".into(),
            "cas" => s.0.lock().unwrap().cas_zero = true,
            "write-timeout" => { let mut d=s.0.lock().unwrap(); d.write_timeout = true; d.readback_error = true; },
            _ => r.mutate = Some(fault),
        }
        let out = run(&s, &r, &Root::new()).await;
        assert_eq!(out.exit_code, 1, "{fault}");
        assert!(out.confirmed_stop.is_none());
        assert_eq!(out.value["stopConfirmation"], "unknown");
        assert_eq!(r.calls.lock().unwrap().len(), 1);
        assert!(s.0.lock().unwrap().writes.len() <= 1);
    }
}

#[tokio::test]
async fn explicit_stop_ignores_stale_history_and_preserves_receipts_on_both_hosts() {
    for remote in [false, true] {
        for status in ["pending", "ready", "running", "launching", "generating", "generation_unknown", "syncing"] {
            let mut task = generated("ready", remote);
            task.extend(doc! {"status":status,"startupBinding":{"consumer_run_id":"old-binding"},
                "simulatorRuntime":{"consumer_run_id":"different-old-run"},"launchAttemptId":"old-launch",
                "launchSettledAttemptId":Bson::Null,"stopConfirmedAt":"stale","acceptedObservation":{"id":"abandoned"}});
            let rows = vec![doc! {"_id":"accepted","status":"processing","acceptance":"ACCEPTED","progressUrl":"retained"}];
            let store = Memory::new(task, Some(rows.clone()));
            let runtime = Runtime::new(&store, remote);
            let root = Root::new();
            let out = run(&store, &runtime, &root).await;
            assert_eq!(out.exit_code, 0, "{remote}/{status}: {}", out.value);
            let first_expected = if matches!(status, "generating" | "generation_unknown") {
                "pending"
            } else {
                "paused"
            };
            assert_eq!(out.value["taskStatus"], first_expected);
            assert_eq!(store.task().get_str("status"), Ok(first_expected));
            let again = run(&store, &runtime, &root).await;
            assert_eq!(again.exit_code, 0, "{remote}/{status}: {}", again.value);
            assert_eq!(*runtime.calls.lock().unwrap(), vec![false, false]);
            let d = store.0.lock().unwrap();
            assert!(d.queries.is_empty(), "no historical inventory gates");
            assert_eq!(d.items[&Host::from_remote(remote)], rows);
            assert_eq!(d.tasks[&Host::Local][0].get_str("status"), Ok("paused"));
        }
    }
}

#[tokio::test]
async fn live_or_uninspectable_scope_refuses_before_signalling() {
    use quantnight_task_core::pause::PauseRuntime;
    struct ScopeError(&'static str);
    #[async_trait]
    impl PauseRuntime for ScopeError {
        async fn inspect_before_stop(&self, _: &Path, _: &dyn TaskStore, _: &Document, _: std::time::Duration)
            -> Result<Option<serde_json::Value>, String> {
            Err(self.0.into())
        }
        async fn stop(&self, _: &Path, _: &Document, _: bool) -> Result<StopOutput, String> {
            panic!("must not send stop to an unconfirmed scope or active writer")
        }
    }
    for reason in ["active generation writer", "SSH inspection failed", "foreign current task"] {
        let store = Memory::new(normal(), None);
        let out = pause::pause(&Root::new().0, &store, &ScopeError(reason), NAME, PauseOptions::default()).await;
        failed(out, reason);
        store.no_write();
    }
}

#[tokio::test]
async fn stop_cas_fences_task_identity_but_not_stale_metadata_or_item_updates() {
    for change in ["task-id", "launch", "observer", "token", "old-event", "capability", "ambiguous",
        "item-add", "item-remove", "receipt", "payload", "type", "empty-type", "nested", "array", "reorder", "items-error"] {
        let s = Memory::new(generated("ready", false), Some(items()));
        let mut r = Runtime::new(&s, false);
        r.mutate = Some(change);
        let out = run(&s, &r, &Root::new()).await;
        let changed_execution_target = change == "task-id";
        assert_eq!(out.exit_code, i32::from(changed_execution_target), "{change}: {}", out.value);
        assert_eq!(*r.calls.lock().unwrap(), vec![false]);
        assert!(s.0.lock().unwrap().queries.is_empty());
    }
    let s = Memory::new(generated("ready", true), Some(items()));
    let mut r = Runtime::new(&s, true);
    r.mutate = Some("remote-payload");
    assert_eq!(run(&s, &r, &Root::new()).await.exit_code, 0);
}

#[test]
fn task_store_additions_use_original_control_limit_and_capability_projection() {
    let controls = task_store::never_started_controls_options();
    assert_eq!(controls.limit, Some(2));
    assert_eq!(controls.batch_size, Some(2));
    assert!(controls.projection.is_none());
    assert_eq!(controls.max_time, Some(std::time::Duration::from_secs(30)));
    let (filter, options) = task_store::consumer_capability_query(NAME);
    assert_eq!(filter, doc! {"task":NAME});
    assert_eq!(options.projection, Some(doc! {"_id":1}));
    assert_eq!(options.max_time, Some(std::time::Duration::from_secs(30)));
    assert_eq!(StopBudget::Cli.seconds(), 60);
    assert_eq!(StopBudget::Gui.seconds(), 75);
}

#[tokio::test]
async fn missing_or_invalid_identity_and_bad_names_do_not_call_runtime() {
    for t in [
        doc! {"_id":"legacy-string","name":NAME,"status":"running","isRemote":false},
        doc! {"_id":ObjectId::parse_str(ID).unwrap(),"name":NAME,"status":"running","isRemote":"remote"},
    ] {
        let s = Memory::new(t, None);
        let r = Runtime::new(&s, false);
        let out = run(&s, &r, &Root::new()).await;
        assert_eq!(out.exit_code, 1);
        assert!(r.calls.lock().unwrap().is_empty());
        s.no_write();
    }
    let s = Memory::default();
    let r = Runtime::new(&s, false);
    failed(run(&s, &r, &Root::new()).await, "task not found");
    let s = Memory::new(normal(), None);
    let r = Runtime::new(&s, false);
    let out = pause::pause(&Root::new().0, &s, &r, "../escape", PauseOptions::default()).await;
    assert_eq!(out.exit_code, 1);
    assert!(s.0.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn pause_normalizes_legacy_intermediate_statuses_after_stop() {
    for status in ["deleting", "syncing", "launching"] {
        let root = Root::new();
        let mut task = normal();
        task.insert("status", status);
        let store = Memory::new(task, Some(items()));
        let runtime = Runtime::new(&store, false);
        let result = run(&store, &runtime, &root).await;
        assert_eq!(result.exit_code, 0, "{}", result.value);
        assert!(result.confirmed_stop.is_some());
        assert_eq!(result.value["taskStatus"], "paused");
        assert_eq!(store.task().get_str("status"), Ok("paused"));
    }
}

#[tokio::test]
async fn malformed_historical_labels_are_not_pause_admission_gates() {
    for (field, value) in [
        ("status", Bson::Array(vec![])),
        ("taskType", Bson::Document(doc! {})),
    ] {
        let mut task = generated("ready", false);
        task.insert(field, value);
        let store = Memory::new(task, Some(items()));
        let runtime = Runtime::new(&store, false);
        assert_eq!(run(&store, &runtime, &Root::new()).await.exit_code, 0);
        assert_eq!(*runtime.calls.lock().unwrap(), vec![false]);
    }
    // A core fixture whose current runtime can establish scope does not need
    // historical eligibility. Production still refuses an uninspectable scope.
    let mut task = normal();
    task.insert("taskType", doc! {});
    let store = Memory::new(task, None);
    let runtime = Runtime::new(&store, false);
    assert_eq!(run(&store, &runtime, &Root::new()).await.exit_code, 0);
    assert_eq!(*runtime.calls.lock().unwrap(), vec![false]);
}

#[tokio::test]
async fn added_write_apis_fail_closed_for_unrelated_existing_store_fakes() {
    let store = Memory::new(normal(), None);
    assert!(store
        .insert_task(Host::Local, doc! {"name": "new"})
        .await
        .unwrap_err()
        .contains("unavailable"));
    assert!(store
        .update_items(
            Host::Local,
            NAME,
            doc! {},
            doc! {"$set": {"runId": "mission:fixture"}}
        )
        .await
        .unwrap_err()
        .contains("unavailable"));
    store.no_write();
    assert!(store.0.lock().unwrap().calls.is_empty());
}


#[tokio::test]
async fn initial_pause_message_is_request_only_before_lock_and_stop() {
    for budget in [StopBudget::Cli, StopBudget::Gui] {
        let root = Root::new();
        let store = Memory::new(normal(), Some(vec![]));
        let mut runtime = Runtime::new(&store, false); runtime.budget = budget;
        let messages = Arc::new(Mutex::new(vec![])); let saved = messages.clone();
        let lock = mission_lock::MissionLocks::acquire(&root.0, &[NAME], false).unwrap();
        let out = pause::pause(&root.0, &store, &runtime, NAME, PauseOptions {
            observer: Some(Arc::new(move |stage| saved.lock().unwrap().push(stage))),
            ..Default::default()
        }).await;
        assert_eq!(out.exit_code, 1);
        assert!(runtime.calls.lock().unwrap().is_empty());
        let stages = messages.lock().unwrap();
        assert_eq!(stages[0].stage, "initial");
        assert!(stages[0].message.contains("尚未取得生命周期锁"));
        assert!(stages[0].message.contains("尚未执行停止"));
        assert!(!stages[0].message.contains("accepted"));
        assert!(stages[0].message.contains(&format!("budget{}s", budget.seconds())));
        assert!(stages[0].message.contains("one attempt"));
        assert_eq!(stages.last().unwrap().stage, "failed");
        drop(lock);
    }
}

#[tokio::test]
async fn committed_pause_lost_ack_is_confirmed_by_exact_readback_without_second_stop() {
    let s = Memory::new(normal(), None);
    s.0.lock().unwrap().write_timeout = true;
    let r = Runtime::new(&s, false);
    let out = run(&s, &r, &Root::new()).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert!(out.confirmed_stop.is_some());
    assert_eq!(r.calls.lock().unwrap().len(), 1);
    let d = s.0.lock().unwrap();
    assert_eq!(d.writes.len(), 1);
    assert_eq!(d.tasks[&Host::Local][0].get_array("events").unwrap().len(), 1);
}

#[path = "support/original_professional_runtime.rs"]
mod original_professional_runtime;
use original_professional_runtime::original_professional_fixture;

#[tokio::test]
async fn public_pause_original_runtime_and_shell_recover_missing_proof_and_lost_stop_receipt() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,
        operations_runtime::{OriginalOperationsRuntime, OriginalPauseRuntime}};
    for remote in [false, true] {
        for lose_receipt in [false, true] {
            let fake = original_professional_fixture(lose_receipt);
            let root = Path::new(&fake.config.env.working_dir);
            if lose_receipt { std::fs::write(root.join("state"), "active").unwrap(); }
            let binding = validate_backend_binding(&fake.config).await.unwrap();
            let runtime = OriginalPauseRuntime { common: OriginalOperationsRuntime { binding:&binding, observer:None }, budget:StopBudget::Cli };
            let mut t=normal(); t.insert("isRemote",remote); t.remove("tmuxContext");t.remove("startupBinding");
            t.insert("taskType","super");t.insert("auth_profile","user1");
            if !lose_receipt { t.extend(doc! {"status":"launching","launchSettledAttemptId":Bson::Null,
                "startupBinding":{"consumer_run_id":"older-startup"},"simulatorRuntime":{"consumer_run_id":"older-run"},
                "acceptedObservation":{"id":"old-observation","finishedAt":Bson::Null}}); }
            original_professional_runtime::install_original_super_process(root,ID,NAME,remote,"launch-1");
            if lose_receipt {t.insert("tmuxContext",doc!{"sessionId":"$4"});}
            let evidence=vec![doc! {"_id":"unresolved","acceptance":"UNKNOWN","superSlotId":"kept","status":"timeout_error"}];
            let store=Memory::new(t, Some(evidence.clone()));
            let out=pause::pause(root,&store,&runtime,NAME,PauseOptions::default()).await;
            assert_eq!(out.exit_code,0,"remote={remote} lost={lose_receipt}: {}",out.value);
            assert_eq!(out.value["confirmationBasis"],"session_absent");
            assert_eq!(out.value["stopConfirmation"],"confirmed");
            if lose_receipt { assert_eq!(out.value["reconciledFromObservation"],true); }
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();
            let signals=trace.lines().filter(|line| line.contains("if-shell")).count();
            assert_eq!(signals,usize::from(lose_receipt),"verify-only must not signal; stop at most once");
            let d=store.0.lock().unwrap();assert_eq!(d.writes.len(),1);
            assert_eq!(d.items[&Host::from_remote(remote)],evidence);
            assert_eq!(d.tasks[&Host::Local][0].get_array("events").unwrap().len(),1);
        }
    }
}

#[tokio::test]
async fn public_pause_refuses_actual_local_writer_even_for_remote_task_without_generation_metadata() {
    use quantnight_task_core::{backend_binding::validate_backend_binding, task_control::TaskControl};
    let fake = original_professional_fixture(false);
    let root = Path::new(&fake.config.env.working_dir);
    let ps = root.join("bin/ps");
    let original = std::fs::read_to_string(&ps).unwrap();
    std::fs::write(&ps, format!(r#"{original}
if [[ "$*" == *comm=* ]]; then echo '43 1 python3'; fi
"#)).unwrap();
    std::fs::write(root.join("psutil.py"), r#"
class Error(Exception): pass
class NoSuchProcess(Error): pass
class Process:
 def __init__(self,pid):
  if pid != 43: raise NoSuchProcess(pid)
 def create_time(self): return 1700000030.0
 def cmdline(self): return ['python3','-m','backend.mongodb.mongo_datum','--alpha_filename=fixture']
 def is_running(self): return True
"#).unwrap();
    let mut task = normal();
    task.insert("isRemote", true);
    task.remove("generation");
    let store = Arc::new(Memory::new(task.clone(), Some(items())));
    let binding = validate_backend_binding(&fake.config).await.unwrap();
    let control = TaskControl::with_store(binding, store.clone());
    let out = control.pause(NAME, PauseOptions::default(), StopBudget::Cli, None).await;
    failed(out, "mongo_datum writer still present");
    assert_eq!(store.task(), task);
    store.no_write();
    assert!(!root.join("trace").exists(), "stop cannot inspect or signal tmux before the live writer is fenced");
}

#[tokio::test]
async fn original_pause_never_signals_foreign_identity_or_confirms_known_live_orphan() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::{OriginalOperationsRuntime,OriginalPauseRuntime}};
    for case in ["foreign_task","stale_saved_birth","known_live_after_stop"] {
        let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
        std::fs::write(root.join("state"),"active").unwrap();
        original_professional_runtime::install_original_super_process(root,if case=="foreign_task"{"ffffffffffffffffffffffff"}else{ID},NAME,false,"launch-1");
        if case=="known_live_after_stop" {
            let p=root.join("psutil.py");let s=std::fs::read_to_string(&p).unwrap()
                .replace(" or not Path(os.environ['STATE']).exists()", "")
                .replace("def is_running(self): return Path(os.environ['STATE']).exists()", "def is_running(self): return self.pid==1002 or Path(os.environ['STATE']).exists()");
            std::fs::write(p,s).unwrap();
        }
        let mut t=normal();t.remove("tmuxContext");t.remove("startupBinding");t.insert("taskType","super");t.insert("auth_profile","user1");
        if case=="stale_saved_birth" {t.insert("simulatorRuntime",doc!{"pid":1002,"pidCreatedAt":1700000999.0,"consumer_run_id":"original-run"});}
        let store=Memory::new(t.clone(),Some(items()));
        let binding=validate_backend_binding(&fake.config).await.unwrap();
        let runtime=OriginalPauseRuntime{common:OriginalOperationsRuntime{binding:&binding,observer:None},budget:StopBudget::Cli};
        let out=pause::pause(root,&store,&runtime,NAME,PauseOptions::default()).await;
        let stopped = case == "stale_saved_birth";
        assert_eq!(out.exit_code, i32::from(!stopped), "{case}: {}", out.value);
        if stopped {
            assert!(out.confirmed_stop.is_some());
            assert_eq!(store.task().get_str("status"), Ok("paused"));
        } else {
            assert!(out.confirmed_stop.is_none());store.no_write();assert_eq!(store.task(),t);
        }
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("if-shell")).count(), usize::from(case != "foreign_task"));
        assert!(!trace.contains("new-session"));
    }
}

#[tokio::test]
async fn original_stop_adapter_zero_remaining_never_restarts_a_budget_or_process() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::{OriginalOperationsRuntime,OriginalPauseRuntime}};
    for budget in [StopBudget::Cli,StopBudget::Gui] {
        let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
        let binding=validate_backend_binding(&fake.config).await.unwrap();
        let runtime=OriginalPauseRuntime{common:OriginalOperationsRuntime{binding:&binding,observer:None},budget};
        assert!(runtime.stop_bounded(root,&normal(),false,std::time::Duration::ZERO).await.is_err());
        assert!(runtime.verify_stopped(root,&normal(),std::time::Duration::ZERO).await.is_err());
        assert!(!root.join("trace").exists(),"no original stop/check process after zero allowance");
    }
}

#[tokio::test]
async fn completion_frozen_pause_absent_exact_and_remaining_are_locked_guards() {
    for case in ["missing","null","present","malformed","empty","both","empty_exact","zero","exact","omitted"] {
        let root=Root::new(); let mut task=normal();
        match case {"missing"|"omitted"=>{task.remove("launchAttemptId");},"null"=>{task.insert("launchAttemptId",Bson::Null);},
            "malformed"=>{task.insert("launchAttemptId",17);},"empty"=>{task.insert("launchAttemptId","");},_=>()};
        let store=Memory::new(task,None); let runtime=Runtime::new(&store,false);
        let out=pause::pause(&root.0,&store,&runtime,NAME,PauseOptions{
            expected_launch_absent:!matches!(case,"exact"|"omitted"|"empty_exact"|"zero"),
            expected_launch_attempt_id:if matches!(case,"exact"|"both"){Some("launch-1")}else if case=="empty_exact"{Some("")}else{None},
            remaining:if case=="zero"{Some(std::time::Duration::ZERO)}else{None},..Default::default()
        }).await;
        let accepted=matches!(case,"missing"|"null"|"exact"|"omitted");
        assert_eq!(out.exit_code,if accepted{0}else{1},"{case}: {}",out.value);
        assert_eq!(runtime.calls.lock().unwrap().len(),usize::from(accepted),"{case}");
        if !accepted {store.no_write();}
    }
}

#[tokio::test]
async fn completion_original_explicit_legacy_file_argv_pauses_without_optional_proofs() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::{OriginalOperationsRuntime,OriginalPauseRuntime}};
    for remote in [false,true] {for kind in ["regular","super","priority"] {
        let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
        std::fs::write(root.join("state"),"active").unwrap();
        original_professional_runtime::install_original_super_process(root,ID,NAME,remote,"launch-1");
        let p=root.join("psutil.py");let source=std::fs::read_to_string(&p).unwrap();
        let mut lines=Vec::new();
        for line in source.lines() {
            if let Some(raw)=line.strip_prefix("ARGV=") {
                let mut argv:Vec<String>=serde_json::from_str(raw).unwrap();
                argv[2]=format!("scripts.{}_simulate_param",if kind=="regular"{"alpha"}else if kind=="super"{"super_alpha"}else{"refine"});
                *argv.last_mut().unwrap()="/original/tmp/tmp_config_fixture.json".into();
                lines.push(format!("ARGV={}",serde_json::to_string(&argv).unwrap()));
            } else {lines.push(line.into());}
        }
        std::fs::write(p,lines.join("\n")).unwrap();
        let mut task=normal();task.remove("startupBinding");task.remove("tmuxContext");task.remove("launchAttemptId");
        task.insert("taskType",kind);task.insert("auth_profile","user1");task.insert("isRemote",remote);
        let evidence=items(); let store=Memory::new(task,Some(evidence.clone()));
        let binding=validate_backend_binding(&fake.config).await.unwrap();
        let runtime=OriginalPauseRuntime{common:OriginalOperationsRuntime{binding:&binding,observer:None},budget:StopBudget::Cli};
        let out=pause::pause(root,&store,&runtime,NAME,PauseOptions::default()).await;
        assert_eq!(out.exit_code,0,"{kind}/{remote}: {}",out.value);
        assert_eq!(out.value["confirmationBasis"],"runners_exited");
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("if-shell")).count(),1);
        assert!(!trace.contains("new-session"));
        assert_eq!(store.0.lock().unwrap().items[&Host::from_remote(remote)],evidence);
    }}
}

#[tokio::test]
async fn completion_zero_process_budget_and_facade_remaining_never_spawn() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl,config::run_process_observed};
    let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
    let mut command=tokio::process::Command::new("/bin/bash");
    command.arg("-c").arg("echo escaped > zero-spawn").current_dir(root);
    assert!(run_process_observed(command,std::time::Duration::ZERO,false,None).await.is_err());
    assert!(!root.join("zero-spawn").exists());
    let binding=validate_backend_binding(&fake.config).await.unwrap();
    let store=Arc::new(Memory::new(normal(),None));
    let control=TaskControl::with_store(binding,store.clone());
    // Force an original binding refresh if wrongly admitted at zero budget.
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).open(root.join("credentials.yaml")).unwrap().write_all(b"\n#changed\n").unwrap();
    let p=root.join(".venv/bin/python");let source=std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p,source.replace("set -euo pipefail",&format!("set -euo pipefail\necho spawned > '{}'",root.join("binding-spawn").display()))).unwrap();
    for budget in [StopBudget::Cli,StopBudget::Gui] {
        let out=control.pause(NAME,PauseOptions{remaining:Some(std::time::Duration::ZERO),..Default::default()},budget,None).await;
        assert_eq!(out.exit_code,1);assert!(!root.join("binding-spawn").exists());assert!(!root.join("trace").exists());
    }
    store.no_write();
}

#[tokio::test]
async fn completion_caller_remaining_includes_original_facade_binding_refresh() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
    let binding=validate_backend_binding(&fake.config).await.unwrap();
    let store=Arc::new(Memory::new(normal(),None));let control=TaskControl::with_store(binding,store.clone());
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).open(root.join("credentials.yaml")).unwrap().write_all(b"\n#new fingerprint\n").unwrap();
    let p=root.join(".venv/bin/python");let source=std::fs::read_to_string(&p).unwrap();
    std::fs::write(p,source.replace("set -euo pipefail","set -euo pipefail\n/bin/sleep 0.1")).unwrap();
    let out=control.pause(NAME,PauseOptions{remaining:Some(std::time::Duration::from_millis(10)),..Default::default()},StopBudget::Gui,None).await;
    assert_eq!(out.exit_code,1);assert!(out.confirmed_stop.is_none());
    assert!(!root.join("trace").exists(),"no stop/check after caller remaining was consumed by binding");
    assert_ne!(out.value["operationSettled"],true);store.no_write();
}

#[tokio::test]
async fn fc_p1_1_public_pause_grace_root_replacement_never_confirms_or_cas() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,task_control::TaskControl};
    for remote in [false,true] {
        let fake=original_professional_fixture(false);let root=Path::new(&fake.config.env.working_dir);
        let backend=std::env::var("QUANTNIGHT_BACKEND_SOURCE").unwrap();
        let python=std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap();
        let code="import sys\nfrom test_system_tmux_context import install_grace_pane_replacement\ninstall_grace_pane_replacement(sys.argv[1],sys.argv[2],name='fixture',remote=sys.argv[3]=='true',launch='launch-1')";
        let setup=std::process::Command::new(python).args(["-B","-c",code]).arg(root).arg(ID).arg(if remote{"true"}else{"false"})
            .env("PYTHONPATH",format!("{backend}:{backend}/tests")).env("PYTHONDONTWRITEBYTECODE","1").output().unwrap();
        assert!(setup.status.success(),"{}",String::from_utf8_lossy(&setup.stderr));
        let mut task=normal();task.remove("tmuxContext");task.remove("startupBinding");
        task.insert("isRemote",remote);task.insert("taskType","super");task.insert("auth_profile","user1");
        let store=Arc::new(Memory::new(task.clone(),Some(items())));
        let binding=validate_backend_binding(&fake.config).await.unwrap();let control=TaskControl::with_store(binding,store.clone());
        let out=control.pause(NAME,PauseOptions::default(),StopBudget::Cli,None).await;
        assert_eq!(out.exit_code,1,"remote={remote} successorKilled={}: {}",root.join("successor-killed").exists(),out.value);
        assert!(out.confirmed_stop.is_none());store.no_write();assert_eq!(store.task(),task);
        assert!(root.join("grace-replaced").exists());
        assert_eq!(std::fs::read_to_string(root.join("state")).unwrap(),"respawned");
        assert!(!root.join("successor-killed").exists());
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert_eq!(trace.lines().filter(|l|l.contains("if-shell")).count(),1);
        assert!(!trace.contains("kill-session")&&!trace.contains("new-session"));
    }
}

// The original inspector validates the primary consumer; its descendants inherit
// that root's scope without needing their own business argv or helper whitelist.
// Keep the original professional script fixture and change only leaf OS facts.
fn explicit_pause_tree(root: &Path, kind: &str, remote: bool) -> Document {
    original_professional_runtime::install_original_super_process(root, ID, NAME, remote, "launch-1");
    std::fs::write(root.join("state"), "active").unwrap();
    let path = root.join("psutil.py");
    let source = std::fs::read_to_string(&path).unwrap();
    let source = source.replace("scripts.super_alpha_simulate_param", match kind {
        "regular" => "scripts.alpha_simulate_param",
        "super" => "scripts.super_alpha_simulate_param",
        "priority" => "scripts.refine_simulate_param",
        _ => panic!("unexpected fixture type"),
    }).replace("(1001,1002)", "(1001,1002,1003,1004,1005)")
      .replace("def create_time(self): return 1700000010.0 if self.pid==1002 else 1700000000.0",
          "def create_time(self): return 1700000000.0 + (self.pid-1001)*10\n def ppid(self): return {1001:1,1002:1001,1003:1002,1004:1002,1005:1003}[self.pid]")
      .replace("return ARGV if self.pid==1002 else ['bash']",
          "return {1001:['bash'],1002:ARGV,1003:['python','-c','import worker_pool; worker_pool.serve()'],1004:['node','queue-worker.js'],1005:['/opt/tools/cache-agent','--stdio']}[self.pid]");
    std::fs::write(path, source).unwrap();
    let ps = root.join("bin/ps");
    let source = std::fs::read_to_string(&ps).unwrap()
        .replace("echo \"1002 1001 Mon Jan 1 00:00:01 2024 S\";",
            "echo \"1002 1001 Mon Jan 1 00:00:01 2024 S\"; echo \"1003 1002 Mon Jan 1 00:00:02 2024 S\"; echo \"1004 1002 Mon Jan 1 00:00:03 2024 S\"; echo \"1005 1003 Mon Jan 1 00:00:04 2024 S\";")
        .replace("echo \"1002 1001 python\"", "echo \"1002 1001 python\"; echo \"1003 1002 python\"; echo \"1004 1002 node\"; echo \"1005 1003 cache-agent\"");
    assert!(source.contains("1005 1003 Mon"), "original ps fixture changed");
    std::fs::write(ps, source).unwrap();
    let mut task = normal();
    task.remove("startupBinding");
    task.insert("taskType", kind);
    task.insert("auth_profile", "user1");
    task.insert("isRemote", remote);
    task.insert("tmuxContext", doc! {
        "schemaVersion":1,"backend":"tmux","socketName":"default",
        "binary":std::fs::canonicalize(root.join("native-tmux")).unwrap().to_string_lossy().as_ref(),
        "taskId":ID,"launchAttemptId":"launch-1","remote":remote,
        "sessionId":"$4","sessionCreatedAt":1700000000000i64,
    });
    task
}

async fn explicit_pause_arbitrary_descendants(kind: &str, remote: bool) {
    use quantnight_task_core::{backend_binding::validate_backend_binding, task_control::TaskControl};
    let fake = original_professional_fixture(false);
    let root = Path::new(&fake.config.env.working_dir);
    let task = explicit_pause_tree(root, kind, remote);
    let evidence = vec![doc! {"_id":"accepted-or-unknown","status":"timeout_error",
        "acceptance":"UNKNOWN","superSlotId":"preserved","progressUrl":"original-receipt"}];
    let store = Arc::new(Memory::new(task, Some(evidence.clone())));
    let binding = validate_backend_binding(&fake.config).await.unwrap();
    let control = TaskControl::with_store(binding, store.clone());
    let out = control.pause(NAME, PauseOptions {
        expected_task_id:Some(ID),expected_remote:Some(remote),
        expected_launch_attempt_id:Some("launch-1"),..Default::default()
    }, StopBudget::Cli, None).await;
    assert_eq!(out.exit_code, 0, "{kind}/{remote}: {}", out.value);
    assert!(out.confirmed_stop.is_some());
    assert_eq!(out.value["stopConfirmation"], "confirmed");
    assert_eq!(out.value["confirmationBasis"], "runners_exited");
    assert!(!root.join("state").exists());
    let trace = std::fs::read_to_string(root.join("trace")).unwrap();
    let signals:Vec<_> = trace.lines().filter(|line| line.contains("if-shell")).collect();
    assert_eq!(signals.len(), 1, "one original interrupt, no retry");
    assert!(signals[0].contains("send-keys -t '$4' C-c"));
    assert!(!trace.contains("kill-session") && !trace.contains("new-session"));
    let data = store.0.lock().unwrap();
    assert_eq!(data.writes.len(), 1);
    assert_eq!(data.tasks[&Host::Local][0].get_str("status").unwrap(), "paused");
    assert_eq!(data.tasks[&Host::Local][0].get_array("events").unwrap().len(), 1);
    assert_eq!(data.items[&Host::from_remote(remote)], evidence);
}

macro_rules! explicit_pause_descendant_case {
    ($name:ident, $kind:literal, $remote:literal) => {
        #[tokio::test]
        async fn $name() { explicit_pause_arbitrary_descendants($kind, $remote).await; }
    };
}
explicit_pause_descendant_case!(generic_explicit_pause_regular_local, "regular", false);
explicit_pause_descendant_case!(generic_explicit_pause_regular_remote, "regular", true);
explicit_pause_descendant_case!(generic_explicit_pause_super_local, "super", false);
explicit_pause_descendant_case!(generic_explicit_pause_super_remote, "super", true);
explicit_pause_descendant_case!(generic_explicit_pause_priority_local, "priority", false);
explicit_pause_descendant_case!(generic_explicit_pause_priority_remote, "priority", true);

#[tokio::test]
async fn generic_explicit_pause_keeps_exact_caller_identity_but_accepts_fresh_process_scope() {
    use quantnight_task_core::{backend_binding::validate_backend_binding, task_control::TaskControl};
    const ALL_CASES: &[&str] = &["request_task", "request_host", "request_launch", "context_task", "context_host", "context_launch", "session_id", "session_birth"];
    const REP_CASES: &[&str] = &["request_task", "context_host", "session_id"];
    for kind in ["regular", "super", "priority"] { for remote in [false, true] {
        let cases = if kind == "regular" && !remote { ALL_CASES } else { REP_CASES };
        for &case in cases {
            let fake = original_professional_fixture(false);
            let root = Path::new(&fake.config.env.working_dir);
            let mut task = explicit_pause_tree(root, kind, remote);
            let context = task.get_document_mut("tmuxContext").unwrap();
            match case {
                "context_task" => {context.insert("taskId", "ffffffffffffffffffffffff");},
                "context_host" => {context.insert("remote", !remote);},
                "context_launch" => {context.insert("launchAttemptId", "replacement-launch");},
                "session_id" => {context.insert("sessionId", "$5");},
                "session_birth" => {context.insert("sessionCreatedAt", 1700000999000i64);},
                _ => (),
            }
            let evidence = items();
            let store = Arc::new(Memory::new(task.clone(), Some(evidence.clone())));
            let binding = validate_backend_binding(&fake.config).await.unwrap();
            let control = TaskControl::with_store(binding, store.clone());
            let out = control.pause(NAME, PauseOptions {
                expected_task_id:Some(if case=="request_task" {"ffffffffffffffffffffffff"} else {ID}),
                expected_remote:Some(if case=="request_host" {!remote} else {remote}),
                expected_launch_attempt_id:Some(if case=="request_launch" {"replacement-launch"} else {"launch-1"}),
                ..Default::default()
            }, StopBudget::Cli, None).await;
            // SUPER fixture has a task/host identity in the actual argv,
            // which supersedes an old saved context. Legacy regular/priority
            // argv alone cannot adopt a replacement. Explicit caller
            // mismatches always fail before inspecting/signalling.
            let refused = case.starts_with("request_") || (kind != "super" && case != "context_launch");
            assert_eq!(out.exit_code, i32::from(refused), "{kind}/{remote}/{case}: {}", out.value);
            assert_eq!(store.0.lock().unwrap().items[&Host::from_remote(remote)], evidence);
            let trace = std::fs::read_to_string(root.join("trace")).unwrap_or_default();
            if refused {
                let expected = match case {
                    "request_task" => "task identity changed",
                    "request_host" => "task execution host changed",
                    "request_launch" => "launch identity changed",
                    "session_id" | "session_birth" => "saved context conflicts with observed session",
                    _ => "conflicting saved context",
                };
                assert!(out.value["error"].as_str().unwrap().contains(expected), "{case}: {}", out.value);
                assert!(out.confirmed_stop.is_none());
                store.no_write();
                assert_eq!(store.task(), task);
                assert_eq!(std::fs::read_to_string(root.join("state")).unwrap(), "active");
                assert!(!trace.contains("if-shell") && !trace.contains("kill-session") && !trace.contains("new-session"), "{case}: {trace}");
            } else {
                assert!(out.confirmed_stop.is_some());
                assert_eq!(store.task().get_str("status"), Ok("paused"));
                assert!(!root.join("state").exists(), "fresh task process scope was stopped");
                assert_eq!(trace.lines().filter(|line| line.contains("if-shell")).count(), 1);
            }
        }
    }}
}

#[tokio::test]
async fn generic_explicit_pause_captured_pane_replacement_and_live_descendant_never_confirm() {
    use quantnight_task_core::{backend_binding::validate_backend_binding, task_control::TaskControl};
    const ALL_CASES: &[&str] = &["pane_replaced", "pane_birth", "orphan_descendant"];
    const REP_CASES: &[&str] = &["pane_replaced"];
    for kind in ["regular", "super", "priority"] { for remote in [false, true] {
        let cases = if kind == "regular" && !remote { ALL_CASES } else { REP_CASES };
        for &case in cases {
            let fake = original_professional_fixture(false);
            let root = Path::new(&fake.config.env.working_dir);
            let task = explicit_pause_tree(root, kind, remote);
            if case == "pane_replaced" {
                let path = root.join("native-tmux");
                let source = std::fs::read_to_string(&path).unwrap()
                    .replace("else '$4|1001|0' if", "else '$4|2001|0' if");
                assert!(source.contains("$4|2001|0"));
                std::fs::write(path, source).unwrap();
            } else {
                let path = root.join("bin/ps");
                let source = std::fs::read_to_string(&path).unwrap();
                let source = if case == "pane_birth" {
                    source.replace("if [[ \"$*\" == *lstart* ]]; then", "if [[ \"$*\" == *lstart* ]]; then\n if [[ -f \"$STATE.snapshot\" ]]; then\n echo \"9 1 Mon Jan 1 00:00:01 2024 S\"; echo \"1001 1 Tue Jan 2 00:00:01 2024 S\"; exit 0\n fi\n echo captured > \"$STATE.snapshot\"")
                } else {
                    source.replace("echo \"9 1 Mon Jan 1 00:00:01 2024 S\"", "echo \"9 1 Mon Jan 1 00:00:01 2024 S\"; if [[ ! -f \"$STATE\" ]]; then echo \"1005 1 Mon Jan 1 00:00:04 2024 S\"; fi")
                };
                std::fs::write(path, source).unwrap();
            }
            let store = Arc::new(Memory::new(task.clone(), Some(items())));
            let binding = validate_backend_binding(&fake.config).await.unwrap();
            let control = TaskControl::with_store(binding, store.clone());
            let out = control.pause(NAME, PauseOptions::default(), StopBudget::Cli, None).await;
            assert_eq!(out.exit_code, 1, "{kind}/{remote}/{case}: {}", out.value);
            let expected = match case {
                "pane_replaced" => "original pane root replaced or added",
                "pane_birth" => "original pane root PID birth changed",
                _ => "known_original_runners_still_live",
            };
            assert!(out.value["error"].as_str().unwrap().contains(expected), "{case}: {}", out.value);
            assert!(out.confirmed_stop.is_none());
            store.no_write(); assert_eq!(store.task(), task);
            let trace = std::fs::read_to_string(root.join("trace")).unwrap();
            assert_eq!(trace.lines().filter(|line| line.contains("if-shell")).count(), usize::from(case=="orphan_descendant"), "{case}: {trace}");
            assert!(!trace.contains("kill-session") && !trace.contains("new-session"));
        }
    }}
}

#[tokio::test]
async fn owned_wrapper_residue_without_tmux_context_pauses_and_settles_while_plain_shell_refuses() {
    use quantnight_task_core::{backend_binding::validate_backend_binding, task_control::TaskControl};
    for owned in [true, false] {
        let fake = original_professional_fixture(false);
        let root = Path::new(&fake.config.env.working_dir);
        std::fs::write(root.join("state"), "active").unwrap();
        let ps = root.join("bin/ps");
        let ps_src = std::fs::read_to_string(&ps)
            .unwrap()
            .replace("echo \"1002 1001 python\"", "echo \"1002 1001 sleep\"");
        std::fs::write(&ps, ps_src).unwrap();
        let wrapper_argv = if owned {
            serde_json::json!([
                "zsh",
                "-c",
                "cd . && if uv run -m scripts.alpha_simulate_param --task fixture --dist local --auth_profile user1 --config '{}'; then echo done; fi; sleep 9999999"
            ])
        } else {
            serde_json::json!(["zsh"])
        };
        std::fs::write(
            root.join("psutil.py"),
            format!(
                r#"import os
from pathlib import Path
STATUS_ZOMBIE='zombie'
WRAPPER={wrapper_argv}
class NoSuchProcess(Exception): pass
class AccessDenied(Exception): pass
class Error(Exception): pass
class Process:
 def __init__(self,pid):
  self.pid=pid
  if pid not in (1001,1002) or not Path(os.environ['STATE']).exists(): raise NoSuchProcess(pid)
 def create_time(self): return 1700000001.0 if self.pid==1002 else 1700000000.0
 def ppid(self): return 1001 if self.pid==1002 else 1
 def cwd(self): return str(Path(os.environ['STATE']).parent)
 def cmdline(self): return ['sleep','9999999'] if self.pid==1002 else WRAPPER
 def is_running(self): return Path(os.environ['STATE']).exists()
 def status(self): return 'running'
"#
            ),
        )
        .unwrap();
        let mut task = doc! {
            "_id": ObjectId::parse_str(ID).unwrap(),
            "name": NAME,
            "status": "completed_with_errors",
            "taskType": "regular",
            "auth_profile": "user1",
            "isRemote": false,
            "launchAttemptId": "launch-unsettled",
            "launchSettledAttemptId": Bson::Null,
            "launchAttemptAt": DateTime::from_millis(1699999990000),
        };
        let store = Arc::new(Memory::new(task.clone(), Some(items())));
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let control = TaskControl::with_store(binding, store.clone());
        let out = control.pause(NAME, PauseOptions::default(), StopBudget::Cli, None).await;
        if owned {
            assert_eq!(out.exit_code, 0, "owned residue should pause cleanly: {}", out.value);
            let saved = store.task();
            assert_eq!(saved.get_str("status"), Ok("paused"));
            assert_eq!(saved.get_str("launchSettledAttemptId"), Ok("launch-unsettled"));
            assert!(!root.join("state").exists(), "tmux session must be stopped");
        } else {
            assert_eq!(out.exit_code, 1, "plain interactive shell without tmuxContext must fail closed: {}", out.value);
            assert!(out.value["error"].as_str().unwrap_or_default().contains("execution scope unconfirmed"));
            store.no_write();
            task.insert("status", "completed_with_errors");
            assert!(root.join("state").exists(), "foreign session must not be signaled");
        }
    }
}
