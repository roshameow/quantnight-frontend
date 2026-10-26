//! Offline real orchestration with ONLY storage/runtime IO replaced.
//! Import the registered production crate; do not compile a parallel copy.
use quantnight_task_core::{cleanup_sync, mission_lock, task_store};

use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, DateTime, Document};
use cleanup_sync::{OperationsRuntime, TaskOutcome};
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
const OTHER: &str = "1123456789abcdef01234567";
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "qn-cleanup-sync-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn receipt(&self) -> Value {
        serde_json::from_slice(
            &std::fs::read(self.0.join("reports/missions/fixture.cleanup.json")).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn registration(status: &str, remote: bool) -> Document {
    doc! {"_id": ObjectId::parse_str(ID).unwrap(), "name": NAME, "status": status,
    "isRemote": remote, "targetRemote": true, "auth_profile": "fixture-profile",
    "taskType": "regular", "templatefile": "fixture-template", "mission_config": {"keep": 7},
    "stopConfirmedAt": DateTime::from_millis(2000), "stopConfirmedTaskId": ID,
    "stopConfirmedRemote": remote, "launchAttemptAt": DateTime::from_millis(1000)}
}
fn rows(n: usize) -> Vec<Document> {
    (0..n).map(|i| doc! {"_id": format!("item-{i:04}"), "status": "pending", "payload": {"expression": format!("x-{i}"), "n": i as i64}}).collect()
}
fn terminal() -> Vec<Document> {
    vec![doc! {"_id": "done", "status": "success", "alphaId": "alpha-1", "acceptance": "ACCEPTED"}]
}
fn archive() -> String {
    format!("{NAME}__archive_{ID}")
}
fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(key, value)| match value {
        Bson::Document(op) if op.contains_key("$exists") => {
            row.contains_key(key) == op.get_bool("$exists").unwrap()
        }
        Bson::Document(op) if op.contains_key("$ne") => row.get(key) != op.get("$ne"),
        Bson::Null => matches!(row.get(key), None | Some(Bson::Null)),
        _ => row.get(key) == Some(value),
    })
}
fn project_row(row: &Document, projection: Option<&Document>) -> Document {
    let Some(projection) = projection else {
        return row.clone();
    };
    assert!(
        projection.values().all(|v| matches!(v, Bson::Int32(0 | 1))),
        "fixture supports exact 0/1 inclusion projections only"
    );
    let inclusion = projection
        .iter()
        .any(|(k, v)| k != "_id" && v == &Bson::Int32(1))
        || projection.get("_id") == Some(&Bson::Int32(1));
    row.iter()
        .filter(|(key, _)| {
            if key.as_str() == "_id" {
                projection.get(*key) != Some(&Bson::Int32(0))
            } else if inclusion {
                projection.get(*key) == Some(&Bson::Int32(1))
            } else {
                projection.get(*key) != Some(&Bson::Int32(0))
            }
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
#[derive(Debug, Clone)]
struct ReadQuery {
    kind: &'static str,
    host: Host,
    name: String,
    projection: Option<Document>,
    limit: Option<i64>,
    filter: Document,
    pipeline: Vec<Document>,
    returned: usize,
    returned_bytes: usize,
}
#[derive(Default)]
struct State {
    tasks: HashMap<Host, Vec<Document>>,
    collections: HashMap<(Host, String), Vec<Document>>,
    calls: Vec<String>,
    writes: Vec<(Host, Document, Document)>,
    fail: Option<String>,
    cas_fail: Option<String>,
    insert_calls: usize,
    fail_insert: Option<usize>,
    extra_after_copy: bool,
    change_source_after_copy: bool,
    replace_remote_after_copy: bool,
    rename_lie: bool,
    corrupt_claim: bool,
    queries: Vec<ReadQuery>,
    full_reads_left: Option<usize>,
    timeout_after_write: Option<String>,
}
#[derive(Clone, Default)]
struct MemoryStore(Arc<Mutex<State>>);
impl MemoryStore {
    fn new(status: &str, remote: bool, local: Vec<Document>) -> Self {
        let s = Self::default();
        {
            let mut st = s.0.lock().unwrap();
            st.tasks
                .insert(Host::Local, vec![registration(status, remote)]);
            st.collections.insert((Host::Local, NAME.into()), local);
        }
        s
    }
    fn collection(&self, host: Host, name: &str) -> Option<Vec<Document>> {
        self.0
            .lock()
            .unwrap()
            .collections
            .get(&(host, name.into()))
            .cloned()
    }
    fn control(&self) -> Document {
        self.0.lock().unwrap().tasks[&Host::Local][0].clone()
    }
    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
    }
    fn remote(&self, data: Vec<Document>) {
        let mut st = self.0.lock().unwrap();
        st.collections.insert((Host::Remote, NAME.into()), data);
        st.tasks
            .insert(Host::Remote, vec![registration("paused", true)]);
    }
    fn fail(&self, at: &str) {
        self.0.lock().unwrap().fail = Some(at.into());
    }
}
fn host(h: Host) -> &'static str {
    if h == Host::Local {
        "local"
    } else {
        "remote"
    }
}
fn call(st: &mut State, label: String) -> Result<(), String> {
    st.calls.push(label.clone());
    if st.fail.as_deref() == Some(label.as_str()) {
        st.fail = None;
        return Err(format!("injected {label}"));
    }
    Ok(())
}
#[async_trait]
impl TaskStore for MemoryStore {
    async fn find_task(
        &self,
        h: Host,
        q: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("find:{}", host(h)))?;
        let mut tasks = st.tasks.get(&h).cloned().unwrap_or_default();
        if newest {
            tasks.sort_by_key(|d| d.get_object_id("_id").unwrap().to_hex());
            tasks.reverse();
        }
        Ok(tasks.into_iter().find(|d| matches(d, &q)))
    }
    async fn update_task(&self, h: Host, q: Document, u: Document) -> Result<u64, String> {
        let mut st = self.0.lock().unwrap();
        let command = u
            .get_document("$push")
            .ok()
            .and_then(|p| p.get_document("events").ok())
            .and_then(|e| e.get_str("command").ok())
            .unwrap_or("remote-hide");
        let label = format!("update:{}:{command}", host(h));
        call(&mut st, label.clone())?;
        st.writes.push((h, q.clone(), u.clone()));
        let timed_out = st.timeout_after_write.as_deref() == Some(label.as_str());
        if timed_out {
            st.timeout_after_write = None;
        }
        let corrupt_claim = st.corrupt_claim && command.ends_with("-begin");
        if st.cas_fail.as_deref() == Some(&label) {
            st.cas_fail = None;
            return Ok(0);
        }
        if let Some(row) = st
            .tasks
            .entry(h)
            .or_default()
            .iter_mut()
            .find(|d| matches(d, &q))
        {
            if let Ok(set) = u.get_document("$set") {
                row.extend(set.clone());
            }
            if let Ok(push) = u.get_document("$push") {
                for (k, v) in push {
                    if !row.contains_key(k) {
                        row.insert(k, Bson::Array(vec![]));
                    }
                    match row.get_mut(k) {
                        Some(Bson::Array(a)) => a.push(v.clone()),
                        _ => return Err("fake bad push target".into()),
                    }
                }
            }
            if corrupt_claim {
                row.insert("isRemote", !row.get_bool("isRemote").unwrap());
            }
            if timed_out {
                return Err(format!("{label}: timeout; result unconfirmed, not retried"));
            }
            Ok(1)
        } else {
            Ok(0)
        }
    }
    async fn collection_names(&self, h: Host) -> Result<Vec<String>, String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("names:{}", host(h)))?;
        Ok(st
            .collections
            .keys()
            .filter(|(side, _)| *side == h)
            .map(|(_, n)| n.clone())
            .collect())
    }
    async fn summary(&self, h: Host, n: &str) -> Result<CollectionSummary, String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("summary:{}:{n}", host(h)))?;
        call(&mut st, format!("names:{}", host(h)))?;
        if !st.collections.contains_key(&(h, n.into())) {
            return Ok(CollectionSummary::default());
        }
        call(&mut st, format!("aggregate:{}:{n}", host(h)))?;
        let pipeline = task_store::summary_pipeline();
        assert_eq!(
            pipeline,
            vec![doc! {"$group":{"_id":{"$ifNull":["$status","missing"]},"count":{"$sum":1}}}]
        );
        let mut summary = CollectionSummary {
            exists: true,
            ..Default::default()
        };
        // Lowest fake storage evaluates status aggregation without cloning any
        // document/payload; production performs this grouping on Mongo itself.
        for row in &st.collections[&(h, n.into())] {
            let key = match row.get("status") {
                None | Some(Bson::Null) => "missing".into(),
                Some(Bson::String(v)) => v.clone(),
                Some(v) => v.to_string(),
            };
            *summary.statuses.entry(key).or_default() += 1;
            summary.total += 1;
        }
        st.queries.push(ReadQuery {
            kind: "aggregate",
            host: h,
            name: n.into(),
            projection: None,
            limit: None,
            filter: doc! {},
            pipeline,
            returned: summary.statuses.len(),
            returned_bytes: 0,
        });
        call(&mut st, format!("heartbeat:{}:{n}", host(h)))?;
        let (filter, options) = task_store::heartbeat_query();
        let latest = st.collections[&(h, n.into())]
            .iter()
            .filter(|row| matches(row, &filter))
            .max_by_key(|row| match row.get("heartbeatAt") {
                Some(Bson::DateTime(v)) => format!("{:020}", v.timestamp_millis()),
                Some(v) => v.to_string(),
                None => String::new(),
            })
            .map(|row| project_row(row, options.projection.as_ref()));
        let bytes = latest
            .as_ref()
            .map(|row| bson::to_vec(row).unwrap().len())
            .unwrap_or(0);
        summary.latest_heartbeat_at = latest
            .as_ref()
            .and_then(|row| row.get("heartbeatAt").cloned());
        st.queries.push(ReadQuery {
            kind: "heartbeat",
            host: h,
            name: n.into(),
            projection: options.projection,
            limit: Some(1),
            filter,
            pipeline: vec![],
            returned: usize::from(latest.is_some()),
            returned_bytes: bytes,
        });
        Ok(summary)
    }
    async fn has_rows(&self, h: Host, n: &str) -> Result<bool, String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("probe:{}:{n}", host(h)))?;
        let options = task_store::nonempty_options();
        assert_eq!(options.limit, Some(1));
        assert_eq!(options.batch_size, Some(1));
        let row = st
            .collections
            .get(&(h, n.into()))
            .and_then(|rows| rows.first())
            .map(|row| project_row(row, options.projection.as_ref()));
        let bytes = row
            .as_ref()
            .map(|row| bson::to_vec(row).unwrap().len())
            .unwrap_or(0);
        st.queries.push(ReadQuery {
            kind: "probe",
            host: h,
            name: n.into(),
            projection: options.projection,
            limit: options.limit,
            filter: doc! {},
            pipeline: vec![],
            returned: usize::from(row.is_some()),
            returned_bytes: bytes,
        });
        Ok(row.is_some())
    }
    async fn rows(
        &self,
        h: Host,
        n: &str,
        projection: Option<Document>,
    ) -> Result<Vec<Document>, String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("rows:{}:{n}", host(h)))?;
        if projection.is_none() {
            if let Some(left) = &mut st.full_reads_left {
                if *left == 0 {
                    return Err("payload-read sentinel: unexpected full collection read".into());
                }
                *left -= 1;
            }
        } else if st.full_reads_left.is_some() {
            for forbidden in [
                "payload",
                "regular",
                "settings",
                "result",
                "resultMetrics",
                "code",
                "expression",
                "super",
            ] {
                if projection.as_ref().unwrap().get(forbidden) == Some(&Bson::Int32(1)) {
                    return Err(format!("payload-read sentinel: projected {forbidden}"));
                }
            }
        }
        let result: Vec<Document> = st
            .collections
            .get(&(h, n.into()))
            .into_iter()
            .flatten()
            .map(|row| project_row(row, projection.as_ref()))
            .collect();
        let bytes = result
            .iter()
            .map(|row| bson::to_vec(row).unwrap().len())
            .sum();
        st.queries.push(ReadQuery {
            kind: "rows",
            host: h,
            name: n.into(),
            projection,
            limit: None,
            filter: doc! {},
            pipeline: vec![],
            returned: result.len(),
            returned_bytes: bytes,
        });
        Ok(result)
    }
    async fn insert_rows(&self, h: Host, n: &str, rows: Vec<Document>) -> Result<(), String> {
        let mut st = self.0.lock().unwrap();
        st.insert_calls += 1;
        let batch = st.insert_calls;
        call(&mut st, format!("insert:{}:{}", host(h), rows.len()))?;
        if st.fail_insert == Some(batch) {
            st.fail_insert = None;
            return Err("injected ordered insert failure".into());
        }
        let timeout_label = format!("insert:{}", host(h));
        let timed_out = st.timeout_after_write.as_deref() == Some(timeout_label.as_str());
        if timed_out {
            st.timeout_after_write = None;
        }
        let collection = st.collections.entry((h, n.into())).or_default();
        for row in rows {
            if collection.iter().any(|v| v.get("_id") == row.get("_id")) {
                return Err("duplicate key; existing item preserved".into());
            }
            collection.push(row);
        }
        if timed_out {
            return Err(format!(
                "{timeout_label}: timeout; result unconfirmed, not retried"
            ));
        }
        if st.extra_after_copy {
            st.extra_after_copy = false;
            st.collections
                .get_mut(&(h, n.into()))
                .unwrap()
                .push(doc! {"_id":"alien", "status":"pending"});
        }
        if st.change_source_after_copy {
            st.change_source_after_copy = false;
            st.collections.get_mut(&(Host::Local, n.into())).unwrap()[0]
                .insert("_id", "replacement");
        }
        if st.replace_remote_after_copy {
            st.replace_remote_after_copy = false;
            let mut t = registration("paused", true);
            t.insert("_id", ObjectId::parse_str(OTHER).unwrap());
            st.tasks.insert(Host::Remote, vec![t]);
        }
        Ok(())
    }
    async fn rename_collection(&self, h: Host, old: &str, new: &str) -> Result<(), String> {
        let mut st = self.0.lock().unwrap();
        call(&mut st, format!("rename:{}", host(h)))?;
        if st.rename_lie {
            return Ok(());
        }
        if st.collections.contains_key(&(h, new.into())) {
            return Err("archive target exists, cannot replace".into());
        }
        let rows = st
            .collections
            .remove(&(h, old.into()))
            .ok_or("source absent")?;
        st.collections.insert((h, new.into()), rows);
        let label = format!("rename:{}", host(h));
        if st.timeout_after_write.as_deref() == Some(label.as_str()) {
            st.timeout_after_write = None;
            return Err(format!("{label}: timeout; result unconfirmed, not retried"));
        }
        Ok(())
    }
    async fn ping(&self, h: Host) -> Result<(), String> {
        call(&mut self.0.lock().unwrap(), format!("ping:{}", host(h)))
    }
}
#[derive(Default)]
struct Runtime {
    calls: Mutex<Vec<String>>,
    fail_absence: Option<Host>,
    fail_archive: bool,
    archive_null: bool,
    // Mutations deliberately bypass the advisory lock to exercise stale CAS.
    mutate_on_archive: Option<MemoryStore>,
    block_report_write_on_archive: bool,
}
#[async_trait]
impl OperationsRuntime for Runtime {
    async fn assert_consumer_absent(
        &self,
        root: &Path,
        task: &Document,
        h: Host,
    ) -> Result<(), String> {
        assert_eq!(task.get_object_id("_id").unwrap().to_hex(), ID);
        assert!(
            mission_lock::MissionLocks::acquire(root, &[NAME], false).is_err(),
            "real mission lock must span runtime IO"
        );
        self.calls
            .lock()
            .unwrap()
            .push(format!("absence:{}", host(h)));
        if self.fail_absence == Some(h) {
            Err("exact consumer live or absent check failed".into())
        } else {
            Ok(())
        }
    }
    async fn archive_research(
        &self,
        root: &Path,
        _task: &Document,
        h: Host,
        n: &str,
    ) -> Result<Value, String> {
        assert!(mission_lock::MissionLocks::acquire(root, &[NAME], false).is_err());
        self.calls
            .lock()
            .unwrap()
            .push(format!("research:{}:{n}", host(h)));
        if self.fail_archive {
            return Err("original research archival failed".into());
        }
        if self.block_report_write_on_archive {
            std::fs::write(root.join("reports"), b"preserve this file").unwrap();
        }
        if let Some(store) = &self.mutate_on_archive {
            store.0.lock().unwrap().tasks.get_mut(&Host::Local).unwrap()[0]
                .insert("launchAttemptId", "concurrent-launch");
        }
        Ok(if self.archive_null {
            Value::Null
        } else {
            json!({"archivePath":"fixture-research.json","evidenceCollection":n,"count":7})
        })
    }
}
fn error(out: TaskOutcome, needle: &str) -> Value {
    assert_eq!(out.exit_code, 1, "{:?}", out.value);
    assert_eq!(out.value["ok"], false);
    assert!(
        out.value.to_string().contains(needle),
        "{} missing {needle}",
        out.value
    );
    out.value
}
async fn sync(s: &MemoryStore, r: &Runtime, root: &Root) -> TaskOutcome {
    cleanup_sync::sync(&root.0, s, r, NAME, Some(ID)).await
}
async fn delete(s: &MemoryStore, r: &Runtime, root: &Root) -> TaskOutcome {
    cleanup_sync::delete(&root.0, s, r, NAME, Some(ID)).await
}

