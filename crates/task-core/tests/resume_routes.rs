use quantnight_task_core::{cleanup_sync, lifecycle_policy, mission_lock, resume, task_store};
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use cleanup_sync::{OperationsRuntime, TaskOutcome};
use resume::ResumeRequest;
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
            "qn-resume-{}-{}",
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
fn task(remote: bool) -> Document {
    doc! {"_id":ObjectId::parse_str(ID).unwrap(),"name":NAME,"status":"paused","isRemote":remote,"taskType":"regular","auth_profile":"fixture-profile","updatedAt":"revision-1","launchAttemptId":"launch-1","tmuxContext":{"sessionId":"$1"}}
}
fn row(id: i32, status: &str) -> Document {
    doc! {"_id":id,"status":status,"_not_posted_":true,"acceptance":"NOT_POSTED","regular":"DO NOT FETCH OR MODIFY payload","result":{"large":"unchanged"}}
}
fn matches(row: &Document, q: &Document) -> bool {
    q.iter().all(|(key, condition)| {
        if key == "$and" {
            return condition
                .as_array()
                .unwrap()
                .iter()
                .all(|v| matches(row, v.as_document().unwrap()));
        }
        let actual = row.get(key);
        match condition {
            Bson::Document(op) if op.contains_key("$exists") => {
                row.contains_key(key) == op.get_bool("$exists").unwrap()
            }
            Bson::Document(op) if op.contains_key("$ne") => actual != op.get("$ne"),
            Bson::Document(op) if op.contains_key("$in") => {
                let set = op.get_array("$in").unwrap();
                match actual {
                    Some(Bson::Array(values)) => values.iter().any(|v| set.contains(v)),
                    Some(v) => set.contains(v),
                    None => set.contains(&Bson::Null),
                }
            }
            Bson::Null => matches!(actual, None | Some(Bson::Null)),
            _ => actual == Some(condition),
        }
    })
}
fn project(row: &Document, p: &Document) -> Document {
    assert_eq!(p, &task_store::receipt_projection());
    row.iter()
        .filter(|(k, _)| p.get(*k) == Some(&Bson::Int32(1)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
#[derive(Default)]
struct Data {
    controls: Vec<Document>,
    items: HashMap<Host, Vec<Document>>,
    calls: Vec<String>,
    queries: Vec<(Host, Document, Document, usize)>,
    item_writes: Vec<(Host, Document, Document)>,
    control_writes: Vec<(Document, Document)>,
    fail: Option<String>,
    after_read_control: Option<Document>,
    after_first_write_control: Option<Document>,
    before_write_patch: Option<Document>,
    before_write_remove: Option<String>,
    fail_write_at: Option<usize>,
    apply_then_fail: bool,
    control_cas_zero: bool,
    item_cas_zero: bool,
    control_write_timeout: bool,
}
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Data>>);
impl Memory {
    fn new(t: Document, rows: Vec<Document>) -> Self {
        let host = Host::from_remote(t.get_bool("isRemote").unwrap_or(false));
        let s = Self::default();
        {
            let mut d = s.0.lock().unwrap();
            d.controls = vec![t];
            d.items.insert(host, rows);
        }
        s
    }
    fn control(&self) -> Document {
        self.0.lock().unwrap().controls[0].clone()
    }
    fn rows(&self, h: Host) -> Vec<Document> {
        self.0
            .lock()
            .unwrap()
            .items
            .get(&h)
            .cloned()
            .unwrap_or_default()
    }
    fn no_writes(&self) {
        let d = self.0.lock().unwrap();
        assert!(d.item_writes.is_empty() && d.control_writes.is_empty());
    }
}
fn call(d: &mut Data, name: &str) -> Result<(), String> {
    d.calls.push(name.into());
    if d.fail.as_deref() == Some(name) {
        return Err(format!("injected {name} unavailable/timeout"));
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
        assert_eq!(h, Host::Local);
        let mut d = self.0.lock().unwrap();
        call(&mut d, "control-read")?;
        let mut rows = d.controls.clone();
        if newest {
            rows.sort_by_key(|v| format!("{:?}", v.get("_id")));
            rows.reverse();
        }
        Ok(rows.into_iter().find(|r| matches(r, &q)))
    }
    async fn find_items(
        &self,
        h: Host,
        n: &str,
        q: Document,
        p: Option<Document>,
    ) -> Result<Vec<Document>, String> {
        assert_eq!(n, NAME);
        let p = p.expect("resume must project receipt fields");
        let mut d = self.0.lock().unwrap();
        call(&mut d, "find-items")?;
        let rows: Vec<_> = d
            .items
            .get(&h)
            .into_iter()
            .flatten()
            .filter(|v| matches(v, &q))
            .map(|v| project(v, &p))
            .collect();
        d.queries.push((h, q, p, rows.len()));
        if let Some(patch) = d.after_read_control.take() {
            d.controls[0].extend(patch);
        }
        Ok(rows)
    }
    async fn update_item(&self, h: Host, n: &str, q: Document, u: Document) -> Result<u64, String> {
        assert_eq!(n, NAME);
        let mut d = self.0.lock().unwrap();
        call(&mut d, "item-write")?;
        d.item_writes.push((h, q.clone(), u.clone()));
        let count = d.item_writes.len();
        if let Some(patch) = d.before_write_patch.take() {
            d.items.get_mut(&h).unwrap()[0].extend(patch);
        }
        if let Some(field) = d.before_write_remove.take() {
            d.items.get_mut(&h).unwrap()[0].remove(field);
        }
        let fail = d.fail_write_at == Some(count);
        if fail && !d.apply_then_fail {
            return Err("item write timeout; result unconfirmed, not retried".into());
        }
        if d.item_cas_zero {
            return Ok(0);
        }
        let result = if let Some(row) = d
            .items
            .entry(h)
            .or_default()
            .iter_mut()
            .find(|r| matches(r, &q))
        {
            let before = row.clone();
            row.extend(u.get_document("$set").unwrap().clone());
            u64::from(before != *row)
        } else {
            0
        };
        if result > 0 && count == 1 {
            if let Some(patch) = d.after_first_write_control.take() {
                d.controls[0].extend(patch);
            }
        }
        if fail {
            return Err("item write timeout after possible application; not retried".into());
        }
        Ok(result)
    }
    async fn update_task(&self, h: Host, q: Document, u: Document) -> Result<u64, String> {
        assert_eq!(h, Host::Local);
        let mut d = self.0.lock().unwrap();
        call(&mut d, "control-write")?;
        d.control_writes.push((q.clone(), u.clone()));
        if d.control_cas_zero {
            return Ok(0);
        }
        let timeout = d.control_write_timeout;
        let Some(row) = d.controls.iter_mut().find(|r| matches(r, &q)) else {
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
            return Err("control write timeout after possible application; not retried".into());
        }
        Ok(1)
    }
    async fn rows(&self, _: Host, _: &str, _: Option<Document>) -> Result<Vec<Document>, String> {
        panic!("resume must filter at Store/server, not scan the task")
    }
    async fn summary(&self, _: Host, _: &str) -> Result<CollectionSummary, String> {
        panic!("resume does not infer noPOST from counts")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("resume cannot truncate by a probe")
    }
    async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
        panic!("resume has no inventory/delete path")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("resume cannot insert")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("resume cannot rename/drop")
    }
    async fn ping(&self, _: Host) -> Result<(), String> {
        panic!("resume does not add a ping")
    }
}
struct Runtime {
    store: Memory,
    error: Option<String>,
    patch: Option<Document>,
    calls: Mutex<Vec<Host>>,
}
impl Runtime {
    fn new(store: &Memory) -> Self {
        Self {
            store: store.clone(),
            error: None,
            patch: None,
            calls: Mutex::new(vec![]),
        }
    }
}
#[async_trait]
impl OperationsRuntime for Runtime {
    async fn assert_consumer_absent(
        &self,
        root: &Path,
        t: &Document,
        h: Host,
    ) -> Result<(), String> {
        assert_eq!(t.get_str("name").unwrap(), NAME);
        assert!(
            mission_lock::MissionLocks::acquire(root, &[NAME], false).is_err(),
            "real original lock spans absence query and item mutations"
        );
        self.calls.lock().unwrap().push(h);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if let Some(patch) = &self.patch {
            self.store.0.lock().unwrap().controls[0].extend(patch.clone());
        }
        Ok(())
    }
    async fn archive_research(
        &self,
        _: &Path,
        _: &Document,
        _: Host,
        _: &str,
    ) -> Result<Value, String> {
        panic!("resume must not archive, stop, start, or touch platform")
    }
}
fn request() -> ResumeRequest {
    ResumeRequest {
        name: NAME.into(),
        expected_task_id: Some(ID.into()),
        ..Default::default()
    }
}
async fn run(
    store: &Memory,
    runtime: &Runtime,
    request: &ResumeRequest,
    root: &Root,
) -> TaskOutcome {
    resume::resume(&root.0, store, runtime, request).await
}
fn failed(out: TaskOutcome, needle: &str) {
    assert_eq!(out.exit_code, 1, "{}", out.value);
    assert_eq!(out.value["ok"], false);
    assert!(
        out.value.to_string().contains(needle),
        "{} missing {needle}",
        out.value
    );
}

#[tokio::test]
async fn flags_only_expand_server_retry_set_and_require_positive_unposted_evidence() {
    for timeout in [false, true] {
        for processing in [false, true] {
            let rows = vec![
                row(1, "transient_error"),
                row(2, "quota_deferred"),
                row(3, "timeout_error"),
                row(4, "processing"),
                row(5, "success"),
                doc! {"_id":6,"status":"transient_error"},
            ];
            let store = Memory::new(task(false), rows.clone());
            let runtime = Runtime::new(&store);
            let mut req = request();
            req.include_timeout = timeout;
            req.recover_processing = processing;
            let out = run(&store, &runtime, &req, &Root::new()).await;
            assert_eq!(out.exit_code, 0, "{}", out.value);
            assert_eq!(out.value["schemaVersion"], 1);
            assert_eq!(out.value["command"], "resume");
            assert_eq!(
                out.value["resumed"],
                2 + u64::from(timeout) + u64::from(processing)
            );
            let mut expected = vec!["transient_error", "quota_deferred"];
            if timeout {
                expected.push("timeout_error");
            }
            if processing {
                expected.push("processing");
            }
            assert_eq!(out.value["statuses"], json!(expected));
            assert_eq!(out.value["recover_processing"], processing);
            let d = store.0.lock().unwrap();
            assert_eq!(d.queries.len(), 1);
            let (_, query, projection, count) = &d.queries[0];
            assert_eq!(query, &doc! {"status":{"$in":expected}});
            assert_eq!(projection, &task_store::receipt_projection());
            assert_eq!(*count, 3 + usize::from(timeout) + usize::from(processing));
            assert_eq!(d.control_writes.len(), 1);
            drop(d);
            assert_eq!(store.control().get_str("status").unwrap(), "ready");
            assert_eq!(store.rows(Host::Local)[4], rows[4]);
            assert_eq!(store.rows(Host::Local)[5], rows[5]);
            assert_eq!(*runtime.calls.lock().unwrap(), vec![Host::Local]);
        }
    }
}

#[tokio::test]
async fn accepted_unknown_url_http_history_slot_child_rows_stay_untouched_without_global_halt() {
    let patches = [
        doc! {"acceptance":"ACCEPTED"},
        doc! {"acceptance":"UNKNOWN"},
        doc! {"terminalStatus":"UNKNOWN"},
        doc! {"alphaId":"a"},
        doc! {"progressUrl":"url"},
        doc! {"progress_url":"url"},
        doc! {"httpStatus":0},
        doc! {"httpStatus":429},
        doc! {"httpStatus":false},
        doc! {"responseHeaders":{"Location":"url"}},
        doc! {"submissionAttempts":[{}]},
        doc! {"superSlotId":"slot"},
        doc! {"childSimulationId":"child"},
        doc! {"childSimulationIds":["child"]},
        doc! {"campaign_post_attempted":true},
        doc! {"errorClass":"simulation_outcome_unknown"},
        doc! {"_not_posted_":1},
        doc! {"_not_posted_":false},
    ];
    let mut rows: Vec<_> = patches
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut r = row(i as i32, "transient_error");
            r.extend(p);
            r
        })
        .collect();
    let protected = rows.clone();
    rows.push(row(999, "quota_deferred"));
    let store = Memory::new(task(false), rows);
    let runtime = Runtime::new(&store);
    let out = run(&store, &runtime, &request(), &Root::new()).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.value["resumed"], 1);
    assert_eq!(
        &store.rows(Host::Local)[..protected.len()],
        protected.as_slice()
    );
    assert_eq!(store.0.lock().unwrap().item_writes.len(), 1);
}

