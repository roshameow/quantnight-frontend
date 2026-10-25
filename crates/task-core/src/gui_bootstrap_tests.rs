//! Actual GUI startup algorithm -> actual original Python owner -> lowest
//! opaque client tokens. No sockets, Mongo clients, task IO or real credentials.
use super::*;
use crate::config::bash_test_support::FakeBash;
use std::{path::{Path, PathBuf}, sync::atomic::{AtomicUsize, Ordering}};
use std::os::unix::fs::PermissionsExt;

const LOCAL: &str = "mongodb://synthetic:private-secret@localhost:27017/?connectTimeoutMS=111&serverSelectionTimeoutMS=222&socketTimeoutMS=333";
const REMOTE: &str = "mongodb://synthetic:private-secret@remote.offline.invalid:27018/?connectTimeoutMS=444&serverSelectionTimeoutMS=555&socketTimeoutMS=666";

fn fixture(mode: &str) -> FakeBash {
    let source = PathBuf::from(std::env::var_os("QUANTNIGHT_BACKEND_SOURCE").expect("gate must bind backend source")).canonicalize().unwrap();
    let python = PathBuf::from(std::env::var_os("QUANTNIGHT_TEST_PYTHON").expect("gate must bind prepared interpreter"));
    assert!(source.join("backend/mongodb/mongo_config.py").is_file());
    let mut fake = FakeBash::new(":");
    let root = PathBuf::from(&fake.config.env.working_dir);
    std::fs::write(root.join("credentials.yaml"), format!("mongodb:\n  local_uri: '{LOCAL}'\n  remote_uri: '{REMOTE}'\n")).unwrap();
    std::fs::write(root.join("mode"), mode).unwrap();
    std::fs::create_dir_all(root.join("other-root")).unwrap();
    let guard = root.join("original_owner_guard.py");
    std::fs::write(&guard, r#"import sys, socket, subprocess, runpy, contextlib, io, json, pathlib
violations=[]
def forbidden(*args, **kwargs):
    violations.append('forbidden IO')
    raise AssertionError('original config owner attempted forbidden IO')
socket.socket.connect=forbidden
socket.socket.connect_ex=forbidden
socket.getaddrinfo=forbidden
subprocess.Popen=forbidden
subprocess.run=forbidden
import pymongo, motor.motor_asyncio
pymongo.MongoClient.__init__=forbidden
motor.motor_asyncio.AsyncIOMotorClient.__init__=forbidden
args=sys.argv[1:]
if args[:1]==['-B']: args=args[1:]
assert args[:2]==['-m','backend.mongodb.mongo_config']
assert not any('private-secret' in a for a in args)
import os
assert not any(k in os.environ for k in ('TMUX','TMUX_PANE','TMUX_TMPDIR'))
operation=args[2]
with open('calls','a') as file: file.write(operation+'\n')
sys.argv=args[1:]
output=io.StringIO()
code=0
with contextlib.redirect_stdout(output):
    try: runpy.run_module('backend.mongodb.mongo_config',run_name='__main__')
    except SystemExit as exc: code=exc.code
assert not violations
mode=pathlib.Path('mode').read_text()
export=operation=='--export-task-config'
# Corrupt only the lowest private pipe AFTER the real owner has executed.
if export and mode in ('wrong-root','swap','omit','namespace'):
    receipt=json.loads(output.getvalue()); result=receipt['result']
    if mode=='wrong-root': result['env']['working_dir']=str(pathlib.Path('other-root').resolve())
    if mode=='swap': result['mongodb']['local_uri'],result['mongodb']['remote_uri']=result['mongodb']['remote_uri'],result['mongodb']['local_uri']
    if mode=='omit': result['mongodb']['remote_uri']=''
    if mode=='namespace': result['mongodb']['databases']['mission']='wrong'
    output=io.StringIO(json.dumps(receipt))
if (export and mode=='export-change') or (not export and mode=='validate-change'):
    with open('credentials.yaml','a') as file: file.write('\n# synthetic change\n')
if mode==('export-secret' if export else 'validate-secret'):
    print('private-secret-stdout');print('private-secret-stderr',file=sys.stderr);sys.exit(23)
if export and mode=='malformed':
    print('private-secret-malformed');sys.exit(0)
print(output.getvalue(),end='')
sys.exit(code)
"#).unwrap();
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    let executable = root.join(".venv/bin/python");
    std::fs::write(&executable, format!("#!/bin/sh\nPYTHONPATH={} exec {} {} \"$@\"\n",quote(&source),quote(&python),quote(&guard))).unwrap();
    std::fs::set_permissions(executable,std::fs::Permissions::from_mode(0o700)).unwrap();
    fake.config.mongodb.local_uri = "mongodb://synthetic:private-secret@127.0.0.1:27017/?directConnection=true".into();
    fake.config.mongodb.remote_uri = "mongodb://synthetic:private-secret@remote.offline.invalid:27018/".into();
    fake.config.mongodb.databases.mission = "simulation_mission".into();
    fake.config.mongodb.databases.simulation = "simulation_db".into();
    fake.config.mongodb.databases.alpha = "custom_alpha".into();
    fake.config.mongodb.databases.dataset = Some("custom_dataset".into());
    fake.config.mongodb.databases.dataset_collection = Some("custom_datasets".into());
    fake.config.mongodb.databases.datafield = Some("custom_field".into());
    fake.config.mongodb.databases.datafield_collection = Some("custom_fields".into());
    fake.config.python.scripts = fake.config.bash.scripts.clone();
    fake
}
fn count(fake: &FakeBash) -> usize {
    std::fs::read_to_string(Path::new(&fake.config.env.working_dir).join("calls")).unwrap().lines().count()
}

#[tokio::test]
async fn incident_uses_exact_backend_transport_preserves_ui_and_injects_identical_store_arcs() {
    let fake = fixture("");
    // Old direct validation MUST still fail. No host alias/options equivalence.
    assert!(validate_backend_binding(&fake.config).await.is_err());
    let original = fake.config.clone();
    let calls = AtomicUsize::new(0);
    let context = bootstrap_gui_with(fake.config.clone(), |role, uri| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(uri, if role == "local" { LOCAL } else { REMOTE });
        async { Ok(Arc::new(uri)) }
    }).await.unwrap();
    let (effective, clients, binding, store) = context.checked_parts().unwrap();
    assert_eq!(effective.mongodb.local_uri, LOCAL);
    assert_eq!(effective.mongodb.remote_uri, REMOTE);
    assert_eq!(effective.env.working_dir, binding.root().to_string_lossy());
    // Includes all UI namespaces/collections and every script field/map.
    assert_eq!(format!("{:?}", effective.mongodb.databases), format!("{:?}", original.mongodb.databases));
    assert_eq!(format!("{:?}", effective.python), format!("{:?}", original.python));
    assert_eq!(format!("{:?}", effective.bash), format!("{:?}", original.bash));
    let read_clients = clients.clone();
    let watcher_clients = clients.clone();
    assert!(Arc::ptr_eq(&read_clients, &watcher_clients));
    for _ in 0..3 {
        assert!(Arc::ptr_eq(&read_clients.local, store.existing_client(Host::Local).await));
        assert!(Arc::ptr_eq(&watcher_clients.remote, store.existing_client(Host::Remote).await));
        binding.verify_current().unwrap();
    }
    assert!(!Arc::ptr_eq(&clients.local, &clients.remote));
    assert_eq!(calls.load(Ordering::SeqCst), 2); // one full-pool build, two roles
    assert_eq!(count(&fake), 3); // rejected old validator + one export + one proof
}

