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
