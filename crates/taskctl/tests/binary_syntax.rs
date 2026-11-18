//! The only real process launched here is the newly built binary, and only
//! help/invalid syntax. No valid business invocation, app config or service.
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(args)
        .env_clear()
        .env("PATH", "/nonexistent-taskctl-fixture")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}
#[test]
fn actual_binary_all_command_help_needs_no_config_runtime_or_python() {
    for cmd in [
        "list",
        "verify-registration",
        "archive-research",
        "create",
        "generate",
        "sync",
        "start",
        "observe-accepted",
        "watch",
        "status",
        "pause",
        "resume",
        "priority-prune",
        "delete",
        "results",
        "collect-results",
        "alphas",
        "alpha-pnl",
        "datasets",
        "datafields",
        "submission-stats",
    ] {
        let out = run(&[
            "--app-config",
            "/nonexistent-taskctl-fixture/config.toml",
            cmd,
            "--help",
        ]);
        assert_eq!(out.status.code(), Some(0), "{cmd}: {:?}", out.stderr);
        assert!(out.stderr.is_empty(), "{cmd}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(
            text.contains(cmd) && text.contains("--app-config PATH"),
            "{text}"
        );
    }
    let out = run(&["--help"]);
    assert!(out.status.success() && out.stderr.is_empty());
}
#[test]
fn actual_binary_invalid_syntax_is_stderr_exit_two_even_with_explicit_config() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["create"],
        vec!["start", "--name", "fixture", "--unknown"],
        vec!["results", "--name", "fixture", "--limit", "not-an-int"],
        vec![
            "watch",
            "--name",
            "fixture",
            "--notify-task-id",
            "w",
            "--no-notify",
        ],
        vec!["status", "--name"],
        vec!["--app-config"],
        vec!["list", "--app-config", "x"],
    ] {
        let mut prefixed = vec!["--app-config", "/nonexistent-taskctl-fixture/config.toml"];
        prefixed.extend(args);
        let out = run(&prefixed);
        assert_eq!(out.status.code(), Some(2), "{prefixed:?}: {:?}", out.stderr);
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}
#[cfg(unix)]
#[test]
fn invalid_utf8_is_a_syntax_error_not_a_panic_or_config_load() {
    use std::os::unix::ffi::OsStringExt;
    let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("valid UTF-8"));
}

#[test]
fn shared_entry_keeps_help_syntax_and_utf8_before_host_admission() {
    use std::{cell::Cell, ffi::OsString};
    let calls = Cell::new(0);
    for (args, code) in [(vec!["--help"], 0), (vec!["unknown"], 2),
        (vec!["--backend-root", "/absent", "status", "--name"], 2)] {
        assert_eq!(quantnight_taskctl::run_os_with_admission(
            args.into_iter().map(OsString::from), |_, _| { calls.set(calls.get() + 1); Err("must not enter".into()) }), code);
    }
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(quantnight_taskctl::run_os_with_admission(
            [OsString::from_vec(vec![0xff])], |_, _| { calls.set(calls.get() + 1); Err("must not enter".into()) }), 2);
    }
    assert_eq!(calls.get(), 0);
    // A valid original command reaches the explicit host hook, but cannot reach
    // runtime/bootstrap/config/DB when the hook refuses it. No dispatcher stub.
    assert_eq!(quantnight_taskctl::run_os_with_admission(
        ["--backend-root", "/nonexistent-taskctl-fixture", "status", "--name", "fixture"].map(OsString::from),
        |app, backend| {
            calls.set(calls.get() + 1);
            assert!(app.is_none());
            assert_eq!(backend.unwrap(), std::path::Path::new("/nonexistent-taskctl-fixture"));
            Err("fixture host is stale".into())
        }), 1);
    assert_eq!(calls.get(), 1);
}

#[cfg(feature = "dev-host-tests")]
#[test]
fn compiled_gui_host_early_entry_matches_standalone_without_gui_config_or_business_io() {
    let host = std::env::var_os("QUANTNIGHT_TEST_DEV_HOST").expect("explicit compiled dev host required");
    assert!(std::path::Path::new(&host).is_absolute());
    let invoke = |args: &[std::ffi::OsString]| {
        Command::new(&host).arg("taskctl").args(args).env_clear()
            .env("PATH", "/nonexistent-taskctl-fixture").env("HOME", "/nonexistent-taskctl-fixture")
            .current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap()
    };
    let mut cases = vec![vec!["--help"], vec!["unknown"], vec!["create"],
        vec!["status", "--name"], vec!["results", "--name", "x", "--limit", "not-an-int"]];
    for command in ["list", "verify-registration", "archive-research", "create", "generate", "sync",
        "start", "observe-accepted", "watch", "status", "pause", "resume", "priority-prune", "delete", "results", "collect-results"] {
        cases.push(vec!["--app-config", "/nonexistent-taskctl-fixture/config.toml", command, "--help"]);
    }
    for args in cases {
        let standalone = run(&args);
        let hosted = invoke(&args.iter().map(std::ffi::OsString::from).collect::<Vec<_>>());
        assert_eq!(hosted.status.code(), standalone.status.code(), "{args:?}");
        assert_eq!(hosted.stdout, standalone.stdout, "{args:?}");
        assert_eq!(hosted.stderr, standalone.stderr, "{args:?}");
    }
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStringExt;
        let args = [std::ffi::OsString::from_vec(vec![0xff])];
        let hosted = invoke(&args);
        assert_eq!(hosted.status.code(), Some(2)); assert!(hosted.stdout.is_empty());
        assert_eq!(hosted.stderr, b"taskctl: arguments must be valid UTF-8\n");
    }
}