#[tokio::test]
async fn server_projection_excludes_large_payload_and_no_candidate_total_limit() {
    let blob = "PAYLOAD_MUST_NOT_BE_READ".repeat(4096);
    let mut rows: Vec<_> = (0..205).map(|i| row(i, "transient_error")).collect();
    for r in &mut rows {
        r.insert("regular", blob.clone());
        r.insert("result", doc! {"huge":&blob});
    }
    let mut excluded = row(999, "success");
    excluded.insert("regular", blob);
    rows.push(excluded.clone());
    let store = Memory::new(task(true), rows);
    store
        .0
        .lock()
        .unwrap()
        .items
        .insert(Host::Local, vec![row(3000, "transient_error")]);
    let runtime = Runtime::new(&store);
    let out = run(&store, &runtime, &request(), &Root::new()).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.value["resumed"], 205);
    let d = store.0.lock().unwrap();
    assert_eq!(d.queries[0].0, Host::Remote);
    assert_eq!(d.queries[0].3, 205);
    assert_eq!(d.item_writes.len(), 205);
    for (_, q, u) in &d.item_writes {
        assert!(!q.contains_key("regular") && !q.contains_key("result"));
        assert!(bson::to_vec(q).unwrap().len() < 2048);
        assert!(!u.to_string().contains("PAYLOAD_MUST_NOT"));
    }
    drop(d);
    assert_eq!(store.rows(Host::Remote)[205], excluded);
    assert_eq!(
        store.rows(Host::Local)[0].get_str("status").unwrap(),
        "transient_error"
    );
}

