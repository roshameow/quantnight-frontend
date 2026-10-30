//! Lower storage and external-process fixtures for the ACTUAL CLI dispatcher.
//! No Runtime/TaskControl/dispatcher implementation is replaced. Professional
//! Python and shell bodies ARE synthetic: coverage is U, not O/end-to-end OS.
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{
    backend_binding::validate_backend_binding,
    cleanup_sync::TaskOutcome,
    config::bash_test_support::FakeBash,
    task_control::TaskControl,
    task_store::{self, CanonicalNamespace, CollectionSummary, Host, TaskStore},
};
use quantnight_taskctl::{dispatch::dispatch, parse, to_request, Entry, Request};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

// PATH is needed only because the real GenerationRuntime invokes `uv`.
// Every test in this integration binary must use this guard; no global cwd.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

pub fn field<'a>(row: &'a Document, key: &str) -> Option<&'a Bson> {
    let (first, rest) = key.split_once('.').unwrap_or((key, ""));
    let value = row.get(first)?;
    if rest.is_empty() {
        Some(value)
    } else {
        field(value.as_document()?, rest)
    }
}
pub fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(key, expected)| {
        if key == "$and" || key == "$or" {
            let terms = expected.as_array().expect("logical query array");
            return if key == "$and" {
                terms.iter().all(|v| matches(row, v.as_document().unwrap()))
            } else {
                terms.iter().any(|v| matches(row, v.as_document().unwrap()))
            };
        }
        let actual = field(row, key);
        if let Bson::Document(ops) = expected {
            if ops.keys().any(|k| k.starts_with('$')) {
                return ops.iter().all(|(op, val)| match op.as_str() {
                    "$exists" => actual.is_some() == val.as_bool().unwrap(),
                    "$ne" => actual != Some(val),
                    "$eq" => actual == Some(val),
                    "$in" => actual.is_some_and(|v| val.as_array().unwrap().contains(v)),
                    "$nin" => actual.is_none_or(|v| !val.as_array().unwrap().contains(v)),
                    _ => panic!("unsupported storage matcher {op}"),
                });
            }
        }
        if expected == &Bson::Null {
            matches!(actual, None | Some(Bson::Null))
        } else {
            actual == Some(expected)
        }
    })
}
fn set(row: &mut Document, key: &str, value: Bson) {
    if let Some((first, rest)) = key.split_once('.') {
        if !row.contains_key(first) {
            row.insert(first, Document::new());
        }
        set(row.get_document_mut(first).unwrap(), rest, value);
    } else {
        row.insert(key, value);
    }
}
fn apply(row: &mut Document, update: &Document) {
    for (op, values) in update {
        for (key, value) in values.as_document().expect("update document") {
            match op.as_str() {
                "$set" => set(row, key, value.clone()),
                "$unset" => {
                    assert!(!key.contains('.'));
                    row.remove(key);
                }
                "$push" => {
                    if !row.contains_key(key) {
                        row.insert(key, Bson::Array(vec![]));
                    }
                    let Some(Bson::Array(items)) = row.get_mut(key) else {
                        panic!("push non-array")
                    };
                    items.push(value.clone());
                }
                _ => panic!("unsupported update operator {op}"),
            }
        }
    }
}
fn project(row: &Document, projection: Option<&Document>) -> Document {
    let Some(p) = projection else {
        return row.clone();
    };
    let include = p.values().any(|v| v == &Bson::Int32(1));
    let mut result = Document::new();
    for (key, value) in row {
        if (key == "_id" && p.get(key) != Some(&Bson::Int32(0)))
            || (key != "_id"
                && if include {
                    p.get(key) == Some(&Bson::Int32(1))
                } else {
                    p.get(key) != Some(&Bson::Int32(0))
                })
        {
            result.insert(key, value.clone());
        }
    }
    result
}
#[derive(Default)]
pub struct Data {
    pub read_queries: Vec<(Host, Document)>,
    pub after_summary: Option<(Host, Document)>,
    pub before_update: Option<(Host, Document)>,
    pub tasks: HashMap<Host, Vec<Document>>,
    pub collections: HashMap<(Host, String), Vec<Document>>,
    pub calls: Vec<String>,
    pub writes: Vec<(Host, Document, Document)>,
    pub item_writes: usize,
    pub fail: Option<String>,
    pub fail_after_update: bool,
    pub canonical: Vec<Document>,
}
pub struct Memory {
    pub data: Mutex<Data>,
    root: PathBuf,
}
impl Memory {
    // Only enabled by the original-owner fixture. This is the lowest fake
    // Mongo storage boundary shared with the real source-registration process.
    fn publish_snapshot(&self, data: &Data) {
        if self.root.join("original_shell_enabled").exists() {
            let rows: Vec<Value> = data
                .tasks
                .get(&Host::Local)
                .into_iter()
                .flatten()
                .map(|d| Bson::Document(d.clone()).into_relaxed_extjson())
                .collect();
            fs::write(
                self.root.join("storage_snapshot.json"),
                serde_json::to_vec(&rows).unwrap(),
            )
            .unwrap();
        }
    }
    pub fn publish(&self) {
        self.publish_snapshot(&self.data.lock().unwrap());
    }
    fn access(data: &mut Data, label: String) -> Result<(), String> {
        data.calls.push(label.clone());
        if data.fail.as_deref() == Some(label.as_str()) {
            data.fail = None;
            return Err(format!("injected storage failure: {label}"));
        }
        Ok(())
    }
    pub fn task(&self, name: &str) -> Document {
        self.data.lock().unwrap().tasks[&Host::Local]
            .iter()
            .find(|d| d.get_str("name") == Ok(name))
            .unwrap()
            .clone()
    }
    pub fn alter(&self, name: &str, fields: Document) {
        let mut data = self.data.lock().unwrap();
        data.tasks
            .get_mut(&Host::Local)
            .unwrap()
            .iter_mut()
            .find(|d| d.get_str("name") == Ok(name))
            .unwrap()
            .extend(fields);
        self.publish_snapshot(&data);
    }
    pub fn remove_field(&self, name: &str, key: &str) {
        self.data
            .lock()
            .unwrap()
            .tasks
            .get_mut(&Host::Local)
            .unwrap()
            .iter_mut()
            .find(|d| d.get_str("name") == Ok(name))
            .unwrap()
            .remove(key);
        self.publish();
    }
    pub fn rows_at(&self, host: Host, name: &str) -> Vec<Document> {
        self.data
            .lock()
            .unwrap()
            .collections
            .get(&(host, name.into()))
            .cloned()
            .unwrap_or_default()
    }
    pub fn put_rows(&self, host: Host, name: &str, rows: Vec<Document>) {
        self.data
            .lock()
            .unwrap()
            .collections
            .insert((host, name.into()), rows);
    }
    fn generated(&self, data: &mut Data, name: &str) {
        let path = self.root.join(format!("generated-{name}.json"));
        if path.exists() {
            let values: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let rows = values
                .into_iter()
                .map(|v| match Bson::try_from(v).unwrap() {
                    Bson::Document(d) => d,
                    _ => panic!("generated fixture row"),
                })
                .collect();
            data.collections.insert((Host::Local, name.into()), rows);
            fs::remove_file(path).unwrap();
        }
    }
}
#[async_trait]
impl TaskStore for Memory {
    async fn execution_receipt_assessment(&self, h: Host, name: &str) -> Result<Document, String> {
        let rows = self.rows_at(h, name);
        let projection = task_store::execution_receipt_projection();
        let projected = rows
            .iter()
            .map(|r| project(r, Some(&projection)))
            .collect::<Vec<_>>();
        Ok(task_store::assess_execution_receipts(&projected))
    }
    async fn consumer_runtime(
        &self,
        h: Host,
        name: &str,
        profile: &str,
    ) -> Result<Option<Document>, String> {
        let path = self.root.join("os_capability.json");
        if !path.exists() {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if value["task"] != name
            || value["auth_profile"] != profile
            || value["source_remote"] != h.is_remote()
        {
            return Ok(None);
        }
        // Lowest storage serialization only; do not expose a private core parser.
        let capability = bson::to_document(&value).map_err(|error| error.to_string())?;
        Ok(Some(capability))
    }
    async fn find_task(
        &self,
        h: Host,
        q: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("find:{h:?}"))?;
        data.read_queries.push((h, q.clone()));
        let mut rows = data.tasks.get(&h).cloned().unwrap_or_default();
        rows.sort_by_key(|d| d.get_object_id("_id").unwrap().to_hex());
        if newest {
            rows.reverse();
        }
        Ok(rows.into_iter().find(|d| matches(d, &q)))
    }
    async fn find_tasks(&self, h: Host, q: Document) -> Result<Vec<Document>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("find-many:{h:?}"))?;
        Ok(data
            .tasks
            .get(&h)
            .into_iter()
            .flatten()
            .filter(|d| matches(d, &q))
            .cloned()
            .take(2)
            .collect())
    }
    async fn consumer_capability_exists(&self, h: Host, name: &str) -> Result<bool, String> {
        Self::access(
            &mut self.data.lock().unwrap(),
            format!("capability:{h:?}:{name}"),
        )?;
        Ok(false)
    }
    async fn insert_task(&self, h: Host, task: Document) -> Result<Bson, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("insert-task:{h:?}"))?;
        let id = task.get("_id").unwrap().clone();
        data.tasks.entry(h).or_default().push(task);
        self.publish_snapshot(&data);
        Ok(id)
    }
    async fn update_task(&self, h: Host, q: Document, u: Document) -> Result<u64, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("update-task:{h:?}"))?;
        data.writes.push((h, q.clone(), u.clone()));
        if let Some((host,row))=data.before_update.take() {
            data.tasks.insert(host,vec![row]);
        }
        let lose_ack = data.fail_after_update;
        data.fail_after_update = false;
        let Some(row) = data
            .tasks
            .entry(h)
            .or_default()
            .iter_mut()
            .find(|d| matches(d, &q))
        else {
            return Ok(0);
        };
        apply(row, &u);
        self.publish_snapshot(&data);
        if lose_ack {
            Err("injected lost control ACK after apply".into())
        } else {
            Ok(1)
        }
    }
    async fn collection_names(&self, h: Host) -> Result<Vec<String>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("names:{h:?}"))?;
        Ok(data
            .collections
            .keys()
            .filter(|(side, _)| *side == h)
            .map(|(_, n)| n.clone())
            .collect())
    }
    async fn summary(&self, h: Host, n: &str) -> Result<CollectionSummary, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("summary:{h:?}:{n}"))?;
        self.generated(&mut data, n);
        let Some(rows) = data.collections.get(&(h, n.into())) else {
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
        s.latest_heartbeat_at=rows.iter().filter_map(|r|r.get("heartbeatAt").cloned()).last();
        if let Some((host,row))=data.after_summary.take() {
            data.tasks.insert(host,vec![row]);
        }
        Ok(s)
    }
    async fn has_rows(&self, h: Host, n: &str) -> Result<bool, String> {
        Ok(!self.rows_at(h, n).is_empty())
    }
    async fn rows(&self, h: Host, n: &str, p: Option<Document>) -> Result<Vec<Document>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("rows:{h:?}:{n}"))?;
        Ok(data
            .collections
            .get(&(h, n.into()))
            .into_iter()
            .flatten()
            .map(|r| project(r, p.as_ref()))
            .collect())
    }
    async fn insert_rows(&self, h: Host, n: &str, rows: Vec<Document>) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("insert-rows:{h:?}:{n}"))?;
        data.collections
            .entry((h, n.into()))
            .or_default()
            .extend(rows);
        Ok(())
    }
    async fn rename_collection(&self, h: Host, old: &str, new: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("rename:{h:?}:{old}"))?;
        if data.collections.contains_key(&(h, new.into())) {
            return Err("archive collision".into());
        }
        let rows = data
            .collections
            .remove(&(h, old.into()))
            .ok_or("missing source")?;
        data.collections.insert((h, new.into()), rows);
        Ok(())
    }
    async fn ping(&self, h: Host) -> Result<(), String> {
        Self::access(&mut self.data.lock().unwrap(), format!("ping:{h:?}"))
    }
    async fn update_items(
        &self,
        h: Host,
        n: &str,
        q: Document,
        u: Document,
    ) -> Result<u64, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("update-items:{h:?}:{n}"))?;
        let mut count = 0;
        for row in data
            .collections
            .entry((h, n.into()))
            .or_default()
            .iter_mut()
            .filter(|r| matches(r, &q))
        {
            let old = row.clone();
            apply(row, &u);
            count += u64::from(*row != old);
        }
        data.item_writes += count as usize;
        Ok(count)
    }
    async fn update_item(&self, h: Host, n: &str, q: Document, u: Document) -> Result<u64, String> {
        assert!(q.contains_key("_id"));
        self.update_items(h, n, q, u).await
    }
    async fn find_items(
        &self,
        h: Host,
        n: &str,
        q: Document,
        p: Option<Document>,
    ) -> Result<Vec<Document>, String> {
        assert_eq!(p, Some(task_store::receipt_projection()));
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("find-items:{h:?}:{n}"))?;
        Ok(data
            .collections
            .get(&(h, n.into()))
            .into_iter()
            .flatten()
            .filter(|r| matches(r, &q))
            .map(|r| project(r, p.as_ref()))
            .collect())
    }
    async fn list_tasks(&self, h: Host, include: bool) -> Result<Vec<Document>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("list:{h:?}:{include}"))?;
        let p = task_store::list_tasks_options().projection;
        let mut rows: Vec<_> = data
            .tasks
            .get(&h)
            .into_iter()
            .flatten()
            .filter(|r| include || r.get_str("status") != Ok("deactive"))
            .map(|r| project(r, p.as_ref()))
            .collect();
        rows.sort_by_key(|r| r.get_object_id("_id").unwrap().to_hex());
        rows.reverse();
        Ok(rows)
    }
    async fn result_items_page(
        &self,
        h: Host,
        n: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Document>, String> {
        let options = task_store::result_items_options(limit, offset)?;
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("page:{h:?}:{n}:{limit}:{offset}"))?;
        let mut rows = data
            .collections
            .get(&(h, n.into()))
            .cloned()
            .unwrap_or_default();
        rows.sort_by_key(|r| r.get_object_id("_id").unwrap().to_hex());
        Ok(rows
            .into_iter()
            .skip(offset as usize)
            .take((limit + 1) as usize)
            .map(|r| project(&r, options.projection.as_ref()))
            .collect())
    }
    async fn canonical_results(
        &self,
        namespace: &CanonicalNamespace,
        ids: &[Bson],
    ) -> Result<Vec<Document>, String> {
        assert_eq!(namespace.database, "fixture_results");
        assert_eq!(namespace.collection, "user1_alphas");
        assert!(!ids.is_empty() && ids.len() <= 200);
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("canonical:{}", ids.len()))?;
        Ok(data
            .canonical
            .iter()
            .filter(|r| ids.contains(r.get("id").unwrap()))
            .cloned()
            .collect())
    }
    async fn collect_result_items(&self, h: Host, n: &str) -> Result<Vec<Document>, String> {
        let mut data = self.data.lock().unwrap();
        Self::access(&mut data, format!("collect:{h:?}:{n}"))?;
        Ok(data
            .collections
            .get(&(h, n.into()))
            .into_iter()
            .flatten()
            .map(|r| project(r, Some(&doc! {"_id":0})))
            .collect())
    }
}