#[test]
fn qn_shim_prefix_management_help_needs_no_binding_or_admission(){
    let code=quantnight_taskctl::run_os_with_admission(["--backend-root","/nonexistent/explicit-shim-root","runtime-plan","--help"].map(std::ffi::OsString::from),|_,_|panic!("management help must precede config/admission"));
    assert_eq!(code,0);
}

#[test]
fn stopped_observation_is_status_only_readonly_flag_and_help_stays_early(){
    let help=run(&["runtime-status","--stopped-observation","--binding","/nonexistent/fixture","--help"]);assert!(help.status.success());assert!(String::from_utf8(help.stdout).unwrap().contains("--stopped-observation"));
    for args in [vec!["runtime-status","--stopped-observation"],vec!["runtime-status","--binding","/fixture/binding","--stopped-observation","--stopped-observation"],vec!["runtime-drain","--binding","/fixture/binding","--stopped-observation"],vec!["runtime-host","--binding","/fixture/binding","--stopped-observation"]]{let output=run(&args);assert_eq!(output.status.code(),Some(2));let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();assert_eq!(value["ok"],false);}
}

#[test]
fn business_and_management_help_explain_intent_and_failure_without_opening_a_runtime() {
    for (args, fragments) in [
        (vec!["--help"], vec!["Usage: taskctl", "create -> generate -> start", "unconfirmed"]),
        (vec!["start", "--help"], vec!["--config JSON", "use watch, not start"]),
        (vec!["delete", "--help"], vec!["--expected-task-id", "archived, not physically purged"]),
        (vec!["runtime-drain", "--help"], vec!["--generation EXACT", "Accepted operations finish"]),
        (vec!["runtime-plan", "--help"], vec!["--metadata-only", "Does not write or activate enrollment"]),
        (vec!["runtime-host", "--help"], vec!["Internal launchd entry point"]),
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        for fragment in fragments { assert!(help.contains(fragment), "{help}"); }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn enrolled_runtime_status_reports_missing_and_refused_socket_without_fallback() {
    use quantnight_task_core::runtime::{identity, RuntimeSelection};
    use std::{fs, os::unix::{fs::PermissionsExt, net::UnixListener}, path::Path};

    let root = Path::new("/private/tmp").join(format!("qnr-{}", &identity::nonce().unwrap()[..8]));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    let binding_path = root.join("binding.json");
    let frontend = root.join("frontend");
    let backend = root.join("backend");
    let home = root.clone(); // Keep the Unix socket pathname below the macOS limit.
    fs::create_dir_all(frontend.join("crates/target/debug")).unwrap();
    let write = |path: &Path, value: serde_json::Value| {
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    };
    for checkout in [&frontend, &backend] {
        fs::create_dir_all(checkout.join(".quantnight")).unwrap();
        write(&checkout.join(".quantnight/taskctl-selection.json"), serde_json::json!({
            "schema_version": 1, "mode": "dev", "binding": binding_path
        }));
    }
    write(&binding_path, serde_json::json!({
        "schema_version": 2, "frontend_root": frontend, "backend_root": backend,
        "executable": frontend.join("unused-gui"), "target": "aarch64-apple-darwin", "profile": "debug",
        "runtime": {
            "executable": frontend.join("crates/target/debug/taskctl"), "protocol_version": 1,
            "configuration": {"kind": "backend_root", "path": backend}
        }
    }));
    let child = || {
        Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .args(["runtime-status", "--binding"]).arg(&binding_path)
            .env_clear().env("HOME", &home).env("PATH", "/nonexistent-taskctl-fixture")
            .output().unwrap()
    };
    let assert_failure = |out: Output, class: &str, stage: &str| {
        assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["transport"]["class"], class, "{value}");
        assert_eq!(value["transport"]["stage"], stage);
        assert_eq!(value["transport"]["execution"], "not_sent");
    };
    assert_failure(child(), "runtime_socket", "connect");
    let selected = RuntimeSelection::from_binding(&binding_path).unwrap();
    // Use the original planner to derive the slot in the child's private HOME,
    // avoiding any path copy/guess or writes to the user's enrollment.
    let plan = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(["runtime-plan", "--metadata-only", "--binding"]).arg(&binding_path)
        .env_clear().env("HOME", &home)
        .output().unwrap();
    assert!(plan.status.success(), "{:?}", plan.stderr);
    let plan: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(plan["selection"]["binding_revision"], selected.binding_revision);
    let socket = std::path::PathBuf::from(plan["paths"]["socket"].as_str().unwrap());
    assert!(socket.starts_with(&home), "test must never touch the real runtime slot");
    let dir = socket.parent().unwrap();
    fs::create_dir_all(dir).unwrap();
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    drop(listener); // Native pathname exists, but no process is listening.
    assert_failure(child(), "connect", "connect");
}