#[tokio::test]
async fn sync_copies_all_rows_preserves_bson_and_uses_original_id_digest() {
    let root = Root::new();
    let original = rows(205);
    let store = MemoryStore::new("ready", false, original.clone());
    let runtime = Runtime::default();
    let out = sync(&store, &runtime, &root).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["schemaVersion"], 1);
    assert_eq!(out.value["command"], "sync");
    assert_eq!(out.value["local"]["total"], 205);
    assert_eq!(out.value["remote"]["total"], 205);
    assert_eq!(store.collection(Host::Remote, NAME).unwrap(), original);
    assert_eq!(store.collection(Host::Local, NAME).unwrap(), original);
    assert_eq!(
        store
            .calls()
            .iter()
            .filter(|x| x.starts_with("insert:"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["insert:remote:100", "insert:remote:100", "insert:remote:5"]
    );
    assert_eq!(store.control().get_bool("isRemote").unwrap(), true);
    assert_eq!(store.control().get_str("status").unwrap(), "ready");
    assert_eq!(
        store.control().get_str("syncedDigest").unwrap(),
        out.value["digest"].as_str().unwrap()
    );
    // SHA256 of original Python sorted string IDs joined by newline, no trailing newline.
    let one = MemoryStore::new("ready", false, vec![doc! {"_id":"abc","status":"pending"}]);
    let out = sync(&one, &Runtime::default(), &Root::new()).await;
    assert_eq!(
        out.value["digest"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[tokio::test]
async fn sync_rejects_preconditions_without_writing_or_overwriting() {
    for case in [
        "status",
        "side",
        "target",
        "consumer-local",
        "consumer-remote",
        "missing",
        "empty",
        "ping",
        "remote-id",
        "remote-running",
        "remote-nonempty",
        "observation",
        "wrong-id",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("ready", false, rows(2));
        let mut runtime = Runtime::default();
        {
            let mut st = store.0.lock().unwrap();
            match case {
                "status" => {
                    st.tasks.get_mut(&Host::Local).unwrap()[0].insert("status", "syncing");
                }
                "side" => {
                    st.tasks.get_mut(&Host::Local).unwrap()[0].insert("isRemote", true);
                }
                "target" => {
                    st.tasks.get_mut(&Host::Local).unwrap()[0].insert("targetRemote", false);
                }
                "consumer-local" => runtime.fail_absence = Some(Host::Local),
                "consumer-remote" => runtime.fail_absence = Some(Host::Remote),
                "missing" => {
                    st.collections.clear();
                }
                "empty" => {
                    st.collections.insert((Host::Local, NAME.into()), vec![]);
                }
                "ping" => st.fail = Some("ping:remote".into()),
                "remote-id" => {
                    let mut t = registration("paused", true);
                    t.insert("_id", ObjectId::parse_str(OTHER).unwrap());
                    st.tasks.insert(Host::Remote, vec![t]);
                }
                "remote-running" => {
                    st.tasks
                        .insert(Host::Remote, vec![registration("running", true)]);
                }
                "remote-nonempty" => {
                    st.collections
                        .insert((Host::Remote, NAME.into()), terminal());
                }
                "observation" => {
                    st.tasks.get_mut(&Host::Local).unwrap()[0]
                        .insert("acceptedObservation", doc! {"finishedAt":Bson::Null});
                }
                _ => (),
            }
        }
        let out = cleanup_sync::sync(
            &root.0,
            &store,
            &runtime,
            NAME,
            Some(if case == "wrong-id" { OTHER } else { ID }),
        )
        .await;
        assert_eq!(out.exit_code, 1, "case={case}: {}", out.value);
        assert!(store.0.lock().unwrap().writes.is_empty(), "case={case}");
        assert!(
            !store.calls().iter().any(|x| x.starts_with("insert:")),
            "case={case}"
        );
        if case == "remote-nonempty" {
            assert_eq!(store.collection(Host::Remote, NAME).unwrap(), terminal());
        }
    }
}

#[tokio::test]
async fn sync_entire_source_matches_original_python_checked_policy() {
    let cases: Vec<Value> = serde_json::from_str(CHECKED_ORACLE).unwrap();
    for case in cases.into_iter().filter(|v| v["command"] == "sync") {
        let row = Bson::try_from(case["row"].clone())
            .unwrap()
            .as_document()
            .unwrap()
            .clone();
        let root = Root::new();
        let original = if "sync" == "sync" {
            let mut source = rows(205);
            source[204] = row.clone();
            source
        } else {
            rows(2)
        };
        let store = MemoryStore::new(
            if "sync" == "sync" { "ready" } else { "paused" },
            "sync" == "delete",
            original.clone(),
        );
        if "sync" == "delete" {
            store.remote(vec![row.clone()]);
        }
        let result = if "sync" == "sync" {
            sync(&store, &Runtime::default(), &root).await
        } else {
            delete(&store, &Runtime::default(), &root).await
        };
        let blocked = case["blocked"].as_bool().unwrap();
        assert_eq!(
            result.exit_code,
            if blocked { 1 } else { 0 },
            "original Python case {}: {}",
            case["label"],
            result.value
        );
        if blocked {
            assert!(
                store.0.lock().unwrap().writes.is_empty(),
                "{}",
                case["label"]
            );
            assert!(!store
                .calls()
                .iter()
                .any(|v| v.starts_with("insert:") || v.starts_with("rename:")));
            assert_eq!(store.collection(Host::Local, NAME).unwrap(), original);
            if "sync" == "delete" {
                assert_eq!(store.collection(Host::Remote, NAME).unwrap(), vec![row]);
            }
            if let Some(error) = case["python_error"].as_str() {
                assert!(
                    result.value["error"].as_str().unwrap().contains(error),
                    "{}",
                    result.value
                );
            }
        } else if "sync" == "sync" {
            assert_eq!(store.collection(Host::Remote, NAME).unwrap(), original);
        } else {
            assert_eq!(
                store.collection(Host::Remote, &archive()).unwrap(),
                vec![row]
            );
        }
    }
}

#[tokio::test]
async fn sync_partial_insert_is_retained_and_cannot_be_retried_as_empty_destination() {
    let root = Root::new();
    let store = MemoryStore::new("ready", false, rows(205));
    store.0.lock().unwrap().fail_insert = Some(2);
    error(
        sync(&store, &Runtime::default(), &root).await,
        "ordered insert failure",
    );
    assert_eq!(store.collection(Host::Remote, NAME).unwrap().len(), 100);
    assert_eq!(store.control().get_str("status").unwrap(), "syncing");
    error(
        sync(&store, &Runtime::default(), &root).await,
        "ready/paused",
    );
    assert_eq!(store.collection(Host::Remote, NAME).unwrap().len(), 100);
    // Even an old client resetting the control state cannot authorize overwrite.
    store.0.lock().unwrap().tasks.get_mut(&Host::Local).unwrap()[0].insert("status", "ready");
    error(
        sync(&store, &Runtime::default(), &root).await,
        "destination is not empty",
    );
    assert_eq!(store.collection(Host::Remote, NAME).unwrap().len(), 100);
}

#[tokio::test]
async fn sync_digest_and_identity_races_never_publish_ready_remote() {
    for race in ["extra", "source", "remote-id", "claim-cas", "final-cas"] {
        let store = MemoryStore::new("ready", false, rows(2));
        {
            let mut st = store.0.lock().unwrap();
            match race {
                "extra" => st.extra_after_copy = true,
                "source" => st.change_source_after_copy = true,
                "remote-id" => st.replace_remote_after_copy = true,
                "claim-cas" => st.cas_fail = Some("update:local:sync-begin".into()),
                _ => st.cas_fail = Some("update:local:sync".into()),
            }
        }
        let out = sync(&store, &Runtime::default(), &Root::new()).await;
        assert_eq!(out.exit_code, 1, "{race}");
        assert_eq!(store.control().get_bool("isRemote").unwrap(), false);
        assert!(!store.calls().iter().any(|x| x.starts_with("rename:")));
    }
}

#[tokio::test]
async fn delete_renames_remote_then_local_preserves_evidence_and_cas_receipt() {
    let root = Root::new();
    let store = MemoryStore::new("paused", true, rows(2));
    store.remote(terminal());
    let runtime = Runtime::default();
    let out = delete(&store, &runtime, &root).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["itemsPreserved"], true);
    assert_eq!(out.value["canonicalResultsDeleted"], false);
    assert_eq!(out.value["localDeleted"], true);
    assert_eq!(out.value["remoteDeleted"], true);
    assert_eq!(out.value["controlHidden"], true);
    assert!(store.collection(Host::Remote, NAME).is_none());
    assert!(store.collection(Host::Local, NAME).is_none());
    assert_eq!(
        store.collection(Host::Remote, &archive()).unwrap(),
        terminal()
    );
    assert_eq!(store.collection(Host::Local, &archive()).unwrap(), rows(2));
    assert_eq!(store.control().get_str("status").unwrap(), "deactive");
    let receipt = root.receipt();
    assert_eq!(receipt["ok"], true);
    assert_eq!(receipt["taskId"], ID);
    assert_eq!(receipt["taskConfig"]["mission_config"]["keep"], 7);
    assert_eq!(
        store
            .calls()
            .iter()
            .filter(|x| x.starts_with("rename:"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["rename:remote", "rename:local"]
    );
    let st = store.0.lock().unwrap();
    let (_, filter, _) = st.writes.last().unwrap();
    for field in [
        "_id",
        "name",
        "status",
        "isRemote",
        "updatedAt",
        "tmuxContext",
        "launchAttemptId",
        "acceptedObservation",
        "stopConfirmedAt",
    ] {
        assert!(filter.contains_key(field), "{field}");
    }
    assert_eq!(
        filter.get_document("acceptedObservation").unwrap(),
        &doc! {"$exists":false}
    );
}

#[tokio::test]
async fn local_delete_avoids_remote_and_idempotent_retry_reads_archive_not_empty_namespace() {
    let root = Root::new();
    let store = MemoryStore::new("paused", false, terminal());
    let first = delete(&store, &Runtime::default(), &root).await;
    assert_eq!(first.exit_code, 0);
    assert!(first.value["remoteDeleted"].is_null());
    assert!(!store.calls().iter().any(|x| x.contains(":remote")));
    let runtime = Runtime {
        archive_null: true,
        ..Default::default()
    };
    let retry = delete(&store, &runtime, &root).await;
    assert_eq!(retry.exit_code, 0, "{}", retry.value);
    assert!(runtime
        .calls
        .lock()
        .unwrap()
        .contains(&format!("research:local:{}", archive())));
    assert_eq!(
        root.receipt()["researchArchive"]["count"],
        7,
        "a null retry must retain previous research stats"
    );
    assert_eq!(
        store.collection(Host::Local, &archive()).unwrap(),
        terminal()
    );
}

#[tokio::test]
async fn partial_remote_success_local_failure_has_receipt_and_safe_retry() {
    let root = Root::new();
    let store = MemoryStore::new("paused", true, rows(2));
    store.remote(terminal());
    store.fail("rename:local");
    let out = error(
        delete(&store, &Runtime::default(), &root).await,
        "rename:local",
    );
    assert_eq!(out["remoteDeleted"], true);
    assert_eq!(out["localDeleted"], false);
    assert_eq!(out["controlHidden"], false);
    assert_eq!(root.receipt()["remoteArchive"], archive());
    assert_eq!(store.control().get_str("status").unwrap(), "deleting");
    assert_eq!(
        store.collection(Host::Remote, &archive()).unwrap(),
        terminal()
    );
    let runtime = Runtime::default();
    let retry = delete(&store, &runtime, &root).await;
    assert_eq!(retry.exit_code, 0, "{}", retry.value);
    assert!(runtime
        .calls
        .lock()
        .unwrap()
        .contains(&format!("research:remote:{}", archive())));
    assert_eq!(
        store.collection(Host::Remote, &archive()).unwrap(),
        terminal()
    );
}

#[tokio::test]
async fn partial_control_failures_do_not_report_hidden_and_retain_archives() {
    for failure in ["update:remote:remote-hide", "update:local:delete"] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        store.0.lock().unwrap().cas_fail = Some(failure.into());
        let out = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(out.exit_code, 1);
        assert_eq!(out.value["controlHidden"], false);
        assert_eq!(root.receipt()["ok"], false);
        assert_eq!(root.receipt()["itemsPreserved"], true);
        assert!(store.collection(Host::Remote, &archive()).is_some());
        assert_ne!(store.control().get_str("status").unwrap(), "deactive");
        let retry = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(retry.exit_code, 0, "{}", retry.value);
    }
}

#[tokio::test]
async fn historical_delete_predicate_is_unchanged_but_new_archive_retains_every_oracle_row() {
    let cases: Vec<Value> = serde_json::from_str(CHECKED_ORACLE).unwrap();
    for case in cases.into_iter().filter(|v| v["command"] == "delete") {
        let row = Bson::try_from(case["row"].clone()).unwrap().as_document().unwrap().clone();
        // HISTORICAL oracle remains authoritative for the pure predicate, not
        // the user-changed archive eligibility contract. Do not rewrite its data.
        let assessment = quantnight_task_core::lifecycle_policy::checked_unresolved_execution_row(&row);
        assert_eq!(assessment.clone().unwrap_or(true), case["blocked"].as_bool().unwrap(), "{}", case["label"]);
        if let Some(error) = case["python_error"].as_str() {
            assert!(assessment.as_ref().unwrap_err().contains(error), "{}", case["label"]);
        }
        let root = Root::new();
        let original = rows(2);
        let store = MemoryStore::new("paused", true, original.clone());
        store.remote(vec![row.clone()]);
        let result = delete(&store, &Runtime::default(), &root).await;
        // NEW archive expectation: neither unresolved nor opaque malformed
        // receipt metadata blocks a stop-confirmed, identity-bound rename.
        assert_eq!(result.exit_code, 0, "new archive case {}: {}", case["label"], result.value);
        assert_eq!(result.value["unresolvedItemCount"], u64::from(assessment.clone().unwrap_or(true)));
        assert_eq!(result.value["receiptAssessmentUnknownCount"], u64::from(assessment.is_err()));
        assert_eq!(store.collection(Host::Local, &archive()).unwrap(), original);
        assert_eq!(bson::to_vec(&store.collection(Host::Remote, &archive()).unwrap()[0]).unwrap(), bson::to_vec(&row).unwrap());
    }
}

#[tokio::test]
async fn delete_rejects_stop_identity_observer_host_consumer_and_research_failures() {
    for case in [
        "stop-missing",
        "stop-host",
        "stop-id",
        "stop-time",
        "running",
        "generating",
        "syncing",
        "observation",
        "wrong-id",
        "remote-id",
        "consumer",
        "ping",
        "archive",
        "cas-claim",
        "changed-launch",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        let mut runtime = Runtime::default();
        {
            let mut st = store.0.lock().unwrap();
            let t = &mut st.tasks.get_mut(&Host::Local).unwrap()[0];
            match case {
                "stop-missing" => {
                    t.remove("stopConfirmedAt");
                }
                "stop-host" => {
                    t.insert("stopConfirmedRemote", false);
                }
                "stop-id" => {
                    t.insert("stopConfirmedTaskId", OTHER);
                }
                "stop-time" => {
                    t.insert("launchAttemptAt", DateTime::from_millis(2000));
                }
                "running" | "generating" | "syncing" => {
                    t.insert("status", case);
                }
                "observation" => {
                    t.insert("acceptedObservation", doc! {"finishedAt":Bson::Null});
                }
                "remote-id" => {
                    st.tasks.get_mut(&Host::Remote).unwrap()[0]
                        .insert("_id", ObjectId::parse_str(OTHER).unwrap());
                }
                "consumer" => runtime.fail_absence = Some(Host::Remote),
                "ping" => st.fail = Some("ping:remote".into()),
                "archive" => runtime.fail_archive = true,
                "cas-claim" => st.cas_fail = Some("update:local:delete-begin".into()),
                "changed-launch" => runtime.mutate_on_archive = Some(store.clone()),
                _ => (),
            }
        }
        let out = cleanup_sync::delete(
            &root.0,
            &store,
            &runtime,
            NAME,
            Some(if case == "wrong-id" { OTHER } else { ID }),
        )
        .await;
        assert_eq!(out.exit_code, 1, "{case}: {}", out.value);
        assert!(
            !store.calls().iter().any(|x| x.starts_with("rename:")),
            "{case}"
        );
        assert_eq!(store.collection(Host::Remote, NAME).unwrap(), terminal());
        assert_ne!(store.control().get_str("status").unwrap(), "deactive");
    }
}

#[tokio::test]
async fn archive_collision_or_unverified_rename_cannot_hide_control() {
    for mode in ["collision", "lie", "remote-failure"] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        {
            let mut st = store.0.lock().unwrap();
            match mode {
                "collision" => {
                    st.collections
                        .insert((Host::Remote, archive()), vec![doc! {"_id":"old-evidence"}]);
                }
                "lie" => st.rename_lie = true,
                _ => st.fail = Some("rename:remote".into()),
            }
        }
        let out = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(out.exit_code, 1);
        assert_eq!(out.value["controlHidden"], false);
        assert_eq!(store.collection(Host::Remote, NAME).unwrap(), terminal());
        assert_eq!(store.collection(Host::Local, NAME).unwrap(), rows(2));
    }
}

#[tokio::test]
async fn real_flock_and_expected_ids_gate_before_io_and_missing_task_is_not_false_confirmation() {
    let root = Root::new();
    let store = MemoryStore::new("ready", false, rows(2));
    let held = mission_lock::MissionLocks::acquire(&root.0, &[NAME], false).unwrap();
    error(
        sync(&store, &Runtime::default(), &root).await,
        "another lifecycle mutator",
    );
    error(
        delete(&store, &Runtime::default(), &root).await,
        "another lifecycle mutator",
    );
    assert!(store.calls().is_empty());
    drop(held);
    let empty = MemoryStore::default();
    let out = cleanup_sync::delete(&root.0, &empty, &Runtime::default(), NAME, None).await;
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.value["alreadyDeleted"], true);
    error(
        cleanup_sync::delete(&root.0, &empty, &Runtime::default(), NAME, Some(ID)).await,
        "expected task not found",
    );
    error(
        cleanup_sync::sync(&root.0, &store, &Runtime::default(), "../escape", None).await,
        "task name",
    );
}

#[tokio::test]
async fn active_replacement_beats_old_deactive_record_and_never_inherits_old_expected_id() {
    let root = Root::new();
    let store = MemoryStore::new("deactive", false, terminal());
    {
        let mut st = store.0.lock().unwrap();
        let mut t = registration("paused", false);
        t.insert("_id", ObjectId::parse_str(OTHER).unwrap());
        st.tasks.get_mut(&Host::Local).unwrap().push(t);
    }
    error(
        delete(&store, &Runtime::default(), &root).await,
        "task identity changed",
    );
    assert!(store.0.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn report_write_failure_is_failure_even_after_verified_archive_and_bad_old_receipt_is_retained(
) {
    let root = Root::new();
    let store = MemoryStore::new("paused", false, terminal());
    // Runtime IO changes the directory after the previous-receipt read, so
    // this actually fails persistence AFTER rename/control-CAS, not preflight.
    let runtime = Runtime {
        block_report_write_on_archive: true,
        ..Default::default()
    };
    let out = delete(&store, &runtime, &root).await;
    assert_eq!(out.exit_code, 1);
    assert!(out.value.to_string().contains("receipt"));
    assert_eq!(out.value["controlHidden"], true);
    assert_eq!(store.control().get_str("status").unwrap(), "deactive");
    assert_eq!(
        store.collection(Host::Local, &archive()).unwrap(),
        terminal()
    );
    assert_eq!(
        std::fs::read(root.0.join("reports")).unwrap(),
        b"preserve this file"
    );
    let root = Root::new();
    std::fs::create_dir_all(root.0.join("reports/missions")).unwrap();
    let p = root.0.join("reports/missions/fixture.cleanup.json");
    std::fs::write(&p, b"{broken evidence").unwrap();
    let store = MemoryStore::new("paused", false, terminal());
    error(
        delete(&store, &Runtime::default(), &root).await,
        "receipt invalid",
    );
    assert!(store.0.lock().unwrap().writes.is_empty());
    assert_eq!(std::fs::read(&p).unwrap(), b"{broken evidence");
}

#[tokio::test]
async fn claim_reread_cannot_adopt_a_changed_execution_host() {
    for command in ["sync", "delete"] {
        let root = Root::new();
        let store = MemoryStore::new(
            if command == "sync" { "ready" } else { "paused" },
            false,
            rows(2),
        );
        store.0.lock().unwrap().corrupt_claim = true;
        let out = if command == "sync" {
            sync(&store, &Runtime::default(), &root).await
        } else {
            delete(&store, &Runtime::default(), &root).await
        };
        error(out, "claim binding changed");
        assert!(!store
            .calls()
            .iter()
            .any(|x| x.starts_with("insert:") || x.starts_with("rename:")));
    }
}

#[tokio::test]
async fn retry_classifies_unknown_archived_evidence_without_clearing_research_summary() {
    let root = Root::new();
    let store = MemoryStore::new("paused", true, rows(2));
    store.remote(terminal());
    store.fail("rename:local");
    error(delete(&store, &Runtime::default(), &root).await, "rename:local");
    let previous = root.receipt()["researchArchive"].clone();
    store.0.lock().unwrap().collections.get_mut(&(Host::Remote, archive())).unwrap()[0]
        .insert("acceptance", "UNKNOWN");
    let retained = store.collection(Host::Remote, &archive()).unwrap();
    let runtime = Runtime { archive_null: true, ..Default::default() };
    // Historical UNKNOWN rejection is intentionally replaced by archive-only
    // success. No clearing, reconciliation or replay of that receipt occurs.
    let out = delete(&store, &runtime, &root).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["unresolvedItemCount"], 1);
    assert_eq!(root.receipt()["researchArchive"], previous);
    assert!(runtime.calls.lock().unwrap().contains(&format!("research:remote:{}", archive())));
    assert_eq!(store.collection(Host::Local, &archive()).unwrap(), rows(2));
    assert_eq!(store.collection(Host::Remote, &archive()).unwrap(), retained);
    assert_eq!(store.control().get_str("status").unwrap(), "deactive");
}

#[tokio::test]
async fn durable_not_posted_and_explicit_platform_terminal_remain_eligible() {
    for evidence in [
        doc! {"_id":"never-posted","status":"processing","_not_posted_":true,"acceptance":"NOT_POSTED"},
        doc! {"_id":"platform-error","status":"error","acceptance":"ACCEPTED","terminalStatus":"ERROR"},
        doc! {"_id":"legacy-no-receipt","status":"timeout_error"},
    ] {
        let root = Root::new();
        let store = MemoryStore::new("paused", false, vec![evidence.clone()]);
        let out = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(out.exit_code, 0, "{}", out.value);
        assert_eq!(
            store.collection(Host::Local, &archive()).unwrap(),
            vec![evidence]
        );
    }
}

#[tokio::test]
async fn no_query_error_is_interpreted_as_empty_collection_or_missing_remote_registration() {
    for point in [
        "find:remote",
        "rows:remote:fixture",
        "names:remote",
        "names:local",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        store.fail(point);
        let out = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(out.exit_code, 1, "{point}: {}", out.value);
        assert!(store.0.lock().unwrap().writes.is_empty());
        assert_eq!(store.collection(Host::Remote, NAME).unwrap(), terminal());
    }
}

// Query-shape/sentinel tests are inserted into the worker-owned integration test.
fn large_payload_rows(n: usize, completed: bool) -> Vec<Document> {
    let blob = "PAYLOAD_MUST_NOT_BE_READ_FOR_METADATA".repeat(2048);
    rows(n)
        .into_iter()
        .enumerate()
        .map(|(i, mut row)| {
            row.insert("regular", blob.clone());
            row.insert("code", blob.clone());
            row.insert("result", doc! {"large":&blob});
            row.insert("settings", doc! {"large":&blob});
            row.insert("heartbeatAt", DateTime::from_millis(1000 + i as i64));
            if completed {
                row.insert("status", "success");
                row.insert("alphaId", format!("alpha-{i}"));
                row.insert("acceptance", "ACCEPTED");
            }
            row
        })
        .collect()
}

#[test]
fn exact_original_receipt_projection_and_fake_projection_are_not_noops() {
    // Data contract copied from original taskctl RECEIPT_PROJECTION, not policy logic.
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
    .map(|k| (k.to_string(), Bson::Int32(1)))
    .collect();
    assert_eq!(task_store::receipt_projection(), expected);
    let row = doc! {"_id":"id","status":"success","heartbeatAt":DateTime::from_millis(1000),"alphaId":"a","regular":"sentinel-code","result":{"sentinel":true},"rejection_evidence_received":true};
    assert_eq!(
        project_row(&row, Some(&task_store::id_projection())),
        doc! {"_id":"id"}
    );
    let projected = project_row(&row, Some(&expected));
    assert!(projected.contains_key("rejection_evidence_received"));
    assert!(
        !projected.contains_key("regular")
            && !projected.contains_key("result")
            && !projected.contains_key("heartbeatAt")
    );
    assert_eq!(
        project_row(&row, Some(&doc! {"heartbeatAt":1,"_id":0})),
        doc! {"heartbeatAt":DateTime::from_millis(1000)}
    );
    assert_eq!(project_row(&row, None), row);
}

#[tokio::test]
async fn delete_large_payload_sentinel_allows_only_summary_and_exact_receipt_fields() {
    let root = Root::new();
    let local = large_payload_rows(8, false);
    let execution = large_payload_rows(8, true);
    let full_bytes: usize = execution
        .iter()
        .map(|r| bson::to_vec(r).unwrap().len())
        .sum();
    assert!(full_bytes > 1_000_000);
    let store = MemoryStore::new("paused", true, local.clone());
    store.remote(execution.clone());
    store.0.lock().unwrap().full_reads_left = Some(0);
    let result = delete(&store, &Runtime::default(), &root).await;
    assert_eq!(result.exit_code, 0, "{}", result.value);
    let queries = store.0.lock().unwrap().queries.clone();
    let rows: Vec<_> = queries.iter().filter(|q| q.kind == "rows").collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].host, Host::Remote);
    assert_eq!(rows[0].name, NAME);
    assert_eq!(rows[0].projection, Some(task_store::receipt_projection()));
    assert_eq!(rows[0].limit, None);
    assert_eq!(rows[0].returned, 8);
    assert!(queries
        .iter()
        .any(|q| q.kind == "aggregate" && q.pipeline == task_store::summary_pipeline()));
    let heartbeat = queries.iter().find(|q| q.kind == "heartbeat").unwrap();
    assert_eq!(heartbeat.projection, Some(doc! {"heartbeatAt":1,"_id":0}));
    assert_eq!(heartbeat.filter, doc! {"heartbeatAt":{"$exists":true}});
    assert_eq!(heartbeat.limit, Some(1));
    assert!(queries.iter().map(|q| q.returned_bytes).sum::<usize>() < 10_000);
    assert_eq!(root.receipt()["localBefore"]["total"], 8);
    assert!(!root.receipt()["localBefore"]["latestHeartbeatAt"].is_null());
    assert_eq!(store.collection(Host::Local, &archive()).unwrap(), local);
    assert_eq!(
        store.collection(Host::Remote, &archive()).unwrap(),
        execution
    );
}

#[tokio::test]
async fn sync_large_payload_sentinel_reads_source_once_and_checks_only_ids_and_counts() {
    let root = Root::new();
    let original = large_payload_rows(8, false);
    let store = MemoryStore::new("ready", false, original.clone());
    store.0.lock().unwrap().full_reads_left = Some(1);
    let result = sync(&store, &Runtime::default(), &root).await;
    assert_eq!(result.exit_code, 0, "{}", result.value);
    let st = store.0.lock().unwrap();
    assert_eq!(st.full_reads_left, Some(0));
    let full: Vec<_> = st
        .queries
        .iter()
        .filter(|q| q.kind == "rows" && q.projection.is_none())
        .collect();
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].host, Host::Local);
    assert_eq!(full[0].limit, None);
    assert_eq!(full[0].returned, 8);
    assert!(full[0].returned_bytes > 1_000_000);
    let ids: Vec<_> = st
        .queries
        .iter()
        .filter(|q| q.kind == "rows" && q.projection.is_some())
        .collect();
    assert_eq!(ids.len(), 2);
    for q in ids {
        assert_eq!(q.projection, Some(doc! {"_id":1}));
        assert_eq!(q.limit, None);
        assert_eq!(q.returned, 8);
        assert!(q.returned_bytes < 1024);
    }
    let probe = st.queries.iter().find(|q| q.kind == "probe").unwrap();
    assert_eq!(probe.host, Host::Remote);
    assert_eq!(probe.limit, Some(1));
    assert_eq!(probe.projection, Some(doc! {"_id":1}));
    assert_eq!(probe.returned, 0);
    assert_eq!(
        st.queries.iter().filter(|q| q.kind == "aggregate").count(),
        2
    );
    drop(st);
    assert_eq!(store.collection(Host::Remote, NAME).unwrap(), original);
}