#[tokio::test]
async fn root_namespace_role_and_private_output_fail_before_any_client_factory() {
    for mode in ["wrong-root", "swap", "omit", "namespace", "export-secret", "validate-secret", "malformed", "export-change", "validate-change"] {
        let fake = fixture(mode);
        let result = bootstrap_gui_with::<String, _, _>(fake.config.clone(), |_, _| async { panic!("factory reached for {mode}") }).await;
        let error = result.err().expect(mode);
        assert!(!error.contains("private-secret"), "private output leaked");
    }
    let mut fake = fixture("");
    fake.config.mongodb.databases.simulation = "old_alias".into();
    assert!(bootstrap_gui_with::<String, _, _>(fake.config.clone(), |_, _| async { panic!("namespace conflict") }).await.is_err());
    assert_eq!(count(&fake),1);
    fake.config.env.working_dir.push_str("/missing");
    assert!(bootstrap_gui_with::<String, _, _>(fake.config.clone(), |_, _| async { panic!("wrong root") }).await.is_err());
}

#[tokio::test]
async fn no_gui_remote_fallback_and_no_publication_after_client_or_context_change() {
    let fake = fixture("");
    let root = PathBuf::from(&fake.config.env.working_dir);
    std::fs::write(root.join("credentials.yaml"), format!("mongodb:\n  local_uri: '{LOCAL}'\n")).unwrap();
    let error = bootstrap_gui_with::<String, _, _>(fake.config.clone(), |_, _| async { panic!("absent remote must guard both roles") }).await.err().unwrap();
    assert!(error.contains("remote MongoDB role is unconfigured"));
    for stage in ["during-client", "after-context"] {
        let fake = fixture("");
        let root = PathBuf::from(&fake.config.env.working_dir);
        let context = bootstrap_gui_with(fake.config.clone(), |role, uri| {
            if stage == "during-client" && role == "remote" {
                std::fs::write(root.join("credentials.yaml"), "changed synthetic context\n").unwrap();
            }
            async { Ok(Arc::new(uri)) }
        }).await;
        if stage == "during-client" { assert!(context.is_err()); }
        else {
            let context = context.unwrap();
            std::fs::write(root.join("credentials.yaml"), "changed synthetic context\n").unwrap();
            assert!(context.checked_parts().is_err());
        }
        assert_eq!(count(&fake),2); // failure is not a retry/re-export
    }
}

#[tokio::test]
async fn canonical_logical_root_symlink_is_preserved_and_target_changes_do_not_rebind() {
    let fake = fixture("");
    let root = PathBuf::from(&fake.config.env.working_dir);
    std::fs::rename(root.join("credentials.yaml"),root.join("synthetic-secret.yaml")).unwrap();
    std::os::unix::fs::symlink("synthetic-secret.yaml", root.join("credentials.yaml")).unwrap();
    let context = bootstrap_gui_with(fake.config.clone(), |_, uri| async { Ok(Arc::new(uri)) }).await.unwrap();
    let (_, clients, binding, store) = context.checked_parts().unwrap();
    std::fs::write(root.join("synthetic-secret.yaml"),format!("mongodb:\n  local_uri: '{REMOTE}'\n  remote_uri: '{LOCAL}'\n")).unwrap();
    assert!(binding.refresh_if_changed().await.is_err());
    assert!(binding.verify_current().is_err());
    assert!(Arc::ptr_eq(&clients.local, store.existing_client(Host::Local).await));
}