#[tokio::test]
async fn receipt_race_and_missing_null_presence_are_really_fenced() {
    for patch in [
        doc! {"acceptance":"ACCEPTED"},
        doc! {"progressUrl":"new-url"},
        doc! {"submissionAttempts":[{}]},
        doc! {"status":"processing"},
        doc! {"rejection_evidence_received":true},
    ] {
        let store = Memory::new(task(false), vec![row(1, "transient_error")]);
        store.0.lock().unwrap().before_write_patch = Some(patch.clone());
        let runtime = Runtime::new(&store);
        let out = run(&store, &runtime, &request(), &Root::new()).await;
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.value["resumed"], 0);
        for (k, v) in patch {
            assert_eq!(store.rows(Host::Local)[0].get(&k), Some(&v));
        }
    }
    for direction in ["missing-to-null", "null-to-missing", "unchanged-null"] {
        let mut r = row(1, "transient_error");
        if direction != "missing-to-null" {
            r.insert("progressUrl", Bson::Null);
        }
        let store = Memory::new(task(false), vec![r]);
        {
            let mut d = store.0.lock().unwrap();
            if direction == "missing-to-null" {
                d.before_write_patch = Some(doc! {"progressUrl":Bson::Null});
            }
            if direction == "null-to-missing" {
                d.before_write_remove = Some("progressUrl".into());
            }
        }
        let runtime = Runtime::new(&store);
        let out = run(&store, &runtime, &request(), &Root::new()).await;
        assert_eq!(out.exit_code, 0, "{direction}:{}", out.value);
        assert_eq!(
            out.value["resumed"],
            if direction == "unchanged-null" { 1 } else { 0 }
        );
        let d = store.0.lock().unwrap();
        let q = &d.item_writes[0].1;
        if direction == "missing-to-null" {
            assert_eq!(
                q.get_document("progressUrl").unwrap(),
                &doc! {"$exists":false}
            );
        } else {
            assert_eq!(q.get("progressUrl"), Some(&Bson::Null));
            assert!(q
                .get_array("$and")
                .unwrap()
                .contains(&Bson::Document(doc! {"progressUrl":{"$exists":true}})));
        }
    }
}