#[tokio::test]
async fn nonempty_destination_uses_limit_one_and_never_fetches_either_payload_set() {
    let root = Root::new();
    let store = MemoryStore::new("ready", false, large_payload_rows(8, false));
    let execution = large_payload_rows(12, true);
    store.remote(execution.clone());
    store.0.lock().unwrap().full_reads_left = Some(0);
    error(
        sync(&store, &Runtime::default(), &root).await,
        "destination is not empty",
    );
    let st = store.0.lock().unwrap();
    assert!(st.writes.is_empty());
    assert!(!st.queries.iter().any(|q| q.kind == "rows"));
    let probe = st.queries.iter().find(|q| q.kind == "probe").unwrap();
    assert_eq!(probe.limit, Some(1));
    assert_eq!(probe.returned, 1);
    assert!(probe.returned_bytes < 128);
    assert_eq!(probe.projection, Some(doc! {"_id":1}));
    drop(st);
    assert_eq!(store.collection(Host::Remote, NAME).unwrap(), execution);
}

#[tokio::test]
async fn summary_probe_and_cursor_errors_cannot_be_successful_empty_results() {
    for point in [
        "summary:local:fixture",
        "aggregate:local:fixture",
        "heartbeat:local:fixture",
        "probe:remote:fixture",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("ready", false, rows(2));
        store.fail(point);
        error(sync(&store, &Runtime::default(), &root).await, point);
        assert!(store.0.lock().unwrap().writes.is_empty());
        assert!(store.collection(Host::Remote, NAME).is_none());
    }
    for point in [
        "aggregate:remote:fixture",
        "heartbeat:remote:fixture",
        "rows:remote:fixture",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("ready", false, rows(2));
        store.fail(point);
        error(sync(&store, &Runtime::default(), &root).await, point);
        assert_eq!(store.control().get_str("status").unwrap(), "syncing");
        assert_eq!(store.collection(Host::Remote, NAME).unwrap(), rows(2));
    }
    for point in [
        "summary:local:fixture",
        "aggregate:local:fixture",
        "heartbeat:local:fixture",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        store.fail(point);
        error(delete(&store, &Runtime::default(), &root).await, point);
        assert!(store.0.lock().unwrap().writes.is_empty());
        assert_eq!(root.receipt()["controlHidden"], false);
    }
}

