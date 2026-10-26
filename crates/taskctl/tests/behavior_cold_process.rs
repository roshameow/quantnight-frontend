//! Cold native executable + ORIGINAL Python exec shim; actual business failure
//! through bootstrap/parser/dispatcher/core/MongoTaskStore, without any socket.
//! A local invalid ClientOptions value must fail before Client construction;
//! missing remote role must not fall back to that invalid local configuration.
//! This is not cold positive DB/lifecycle or real Mongo wire coverage.
#![cfg(unix)]
#[path = "support/dispatch_fixture.rs"]
mod support;
use serde_json::Value;
use std::{fs, io::ErrorKind, net::TcpStream, path::PathBuf, time::Duration};
use support::*;

async fn cold(f: &Fixture, shim: bool, remote: bool) -> std::process::Output {
    let native = PathBuf::from(env!("CARGO_BIN_EXE_taskctl"));
    let mut command = if shim {
        let mut cmd =
            tokio::process::Command::new(std::env::var_os("QUANTNIGHT_TEST_PYTHON").unwrap());
        cmd.arg("-B").arg(
            f.root
                .join(".agents/skills/manage-quantnight-frontend/scripts/taskctl.py"),
        );
        cmd
    } else {
        let mut cmd = tokio::process::Command::new(&native);
        cmd.arg("--backend-root").arg(&f.root);
        cmd
    };
    command.arg("list");
    if remote {
        command.arg("--remote");
    }
    command
        .current_dir(&f.root)
        .env_clear()
        .env("HOME", &f.root)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", f.root.join("bin").display()),
        )
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("QUANTNIGHT_TASKCTL_BIN", &native)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(25), command.output())
        .await
        .expect("bounded cold command")
        .unwrap()
}
fn assert_negative(f: &Fixture, result: &std::process::Output, reason: &str) {
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "list");
    assert!(value["task"].is_null());
    assert_eq!(value["ok"], false);
    assert!(value["error"].as_str().unwrap().contains(reason), "{value}");
    assert!(
        !f.root.join("VIOLATIONS").exists(),
        "{}",
        fs::read_to_string(f.root.join("VIOLATIONS")).unwrap_or_default()
    );
    assert!(
        !f.root.join(".quantnight").exists(),
        "read-only business failure must not create mutation or enrollment state"
    );
    assert!(
        f.os_trace().is_empty(),
        "cold list must not invoke shell/task execution"
    );
}
fn owner_count(f: &Fixture, flag: &str) -> usize {
    f.owner_trace()
        .iter()
        .filter(|v| v["argv"].as_array().unwrap().iter().any(|a| a == flag))
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn cold_native_and_original_shim_reach_business_core_without_remote_role_fallback() {
    let mut f = Fixture::new().await;
    // Original Python export/binding accepts the exact synthetic URI, while
    // original Rust ClientOptions must reject this known invalid numeric value
    // before constructing a client or performing DNS/socket I/O.
    let uri = "mongodb://127.0.0.1:1/?maxPoolSize=not-an-integer".to_string();
    f.install_original_owners(Some(uri)).await;
    fs::write(f.root.join("owner_trace.jsonl"), "").unwrap();
    let credentials = fs::read(f.root.join("credentials.yaml")).unwrap();
    for shim in [false, true] {
        let result = cold(&f, shim, true).await;
        assert_negative(&f, &result, "remote MongoDB role is unconfigured");
    }
    assert_eq!(owner_count(&f, "--export-task-config"), 2);
    assert_eq!(owner_count(&f, "--validate-task-control-binding"), 2);
    assert_eq!(
        fs::read(f.root.join("credentials.yaml")).unwrap(),
        credentials
    );
    assert!(f.store.data.lock().unwrap().writes.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn cold_native_and_original_shim_local_invalid_options_fail_in_core_before_any_client() {
    let mut f = Fixture::new().await;
    // Original Python export/binding accepts the exact synthetic URI, while
    // original Rust ClientOptions must reject this known invalid numeric value
    // before constructing a client or performing DNS/socket I/O.
    let uri = "mongodb://127.0.0.1:1/?maxPoolSize=not-an-integer".to_string();
    f.install_original_owners(Some(uri)).await;
    fs::write(f.root.join("owner_trace.jsonl"), "").unwrap();
    let credentials = fs::read(f.root.join("credentials.yaml")).unwrap();
    for shim in [false, true] {
        let result = cold(&f, shim, false).await;
        assert_negative(
            &f,
            &result,
            "invalid local MongoDB configuration (details redacted)",
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains("export-task-config"));
    }
    assert_eq!(owner_count(&f, "--export-task-config"), 2);
    assert_eq!(owner_count(&f, "--validate-task-control-binding"), 2);
    assert_eq!(
        fs::read(f.root.join("credentials.yaml")).unwrap(),
        credentials
    );
}
