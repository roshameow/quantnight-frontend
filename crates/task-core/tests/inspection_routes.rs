use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{
    cleanup_sync::OperationsRuntime,
    inspection::*,
    mission_lock::MissionLocks,
    task_store::{self, CanonicalNamespace, CollectionSummary, Host, TaskStore},
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
const NAME: &str = "inspection_fixture";
const ID: &str = "0123456789abcdef01234567";
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "qn-inspection-{}-{}",
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
    doc! {"_id":ObjectId::parse_str(ID).unwrap(),"name":NAME,"status":"running","taskType":"regular","isRemote":remote,"auth_profile":"fixture-profile","updatedAt":"revision-1","launchAttemptId":"launch-1","runId":"mission:inspection_fixture"}
}
fn get<'a>(d: &'a Document, key: &str) -> Option<&'a Bson> {
    let mut p = key.splitn(2, '.');
    let v = d.get(p.next().unwrap())?;
    match p.next() {
        Some(rest) => get(v.as_document()?, rest),
        None => Some(v),
    }
}
fn set(d: &mut Document, key: &str, value: Bson) {
    if let Some((head, tail)) = key.split_once('.') {
        if !d.contains_key(head) {
            d.insert(head, doc! {});
        }
        set(d.get_document_mut(head).unwrap(), tail, value);
    } else {
        d.insert(key, value);
    }
}
fn matches(d: &Document, q: &Document) -> bool {
    q.iter().all(|(key, condition)| {
        let actual = get(d, key);
        match condition {
            Bson::Document(op) if op.contains_key("$exists") => {
                actual.is_some() == op.get_bool("$exists").unwrap()
            }
            Bson::Document(op) if op.contains_key("$ne") => actual != op.get("$ne"),
            Bson::Null => matches!(actual, None | Some(Bson::Null)),
            _ => actual == Some(condition),
        }
    })
}
fn project(d: &Document, p: &Document) -> Document {
    if p.values().all(|v| *v == Bson::Int32(0)) {
        return d
            .iter()
            .filter(|(k, _)| !p.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
    let mut result = doc! {};
    for (k, v) in p {
        if *v == Bson::Int32(1) {
            if let Some(value) = get(d, k) {
                set(&mut result, k, value.clone());
            }
        }
    }
    if !p.contains_key("_id") {
        if let Some(id) = d.get("_id") {
            result.insert("_id", id.clone());
        }
    }
    result
}
#[derive(Default)]
struct ReadGate { entered: tokio::sync::Notify, release: tokio::sync::Notify }
#[derive(Default)]
struct Data {
    summary_gate: Option<Arc<ReadGate>>,
    controls: HashMap<Host, Vec<Document>>,
    items: HashMap<Host, Vec<Document>>,
    canonical: Vec<Document>,
    calls: Vec<String>,
    writes: Vec<(Host, Document, Document)>,
    fail: Option<String>,
    patch_before_write: Option<(Host, Document)>,
    indexes: Vec<Document>,
}
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Data>>);
fn call(d: &mut Data, name: &str) -> Result<(), String> {
    d.calls.push(name.into());
    if d.fail.as_deref() == Some(name) {
        Err(format!("injected {name} timeout; not retried"))
    } else {
        Ok(())
    }
}
impl Memory {
    fn new(t: Document, rows: Vec<Document>) -> Self {
        let h = Host::from_remote(t.get_bool("isRemote").unwrap());
        let s = Self::default();
        {
            let mut d = s.0.lock().unwrap();
            d.controls.insert(Host::Local, vec![t]);
            d.items.insert(h, rows);
            d.indexes = vec![doc! {"name":"id_1","key":{"id":1}}];
        }
        s
    }
    fn no_writes(&self) {
        assert!(self.0.lock().unwrap().writes.is_empty());
    }
    fn control(&self) -> Document {
        self.0.lock().unwrap().controls[&Host::Local][0].clone()
    }
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
        call(
            &mut d,
            if newest {
                "control-read"
            } else if q.contains_key("acceptedObservation") {
                "status-revalidate"
            } else {
                "startup-read"
            },
        )?;
        let mut rows = d.controls.get(&h).cloned().unwrap_or_default();
        if newest {
            rows.sort_by_key(|r| r.get_object_id("_id").unwrap());
            rows.reverse();
        }
        Ok(rows.into_iter().find(|r| matches(r, &q)))
    }
    async fn update_task(&self, h: Host, q: Document, u: Document) -> Result<u64, String> {
        let mut d = self.0.lock().unwrap();
        call(
            &mut d,
            if h == Host::Remote {
                "startup-confirm"
            } else {
                "terminal-cas"
            },
        )?;
        d.writes.push((h, q.clone(), u.clone()));
        if let Some((host, patch)) = d.patch_before_write.take() {
            d.controls.get_mut(&host).unwrap()[0].extend(patch);
        }
        let Some(row) = d
            .controls
            .entry(h)
            .or_default()
            .iter_mut()
            .find(|r| matches(r, &q))
        else {
            return Ok(0);
        };
        for (k, v) in u.get_document("$set").unwrap() {
            set(row, k, v.clone());
        }
        if let Ok(push) = u.get_document("$push") {
            for (k, v) in push {
                row.entry(k.clone())
                    .or_insert(Bson::Array(vec![]))
                    .as_array_mut()
                    .unwrap()
                    .push(v.clone());
            }
        }
        Ok(1)
    }
    async fn summary(&self, h: Host, _: &str) -> Result<CollectionSummary, String> {
        let gate = self.0.lock().unwrap().summary_gate.take();
        if let Some(gate) = gate { gate.entered.notify_one(); gate.release.notified().await; }
        let mut d = self.0.lock().unwrap();
        call(&mut d, "summary")?;
        let Some(rows) = d.items.get(&h) else {
            return Ok(CollectionSummary::default());
        };
        let mut s = CollectionSummary {
            exists: true,
            total: rows.len() as u64,
            ..Default::default()
        };
        for row in rows {
            *s.statuses
                .entry(row.get_str("status").unwrap_or("missing").into())
                .or_default() += 1;
        }
        Ok(s)
    }
    async fn list_tasks(&self, h: Host, include: bool) -> Result<Vec<Document>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, "list")?;
        let p = task_store::list_tasks_options().projection.unwrap();
        let mut rows: Vec<_> = d
            .controls
            .get(&h)
            .into_iter()
            .flatten()
            .filter(|r| include || r.get_str("status").ok() != Some("deactive"))
            .map(|r| project(r, &p))
            .collect();
        rows.sort_by_key(|r| r.get_object_id("_id").unwrap());
        rows.reverse();
        Ok(rows)
    }
    async fn result_items_page(
        &self,
        h: Host,
        _: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Document>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, "page")?;
        let o = task_store::result_items_options(limit, offset)?;
        let mut rows = d.items.get(&h).cloned().unwrap_or_default();
        rows.sort_by_key(|r| r.get_i32("_id").unwrap());
        Ok(rows
            .into_iter()
            .skip(o.skip.unwrap() as usize)
            .take(o.limit.unwrap() as usize)
            .map(|r| project(&r, o.projection.as_ref().unwrap()))
            .collect())
    }
    async fn canonical_results(
        &self,
        n: &CanonicalNamespace,
        ids: &[Bson],
    ) -> Result<Vec<Document>, String> {
        assert_eq!(n.database, "profile_results");
        assert_eq!(n.collection, "canonical_fixture");
        let mut d = self.0.lock().unwrap();
        call(&mut d, "canonical")?;
        let hint = task_store::canonical_id_hint(&d.indexes)?;
        let o = task_store::canonical_result_options(hint, ids.len())?;
        Ok(d.canonical
            .iter()
            .filter(|r| r.get("id").map(|id| ids.contains(id)).unwrap_or(false))
            .take(o.limit.unwrap() as usize)
            .map(|r| project(r, o.projection.as_ref().unwrap()))
            .collect())
    }
    async fn collect_result_items(&self, h: Host, _: &str) -> Result<Vec<Document>, String> {
        let mut d = self.0.lock().unwrap();
        call(&mut d, "collect")?;
        let p = task_store::collect_result_options().projection.unwrap();
        Ok(d.items
            .get(&h)
            .into_iter()
            .flatten()
            .map(|r| project(r, &p))
            .collect())
    }
    async fn execution_receipt_assessment(&self,h:Host,_:&str)->Result<Document,String>{
        let mut d=self.0.lock().unwrap();call(&mut d,"receipt-assessment")?;
        Ok(task_store::assess_execution_receipts(d.items.get(&h).map(Vec::as_slice).unwrap_or(&[])))
    }
    async fn rows(&self, _: Host, _: &str, _: Option<Document>) -> Result<Vec<Document>, String> {
        panic!("inspection only reads bounded projections except explicit collect API")
    }
    async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
        panic!("unexpected inventory")
    }
    async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
        panic!("unexpected probe")
    }
    async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
        panic!("no writes")
    }
    async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
        panic!("no archive rename")
    }
    async fn ping(&self, _: Host) -> Result<(), String> {
        panic!("no probe")
    }
}
#[derive(Default)]
struct Runtime {
    priority_gate: Option<Arc<ReadGate>>,
    calls: Mutex<Vec<String>>,
    priority: Option<Value>,
    namespace_fail: bool,
}
#[async_trait]
impl OperationsRuntime for Runtime {
    async fn assert_consumer_absent(&self, _: &Path, _: &Document, _: Host) -> Result<(), String> {
        panic!("inspection never launches or probes consumers")
    }
    async fn archive_research(
        &self,
        _: &Path,
        t: &Document,
        h: Host,
        n: &str,
    ) -> Result<Value, String> {
        assert_eq!(t.get_object_id("_id").unwrap().to_hex(), ID);
        assert_eq!(n, NAME);
        self.calls.lock().unwrap().push(format!("archive:{h:?}"));
        Ok(json!({"archivePath":"original/archive.json","ideas":2,"platformFetched":false}))
    }
}
#[async_trait]
impl InspectionRuntime for Runtime {
    async fn inspect_execution(&self,_:&Path,_:&dyn TaskStore,_:&Document,_:Host)->Result<Option<Value>,String>{
        self.calls.lock().unwrap().push("process-inspection".into());Ok(None)
    }
    async fn canonical_namespace(
        &self,
        _: &Path,
        profile: &str,
    ) -> Result<CanonicalNamespace, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("namespace:{profile}"));
        if self.namespace_fail {
            return Err("original profile mapping unavailable".into());
        }
        CanonicalNamespace::new("profile_results".into(), "canonical_fixture".into())
    }
    async fn priority_snapshot(
        &self,
        _: &Path,
        _: &Document,
        h: Host,
        inspect: bool,
    ) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("priority:{h:?}:{inspect}"));
        if let Some(gate) = &self.priority_gate { gate.entered.notify_one(); gate.release.notified().await; }
        Ok(self
            .priority
            .clone()
            .unwrap_or(json!({"ok":true,"taskStatus":"active","budget":{"mode":"per_root"}})))
    }
    async fn priority_default_config(&self, _: &Path, _: &Document) -> Result<Document, String> {
        self.calls.lock().unwrap().push("priority-default".into());
        Ok(doc! {"tag":{"tags":[NAME]}})
    }
    async fn verify_registration(&self, _: &Path, t: &Document) -> Result<Value, String> {
        assert_eq!(t.get_object_id("_id").unwrap().to_hex(), ID);
        self.calls.lock().unwrap().push("verify:Local".into());
        Ok(
            json!({"registeredCount":17,"researchUnit":"unit","receiptPath":"original/status.json","payloadPath":"original/payload.json","simulationStarted":false}),
        )
    }
}
fn success_rows() -> Vec<Document> {
    vec![doc! {"_id":1,"status":"success","alphaId":"a","payload":"not read"}]
}
fn super_task(remote: bool) -> Document {
    let mut t = task(remote);
    t.insert("taskType", "super");
    let b = doc! {"taskId":ID,"name":NAME,"auth_profile":"fixture-profile","remote":remote,"consumer_run_id":"consumer-1","launchAttemptId":"launch-1"};
    let mut proof = b.clone();
    proof.extend(doc! {"phase":"complete","completedAt":"stamp"});
    let mut closure = proof.clone();
    closure.insert("schemaVersion", 1);
    t.insert("startupBinding", b);
    t.insert(
        "simulatorRuntime",
        doc! {"consumer_run_id":"consumer-1","startupRecovery":proof,"lifecycleClosure":closure},
    );
    t
}