#[tokio::test]
async fn mutation_timeout_after_apply_keeps_partial_evidence_and_never_resubmits() {
    for point in ["update:local:sync-begin", "insert:remote"] {
        let root = Root::new();
        let store = MemoryStore::new("ready", false, rows(2));
        store.0.lock().unwrap().timeout_after_write = Some(point.into());
        error(
            sync(&store, &Runtime::default(), &root).await,
            "timeout; result unconfirmed",
        );
        assert_eq!(store.control().get_str("status").unwrap(), "syncing");
        let label = if point == "insert:remote" {
            "insert:remote:2"
        } else {
            point
        };
        assert_eq!(
            store.calls().iter().filter(|c| c.as_str() == label).count(),
            1
        );
        if point == "insert:remote" {
            assert_eq!(store.collection(Host::Remote, NAME).unwrap(), rows(2));
        }
    }
    for point in [
        "rename:remote",
        "update:remote:remote-hide",
        "update:local:delete",
    ] {
        let root = Root::new();
        let store = MemoryStore::new("paused", true, rows(2));
        store.remote(terminal());
        store.0.lock().unwrap().timeout_after_write = Some(point.into());
        let out = error(
            delete(&store, &Runtime::default(), &root).await,
            "timeout; result unconfirmed",
        );
        assert_eq!(out["controlHidden"], false);
        assert_eq!(root.receipt()["ok"], false);
        assert_eq!(
            store.calls().iter().filter(|c| c.as_str() == point).count(),
            1
        );
        assert_eq!(
            store.collection(Host::Remote, &archive()).unwrap(),
            terminal()
        );
        let retry = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(retry.exit_code, 0, "{}", retry.value);
        assert_eq!(
            store.collection(Host::Remote, &archive()).unwrap(),
            terminal()
        );
    }
}

