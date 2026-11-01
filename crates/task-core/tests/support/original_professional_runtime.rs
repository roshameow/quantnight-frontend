// Reuse original Python shell/OS fixture; never production DB or runtime.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct CachedTemplate {
    dir: PathBuf,
}

fn copy_tree_relocating(src: &Path, dst: &Path, from_prefix: &str, to_prefix: &str, relocate_text: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let file_type = entry.file_type().unwrap();
        let src_path = entry.path();
        let name = entry.file_name();
        let dst_path = dst.join(&name);
        if file_type.is_dir() {
            let sub_relocate = relocate_text
                && matches!(
                    name.to_str(),
                    Some("scripts" | "bin" | ".venv")
                );
            copy_tree_relocating(&src_path, &dst_path, from_prefix, to_prefix, sub_relocate);
        } else if file_type.is_file() {
            let mode = entry.metadata().unwrap().permissions().mode();
            if relocate_text {
                if let Ok(text) = std::fs::read_to_string(&src_path) {
                    if text.contains(from_prefix) {
                        std::fs::write(&dst_path, text.replace(from_prefix, to_prefix)).unwrap();
                    } else {
                        std::fs::copy(&src_path, &dst_path).unwrap();
                    }
                } else {
                    std::fs::copy(&src_path, &dst_path).unwrap();
                }
            } else {
                std::fs::copy(&src_path, &dst_path).unwrap();
            }
            std::fs::set_permissions(&dst_path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

fn base_template() -> &'static CachedTemplate {
    static BASE: OnceLock<CachedTemplate> = OnceLock::new();
    BASE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("qn-prof-base-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let backend = std::env::var("QUANTNIGHT_BACKEND_SOURCE")
            .expect("original gate supplies isolated backend source");
        let python = std::env::var("QUANTNIGHT_TEST_PYTHON")
            .expect("original gate supplies fixture Python");
        let setup = r#"
import sys
from pathlib import Path
from test_system_tmux_context import fixture
root=Path(sys.argv[1])
scripts,env,binding,native=fixture(root)
native.write_text(native.read_text().replace('|job','|fixture'))
(root/'sitecustomize.py').write_text('import time\ntime.sleep=lambda seconds: None\n')
"#;
        let output = std::process::Command::new(&python)
            .args(["-B", "-c", setup])
            .arg(&dir)
            .env("PYTHONPATH", format!("{backend}:{backend}/tests"))
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        CachedTemplate { dir }
    })
}

pub fn original_professional_fixture(
    drop_stop_receipt: bool,
) -> quantnight_task_core::config::bash_test_support::FakeBash {
    use quantnight_task_core::config::bash_test_support::FakeBash;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let tpl = base_template();
    let fake = FakeBash::new(":");
    let root = Path::new(&fake.config.env.working_dir);
    copy_tree_relocating(
        &tpl.dir,
        root,
        &tpl.dir.to_string_lossy(),
        &root.to_string_lossy(),
        true,
    );
    let python = std::env::var("QUANTNIGHT_TEST_PYTHON")
        .expect("original gate supplies fixture Python");
    std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    let quote = |v: &str| format!("'{}'", v.replace('\'', "'\\''"));
    let wrapper = format!(
        r#"#!/bin/bash
set -euo pipefail
if [[ "${{3:-}}" == backend.mongodb.mongo_config ]]; then
 echo '{{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}}'; exit 0
fi
export PATH={bins}:/bin:/usr/bin
export STATE={state} TRACE={trace} PYTHONPATH={root} QUANTNIGHT_WAIT_CONSUMER_MS=50
printf '%s\n' "${{@: -1}}" >> {calls}
if [[ {drop} == true && "${{@: -1}}" == '' ]]; then
 {python} "$@" | /usr/bin/sed '/^QUANTNIGHT_PAUSE_CONFIRMED /d'
else
 exec {python} "$@"
fi
"#,
        bins = quote(&root.join("bin").to_string_lossy()),
        state = quote(&root.join("state").to_string_lossy()),
        trace = quote(&root.join("trace").to_string_lossy()),
        root = quote(&root.to_string_lossy()),
        calls = quote(&root.join("python.calls").to_string_lossy()),
        python = quote(&python),
        drop = drop_stop_receipt
    );
    // Original shell entry must inherit the SAME lowest fake OS environment,
    // not just Python grandchildren (otherwise a parent uv could escape).
    let mut shell = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("scripts/script_config.sh"))
        .unwrap();
    writeln!(
        shell,
        "\nexport PATH={}:{} STATE={} TRACE={} PYTHONPATH={} UV_OFFLINE=1 QUANTNIGHT_WAIT_CONSUMER_MS=50",
        quote(&root.join("bin").to_string_lossy()),
        "/bin:/usr/bin",
        quote(&root.join("state").to_string_lossy()),
        quote(&root.join("trace").to_string_lossy()),
        quote(&root.to_string_lossy())
    )
    .unwrap();
    let path = root.join(".venv/bin/python");
    std::fs::write(&path, wrapper).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    fake
}