pub fn parsed(args: &[&str]) -> Request {
    let tokens = args.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    let Entry::Command { invocation, .. } = parse(&tokens).unwrap() else {
        panic!("business command required")
    };
    to_request(&invocation).unwrap()
}
pub fn ok(out: &TaskOutcome) {
    assert_eq!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["ok"], true, "{}", out.value);
}
pub fn failed(out: &TaskOutcome) {
    assert_ne!(out.exit_code, 0, "{}", out.value);
    assert_eq!(out.value["ok"], false, "{}", out.value);
}

pub struct Fixture {
    pub core: TaskControl,
    pub store: Arc<Memory>,
    pub root: PathBuf,
    _fake: FakeBash,
    previous_path: Option<OsString>,
    _environment: MutexGuard<'static, ()>,
}
impl Fixture {
    pub async fn new() -> Self {
        let guard = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
        let fake = FakeBash::new(":");
        let root = PathBuf::from(&fake.config.env.working_dir)
            .canonicalize()
            .unwrap();
        fs::write(
            root.join("credentials.yaml"),
            "synthetic non-secret test configuration\n",
        )
        .unwrap();
        for dir in [".venv/bin", "bin", "scripts"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("wire.py"), include_str!("dispatch_wire.py")).unwrap();
        let python = PathBuf::from(std::env::var_os("QUANTNIGHT_TEST_PYTHON").expect(
            "set existing absolute QUANTNIGHT_TEST_PYTHON; no interpreter discovery/download",
        ));
        assert!(
            python.is_absolute() && python.is_file(),
            "set explicit installed QUANTNIGHT_TEST_PYTHON; no download"
        );
        let quoted = |p: &Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
        let shim = format!(
            "#!/bin/sh\nexec {} {} \"$@\"\n",
            quoted(&python),
            quoted(&root.join("wire.py"))
        );
        for dest in [".venv/bin/python", "bin/uv"] {
            fs::write(root.join(dest), &shim).unwrap();
            fs::set_permissions(root.join(dest), fs::Permissions::from_mode(0o700)).unwrap();
        }
        for (script, kind) in [
            ("check_tmux_python.sh", "checker"),
            ("pause_task.sh", "pause"),
            ("start_task.sh", "regular"),
            ("start_super_task.sh", "super"),
            ("start_priority_task.sh", "priority"),
        ] {
            fs::write(
                root.join("scripts").join(script),
                format!(
                    "exec {} {} {} \"$@\"\n",
                    quoted(&python),
                    quoted(&root.join("wire.py")),
                    kind
                ),
            )
            .unwrap();
        }
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let store = Arc::new(Memory {
            data: Mutex::new(Data::default()),
            root: root.clone(),
        });
        let core = TaskControl::with_store(binding, store.clone());
        let previous_path = std::env::var_os("PATH");
        // Change only after fallible setup: Fixture::drop restores it even when
        // an assertion panics. No real uv is reachable through this PATH.
        std::env::set_var(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        );
        Self {
            core,
            store,
            root,
            _fake: fake,
            previous_path,
            _environment: guard,
        }
    }
    pub async fn install_original_owners(&mut self, cold_local_uri: Option<String>) {
        let source = PathBuf::from(
            std::env::var_os("QUANTNIGHT_BACKEND_SOURCE")
                .expect("explicit frozen backend source required"),
        )
        .canonicalize()
        .unwrap();
        let python = PathBuf::from(std::env::var_os("QUANTNIGHT_TEST_PYTHON").unwrap());
        for (name, body) in [
            (
                "original_owner_guard.py",
                include_str!("original_owner_guard.py"),
            ),
            ("original_os_leaf.py", include_str!("original_os_leaf.py")),
            ("psutil.py", include_str!("original_psutil_leaf.py")),
            (
                "install_original_owner_fixture.py",
                include_str!("install_original_owner_fixture.py"),
            ),
        ] {
            fs::write(self.root.join(name), body).unwrap();
        }
        if let Some(uri) = cold_local_uri {
            self._fake.config.mongodb.local_uri = uri;
            self._fake.config.mongodb.remote_uri = String::new();
        }
        self._fake.config.mongodb.databases.mission = "simulation_mission".into();
        self._fake.config.mongodb.databases.simulation = "simulation_db".into();
        let mut command = tokio::process::Command::new(&python);
        command
            .arg("-B")
            .arg(self.root.join("install_original_owner_fixture.py"))
            .arg(&self.root)
            .arg(&source)
            .arg(&self._fake.config.mongodb.local_uri)
            .arg(&self._fake.config.mongodb.remote_uri)
            .current_dir(&self.root)
            .env("PYTHONPATH", &self.root)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
            .await
            .expect("fixture setup bounded")
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["ok"], true);
        assert_eq!(
            receipt["resolver"],
            self.root.join("native-tmux").to_string_lossy().as_ref()
        );
        self.store.publish();
        let binding = validate_backend_binding(&self._fake.config).await.unwrap();
        self.core = TaskControl::with_store(binding, self.store.clone());
    }
    pub fn os_faults(&self, value: Value) {
        fs::write(self.root.join("os_fault.json"), value.to_string()).unwrap();
    }
    pub fn os_trace(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("os_trace.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    pub fn owner_trace(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("owner_trace.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    pub async fn run(&self, args: &[&str]) -> TaskOutcome {
        let request = parsed(args);
        let out = dispatch(&self.core, &request).await;
        assert_eq!(out.value["command"], args[0]);
        assert!(
            !self.root.join("VIOLATIONS").exists(),
            "{}",
            fs::read_to_string(self.root.join("VIOLATIONS")).unwrap_or_default()
        );
        out
    }
    pub fn faults(&self, v: Value) {
        fs::write(self.root.join("fault.json"), v.to_string()).unwrap();
    }
    pub fn wire(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("wire.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    pub fn count(&self, first: &str) -> usize {
        self.wire().iter().filter(|v| v["argv"][0] == first).count()
    }
    pub async fn create_generate(&self, name: &str, kind: &str, remote: bool) {
        let mut args = vec![
            "create",
            "--name",
            name,
            "--templatefile",
            "frozen",
            "--type",
            kind,
        ];
        if remote {
            args.push("--remote")
        };
        ok(&self.run(&args).await);
        if kind != "priority" {
            ok(&self.run(&["generate", "--name", name]).await);
            if remote {
                ok(&self.run(&["sync", "--name", name]).await);
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.previous_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
}

pub fn unposted(id: &str, status: &str) -> Document {
    doc! {"_id":ObjectId::parse_str(id).unwrap(),"status":status,"campaign_post_attempted":false,"_not_posted_":true}
}
pub fn config(name: &str) -> String {
    json!({"max_concurrent":2,"max_multi_simulation_children":1,"tag":{"tags":[name]},"literal":{"$oid":"opaque-not-bson","null":null,"flags":[false,true],"unicode":"原值"}}).to_string()
}