// BEGIN ORIGINAL PYTHON CHECKED ORACLE
const CHECKED_ORACLE: &str = r###"[
  {
    "command": "sync",
    "label": "plain",
    "row": {
      "_id": "probe",
      "status": "pending"
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "status_success",
    "row": {
      "_id": "probe",
      "status": "success"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "status_null",
    "row": {
      "_id": "probe",
      "status": null
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "status_array",
    "row": {
      "_id": "probe",
      "status": []
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "status_object",
    "row": {
      "_id": "probe",
      "status": {}
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "error_class_object",
    "row": {
      "_id": "probe",
      "status": "pending",
      "errorClass": {}
    },
    "blocked": true,
    "python_error": "unhashable type: 'dict'",
    "python_error_type": "TypeError"
  },
  {
    "command": "sync",
    "label": "error_class_array",
    "row": {
      "_id": "probe",
      "status": "pending",
      "errorClass": []
    },
    "blocked": true,
    "python_error": "unhashable type: 'list'",
    "python_error_type": "TypeError"
  },
  {
    "command": "sync",
    "label": "acceptance_object",
    "row": {
      "_id": "probe",
      "status": "pending",
      "acceptance": {}
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "terminal_array",
    "row": {
      "_id": "probe",
      "status": "pending",
      "terminalStatus": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "attempts_object",
    "row": {
      "_id": "probe",
      "status": "pending",
      "submissionAttempts": {}
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "attempts_null_entry",
    "row": {
      "_id": "probe",
      "status": "pending",
      "submissionAttempts": [
        null
      ]
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "not_posted_int",
    "row": {
      "_id": "probe",
      "status": "pending",
      "_not_posted_": 1
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "post_attempted_int",
    "row": {
      "_id": "probe",
      "status": "pending",
      "campaign_post_attempted": 1
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "http_status_zero",
    "row": {
      "_id": "probe",
      "status": "pending",
      "httpStatus": 0
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "http_status_false",
    "row": {
      "_id": "probe",
      "status": "pending",
      "httpStatus": false
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "slot",
    "row": {
      "_id": "probe",
      "status": "pending",
      "superSlotId": "slot"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "headers_array",
    "row": {
      "_id": "probe",
      "status": "pending",
      "responseHeaders": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "progress",
    "row": {
      "_id": "probe",
      "status": "pending",
      "progressUrl": "synthetic-receipt"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "unknown_acceptance",
    "row": {
      "_id": "probe",
      "status": "pending",
      "acceptance": "UNKNOWN"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "unknown_terminal",
    "row": {
      "_id": "probe",
      "status": "pending",
      "terminalStatus": "UNKNOWN"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "timeout_accepted",
    "row": {
      "_id": "probe",
      "status": "timeout_error",
      "alphaId": null,
      "acceptance": "ACCEPTED"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "sync",
    "label": "processing_no_receipt",
    "row": {
      "_id": "probe",
      "status": "processing",
      "alphaId": null,
      "acceptance": null
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "plain",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED"
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "status_success",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED"
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "status_null",
    "row": {
      "_id": "probe",
      "status": null,
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "status_array",
    "row": {
      "_id": "probe",
      "status": [],
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED"
    },
    "blocked": true,
    "python_error": "unhashable type: 'list'",
    "python_error_type": "TypeError"
  },
  {
    "command": "delete",
    "label": "status_object",
    "row": {
      "_id": "probe",
      "status": {},
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED"
    },
    "blocked": true,
    "python_error": "unhashable type: 'dict'",
    "python_error_type": "TypeError"
  },
  {
    "command": "delete",
    "label": "error_class_object",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "errorClass": {}
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "error_class_array",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "errorClass": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "acceptance_object",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": {}
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "terminal_array",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "terminalStatus": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "attempts_object",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "submissionAttempts": {}
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "attempts_null_entry",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "submissionAttempts": [
        null
      ]
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "not_posted_int",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "_not_posted_": 1
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "post_attempted_int",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "campaign_post_attempted": 1
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "http_status_zero",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "httpStatus": 0
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "http_status_false",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "httpStatus": false
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "slot",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "superSlotId": "slot"
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "headers_array",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "responseHeaders": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "progress",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "progressUrl": "synthetic-receipt"
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "unknown_acceptance",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "UNKNOWN"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "unknown_terminal",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "acceptance": "ACCEPTED",
      "terminalStatus": "UNKNOWN"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "timeout_accepted",
    "row": {
      "_id": "probe",
      "status": "timeout_error",
      "alphaId": null,
      "acceptance": "ACCEPTED"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "processing_no_receipt",
    "row": {
      "_id": "probe",
      "status": "processing",
      "alphaId": null,
      "acceptance": null
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "unposted_skips_status",
    "row": {
      "_id": "probe",
      "_not_posted_": true,
      "acceptance": "NOT_POSTED",
      "status": []
    },
    "blocked": false,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "pending_bad_error_class",
    "row": {
      "_id": "probe",
      "status": "pending",
      "errorClass": {}
    },
    "blocked": true,
    "python_error": "unhashable type: 'dict'",
    "python_error_type": "TypeError"
  },
  {
    "command": "delete",
    "label": "pending_bad_terminal",
    "row": {
      "_id": "probe",
      "status": "error",
      "terminalStatus": []
    },
    "blocked": true,
    "python_error": "unhashable type: 'list'",
    "python_error_type": "TypeError"
  },
  {
    "command": "delete",
    "label": "unknown_skips_status",
    "row": {
      "_id": "probe",
      "status": [],
      "acceptance": "UNKNOWN"
    },
    "blocked": true,
    "python_error": null
  },
  {
    "command": "delete",
    "label": "success_skips_bad_error",
    "row": {
      "_id": "probe",
      "status": "success",
      "alphaId": "alpha-1",
      "errorClass": {}
    },
    "blocked": false,
    "python_error": null
  }
]"###;
// END ORIGINAL PYTHON CHECKED ORACLE

#[tokio::test]
async fn captured_execution_host_is_rechecked_inside_original_sync_and_cleanup_locks() {
    let root = Root::new();
    let runtime = Runtime::default();
    let local = MemoryStore::new("ready", false, rows(2));
    let before = local.control();
    let result = cleanup_sync::sync_with_expected_host(&root.0, &local, &runtime, NAME, Some(ID), Some(true)).await;
    assert_eq!(result.exit_code, 1);
    assert_eq!(local.control(), before);
    assert!(runtime.calls.lock().unwrap().is_empty());
    let remote = MemoryStore::new("paused", true, rows(2));
    let before = remote.control();
    let result = cleanup_sync::delete_with_expected_host(&root.0, &remote, &runtime, NAME, Some(ID), Some(false)).await;
    assert_eq!(result.exit_code, 1);
    assert_eq!(remote.control(), before);
    assert!(runtime.calls.lock().unwrap().is_empty());
    assert!(remote.calls().iter().all(|call| !call.starts_with("rename:") && !call.starts_with("update:")));
    assert!(!root.0.join("reports/missions/fixture.cleanup.json").exists());
}

// Synthetic receipt evidence only; no live task/DB/platform is consulted.
fn archive_evidence_rows() -> Vec<Document> {
    vec![
        doc! {"_id":"pending", "status":"pending", "_not_posted_":true},
        doc! {"_id":"accepted", "status":"error", "acceptance":"ACCEPTED", "httpStatus":201,
            "terminalStatus":Bson::Null, "alphaId":Bson::Null, "errorClass":"simulation_batch_identity_error",
            "submissionAttempts":[{"httpStatus":201, "progressUrl":"https://example.invalid/original-secret", "acceptance":"ACCEPTED"}],
            "childSimulationIds":["child-a", "child-b"], "superSlotId":"reserved-token",
            "reservedHold":{"state":"held", "token":"hold-token"}, "capacityReleased":false},
        doc! {"_id":"unknown", "status":"timeout_error", "acceptance":"UNKNOWN", "terminalStatus":Bson::Null,
            "submissionAttempts":[{"httpStatus":Bson::Null}], "superSlotId":"unknown-token"},
        doc! {"_id":"opaque", "status":"error", "errorClass":[], "submissionAttempts":["malformed-retained"]},
        terminal().remove(0),
    ]
}
fn row_bytes(rows: &[Document]) -> Vec<Vec<u8>> {
    rows.iter().map(|row| bson::to_vec(row).unwrap()).collect()
}
#[tokio::test]
async fn archive_retains_pending_accepted_unknown_opaque_and_holds_byte_for_byte_on_both_hosts() {
    for remote in [false, true] {
        let root = Root::new();
        let original = archive_evidence_rows();
        let store = MemoryStore::new("paused", remote, original.clone());
        if remote { store.remote(original.clone()); }
        let runtime = Runtime::default();
        let out = delete(&store, &runtime, &root).await;
        assert_eq!(out.exit_code, 0, "{}", out.value);
        for receipt in [&out.value, &root.receipt()] {
            assert_eq!(receipt["taskId"], ID);
            assert_eq!(receipt["remote"], remote);
            assert_eq!(receipt["unresolvedItemCount"], 3);
            assert_eq!(receipt["receiptAssessmentUnknownCount"], 1);
            assert_eq!(receipt["platformRequestsCancelled"], false);
            assert_eq!(receipt["capacityReleased"], false);
            assert_eq!(receipt["localArchive"], archive());
            assert_eq!(receipt["remoteArchive"], if remote { json!(archive()) } else { Value::Null });
            assert!(receipt["receiptPath"].as_str().unwrap().ends_with("fixture.cleanup.json"));
            assert!(!receipt.to_string().contains("original-secret"));
            assert!(!receipt.to_string().contains("submissionAttempts"));
        }
        for h in if remote { vec![Host::Local, Host::Remote] } else { vec![Host::Local] } {
            assert_eq!(row_bytes(&store.collection(h, &archive()).unwrap()), row_bytes(&original));
            assert!(store.collection(h, NAME).is_none());
        }
        assert_eq!(runtime.calls.lock().unwrap().as_slice(), [
            format!("absence:{}", if remote { "remote" } else { "local" }),
            format!("research:{}:{NAME}", if remote { "remote" } else { "local" }),
        ]);
        // Runtime has only absence/research IO; storage operations are strictly
        // control CAS + collection rename, never item reset/recovery or release.
        assert!(!store.calls().iter().any(|c| c.starts_with("insert:") || c.starts_with("reset:")));
        assert_eq!(store.control().get_str("status").unwrap(), "deactive");
        let retry = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(retry.exit_code, 0, "{}", retry.value);
        assert_eq!(retry.value["unresolvedItemCount"], 3);
        assert_eq!(retry.value["receiptAssessmentUnknownCount"], 1);
    }
}
#[tokio::test]
async fn partial_retry_retains_diagnostics_and_archive_identity_even_when_next_read_fails() {
    let root = Root::new();
    let original = archive_evidence_rows();
    let store = MemoryStore::new("paused", true, original.clone());
    store.remote(original.clone());
    store.fail("rename:local");
    let first = error(delete(&store, &Runtime::default(), &root).await, "rename:local");
    assert_eq!(first["unresolvedItemCount"], 3);
    store.fail("ping:remote");
    let failed = error(delete(&store, &Runtime::default(), &root).await, "ping:remote");
    for key in ["unresolvedItemCount", "receiptAssessmentUnknownCount", "remoteArchive", "taskId", "receiptPath"] {
        assert_eq!(failed[key], first[key], "{key}");
        assert_eq!(root.receipt()[key], first[key], "{key}");
    }
    assert_eq!(failed["controlHidden"], false);
    let retry = delete(&store, &Runtime::default(), &root).await;
    assert_eq!(retry.exit_code, 0, "{}", retry.value);
    assert_eq!(retry.value["unresolvedItemCount"], 3);
    for h in [Host::Local, Host::Remote] {
        assert_eq!(row_bytes(&store.collection(h, &archive()).unwrap()), row_bytes(&original));
    }
}

#[tokio::test]
async fn unresolved_rows_do_not_bypass_control_proof_or_source_archive_failures() {
    for case in ["generation_unknown", "launch_unknown", "stop-malformed", "host-malformed", "status-malformed", "remote-status-malformed", "source-dependency"] {
        let root = Root::new();
        let original = archive_evidence_rows();
        let store = MemoryStore::new("paused", true, original.clone());
        store.remote(original.clone());
        let mut runtime = Runtime::default();
        {
            let mut st = store.0.lock().unwrap();
            let task = &mut st.tasks.get_mut(&Host::Local).unwrap()[0];
            match case {
                "generation_unknown" | "launch_unknown" => { task.insert("status", case); }
                "stop-malformed" => { task.insert("stopConfirmedAt", "invalid-proof"); }
                "host-malformed" => { task.insert("isRemote", "true"); }
                "status-malformed" => { task.insert("status", doc! {"not":"a status"}); }
                "remote-status-malformed" => { st.tasks.get_mut(&Host::Remote).unwrap()[0].insert("status", Bson::Null); }
                "source-dependency" => runtime.fail_archive = true,
                _ => unreachable!(),
            }
        }
        let out = delete(&store, &runtime, &root).await;
        assert_eq!(out.exit_code, 1, "{case}: {}", out.value);
        assert!(!store.calls().iter().any(|c| c.starts_with("rename:")), "{case}");
        for h in [Host::Local, Host::Remote] {
            assert_eq!(row_bytes(&store.collection(h, NAME).unwrap()), row_bytes(&original));
        }
    }
}

#[tokio::test]
async fn diagnostic_count_is_execution_items_not_unique_batch_or_local_plus_remote_rows() {
    let root = Root::new();
    let mut executed = Vec::new();
    for i in 0..52 {
        let mut row = if i < 10 { archive_evidence_rows().remove(1) } else { terminal().remove(0) };
        row.insert("_id", format!("batch-item-{i}"));
        executed.push(row);
    }
    let store = MemoryStore::new("paused", true, rows(52));
    store.remote(executed.clone());
    let out = delete(&store, &Runtime::default(), &root).await;
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["unresolvedItemCount"], 10);
    assert_eq!(out.value["receiptAssessmentUnknownCount"], 0);
    assert_eq!(row_bytes(&store.collection(Host::Remote, &archive()).unwrap()), row_bytes(&executed));
}


#[tokio::test]
async fn archive_preconditions_report_the_original_action_without_attempting_archive() {
    for (status, action) in [
        ("running", "pause"), ("launch_unknown", "pause"),
        ("generating", "reconcile_generation_or_sync"),
        ("generation_unknown", "reconcile_generation_or_sync"),
        ("syncing", "reconcile_generation_or_sync"),
    ] {
        let root = Root::new();
        let store = MemoryStore::new(status, false, terminal());
        let out = delete(&store, &Runtime::default(), &root).await;
        assert_eq!(out.value["ok"], false);
        assert_eq!(out.value["requiredAction"], action);
        assert_eq!(out.value["localDeleted"], false);
        assert_eq!(out.value["controlHidden"], false);
    }
}

#[tokio::test]
async fn legacy_super_missing_enhancements_can_archive_unresolved_receipts_after_original_stop_checks() {
    for remote in [false, true] {
        let root = Root::new();
        let retained = vec![doc! {"_id":"accepted-unknown","status":"timeout_error","acceptance":"UNKNOWN",
            "campaign_post_attempted":true,"superSlotId":"retain-original-slot","submissionAttempts":[{"acceptance":"UNKNOWN"}],
            "payload":{"selection":"original"}},
            doc! {"_id":"success","status":"success","alphaId":"old-alpha"}];
        let store = MemoryStore::new("paused", remote, retained.clone());
        if remote { store.remote(retained.clone()); }
        { let mut d = store.0.lock().unwrap();
          for tasks in d.tasks.values_mut() {
              tasks[0].insert("taskType", "super");
              for key in ["startupBinding","simulatorRuntime","tmuxContext"] { tasks[0].remove(key); }
          }
        }
        let runtime = Runtime::default();
        let out = delete(&store, &runtime, &root).await;
        assert_eq!(out.exit_code, 0, "{}", out.value);
        assert_eq!(out.value["itemsPreserved"], true);
        assert_eq!(out.value["canonicalResultsDeleted"], false);
        assert_eq!(store.collection(Host::Local, &archive()).unwrap(), retained);
        if remote { assert_eq!(store.collection(Host::Remote, &archive()).unwrap(), retained); }
        assert_eq!(store.control().get_str("status").unwrap(), "deactive");
        assert!(runtime.calls.lock().unwrap().contains(&format!("absence:{}", if remote {"remote"} else {"local"})));
        assert!(!store.control().contains_key("simulatorRuntime"), "archive cannot invent runner closure or platform terminal");
    }
}

#[path = "support/original_professional_runtime.rs"]
mod original_professional_runtime;

#[tokio::test]
async fn public_archive_missing_stop_automatically_uses_original_verify_only_without_signal() {
    use quantnight_task_core::{backend_binding::validate_backend_binding,operations_runtime::OriginalOperationsRuntime};
    struct OriginalStopUnboundResearch<'a>(OriginalOperationsRuntime<'a>);
    #[async_trait]
    impl OperationsRuntime for OriginalStopUnboundResearch<'_> {
        async fn assert_consumer_absent(&self,root:&Path,task:&Document,host:Host)->Result<(),String> {
            self.0.assert_consumer_absent(root,task,host).await
        }
        async fn verify_stopped(&self,root:&Path,task:&Document,remaining:std::time::Duration)->Result<Option<quantnight_task_core::pause::StopOutput>,String> {
            self.0.verify_stopped(root,task,remaining).await
        }
        async fn confirm_stopped_scope(&self,r:&Path,s:&dyn TaskStore,t:&Document,d:std::time::Duration)->Result<(),String>{
            self.0.confirm_stopped_scope(r,s,t,d).await
        }
        async fn archive_research(&self,_:&Path,_:&Document,_:Host,_:&str)->Result<Value,String> {
            // Independent unbound research leaf; original stop/check shell IO is NOT mocked.
            Ok(Value::Null)
        }
    }
    for remote in [false,true] {
        let fake=original_professional_runtime::original_professional_fixture(false);
        let root=Path::new(&fake.config.env.working_dir);
        let binding=validate_backend_binding(&fake.config).await.unwrap();
        let runtime=OriginalStopUnboundResearch(OriginalOperationsRuntime{binding:&binding,observer:None});
        let retained=vec![doc!{"_id":"unknown","status":"timeout_error","acceptance":"UNKNOWN","superSlotId":"original-token"}];
        let store=MemoryStore::new("paused",remote,retained.clone());
        if remote {store.remote(retained.clone());}
        {let mut d=store.0.lock().unwrap();for tasks in d.tasks.values_mut(){
            for key in ["stopConfirmedAt","stopConfirmedRemote","stopConfirmedTaskId","startupBinding","simulatorRuntime","tmuxContext"] {tasks[0].remove(key);}
            tasks[0].insert("taskType","super");
            tasks[0].insert("tmuxContext",doc!{"sessionId":"$4"});
        }}
        let out=cleanup_sync::delete(root,&store,&runtime,NAME,Some(ID)).await;
        assert_eq!(out.exit_code,0,"{remote}: {}",out.value);
        assert_eq!(out.value["itemsPreserved"],true);
        assert_eq!(out.value["unresolvedItemCount"],1);
        assert_eq!(store.collection(Host::Local,&archive()).unwrap(),retained);
        if remote{assert_eq!(store.collection(Host::Remote,&archive()).unwrap(),retained);}
        let trace=std::fs::read_to_string(root.join("trace")).unwrap();
        assert!(!trace.contains("if-shell")&&!trace.contains("new-session"));
        let control=store.control();
        assert_eq!(control.get_str("stopConfirmationBasis").unwrap(),"session_absent");
        assert!(!control.contains_key("simulatorRuntime"));
    }
}
