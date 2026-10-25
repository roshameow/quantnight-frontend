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