#[allow(dead_code)]
pub fn priority_professional_fixture() -> quantnight_task_core::config::bash_test_support::FakeBash {
    static PRIO: OnceLock<CachedTemplate> = OnceLock::new();
    let tpl = PRIO.get_or_init(|| {
        let base = original_professional_fixture(false);
        let base_root = Path::new(&base.config.env.working_dir);
        let dir = std::env::temp_dir().join(format!("qn-prof-prio-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_tree_relocating(
            base_root,
            &dir,
            &base_root.to_string_lossy(),
            &dir.to_string_lossy(),
            true,
        );
        let backend = std::env::var("QUANTNIGHT_BACKEND_SOURCE").unwrap();
        let python = std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap();
        let support = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/priority_start_os.py");
        let out = std::process::Command::new(&python)
            .args(["-B"])
            .arg(support)
            .arg(&dir)
            .arg(&backend)
            .env("PYTHONPATH", &backend)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        CachedTemplate { dir }
    });
    let mut fake = quantnight_task_core::config::bash_test_support::FakeBash::new(":");
    let root = Path::new(&fake.config.env.working_dir);
    copy_tree_relocating(
        &tpl.dir,
        root,
        &tpl.dir.to_string_lossy(),
        &root.to_string_lossy(),
        true,
    );
    fake.config.mongodb.databases.mission = "simulation_mission".into();
    fake.config.mongodb.databases.simulation = "simulation_db".into();
    fake
}

#[allow(dead_code)]
pub fn install_original_super_process(
    root: &Path,
    task_id: &str,
    name: &str,
    remote: bool,
    launch: &str,
) {
    let config = serde_json::json!({"startup_binding":{"taskId":task_id,"name":name,"remote":remote,"launchAttemptId":launch,"consumer_run_id":"original-run","auth_profile":"user1"}}).to_string();
    let argv = vec![
        "python".to_string(),
        "-m".into(),
        "scripts.super_alpha_simulate_param".into(),
        "--task".into(),
        name.into(),
        "--dist".into(),
        if remote { "remote".into() } else { "local".into() },
        "--auth_profile".into(),
        "user1".into(),
        "--config".into(),
        config,
    ];
    std::fs::write(
        root.join("psutil.py"),
        format!(
            r#"import os
from pathlib import Path
STATUS_ZOMBIE='zombie'
ARGV={argv}
class NoSuchProcess(Exception): pass
class AccessDenied(Exception): pass
class Error(Exception): pass
class Process:
 def __init__(self,pid):
  self.pid=pid
  if pid not in (1001,1002) or not Path(os.environ['STATE']).exists(): raise NoSuchProcess(pid)
 def create_time(self): return 1700000010.0 if self.pid==1002 else 1700000000.0
 def cwd(self): return str(Path(os.environ['STATE']).parent)
 def cmdline(self): return ARGV if self.pid==1002 else ['bash']
 def is_running(self): return Path(os.environ['STATE']).exists()
 def status(self): return 'running'
"#,
            argv = serde_json::to_string(&argv).unwrap()
        ),
    )
    .unwrap();
}