#[tokio::test]
async fn blocked_states_open_observer_bad_id_and_live_or_unknown_consumer_do_not_touch_rows() {
    for status in [
        "generating",
        "generation_unknown",
        "launch_unknown",
        "syncing",
        "deleting",
    ] {
        let mut t = task(false);
        t.insert("status", status);
        let store = Memory::new(t, vec![row(1, "transient_error")]);
        let runtime = Runtime::new(&store);
        failed(
            run(&store, &runtime, &request(), &Root::new()).await,
            "ownership unresolved",
        );
        store.no_writes();
        assert!(runtime.calls.lock().unwrap().is_empty());
    }
    for observer in [
        Bson::Boolean(false),
        Bson::Document(doc! {}),
        Bson::Document(doc! {"finishedAt":Bson::Null}),
    ] {
        let mut t = task(false);
        t.insert("acceptedObservation", observer);
        let store = Memory::new(t, vec![row(1, "transient_error")]);
        let runtime = Runtime::new(&store);
        failed(
            run(&store, &runtime, &request(), &Root::new()).await,
            "observation is active/unconfirmed",
        );
        store.no_writes();
    }
    let store = Memory::new(task(false), vec![row(1, "transient_error")]);
    let runtime = Runtime::new(&store);
    let mut req = request();
    req.expected_task_id = Some("another-task".into());
    failed(
        run(&store, &runtime, &req, &Root::new()).await,
        "task identity changed",
    );
    store.no_writes();
    assert!(runtime.calls.lock().unwrap().is_empty());
    for error in [
        "cannot resume rows while the task tmux session is alive",
        "consumer probe unknown",
    ] {
        let store = Memory::new(task(false), vec![row(1, "transient_error")]);
        let mut runtime = Runtime::new(&store);
        runtime.error = Some(error.into());
        failed(run(&store, &runtime, &request(), &Root::new()).await, error);
        store.no_writes();
        assert!(store.0.lock().unwrap().queries.is_empty());
    }
}

#[tokio::test]
async fn task_replacement_launch_and_observer_changes_cannot_overwrite_new_binding() {
    for point in ["runtime", "item-read"] {
        for patch in [
            doc! {"_id":ObjectId::new()},
            doc! {"isRemote":true},
            doc! {"launchAttemptId":"replacement"},
            doc! {"acceptedObservation":{"finishedAt":Bson::Null}},
            doc! {"auth_profile":"other-profile"},
            doc! {"taskType":"super"},
        ] {
            let store = Memory::new(task(false), vec![row(1, "transient_error")]);
            let mut runtime = Runtime::new(&store);
            if point == "runtime" {
                runtime.patch = Some(patch);
            } else {
                store.0.lock().unwrap().after_read_control = Some(patch);
            }
            failed(
                run(&store, &runtime, &request(), &Root::new()).await,
                "binding changed",
            );
            store.no_writes();
        }
    }
    let store = Memory::new(task(false), vec![row(1, "transient_error")]);
    store.0.lock().unwrap().control_cas_zero = true;
    let runtime = Runtime::new(&store);
    failed(
        run(&store, &runtime, &request(), &Root::new()).await,
        "control CAS failed",
    );
    assert_eq!(
        store.rows(Host::Local)[0].get_str("status").unwrap(),
        "pending"
    );
    assert_eq!(store.control().get_str("status").unwrap(), "paused");
}