#[tokio::test]
async fn list_projects_sorts_filters_and_uses_explicit_host() {
    let mut a = task(false);
    a.insert("secret", "not fetched");
    let m = Memory::new(a, vec![]);
    let mut old = task(false);
    old.insert(
        "_id",
        ObjectId::parse_str("000000000000000000000001").unwrap(),
    );
    old.insert("status", "deactive");
    m.0.lock()
        .unwrap()
        .controls
        .get_mut(&Host::Local)
        .unwrap()
        .push(old.clone());
    let r = list(&m, &ListRequest::default()).await;
    assert!(r.value["task"].is_null());
    assert_eq!(r.value["tasks"].as_array().unwrap().len(), 1);
    assert!(r.value["tasks"][0].get("secret").is_none());
    assert!(r.value["tasks"][0].get("runId").is_none());
    m.0.lock().unwrap().controls.insert(Host::Remote, vec![old]);
    let r = list(
        &m,
        &ListRequest {
            remote: true,
            include_deactive: true,
        },
    )
    .await;
    assert_eq!(r.value["tasks"][0]["status"], "deactive");
    m.no_writes();
}
#[tokio::test]
async fn regular_status_aggregates_execution_then_exact_terminal_cas() {
    let root = Root::new();
    let m = Memory::new(task(true), success_rows());
    m.0.lock()
        .unwrap()
        .items
        .insert(Host::Local, vec![doc! {"_id":1,"status":"pending"}]);
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(r.value["taskStatus"], "completed");
    assert_eq!(r.value["local"]["statuses"]["pending"], 1);
    assert_eq!(r.value["execution"]["statuses"]["success"], 1);
    assert_eq!(r.value["unfinished"], 0);
    let d = m.0.lock().unwrap();
    assert_eq!(
        d.calls,
        vec!["control-read", "summary", "summary", "status-revalidate", "terminal-cas"]
    );
    let q = &d.writes[0].1;
    assert_eq!(q.get_str("launchAttemptId").unwrap(), "launch-1");
    assert_eq!(
        q.get_document("acceptedObservation").unwrap(),
        &doc! {"$exists":false}
    );
    assert_eq!(
        q.get_document("generation").unwrap(),
        &doc! {"$exists":false}
    );
    assert!(d.writes[0]
        .2
        .get_document("$push")
        .unwrap()
        .contains_key("events"));
}
#[tokio::test]
async fn status_failure_counts_and_nonterminal_empty_are_not_success() {
    let root = Root::new();
    for (rows, expected, terminal) in [
        (
            vec![doc! {"_id":1,"status":"timeout_error"}],
            "completed_with_errors",
            true,
        ),
        (vec![doc! {"_id":1,"status":"UNKNOWN"}], "running", false),
        (vec![], "running", false),
    ] {
        let m = Memory::new(task(false), rows);
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.value["taskStatus"], expected);
        assert_eq!(r.value["terminal"], terminal);
    }
}
#[tokio::test]
async fn terminal_reconciliation_preserves_all_guarded_control_states() {
    let root = Root::new();
    for state in [
        "completed",
        "completed_with_errors",
        "deactive",
        "generating",
        "generation_unknown",
        "launch_unknown",
        "syncing",
        "deleting",
    ] {
        let mut t = task(false);
        t.insert("status", state);
        let m = Memory::new(t, success_rows());
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.value["taskStatus"], state);
        m.no_writes();
    }
}
#[tokio::test]
async fn observation_claim_blocks_mutation_but_paused_observation_remains_readable() {
    let root = Root::new();
    for state in ["running", "paused"] {
        let mut t = task(false);
        t.insert("status", state);
        t.insert("acceptedObservation", doc! {"id":"claim"});
        let m = Memory::new(t, success_rows());
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.value["ok"], state == "paused");
        if state == "paused" {
            assert_eq!(r.value["taskStatus"], "unknown");
            assert_eq!(r.value["reason"], "pause_unconfirmed");
            assert_eq!(r.value["acceptedObservation"]["id"], "claim");
        }
        m.no_writes();
    }
}
#[tokio::test]
async fn changed_generation_launch_observation_or_revision_loses_terminal_cas() {
    let root = Root::new();
    for patch in [
        doc! {"launchAttemptId":"other"},
        doc! {"generation":{"id":"other"}},
        doc! {"acceptedObservation":{"id":"other"}},
        doc! {"updatedAt":"other"},
        doc! {"tmuxContext":{"sessionId":"other"}},
    ] {
        let m = Memory::new(task(false), success_rows());
        m.0.lock().unwrap().patch_before_write = Some((Host::Local, patch));
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(m.control().get_str("status").unwrap(), "running");
    }
}
#[tokio::test]
async fn super_local_runtime_proof_is_bound_in_final_cas() {
    let root = Root::new();
    let m = Memory::new(super_task(false), success_rows());
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(r.value["taskStatus"], "completed");
    let d = m.0.lock().unwrap();
    assert_eq!(
        d.calls,
        vec![
            "control-read",
            "startup-read",
            "summary",
            "summary",
            "status-revalidate",
            "terminal-cas"
        ]
    );
    assert!(d.writes[0]
        .1
        .contains_key("simulatorRuntime.startupRecovery"));
}
#[tokio::test]
async fn super_remote_proof_reconfirmed_after_counts_before_local_cas() {
    let root = Root::new();
    let t = super_task(true);
    let m = Memory::new(t.clone(), success_rows());
    m.0.lock().unwrap().controls.insert(Host::Remote, vec![t]);
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(r.value["taskStatus"], "completed");
    let d = m.0.lock().unwrap();
    assert_eq!(
        d.calls,
        vec![
            "control-read",
            "startup-read",
            "summary",
            "summary",
            "status-revalidate",
            "startup-confirm",
            "terminal-cas"
        ]
    );
    assert_eq!(d.writes[0].0, Host::Remote);
    assert!(!d.writes[1]
        .1
        .contains_key("simulatorRuntime.startupRecovery"));
    assert!(d.writes[1].1.contains_key("startupBinding"));
}
#[tokio::test]
async fn super_remote_confirmation_race_and_timeout_never_write_local_terminal() {
    let root = Root::new();
    for timeout in [false, true] {
        let t = super_task(true);
        let m = Memory::new(t.clone(), success_rows());
        {
            let mut d = m.0.lock().unwrap();
            d.controls.insert(Host::Remote, vec![t]);
            if timeout {
                d.fail = Some("startup-confirm".into());
            } else {
                d.patch_before_write = Some((
                    Host::Remote,
                    doc! {"simulatorRuntime":{"startupRecovery":{"phase":"started"}}},
                ));
            }
        }
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.exit_code, 1);
        assert!(!m.0.lock().unwrap().calls.contains(&"terminal-cas".into()));
    }
}
#[tokio::test]
async fn super_missing_or_mismatched_proof_fails_closed_without_failing_read() {
    let root = Root::new();
    for mode in 0..5 {
        let mut t = super_task(true);
        let mut execution = t.clone();
        match mode {
            0 => {
                t.remove("startupBinding");
            }
            1 => {
                execution.insert("launchAttemptId", "other");
            }
            2 => {
                execution.insert("simulatorRuntime", doc! {});
            }
            3 => {
                t.get_document_mut("startupBinding")
                    .unwrap()
                    .insert("remote", "true");
            }
            _ => {
                execution
                    .get_document_mut("simulatorRuntime")
                    .unwrap()
                    .insert("consumer_run_id", "other");
            }
        }
        let m = Memory::new(t, success_rows());
        m.0.lock()
            .unwrap()
            .controls
            .insert(Host::Remote, vec![execution]);
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.value["ok"], true);
        assert_eq!(r.value["terminal"], false);
        assert_eq!(r.value["startupRecovery"]["ready"], false);
        m.no_writes();
    }
}
#[tokio::test]
async fn positive_generation_proof_is_not_a_fake_startup_recovery() {
    let root = Root::new();
    let mut t = task(false);
    t.insert("taskType", "super");
    t.insert("status", "ready");
    t.remove("launchAttemptId");
    t.insert(
        "generation",
        doc! {"id":"generation-1","phase":"complete","finishedAt":"stamp"},
    );
    let m = Memory::new(t.clone(), vec![doc! {"_id":1,"status":"pending"}]);
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(
        r.value["startupRecovery"],
        json!({"ready":true,"required":false})
    );
    t.insert("events", vec![doc! {"command":"resume"}]);
    let m = Memory::new(t, success_rows());
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(r.value["startupRecovery"]["required"], true);
    assert_eq!(r.value["terminal"], false);
}
#[tokio::test]
async fn status_lock_and_store_errors_do_not_look_like_empty_success() {
    let root = Root::new();
    let m = Memory::new(task(false), success_rows());
    let lock = MissionLocks::acquire(&root.0, &[NAME], false).unwrap();
    let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read", "summary", "summary"]);
    m.no_writes(); // Long reads are now allowed; the terminal EX must still fail.
    drop(lock);
    for fail in ["control-read", "summary", "terminal-cas"] {
        let m = Memory::new(task(false), success_rows());
        m.0.lock().unwrap().fail = Some(fail.into());
        let r = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.value["error"].as_str().unwrap().contains("not retried"));
    }
}
#[tokio::test]
async fn priority_uses_original_owner_override_inspect_and_preserves_invalid_exit_zero() {
    let root = Root::new();
    let mut t = task(false);
    t.insert("taskType", "priority");
    let m = Memory::new(t, vec![]);
    let rt = Runtime {
        priority: Some(
            json!({"ok":false,"taskStatus":"unknown","reason":"root_checkpoint_unavailable_or_invalid","rootProgress":null,"budget":null}),
        ),
        ..Default::default()
    };
    let r = status(
        &root.0,
        &m,
        &rt,
        &StatusRequest {
            name: NAME.into(),
            remote: Some(true),
            inspect_items: true,
        },
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.value["ok"], false);
    assert_eq!(r.value["reason"], "root_checkpoint_unavailable_or_invalid");
    assert_eq!(*rt.calls.lock().unwrap(), vec!["priority:Remote:true"]);
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read", "status-revalidate"]);
    m.no_writes();
}
#[tokio::test]
async fn results_bounded_page_ids_dedup_stable_fitness_sort_and_no_payload() {
    let root = Root::new();
    let m = Memory::new(
        task(true),
        vec![
            doc! {"_id":4,"alphaId":"d","status":"success"},
            doc! {"_id":2,"alphaId":"b","status":"success","payload":"secret"},
            doc! {"_id":1,"alphaId":"a","status":"success","ideaId":"idea","regular":"secret"},
            doc! {"_id":3,"alphaId":"b","status":"success"},
        ],
    );
    m.0.lock().unwrap().canonical = vec![
        doc! {"id":"a","is":{"fitness":0.0,"checks":["not fetched"]},"regular":"secret"},
        doc! {"id":"b","is":{"fitness":-1.0},"pnl":"not fetched"},
    ];
    let rt = Runtime::default();
    let r = results(
        &root.0,
        &m,
        &rt,
        &ResultsRequest {
            name: NAME.into(),
            limit: 3,
            offset: 0,
            output: None,
        },
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.value);
    assert_eq!(r.value["pageAlphaIds"], 2);
    assert_eq!(r.value["pageItems"], 3);
    assert_eq!(r.value["hasMore"], true);
    assert_eq!(r.value["rows"][0]["id"], "b");
    assert_eq!(r.value["items"][0]["itemId"], "1");
    assert_eq!(r.value["items"][0]["ideaId"], "idea");
    for forbidden in ["regular", "payload", "pnl", "checks"] {
        assert!(!r.value.to_string().contains(forbidden));
    }
    assert_eq!(r.value["source"], "profile_results.canonical_fixture");
    assert_eq!(*rt.calls.lock().unwrap(), vec!["namespace:fixture-profile"]);
    assert_eq!(
        m.0.lock().unwrap().calls,
        vec!["control-read", "page", "canonical"]
    );
    m.no_writes();
}
#[tokio::test]
async fn results_validation_missing_index_and_mapping_fail_without_scan() {
    let root = Root::new();
    for (l, o) in [(0, 0), (201, 0), (1, -1), (1, 10001)] {
        let m = Memory::new(task(false), success_rows());
        let r = results(
            &root.0,
            &m,
            &Runtime::default(),
            &ResultsRequest {
                name: NAME.into(),
                limit: l,
                offset: o,
                output: None,
            },
        )
        .await;
        assert_eq!(r.value["errorType"], "ValueError");
        assert_eq!(m.0.lock().unwrap().calls, vec!["control-read"]);
    }
    let m = Memory::new(task(false), success_rows());
    m.0.lock().unwrap().indexes.clear();
    let r = results(&root.0, &m, &Runtime::default(), &ResultsRequest::new(NAME)).await;
    assert!(r.value["error"]
        .as_str()
        .unwrap()
        .contains("refusing a full collection scan"));
    let m = Memory::new(task(false), success_rows());
    let rt = Runtime {
        namespace_fail: true,
        ..Default::default()
    };
    let r = results(&root.0, &m, &rt, &ResultsRequest::new(NAME)).await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read"]);
}
#[tokio::test]
async fn results_empty_alpha_ids_skip_canonical_and_output_is_compact() {
    let root = Root::new();
    let m = Memory::new(task(false), vec![doc! {"_id":1,"status":"pending"}]);
    let output = root.0.join("nested/page.json");
    let r = results(
        &root.0,
        &m,
        &Runtime::default(),
        &ResultsRequest {
            name: NAME.into(),
            limit: 1,
            offset: 0,
            output: Some(output.clone()),
        },
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert!(r.value.get("items").is_none());
    assert_eq!(r.value["returned"], 0);
    let file: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(file["pageItems"], 1);
    assert_eq!(file["items"][0]["status"], "pending");
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read", "page"]);
}
#[tokio::test]
async fn results_retains_missing_ids_and_deactive_metadata_no_global_topk() {
    let root = Root::new();
    let mut t = task(true);
    t.insert("status", "deactive");
    let m = Memory::new(
        t,
        vec![doc! {"_id":1,"alphaId":"a"}, doc! {"_id":2,"alphaId":"b"}],
    );
    let r = results(
        &root.0,
        &m,
        &Runtime::default(),
        &ResultsRequest {
            name: NAME.into(),
            limit: 1,
            offset: 1,
            output: None,
        },
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.value["notInCanonicalStore"], json!(["b"]));
    assert_eq!(r.value["taskDataRemoved"], true);
    assert_eq!(r.value["hasMore"], false);
    assert_eq!(r.value["offset"], 1);
}
#[tokio::test]
async fn results_saved_tag_guard_and_priority_default_owner() {
    let root = Root::new();
    let mut t = task(false);
    t.insert("startConfig", doc! {"tag":{"tags":["other"]}});
    let m = Memory::new(t, success_rows());
    let r = results(&root.0, &m, &Runtime::default(), &ResultsRequest::new(NAME)).await;
    assert_eq!(r.value["errorType"], "ValueError");
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read"]);
    let mut t = task(false);
    t.insert("taskType", "priority");
    let m = Memory::new(t, vec![]);
    let rt = Runtime::default();
    assert_eq!(
        results(&root.0, &m, &rt, &ResultsRequest::new(NAME))
            .await
            .exit_code,
        0
    );
    assert_eq!(rt.calls.lock().unwrap()[0], "priority-default");
}
#[tokio::test]
async fn collect_is_the_only_full_payload_export_with_original_path_and_sort() {
    let root = Root::new();
    let m = Memory::new(
        task(true),
        vec![
            doc! {"_id":1,"variantId":"b","payloadHash":"z","payload":{"large":"allowed"},"alphaId":"a"},
            doc! {"_id":2,"variantId":"a","variantHash":"y"},
            doc! {"_id":3,"variantId":"a","payloadHash":"x"},
        ],
    );
    let r = collect_results(
        &root.0,
        &m,
        &CollectRequest {
            name: NAME.into(),
            output: None,
        },
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.value["total"], 3);
    assert_eq!(r.value["alphaIds"], 1);
    let path = root.0.join("reports/missions").join(format!("{NAME}.json"));
    assert_eq!(r.value["output"], path.to_string_lossy().as_ref());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(v["rows"][0]["payloadHash"], "x");
    assert_eq!(v["rows"][2]["payload"]["large"], "allowed");
    assert!(v["rows"][0].get("_id").is_none());
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read", "collect"]);
    m.no_writes();
}
#[tokio::test]
async fn dataset_evidence_keeps_original_owner_paths_and_host_identity() {
    let root = Root::new();
    let m = Memory::new(task(true), vec![]);
    let rt = Runtime::default();
    for action in [
        EvidenceAction::VerifyRegistration,
        EvidenceAction::ArchiveResearch,
    ] {
        let req = EvidenceRequest {
            name: NAME.into(),
            action,
            expected_task_id: Some(ID.into()),
            expected_remote: Some(true),
        };
        let r = dataset_evidence(&root.0, &m, &rt, &req).await;
        assert_eq!(r.exit_code, 0);
        if action == EvidenceAction::VerifyRegistration {
            assert_eq!(r.value["payloadPath"], "original/payload.json");
            assert_eq!(r.value["registeredCount"], 17);
        } else {
            assert_eq!(r.value["archivePath"], "original/archive.json");
        }
    }
    assert_eq!(
        *rt.calls.lock().unwrap(),
        vec!["verify:Local", "archive:Remote"]
    );
    for (id, remote) in [("different", true), (ID, false)] {
        let before = rt.calls.lock().unwrap().len();
        let r = dataset_evidence(
            &root.0,
            &m,
            &rt,
            &EvidenceRequest {
                name: NAME.into(),
                action: EvidenceAction::ArchiveResearch,
                expected_task_id: Some(id.into()),
                expected_remote: Some(remote),
            },
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(rt.calls.lock().unwrap().len(), before);
    }
    m.no_writes();
}

#[path = "support/inspection_status_cases.rs"]
mod inspection_status_cases;
#[tokio::test]
async fn original_python_status_and_startup_ast_oracle_228_cases() {
    let cases: Vec<Value> = serde_json::from_str(inspection_status_cases::CASES).unwrap();
    assert_eq!(cases.len(), 228);
    let root = Root::new();
    for mut case in cases {
        // Evolve the legacy startup-complete fixture, not its 228 old oracles.
        // New negative cases above exercise absent/stale closure independently.
        for side in ["task", "runtime"] {
            if let Some(proof) = case[side]["simulatorRuntime"]["startupRecovery"].as_object().cloned() {
                let mut closure = proof;
                closure.insert("schemaVersion".into(), json!(1));
                case[side]["simulatorRuntime"]["lifecycleClosure"] = json!(closure);
            }
        }
        let mut t = bson::to_document(&case["task"]).unwrap();
        t.insert("_id", ObjectId::parse_str(ID).unwrap());
        let rows: Vec<Document> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| bson::to_document(r).unwrap())
            .collect();
        let remote = t.get_bool("isRemote").unwrap();
        let m = Memory::new(t.clone(), rows);
        if remote {
            let mut runtime = if case.get("runtime").is_some() {
                bson::to_document(&case["runtime"]).unwrap()
            } else {
                t
            };
            runtime.insert("_id", ObjectId::parse_str(ID).unwrap());
            let mut d = m.0.lock().unwrap();
            d.controls.insert(Host::Remote, vec![runtime]);
            d.items.insert(Host::Local, vec![]);
        }
        let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        let expected = &case["expected"];
        if let Some(value) = expected.get("value") {
            assert_eq!(out.exit_code, 0, "{}: {}", case["label"], out.value);
            for (key, value) in value.as_object().unwrap() {
                assert_eq!(&out.value[key], value, "{} key {key}", case["label"]);
            }
        } else {
            assert_eq!(out.exit_code, 1, "{}: {}", case["label"], out.value);
            assert_eq!(
                out.value["errorType"], expected["errorType"],
                "{}",
                case["label"]
            );
            assert_eq!(out.value["error"], expected["error"], "{}", case["label"]);
        }
        assert_eq!(
            m.0.lock().unwrap().writes.len() as u64,
            expected["writeCount"].as_u64().unwrap(),
            "{}",
            case["label"]
        );
    }
}

#[tokio::test]
async fn results_and_explicit_collect_preserve_python_datetime_wire_text() {
    let root = Root::new();
    let m = Memory::new(task(false), success_rows());
    m.0.lock().unwrap().canonical = vec![
        doc! {"id":"a","dateCreated":bson::DateTime::from_millis(1234),"dateModified":bson::DateTime::from_millis(1000)},
    ];
    let result = results(&root.0, &m, &Runtime::default(), &ResultsRequest::new(NAME)).await;
    assert_eq!(
        result.value["rows"][0]["dateCreated"],
        "1970-01-01 00:00:01.234000"
    );
    assert_eq!(
        result.value["rows"][0]["dateModified"],
        "1970-01-01 00:00:01"
    );
    m.0.lock().unwrap().items.get_mut(&Host::Local).unwrap()[0]
        .insert("finishedAt", bson::DateTime::from_millis(1234));
    let output = root.0.join("audit.json");
    let result = collect_results(
        &root.0,
        &m,
        &CollectRequest {
            name: NAME.into(),
            output: Some(output.clone()),
        },
    )
    .await;
    assert_eq!(result.exit_code, 0);
    let value: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(value["rows"][0]["finishedAt"], "1970-01-01 00:00:01.234000");
}
#[tokio::test]
async fn malformed_sort_keys_or_unhashable_ids_do_not_silently_make_success() {
    let root = Root::new();
    let m = Memory::new(task(false), vec![doc! {"_id":1,"alphaId":{"bad":"id"}}]);
    let result = results(&root.0, &m, &Runtime::default(), &ResultsRequest::new(NAME)).await;
    assert_eq!(result.value["errorType"], "TypeError");
    assert_eq!(m.0.lock().unwrap().calls, vec!["control-read", "page"]);
    let m = Memory::new(
        task(false),
        vec![
            doc! {"_id":1,"variantId":"a"},
            doc! {"_id":2,"variantId":null},
        ],
    );
    let output = root.0.join("must-not-exist.json");
    let result = collect_results(
        &root.0,
        &m,
        &CollectRequest {
            name: NAME.into(),
            output: Some(output.clone()),
        },
    )
    .await;
    assert_eq!(result.value["errorType"], "TypeError");
    assert!(!output.exists());
}
#[tokio::test]
async fn paused_stop_proof_remains_distinct_from_terminal_and_fresh_launch() {
    let root = Root::new();
    let mut t = task(false);
    t.insert("status", "paused");
    t.insert("stopConfirmedAt", bson::DateTime::from_millis(2000));
    t.insert("stopConfirmedTaskId", ID);
    t.insert("stopConfirmedRemote", false);
    t.insert("launchAttemptAt", bson::DateTime::from_millis(1000));
    let m = Memory::new(t.clone(), vec![doc! {"_id":1,"status":"pending"}]);
    let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(out.value["taskStatus"], "paused");
    assert_eq!(out.value["stopConfirmation"], "confirmed");
    assert_eq!(out.value["terminal"], false);
    m.no_writes();
    t.insert("launchAttemptAt", bson::DateTime::from_millis(3000));
    let m = Memory::new(t, vec![doc! {"_id":1,"status":"pending"}]);
    let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(out.value["taskStatus"], "unknown");
    assert_eq!(out.value["stopConfirmation"], "unknown");
    m.no_writes();
}

#[tokio::test]
async fn local_super_changed_runtime_proof_never_commits_cached_terminal_counts() {
    let root = Root::new();
    let t = super_task(false);
    let m = Memory::new(t, success_rows());
    m.0.lock().unwrap().patch_before_write = Some((
        Host::Local,
        doc! {"simulatorRuntime":{"consumer_run_id":"successor","startupRecovery":{"phase":"started"}}},
    ));
    let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(out.exit_code, 1);
    assert_eq!(m.control().get_str("status").unwrap(), "running");
}
#[tokio::test]
async fn super_malformed_unhashable_status_and_history_keep_original_type_failure() {
    let root = Root::new();
    for mutation in [doc! {"status":[]}, doc! {"events":[{"command":{}}]}] {
        let mut t = task(false);
        t.remove("launchAttemptId");
        t.insert("taskType", "super");
        t.insert("status", "ready");
        t.insert(
            "generation",
            doc! {"id":"g","phase":"complete","finishedAt":"stamp"},
        );
        t.extend(mutation);
        let m = Memory::new(t, success_rows());
        let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(out.value["errorType"], "TypeError", "{}", out.value);
        m.no_writes();
    }
}

#[tokio::test]
async fn final_closure_not_startup_gates_twelve_timeout_wave_and_same_run_continuation() {
    for remote in [false, true] {
        let root = Root::new();
        let h = Host::from_remote(remote);
        let mut t = super_task(remote);
        let closure = t.get_document_mut("simulatorRuntime").unwrap().remove("lifecycleClosure").unwrap();
        let rows = (0..12).map(|id| doc! {"_id":id,"status":"timeout_error"}).collect();
        let m = Memory::new(t.clone(), rows);
        if remote { m.0.lock().unwrap().controls.insert(h, vec![t.clone()]); }
        let first = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(first.value["terminal"], false, "{}", first.value);
        assert_eq!(first.value["startupRecovery"]["ready"], true);
        assert_eq!(first.value["lifecycleClosure"]["ready"], false);
        assert_eq!(first.value["unfinished"], 0);
        m.no_writes();
        // Same owner can still release capacity noPOST rows after this snapshot.
        { let mut d = m.0.lock().unwrap();
          for row in d.items.get_mut(&h).unwrap() { row.insert("status", "pending"); }
          d.items.get_mut(&h).unwrap()[0].insert("status", "processing"); }
        let next = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(next.value["terminal"], false);
        assert_eq!(next.value["unfinished"], 12);
        assert_eq!(m.control().get_str("launchAttemptId").unwrap(), "launch-1");
        m.no_writes();
        // A genuinely closed timeout/error wave remains terminal, not excluded.
        { let mut d = m.0.lock().unwrap();
          for row in d.items.get_mut(&h).unwrap() { row.insert("status", "timeout_error"); }
          set(&mut d.controls.get_mut(&h).unwrap()[0], "simulatorRuntime.lifecycleClosure", closure); }
        let last = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(last.value["terminal"], true, "{}", last.value);
        assert_eq!(last.value["taskStatus"], "completed_with_errors");
    }
}

#[tokio::test]
async fn final_closure_identity_and_schema_are_exact_on_both_hosts() {
    for remote in [false, true] {
        for (field, value) in [
            ("schemaVersion", Bson::Int32(0)), ("schemaVersion", Bson::Boolean(true)),
            ("phase", "recovering".into()), ("completedAt", Bson::Null),
            ("taskId", "other".into()), ("name", "other".into()),
            ("auth_profile", "other".into()), ("remote", (!remote).into()),
            ("launchAttemptId", "old".into()), ("consumer_run_id", "old".into()),
        ] {
            let root = Root::new();
            let mut t = super_task(remote);
            set(&mut t, &format!("simulatorRuntime.lifecycleClosure.{field}"), value);
            let m = Memory::new(t.clone(), success_rows());
            if remote { m.0.lock().unwrap().controls.insert(Host::Remote, vec![t]); }
            let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
            assert_eq!(out.value["terminal"], false, "{field}: {}", out.value);
            assert_eq!(out.value["startupRecovery"]["ready"], true);
            m.no_writes();
        }
    }
}

#[tokio::test]
async fn historical_completed_unfinished_is_display_only_even_without_control_revision_change() {
    for remote in [false, true] {
        let root = Root::new();
        let mut t = super_task(remote);
        t.insert("status", "completed_with_errors");
        t.get_document_mut("simulatorRuntime").unwrap().remove("lifecycleClosure");
        let m = Memory::new(t.clone(), vec![doc! {"_id":1,"status":"processing"}]);
        if remote { m.0.lock().unwrap().controls.insert(Host::Remote, vec![t.clone()]); }
        let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(out.value["terminal"], false);
        assert_eq!(out.value["taskStatus"], "completed_with_errors");
        assert_eq!(out.value["controlStatus"], "completed_with_errors");
        assert_eq!(out.value["lifecycleDiagnostic"]["unfinished"], 1);
        assert_eq!(m.control(), t);
        m.no_writes();
    }
}

#[tokio::test]
async fn closure_cas_cannot_commit_across_pause_observation_or_successor() {
    for patch in [
        doc! {"status":"paused"}, doc! {"status":"deactive"},
        doc! {"launchAttemptId":"successor"},
        doc! {"acceptedObservation":{"id":"new-observer"}},
        doc! {"simulatorRuntime":{"consumer_run_id":"consumer-1","lifecycleClosure":null}},
    ] {
        let root = Root::new();
        let m = Memory::new(super_task(false), success_rows());
        m.0.lock().unwrap().patch_before_write = Some((Host::Local, patch));
        let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
        assert_eq!(out.exit_code, 1, "{}", out.value);
        assert_ne!(m.control().get_str("status").unwrap(), "completed");
    }
}

#[tokio::test]
async fn remote_final_closure_cas_rejects_replacement_after_snapshot() {
    let root = Root::new();
    let t = super_task(true);
    let m = Memory::new(t.clone(), success_rows());
    { let mut d = m.0.lock().unwrap();
      d.controls.insert(Host::Remote, vec![t]);
      d.patch_before_write = Some((Host::Remote, doc! {
          "simulatorRuntime":{"consumer_run_id":"consumer-1","lifecycleClosure":null}
      })); }
    let out = status(&root.0, &m, &Runtime::default(), &StatusRequest::new(NAME)).await;
    assert_eq!(out.exit_code, 1);
    assert_eq!(m.control().get_str("status").unwrap(), "running");
    assert!(m.0.lock().unwrap().writes.iter().all(|(h, _, _)| *h == Host::Remote));
}

#[tokio::test]
async fn common_task_control_status_facade_preserves_open_wave_then_closed_same_run() {
    use quantnight_task_core::{
        backend_binding::validate_backend_binding,
        config::bash_test_support::FakeBash,
        task_control::TaskControl,
    };
    use std::os::unix::fs::PermissionsExt;
    for remote in [false, true] {
        // Reuse the original binding-owner fixture: only its external process
        // response and TaskStore are fake. No inspection/facade success mock.
        let fixture = FakeBash::new("exit 91 # non-priority status must not launch runtime scripts");
        let root = Path::new(&fixture.config.env.working_dir);
        std::fs::write(root.join("credentials.yaml"), "synthetic fixture only\n").unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let python = root.join(".venv/bin/python");
        std::fs::write(&python, r#"#!/bin/sh
[ "$1" = '-B' ] && [ "$2" = '-m' ] && [ "$3" = 'backend.mongodb.mongo_config' ] && [ "$4" = '--validate-task-control-binding' ] || exit 92
printf 'binding-process\n' >> binding_calls
printf '%s\n' '{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}'
"#).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
        let binding = validate_backend_binding(&fixture.config).await.unwrap();
        let h = Host::from_remote(remote);
        let mut t = super_task(remote);
        let proof = t.get_document_mut("simulatorRuntime").unwrap().remove("lifecycleClosure").unwrap();
        let m = Memory::new(t.clone(), (0..12).map(|id| doc! {"_id":id,"status":"timeout_error"}).collect());
        if remote { m.0.lock().unwrap().controls.insert(h, vec![t]); }
        let control = TaskControl::with_store(binding, Arc::new(m.clone()));
        let request = StatusRequest::new(NAME);
        let first = control.status(&request).await;
        assert_eq!(first.exit_code, 0, "{}", first.value);
        assert_eq!(first.value["terminal"], false, "{}", first.value);
        assert_eq!(first.value["taskStatus"], "running");
        assert_eq!(first.value["startupRecovery"]["ready"], true);
        assert_eq!(first.value["lifecycleClosure"]["ready"], false);
        m.no_writes();
        { let mut d = m.0.lock().unwrap();
          for row in d.items.get_mut(&h).unwrap() { row.insert("status", "pending"); }
          d.items.get_mut(&h).unwrap()[0].insert("status", "processing"); }
        let continued = control.status(&request).await;
        assert_eq!(continued.value["unfinished"], 12);
        assert_eq!(continued.value["terminal"], false);
        m.no_writes();
        { let mut d = m.0.lock().unwrap();
          for row in d.items.get_mut(&h).unwrap() { row.insert("status", "success"); }
          set(&mut d.controls.get_mut(&h).unwrap()[0], "simulatorRuntime.lifecycleClosure", proof); }
        let closed = control.status(&request).await;
        assert_eq!(closed.value["terminal"], true, "{}", closed.value);
        assert_eq!(closed.value["taskStatus"], "completed");
        let stored = m.control();
        assert_eq!(stored.get_str("launchAttemptId").unwrap(), "launch-1");
        assert_eq!(stored.get_array("events").unwrap().len(), 1);
        assert_eq!(stored.get_array("events").unwrap()[0].as_document().unwrap().get_str("command").unwrap(), "status-terminal-reconcile");
        // Bypassing the original public implementation loses its counted reads
        // or CAS/event, and cannot satisfy this oracle. No runtime shell fallback.
        let d = m.0.lock().unwrap();
        assert_eq!(d.calls.iter().filter(|c| c.as_str() == "control-read").count(), 3);
        assert_eq!(d.calls.iter().filter(|c| c.as_str() == "startup-read").count(), 3);
        assert_eq!(d.writes.iter().filter(|(host, _, _)| *host == Host::Local).count(), 1);
        assert_eq!(std::fs::read_to_string(root.join("binding_calls")).unwrap(), "binding-process\n");
    }
}


#[async_trait]
impl quantnight_task_core::pause::PauseRuntime for Runtime {
    async fn stop(&self, _: &Path, task: &Document, never: bool) -> Result<quantnight_task_core::pause::StopOutput, String> {
        assert!(!never);
        assert_eq!(task.get_str("launchAttemptId").unwrap(), "launch-1");
        self.calls.lock().unwrap().push("original-stop-once".into());
        Ok(quantnight_task_core::pause::StopOutput {
            stdout: format!("QUANTNIGHT_PAUSE_CONFIRMED {}", json!({"schema_version":1,"task_name":NAME,
                "target_remote":task.get_bool("isRemote").unwrap(),"session_absent":true,"runners_exited":true})),
            stderr: String::new(),
        })
    }
}

#[tokio::test]
async fn long_status_reads_allow_real_pause_and_never_overwrite_its_new_state() {
    use quantnight_task_core::pause::{pause, PauseOptions};
    for kind in ["regular", "super", "priority"] {
      for remote in [false, true] {
        let root = Root::new();
        let gate = Arc::new(ReadGate::default());
        let mut t = if kind == "super" { super_task(remote) } else { task(remote) };
        t.insert("taskType", kind);
        let m = Memory::new(t.clone(), success_rows());
        if remote { m.0.lock().unwrap().controls.insert(Host::Remote, vec![t]); }
        let runtime = Arc::new(Runtime { priority_gate: (kind == "priority").then(|| gate.clone()), ..Default::default() });
        if kind != "priority" { m.0.lock().unwrap().summary_gate = Some(gate.clone()); }
        let (r, store, rt) = (root.0.clone(), m.clone(), runtime.clone());
        let reading = tokio::spawn(async move { status(&r, &store, rt.as_ref(), &StatusRequest::new(NAME)).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.entered.notified()).await.unwrap();
        let stopped = pause(&root.0, &m, runtime.as_ref(), NAME, PauseOptions::default()).await;
        assert_eq!(stopped.exit_code, 0, "{kind}: {}", stopped.value);
        assert!(stopped.confirmed_stop.is_some());
        assert!(!reading.is_finished(), "status I/O must remain suspended, not faked complete");
        let paused = m.control();
        gate.release.notify_one();
        let stale = tokio::time::timeout(std::time::Duration::from_secs(2), reading).await.unwrap().unwrap();
        assert_eq!(stale.exit_code, 1, "stale {kind} read cannot report current completion: {}", stale.value);
        assert_eq!(m.control(), paused);
        assert_eq!(runtime.calls.lock().unwrap().iter().filter(|c| *c == "original-stop-once").count(), 1);
        let d = m.0.lock().unwrap();
        assert_eq!(d.writes.len(), 1, "only the real pause CAS may write");
        assert_eq!(d.controls[&Host::Local][0].get_str("status").unwrap(), "paused");
    }
  }
}

#[tokio::test]
async fn long_status_snapshot_must_revalidate_identity_before_terminal_lock_and_cas() {
    for patch in [doc! {"launchAttemptId":"next"}, doc! {"isRemote":true},
        doc! {"generation":{"id":"next"}}, doc! {"acceptedObservation":{"id":"new"}},
        doc! {"updatedAt":"next"}, doc! {"runId":"next"}, doc! {"status":"deactive"},
        doc! {"simulatorRuntime":{"consumer_run_id":"consumer-1","lifecycleClosure":null}}] {
        let root = Root::new(); let gate = Arc::new(ReadGate::default());
        let m = Memory::new(super_task(false), success_rows());
        m.0.lock().unwrap().summary_gate = Some(gate.clone());
        let (r, store) = (root.0.clone(), m.clone());
        let reading = tokio::spawn(async move { status(&r, &store, &Runtime::default(), &StatusRequest::new(NAME)).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.entered.notified()).await.unwrap();
        m.0.lock().unwrap().controls.get_mut(&Host::Local).unwrap()[0].extend(patch);
        let changed = m.control(); gate.release.notify_one();
        let out = reading.await.unwrap();
        assert_eq!(out.exit_code, 1, "{}", out.value);
        assert_eq!(m.control(), changed); m.no_writes();
    }
}

#[tokio::test]
async fn terminal_reconcile_conflicting_original_lock_never_reports_committed_success() {
    let root = Root::new(); let gate = Arc::new(ReadGate::default());
    let m = Memory::new(super_task(false), success_rows());
    m.0.lock().unwrap().summary_gate = Some(gate.clone());
    let (r, store) = (root.0.clone(), m.clone());
    let reading = tokio::spawn(async move { status(&r, &store, &Runtime::default(), &StatusRequest::new(NAME)).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), gate.entered.notified()).await.unwrap();
    let lock = MissionLocks::acquire(&root.0, &[NAME], false).expect("long read must not hold EX or SH");
    gate.release.notify_one(); let out = reading.await.unwrap();
    assert_eq!(out.exit_code, 1); assert_eq!(out.value["ok"], false);
    assert!(out.value["error"].as_str().unwrap().contains("lifecycle mutator"));
    m.no_writes(); drop(lock);
}

#[tokio::test]
async fn legacy_missing_enhancements_do_not_block_status_results_or_original_pause() {
    use quantnight_task_core::pause::{pause, PauseOptions};
    for remote in [false, true] {
        for missing in ["closure", "startup", "all"] {
            let root = Root::new(); let mut t = super_task(remote);
            t.remove("tmuxContext");
            t.get_document_mut("simulatorRuntime").unwrap().remove("lifecycleClosure");
            if missing != "closure" { t.get_document_mut("simulatorRuntime").unwrap().remove("startupRecovery"); }
            if missing == "all" { t.remove("startupBinding"); t.remove("simulatorRuntime"); }
            let rows = vec![doc! {"_id":1,"status":"timeout_error","acceptance":"UNKNOWN","campaign_post_attempted":true,"superSlotId":"retain"},
                doc! {"_id":2,"status":"success","alphaId":"old-alpha","acceptance":"ACCEPTED"}];
            let m = Memory::new(t.clone(), rows.clone());
            if remote { m.0.lock().unwrap().controls.insert(Host::Remote, vec![t.clone()]); }
            let rt = Runtime::default();
            let observed = status(&root.0, &m, &rt, &StatusRequest::new(NAME)).await;
            assert_eq!(observed.exit_code, 0, "{missing}: {}", observed.value);
            assert_eq!(observed.value["taskStatus"], "running");
            assert_eq!(observed.value["terminal"], false);
            let saved = results(&root.0, &m, &rt, &ResultsRequest::new(NAME)).await;
            assert_eq!(saved.exit_code, 0, "{}", saved.value);
            assert_eq!(saved.value["pageItems"], 2);
            m.no_writes();
            let stopped = pause(&root.0, &m, &rt, NAME, PauseOptions::default()).await;
            assert_eq!(stopped.exit_code, 0, "{}", stopped.value);
            assert!(stopped.confirmed_stop.is_some());
            assert_eq!(rt.calls.lock().unwrap().iter().filter(|c| *c == "original-stop-once").count(), 1);
            let d = m.0.lock().unwrap();
            assert_eq!(d.items[&Host::from_remote(remote)], rows, "stop must retain unresolved platform receipts/slots/results");
            assert!(!d.controls[&Host::Local][0].get_document("simulatorRuntime").map(|r| r.contains_key("lifecycleClosure")).unwrap_or(false));
        }
    }
}

#[tokio::test]
async fn stale_status_retains_successful_facts_but_never_calls_them_current() {
    let root=Root::new(); let gate=Arc::new(ReadGate::default());
    let m=Memory::new(super_task(false),success_rows());
    m.0.lock().unwrap().summary_gate=Some(gate.clone());
    let (r,store)=(root.0.clone(),m.clone());
    let reading=tokio::spawn(async move {status(&r,&store,&Runtime::default(),&StatusRequest::new(NAME)).await});
    tokio::time::timeout(std::time::Duration::from_secs(2),gate.entered.notified()).await.unwrap();
    m.0.lock().unwrap().controls.get_mut(&Host::Local).unwrap()[0].insert("status","paused");
    gate.release.notify_one();let out=reading.await.unwrap();
    assert_eq!(out.exit_code,1);assert_eq!(out.value["snapshotCurrent"],false);
    assert_eq!(out.value["execution"]["total"],1);
    assert_eq!(out.value["reconcileSkipped"],true);
    assert_eq!(out.value["terminal"],false);
    assert_eq!(out.value["capturedTaskStatus"],"running");
    assert!(out.value.get("taskStatus").is_none());
    assert_eq!(out.value["observationIdentity"]["launchAttemptId"],"launch-1");
    m.no_writes();
}

#[path = "support/original_professional_runtime.rs"]
mod original_professional_runtime;

#[tokio::test]
async fn original_legacy_status_separates_stop_platform_and_result_facts_without_forged_closure() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::OriginalOperationsRuntime};
    for remote in [false,true] {for case in ["v1_complete","v2_only","unknown_receipt","result_pending","legacy_event","conflicting_stop"] {
        let fixture=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fixture.config.env.working_dir);
        let mut t=task(remote);t.insert("taskType","super");t.insert("status","completed_with_errors");
        t.insert("stopConfirmedAt",bson::DateTime::from_millis(2000));t.insert("stopConfirmedTaskId",ID);t.insert("stopConfirmedRemote",remote);
        if case=="legacy_event" {
            t.insert("events",vec![doc!{"command":"pause","status":"paused","at":"1970-01-01T00:00:02Z",
                "detail":{"schema_version":1,"task_name":NAME,"target_remote":remote,"session_absent":true,"runners_exited":true}}]);
        } else {t.insert("stopConfirmationBasis",if case=="v2_only"{"session_absent"}else{"runners_exited"});}
        if case=="conflicting_stop"{t.insert("stopConfirmedTaskId","foreign");}
        let mut row=doc!{"_id":1,"status":"success","alphaId":"old-alpha","detailsFetchedAt":"saved","resultMetrics":{"fitness":1.0,"sharpe":1.0,"turnover":0.1,"returns":0.1,"drawdown":0.1,"margin":0.01}};
        if case=="unknown_receipt"{row.insert("acceptance","UNKNOWN");}
        if case=="result_pending"{row.remove("detailsFetchedAt");row.remove("resultMetrics");}
        let m=Memory::new(t.clone(),vec![row]);if remote{m.0.lock().unwrap().controls.insert(Host::Remote,vec![t.clone()]);}
        let binding=validate_backend_binding(&fixture.config).await.unwrap();
        let runtime=OriginalOperationsRuntime{binding:&binding,observer:None};
        let out=status(root,&m,&runtime,&StatusRequest::new(NAME)).await;
        assert_eq!(out.exit_code,0,"{remote}/{case}: {}",out.value);
        let ready=matches!(case,"v1_complete"|"legacy_event");
        assert_eq!(out.value["terminal"],ready);
        if matches!(case,"v2_only"|"conflicting_stop") {
            // Frozen narrowed scope: no PID query can promote V2/foreign proof.
            // Preserve the old nonterminal/zero-write oracle and additionally
            // forbid manufacturing current stopped facts from a persisted proof.
            assert!(out.value.get("executionObservation").is_none());
            assert!(out.value.get("completionVerification").is_none());
            assert!(!root.join("trace").exists());
        } else {
            assert_eq!(out.value["executionObservation"]["consumerStopped"],true);
            assert_eq!(out.value["executionObservation"]["naturalClosure"],false);
            assert_eq!(out.value["completionVerification"]["ready"],ready,"{remote}/{case}: {}",out.value);
            assert_eq!(out.value["completionVerification"]["runnerProofCreated"],false);
            assert_eq!(out.value["receiptObservation"]["complete"],true);
            if case=="unknown_receipt"{assert_eq!(out.value["receiptObservation"]["receiptUnresolvedItemCount"],1);}
            if case=="result_pending"{assert_eq!(out.value["receiptObservation"]["resultPendingItemCount"],1);}
            let trace=std::fs::read_to_string(root.join("trace")).unwrap();assert!(!trace.contains("if-shell")&&!trace.contains("new-session"));
        }
        assert_eq!(m.control(),t);m.no_writes();
    }}
}

#[test]
fn legacy_assessment_preserves_unknown_even_with_success_and_all_result_fields() {
    for unknown in ["UNKNOWN","unknown","Unknown"] {
        let row=doc!{"_id":1,"status":"success","alphaId":"old-alpha","acceptance":unknown,"detailsFetchedAt":"saved",
            "resultMetrics":{"fitness":Bson::Null,"sharpe":Bson::Null,"turnover":Bson::Null,"returns":Bson::Null,"drawdown":Bson::Null,"margin":Bson::Null}};
        let assessed=task_store::assess_execution_receipts(&[row]);
        assert_eq!(assessed.get_i64("unresolvedItemCount").unwrap(),1);
        assert_eq!(assessed.get_i64("receiptUnresolvedItemCount").unwrap(),1);
        assert_eq!(assessed.get_i64("resultPendingItemCount").unwrap(),0);
    }
}

#[tokio::test]
async fn completion_ordinary_status_missing_closure_does_not_query_processes() {
    for remote in [false,true] {for state in ["processing","pending","timeout_error","success"] {
        let root=Root::new();let mut t=super_task(remote);
        t.get_document_mut("simulatorRuntime").unwrap().remove("lifecycleClosure");
        let m=Memory::new(t.clone(),vec![doc!{"_id":1,"status":state}]);
        if remote{m.0.lock().unwrap().controls.insert(Host::Remote,vec![t]);}
        let runtime=Runtime::default();
        let out=status(&root.0,&m,&runtime,&StatusRequest::new(NAME)).await;
        assert_eq!(out.exit_code,0,"{}",out.value);assert_eq!(out.value["terminal"],false);
        assert_eq!(out.value["execution"]["total"],1);
        assert!(!runtime.calls.lock().unwrap().iter().any(|c|c=="process-inspection"));m.no_writes();
    }}
}
