//! Actual bootstrap/process owner, synthetic child only; never a Mongo client.
use quantnight_taskctl::{bootstrap, parse, Entry};
use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root=std::env::temp_dir().join(format!("qn-cli-bootstrap-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap(); Self(root)
    }
    #[cfg(unix)]
    fn child(&self, body:&str) {
        use std::os::unix::fs::PermissionsExt;
        let file=self.0.join(".venv/bin/python");
        std::fs::write(&file,format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(file,std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
impl Drop for Root {fn drop(&mut self){std::fs::remove_dir_all(&self.0).unwrap();}}
#[test]
fn backend_selection_does_not_steal_business_config_or_initialize_for_help() {
    let args=|values:&[&str]| values.iter().map(|s|s.to_string()).collect::<Vec<_>>();
    match parse(&args(&["--backend-root=/missing-fixture","start","--name","x","--config","{\"v\":\"--backend-root\"}"])).unwrap() {
        Entry::Command {app_config,backend_root,invocation}=>{
            assert!(app_config.is_none());assert_eq!(backend_root.unwrap(),PathBuf::from("/missing-fixture"));
            assert_eq!(invocation.command,"start");
        },_=>panic!("command")
    }
    assert!(matches!(parse(&args(&["--backend-root","/missing-fixture","--help"])).unwrap(),Entry::Help(_)));
    assert!(parse(&args(&["--backend-root","a","--app-config","b","list"])).is_err());
    assert!(parse(&args(&["list","--backend-root","a"])).is_err());
    assert!(parse(&args(&["--backend-root"])).is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn original_owner_private_result_can_bootstrap_without_gui_config_or_remote_role() {
    let root=Root::new();
    let value=serde_json::json!({"schemaVersion":1,"command":"export-task-config","ok":true,"result":{
        "env":{"working_dir":root.0},"python":{"scripts":{}},"bash":{"scripts":{}},
        "mongodb":{"local_uri":"mongodb://synthetic:private-secret@offline.invalid","remote_uri":"",
            "databases":{"mission":"simulation_mission","simulation":"simulation_db","alpha":""}}}});
    root.child(&format!("printf '%s\\n' \"$@\" > owner.argv\nprintf '%s\\n' '{}'",value));
    let config=bootstrap::load(None,Some(&root.0)).await.unwrap();
    assert_eq!(config.env.working_dir,root.0.to_string_lossy());
    assert!(config.mongodb.remote_uri.is_empty());
    assert!(config.mongodb.local_uri.contains("private-secret"));
    let args=std::fs::read_to_string(root.0.join("owner.argv")).unwrap();
    assert!(args.contains("backend.mongodb.mongo_config\n--export-task-config\n"));
    assert!(!args.contains("private-secret"));
    assert!(!root.0.join("config.toml").exists());
}
#[cfg(unix)]
#[tokio::test]
async fn private_output_and_schema_failures_do_not_escape_or_fall_back() {
    for body in ["echo private-secret; exit 23", "echo private-secret", "echo '{\"ok\":true}'"] {
        let root=Root::new(); root.child(body);
        let error=bootstrap::load(None,Some(&root.0)).await.err().unwrap();
        assert!(!error.contains("private-secret"));
    }
    let root=Root::new();
    assert!(bootstrap::load(None,Some(&root.0)).await.is_err());
    assert!(bootstrap::load(None,None).await.is_err());
}

#[cfg(all(unix, feature = "backend-owner-tests"))]
#[tokio::test]
async fn actual_original_python_owner_exports_and_validates_local_only_without_any_client() {
    let source = PathBuf::from(std::env::var_os("QUANTNIGHT_BACKEND_SOURCE")
        .expect("required gate must bind the backend source explicitly")).canonicalize().unwrap();
    let python = PathBuf::from(std::env::var_os("QUANTNIGHT_TEST_PYTHON")
        .expect("required gate must supply its existing Python interpreter"));
    assert!(source.join("backend/mongodb/mongo_config.py").is_file());
    assert!(python.is_file());
    let root = Root::new();
    std::fs::write(root.0.join("credentials.yaml"), "mongodb:\n  local_uri: mongodb://synthetic:private-secret@local.offline.invalid:27017\n").unwrap();
    let guard = root.0.join("original_owner_guard.py");
    std::fs::write(&guard, r#"import sys, socket, subprocess, runpy
violations=[]
def forbidden(*args, **kwargs):
    violations.append('forbidden IO')
    raise AssertionError('original config owner cannot perform IO beyond its synthetic file')
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
sys.argv=args[1:]
try:
    runpy.run_module('backend.mongodb.mongo_config',run_name='__main__')
except SystemExit:
    if violations: raise SystemExit(96)
    raise
assert not violations
"#).unwrap();
    let quote=|path:&std::path::Path| format!("'{}'",path.to_string_lossy().replace('\'',"'\\''"));
    root.child(&format!("PYTHONPATH={} exec {} {} \"$@\"",quote(&source),quote(&python),quote(&guard)));
    let config=bootstrap::load(None,Some(&root.0)).await.unwrap();
    assert_eq!(config.mongodb.local_uri,"mongodb://synthetic:private-secret@local.offline.invalid:27017");
    assert!(config.mongodb.remote_uri.is_empty());
    let binding=quantnight_task_core::backend_binding::validate_backend_binding(&config).await.unwrap();
    assert!(binding.request()["remoteUriSha256"].is_null());
    binding.verify_current().unwrap();
}

#[cfg(feature = "backend-owner-tests")]
#[test]
fn actual_python_compatibility_path_executes_this_built_native_cli_for_help_and_errors() {
    let source=PathBuf::from(std::env::var_os("QUANTNIGHT_BACKEND_SOURCE").expect("gate backend source"));
    let python=PathBuf::from(std::env::var_os("QUANTNIGHT_TEST_PYTHON").expect("gate interpreter"));
    let root=Root::new();
    let relative=".agents/skills/manage-quantnight-frontend/scripts/taskctl.py";
    let shim=root.0.join(relative); std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
    std::fs::copy(source.join(relative),&shim).unwrap();
    for (arg,expected) in [("--help",0),("--definitely-invalid-native-option",2)] {
        let result=std::process::Command::new(&python).args(["-B"]).arg(&shim).arg(arg)
            .current_dir(&root.0).env_clear().env("PATH","/usr/bin:/bin")
            .env("QUANTNIGHT_TASKCTL_BIN",env!("CARGO_BIN_EXE_taskctl"))
            .output().unwrap();
        assert_eq!(result.status.code(),Some(expected),"{}",String::from_utf8_lossy(&result.stderr));
        if expected==0 {assert!(String::from_utf8_lossy(&result.stdout).contains("observe-accepted"));}
    }
    assert!(!root.0.join("credentials.yaml").exists());
    assert!(!root.0.join(".quantnight").exists());
}