#[tokio::test]
async fn partial_write_errors_never_retry_or_rollback_and_explicit_next_resume_skips_pending() {
    for applied in [false, true] {
        let root = Root::new();
        let store = Memory::new(
            task(false),
            vec![
                row(1, "transient_error"),
                row(2, "transient_error"),
                row(3, "transient_error"),
            ],
        );
        {
            let mut d = store.0.lock().unwrap();
            d.fail_write_at = Some(2);
            d.apply_then_fail = applied;
        }
        let runtime = Runtime::new(&store);
        failed(run(&store, &runtime, &request(), &root).await, "timeout");
        let d = store.0.lock().unwrap();
        assert_eq!(d.item_writes.len(), 2);
        assert!(d.control_writes.is_empty());
        drop(d);
        let rows = store.rows(Host::Local);
        assert_eq!(rows[0].get_str("status").unwrap(), "pending");
        assert_eq!(
            rows[1].get_str("status").unwrap(),
            if applied {
                "pending"
            } else {
                "transient_error"
            }
        );
        assert_eq!(rows[2].get_str("status").unwrap(), "transient_error");
        let first = rows[0].clone();
        store.0.lock().unwrap().fail_write_at = None;
        let retry = run(&store, &runtime, &request(), &root).await;
        assert_eq!(retry.exit_code, 0);
        assert_eq!(retry.value["resumed"], if applied { 1 } else { 2 });
        assert_eq!(store.rows(Host::Local)[0], first);
        let writes = store.0.lock().unwrap().item_writes.clone();
        assert_eq!(
            writes
                .iter()
                .filter(|(_, q, _)| q.get_i32("_id").ok() == Some(1))
                .count(),
            1
        );
        assert_eq!(
            runtime.calls.lock().unwrap().len(),
            2,
            "only explicit second command probes again"
        );
    }
}

#[tokio::test]
async fn counts_use_modified_not_matched_and_read_control_write_errors_never_become_success() {
    let store = Memory::new(task(false), vec![row(1, "transient_error")]);
    store.0.lock().unwrap().item_cas_zero = true;
    let runtime = Runtime::new(&store);
    let out = run(&store, &runtime, &request(), &Root::new()).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.value["resumed"], 0);
    assert_eq!(
        store.rows(Host::Local)[0].get_str("status").unwrap(),
        "transient_error"
    );
    for point in ["control-read", "find-items", "item-write", "control-write"] {
        let store = Memory::new(task(false), vec![row(1, "transient_error")]);
        store.0.lock().unwrap().fail = Some(point.into());
        let runtime = Runtime::new(&store);
        failed(run(&store, &runtime, &request(), &Root::new()).await, point);
        assert_eq!(store.control().get_str("status").unwrap(), "paused");
        if point != "control-write" {
            assert!(store.0.lock().unwrap().control_writes.is_empty());
        }
    }
    let store = Memory::new(task(false), vec![row(1, "transient_error")]);
    store.0.lock().unwrap().control_write_timeout = true;
    let runtime = Runtime::new(&store);
    failed(
        run(&store, &runtime, &request(), &Root::new()).await,
        "control write timeout",
    );
    assert_eq!(store.0.lock().unwrap().control_writes.len(), 1);
    assert_eq!(store.control().get_str("status").unwrap(), "ready");
}

#[tokio::test]
async fn no_new_kind_profile_paused_only_or_global_unresolved_gate_is_introduced() {
    for kind in [
        Bson::String("regular".into()),
        Bson::String("super".into()),
        Bson::String("priority".into()),
        Bson::Document(doc! {"legacy":true}),
    ] {
        for status in ["ready", "pending", "running", "completed", "paused"] {
            let mut t = task(false);
            t.insert("taskType", kind.clone());
            t.insert("status", status);
            t.remove("auth_profile");
            t.insert(
                "acceptedObservation",
                doc! {"finishedAt":"original-closed-proof"},
            );
            let store = Memory::new(
                t,
                vec![doc! {"_id":1,"status":"processing","acceptance":"UNKNOWN"}],
            );
            let runtime = Runtime::new(&store);
            let mut req = request();
            req.recover_processing = true;
            let out = run(&store, &runtime, &req, &Root::new()).await;
            assert_eq!(out.exit_code, 0, "{kind:?}:{status}:{}", out.value);
            assert_eq!(out.value["resumed"], 0);
            assert_eq!(
                store.rows(Host::Local)[0].get_str("acceptance").unwrap(),
                "UNKNOWN"
            );
        }
    }
}

#[tokio::test]
async fn original_lock_invalid_names_and_unknown_store_do_not_produce_fake_success() {
    let root = Root::new();
    let store = Memory::new(task(false), vec![row(1, "transient_error")]);
    let runtime = Runtime::new(&store);
    let guard = mission_lock::MissionLocks::acquire(&root.0, &[NAME], false).unwrap();
    failed(
        run(&store, &runtime, &request(), &root).await,
        "another lifecycle mutator",
    );
    assert!(store.0.lock().unwrap().calls.is_empty());
    drop(guard);
    let mut req = request();
    req.name = "../escape".into();
    let out = run(&store, &runtime, &req, &root).await;
    assert_eq!(out.value["errorType"], "ValueError");
    store.no_writes();
    let empty = Memory::default();
    let runtime = Runtime::new(&empty);
    failed(
        run(&empty, &runtime, &request(), &root).await,
        "task not found",
    );
}

#[tokio::test]
async fn row_and_control_cas_include_original_values_presence_and_full_scope() {
    let mut t = task(false);
    t.insert("acceptedObservation", Bson::Null);
    t.insert("startupBinding", doc! {"token":"startup"});
    let mut r = row(1, "transient_error");
    r.insert("progressUrl", Bson::Null);
    let store = Memory::new(t.clone(), vec![r.clone()]);
    let runtime = Runtime::new(&store);
    assert_eq!(
        run(&store, &runtime, &request(), &Root::new())
            .await
            .exit_code,
        0
    );
    let d = store.0.lock().unwrap();
    let q = &d.item_writes[0].1;
    for key in task_store::receipt_projection().keys() {
        let expected = r
            .get(key)
            .cloned()
            .unwrap_or(Bson::Document(doc! {"$exists":false}));
        assert_eq!(q.get(key), Some(&expected), "{key}");
    }
    let cq = &d.control_writes[0].0;
    for field in [
        "_id",
        "name",
        "status",
        "isRemote",
        "updatedAt",
        "tmuxContext",
        "launchAttemptId",
        "launchAttemptAt",
        "acceptedObservation",
        "generation",
        "startupBinding",
        "taskType",
        "auth_profile",
    ] {
        assert!(cq.contains_key(field), "{field}");
    }
    assert!(cq.get_array("$and").unwrap().contains(&Bson::Document(
        doc! {"acceptedObservation":{"$exists":true}}
    )));
    drop(d);
    let events = store.control().get_array("events").unwrap().clone();
    let event = events[0].as_document().unwrap();
    assert_eq!(event.get_str("command").unwrap(), "resume");
    assert_eq!(
        event
            .get_document("detail")
            .unwrap()
            .get_i64("rows")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn checked_no_post_short_circuit_and_type_errors_are_not_reimplemented_as_schema_validation()
{
    let mut first = row(1, "transient_error");
    first.insert("_not_posted_", false);
    first.insert("errorClass", doc! {});
    let store = Memory::new(task(false), vec![first.clone(), row(2, "transient_error")]);
    let runtime = Runtime::new(&store);
    let out = run(&store, &runtime, &request(), &Root::new()).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.value["resumed"], 1);
    assert_eq!(store.rows(Host::Local)[0], first);
    let mut bad = row(2, "transient_error");
    bad.insert("errorClass", doc! {});
    let store = Memory::new(task(false), vec![row(1, "transient_error"), bad]);
    let runtime = Runtime::new(&store);
    let out = run(&store, &runtime, &request(), &Root::new()).await;
    assert_eq!(out.value["errorType"], "TypeError");
    failed(out, "unhashable type");
    assert_eq!(
        store.rows(Host::Local)[0].get_str("status").unwrap(),
        "pending"
    );
    assert!(store.0.lock().unwrap().control_writes.is_empty());
}

#[tokio::test]
async fn original_python_resume_oracle() {
    let cases: Vec<Value> = serde_json::from_str(RESUME_ORACLE).unwrap();
    assert!(
        cases.len() >= 60,
        "original Python oracle fixtures are missing/truncated"
    );
    for case in cases {
        let t = Bson::try_from(case["task"].clone())
            .unwrap()
            .as_document()
            .unwrap()
            .clone();
        let rows: Vec<Document> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                Bson::try_from(r.clone())
                    .unwrap()
                    .as_document()
                    .unwrap()
                    .clone()
            })
            .collect();
        let store = Memory::new(t.clone(), rows);
        let mut runtime = Runtime::new(&store);
        if case["consumer_alive"] == true {
            runtime.error = Some("cannot resume rows while the task tmux session is alive".into());
        }
        if let Some(error) = case["consumer_error"].as_str() {
            runtime.error = Some(error.into());
        }
        let req = ResumeRequest {
            name: NAME.into(),
            expected_task_id: case["expected_task_id"].as_str().map(str::to_string),
            include_timeout: case["include_timeout"].as_bool().unwrap(),
            recover_processing: case["recover_processing"].as_bool().unwrap(),
        };
        let out = run(&store, &runtime, &req, &Root::new()).await;
        let expected = &case["expected"];
        assert_eq!(
            out.exit_code,
            expected["exit_code"].as_i64().unwrap() as i32,
            "{}:{}",
            case["label"],
            out.value
        );
        if expected["exit_code"] == 0 {
            assert_eq!(
                out.value["resumed"], expected["resumed"],
                "{}",
                case["label"]
            );
            assert_eq!(out.value["statuses"], expected["statuses"]);
            assert_eq!(
                out.value["recover_processing"],
                expected["recover_processing"]
            );
        } else {
            assert_eq!(
                out.value["errorType"], expected["error_type"],
                "{}:{}",
                case["label"], out.value
            );
            assert!(
                out.value["error"]
                    .as_str()
                    .unwrap()
                    .contains(expected["error"].as_str().unwrap()),
                "{}:{}",
                case["label"],
                out.value
            );
        }
        let host = Host::from_remote(t.get_bool("isRemote").unwrap_or(false));
        let actual: Vec<Value> = store
            .rows(host)
            .iter()
            .map(|r| {
                r.get("status")
                    .cloned()
                    .unwrap_or(Bson::Null)
                    .into_relaxed_extjson()
            })
            .collect();
        assert_eq!(
            Value::Array(actual),
            expected["row_statuses"],
            "{}",
            case["label"]
        );
        assert_eq!(
            store
                .control()
                .get("status")
                .cloned()
                .unwrap_or(Bson::Null)
                .into_relaxed_extjson(),
            expected["control_status"],
            "{}",
            case["label"]
        );
    }
}
#[test]
fn original_seventeen_receipt_fields_are_the_only_projection() {
    let expected: Document = [
        "_id",
        "status",
        "acceptance",
        "terminalStatus",
        "alphaId",
        "progressUrl",
        "progress_url",
        "responseHeaders",
        "submissionAttempts",
        "campaign_post_attempted",
        "_not_posted_",
        "httpStatus",
        "rejection_evidence_received",
        "errorClass",
        "superSlotId",
        "childSimulationIds",
        "childSimulationId",
    ]
    .into_iter()
    .map(|key| (key.to_string(), Bson::Int32(1)))
    .collect();
    assert_eq!(task_store::receipt_projection(), expected);
    assert_eq!(expected.len(), 17);
}

#[tokio::test]
async fn replacement_after_first_row_prevents_remaining_rows_and_ready_publish() {
    for patch in [
        doc! {"_id": ObjectId::new(), "status": "pending"},
        doc! {"acceptedObservation": {"finishedAt": Bson::Null}},
    ] {
        let store = Memory::new(
            task(false),
            vec![row(1, "transient_error"), row(2, "transient_error")],
        );
        store.0.lock().unwrap().after_first_write_control = Some(patch.clone());
        let runtime = Runtime::new(&store);
        failed(
            run(&store, &runtime, &request(), &Root::new()).await,
            "binding changed",
        );
        let data = store.0.lock().unwrap();
        assert_eq!(data.item_writes.len(), 1);
        assert!(data.control_writes.is_empty());
        drop(data);
        assert_eq!(
            store.rows(Host::Local)[0].get_str("status").unwrap(),
            "pending"
        );
        assert_eq!(
            store.rows(Host::Local)[1].get_str("status").unwrap(),
            "transient_error"
        );
        for (key, value) in patch {
            assert_eq!(store.control().get(&key), Some(&value));
        }
    }
}

// BEGIN ORIGINAL PYTHON RESUME ORACLE
const RESUME_ORACLE: &str = r###"[
  {
    "label": "flags:False:False",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000003"
        },
        "status": "timeout_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000004"
        },
        "status": "processing",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000005"
        },
        "status": "success",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$numberInt": "6"
        },
        "status": "transient_error"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending",
        "timeout_error",
        "processing",
        "success",
        "transient_error"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "flags:False:True",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000003"
        },
        "status": "timeout_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000004"
        },
        "status": "processing",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000005"
        },
        "status": "success",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$numberInt": "6"
        },
        "status": "transient_error"
      }
    ],
    "include_timeout": false,
    "recover_processing": true,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 3,
      "statuses": [
        "transient_error",
        "quota_deferred",
        "processing"
      ],
      "recover_processing": true,
      "row_statuses": [
        "pending",
        "pending",
        "timeout_error",
        "pending",
        "success",
        "transient_error"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "flags:True:False",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000003"
        },
        "status": "timeout_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000004"
        },
        "status": "processing",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000005"
        },
        "status": "success",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$numberInt": "6"
        },
        "status": "transient_error"
      }
    ],
    "include_timeout": true,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 3,
      "statuses": [
        "transient_error",
        "quota_deferred",
        "timeout_error"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending",
        "pending",
        "processing",
        "success",
        "transient_error"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "flags:True:True",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000003"
        },
        "status": "timeout_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000004"
        },
        "status": "processing",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000005"
        },
        "status": "success",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$numberInt": "6"
        },
        "status": "transient_error"
      }
    ],
    "include_timeout": true,
    "recover_processing": true,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 4,
      "statuses": [
        "transient_error",
        "quota_deferred",
        "timeout_error",
        "processing"
      ],
      "recover_processing": true,
      "row_statuses": [
        "pending",
        "pending",
        "pending",
        "pending",
        "success",
        "transient_error"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "acceptance_accepted",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "ACCEPTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "acceptance_unknown",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "UNKNOWN",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "terminal_unknown",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "terminalStatus": "UNKNOWN"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "alpha",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "alphaId": "a"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "url",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "progressUrl": "u"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "url_alias",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "progress_url": "u"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "http_zero",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "httpStatus": {
          "$numberInt": "0"
        }
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "http_false",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "httpStatus": false
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "http_429",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "httpStatus": {
          "$numberInt": "429"
        }
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "headers",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "responseHeaders": {
          "Location": "u"
        }
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "attempts",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "submissionAttempts": [
          {}
        ]
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "slot",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "superSlotId": "slot"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "child",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "childSimulationId": "c"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "children",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "childSimulationIds": [
          "c"
        ]
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "post_attempt",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "campaign_post_attempted": true
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "error_class",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "errorClass": "simulation_outcome_unknown"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "no_post_false",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": false,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "no_post_int",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": {
          "$numberInt": "1"
        },
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "no_post_null",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": null,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "nulls",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "progressUrl": null,
        "httpStatus": null,
        "submissionAttempts": []
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "empty_headers",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "responseHeaders": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "empty_attempts_object",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "submissionAttempts": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "bad_error_class",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "errorClass": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "TypeError",
      "error": "unhashable type: 'dict'",
      "row_statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "bad_error_skipped",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": false,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "errorClass": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "bad_error_receipt_short_circuit",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "progressUrl": "u",
        "errorClass": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "bad_acceptance",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": {},
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "transient_error",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "array_status",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": [
          "transient_error"
        ],
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "null_acceptance",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": null,
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "rejection_only",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "rejection_evidence_received": true
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "quota_deferred",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 2,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending",
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "partial-before-policy-error",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      },
      {
        "_id": {
          "$oid": "000000000000000000000002"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload",
        "errorClass": {}
      },
      {
        "_id": {
          "$oid": "000000000000000000000003"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "TypeError",
      "error": "unhashable type: 'dict'",
      "row_statuses": [
        "pending",
        "transient_error",
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "control-status:'generating'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "generating",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "generation/launch ownership unresolved; resume cannot reconcile it",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "generating"
    }
  },
  {
    "label": "control-status:'generation_unknown'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "generation_unknown",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "generation/launch ownership unresolved; resume cannot reconcile it",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "generation_unknown"
    }
  },
  {
    "label": "control-status:'launch_unknown'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "launch_unknown",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "generation/launch ownership unresolved; resume cannot reconcile it",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "launch_unknown"
    }
  },
  {
    "label": "control-status:'syncing'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "syncing",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "generation/launch ownership unresolved; resume cannot reconcile it",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "syncing"
    }
  },
  {
    "label": "control-status:'deleting'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "deleting",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "generation/launch ownership unresolved; resume cannot reconcile it",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "deleting"
    }
  },
  {
    "label": "control-status:'running'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "running",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:'ready'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "ready",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:'pending'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "pending",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:'completed'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "completed",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:'generation_failed'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "generation_failed",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:None",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": null,
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "control-status:[]",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": [],
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "TypeError",
      "error": "unhashable type: 'list'",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": []
    }
  },
  {
    "label": "control-status:{}",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": {},
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "TypeError",
      "error": "unhashable type: 'dict'",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": {}
    }
  },
  {
    "label": "kind:'regular'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:'super'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "super",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:'priority'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "priority",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:'future-kind'",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "future-kind",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:None",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": null,
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:{}",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": {},
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "kind:[]",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": [],
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "observer:None",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1",
      "acceptedObservation": null
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "observer:False",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1",
      "acceptedObservation": false
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "accepted observation is active/unconfirmed; retain its claim and reconcile before mutation",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "observer:{}",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1",
      "acceptedObservation": {}
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "accepted observation is active/unconfirmed; retain its claim and reconcile before mutation",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "observer:{'finishedAt': None}",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1",
      "acceptedObservation": {
        "finishedAt": null
      }
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "accepted observation is active/unconfirmed; retain its claim and reconcile before mutation",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "observer:{'finishedAt': 'closed-original'}",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1",
      "acceptedObservation": {
        "finishedAt": "closed-original"
      }
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "live-consumer",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": true,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "cannot resume rows while the task tmux session is alive",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "unknown-consumer",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": "consumer probe unknown",
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "consumer probe unknown",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "wrong-task-id",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "other",
    "expected": {
      "exit_code": 1,
      "error_type": "RuntimeError",
      "error": "task identity changed before mutation",
      "row_statuses": [
        "transient_error"
      ],
      "control_status": "paused"
    }
  },
  {
    "label": "remote",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": true,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "legacy-id:'legacy-task-id'",
    "task": {
      "_id": "legacy-task-id",
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "legacy-task-id",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "legacy-id:7",
    "task": {
      "_id": {
        "$numberInt": "7"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "7",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "legacy-id:None",
    "task": {
      "_id": null,
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "None",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "missing-control-status",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [
      {
        "_id": {
          "$oid": "000000000000000000000001"
        },
        "status": "transient_error",
        "_not_posted_": true,
        "acceptance": "NOT_POSTED",
        "regular": "excluded payload"
      }
    ],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 1,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [
        "pending"
      ],
      "control_status": "ready"
    }
  },
  {
    "label": "empty-items",
    "task": {
      "_id": {
        "$oid": "0123456789abcdef01234567"
      },
      "name": "fixture",
      "status": "paused",
      "isRemote": false,
      "taskType": "regular",
      "auth_profile": "fixture-profile",
      "updatedAt": "revision-1",
      "launchAttemptId": "launch-1"
    },
    "rows": [],
    "include_timeout": false,
    "recover_processing": false,
    "consumer_alive": false,
    "consumer_error": null,
    "expected_task_id": "0123456789abcdef01234567",
    "expected": {
      "exit_code": 0,
      "resumed": 0,
      "statuses": [
        "transient_error",
        "quota_deferred"
      ],
      "recover_processing": false,
      "row_statuses": [],
      "control_status": "ready"
    }
  }
]"###;
// END ORIGINAL PYTHON RESUME ORACLE
