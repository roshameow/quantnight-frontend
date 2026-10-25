use quantnight_task_core::{
    cli_args::{parse_args, CliParse, CliValue},
    inspection::EvidenceAction,
    start::NotificationTarget,
};
use quantnight_taskctl::{parse, to_request, Entry, Request};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
fn invocation(args: &[&str]) -> quantnight_task_core::cli_args::CliInvocation {
    match parse_args(&strings(args)).unwrap() {
        CliParse::Command(i) => i,
        other => panic!("{other:?}"),
    }
}
fn request(args: &[&str]) -> Request {
    to_request(&invocation(args)).unwrap()
}

#[test]
fn all_sixteen_default_routes() {
    match request(&["list"]) {
        Request::List(r) => assert!(!r.include_deactive && !r.remote),
        _ => panic!(),
    }
    for (command, action) in [
        ("verify-registration", EvidenceAction::VerifyRegistration),
        ("archive-research", EvidenceAction::ArchiveResearch),
    ] {
        let r = match request(&[command, "--name", "task"]) {
            Request::VerifyRegistration(r) | Request::ArchiveResearch(r) => r,
            _ => panic!(),
        };
        assert_eq!(r.action, action);
        assert_eq!(r.name, "task");
        assert!(r.expected_task_id.is_none() && r.expected_remote.is_none());
    }
    match request(&["create", "--name", "task", "--templatefile", "frozen"]) {
        Request::Create(r) => {
            assert_eq!(
                (
                    r.name.as_str(),
                    r.templatefile.as_str(),
                    r.task_type.as_str(),
                    r.auth_profile.as_str()
                ),
                ("task", "frozen", "regular", "user1")
            );
            assert!(!r.target_remote && !r.force);
            assert!(r.expected_task_id.is_none());
            assert!(r.template.is_empty() && r.mission_config.is_none());
        }
        _ => panic!(),
    }
    match request(&["generate", "--name", "task"]) {
        Request::Generate(r) => {
            assert_eq!(r.name, "task");
            assert_eq!(r.timeout_seconds, 900);
            assert!(!r.replace);
            assert!(r.expected_task_id.is_none());
        }
        _ => panic!(),
    }
    match request(&["sync", "--name", "task"]) {
        Request::Sync(r) => {
            assert_eq!(r.name, "task");
            assert!(r.expected_task_id.is_none());
        }
        _ => panic!(),
    }
    match request(&["start", "--name", "task"]) {
        Request::Start(r) => {
            assert_eq!(r.name, "task");
            assert!(
                r.config_json.is_none()
                    && r.remote.is_none()
                    && r.expected_task_id.is_none()
                    && r.expected_launch_attempt_id.is_none()
            );
            let n = r.notification;
            assert!(
                n.target.is_none()
                    && !n.no_notify
                    && n.item_key.is_none()
                    && n.review_seconds.is_none()
                    && n.watch_timeout.is_none()
            );
        }
        _ => panic!(),
    }
    match request(&["observe-accepted", "--name", "task"]) {
        Request::ObserveAccepted(r) => {
            assert_eq!(r.name, "task");
            assert!(r.expected_task_id.is_none());
        }
        _ => panic!(),
    }
    match request(&["watch", "--name", "task"]) {
        Request::Watch(r) => {
            assert_eq!(r.name, "task");
            assert!(!r.replace_watcher);
            let n = r.notification;
            assert!(
                n.target.is_none()
                    && !n.no_notify
                    && n.item_key.is_none()
                    && n.review_seconds.is_none()
                    && n.watch_timeout.is_none()
            );
        }
        _ => panic!(),
    }
    match request(&["status", "--name", "task"]) {
        Request::Status(r) => {
            assert_eq!(r.name, "task");
            assert!(r.remote.is_none() && !r.inspect_items);
        }
        _ => panic!(),
    }
    match request(&["pause", "--name", "task"]) {
        Request::Pause(r) => {
            assert_eq!(r.name, "task");
            assert!(r.expected_task_id.is_none() && r.expected_launch_attempt_id.is_none());
        }
        _ => panic!(),
    }
    match request(&["resume", "--name", "task"]) {
        Request::Resume(r) => {
            assert_eq!(r.name, "task");
            assert!(!r.include_timeout && !r.recover_processing);
            assert!(r.expected_task_id.is_none());
        }
        _ => panic!(),
    }
    match request(&[
        "priority-prune",
        "--name",
        "task",
        "--confirm",
        "task",
        "--sharpe",
        "1.2",
        "--fitness",
        "0.7",
    ]) {
        Request::PriorityPrune(r) => {
            assert_eq!(r.name, "task");
            assert_eq!(r.confirm, "task");
            assert_eq!(r.sharpe, 1.2);
            assert_eq!(r.fitness, 0.7);
            assert!(!r.dry_run);
        }
        _ => panic!(),
    }
    match request(&["delete", "--name", "task"]) {
        Request::Delete(r) => {
            assert_eq!(r.name, "task");
            assert!(r.confirm.is_none() && r.expected_task_id.is_none());
        }
        _ => panic!(),
    }
    match request(&["results", "--name", "task"]) {
        Request::Results(r) => {
            assert_eq!(r.name, "task");
            assert_eq!((r.limit, r.offset), (20, 0));
            assert!(r.output.is_none());
        }
        _ => panic!(),
    }
    match request(&["collect-results", "--name", "task"]) {
        Request::CollectResults(r) => {
            assert_eq!(r.name, "task");
            assert!(r.output.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn explicit_identity_creation_and_lifecycle_flags() {
    match request(&["list", "--remote", "--include-deactive"]) {
        Request::List(r) => assert!(r.remote && r.include_deactive),
        _ => panic!(),
    }
    for (cmd, remote, expected) in [
        ("verify-registration", "false", false),
        ("archive-research", "true", true),
    ] {
        let r = match request(&[
            cmd,
            "--name",
            "任务",
            "--expected-task-id",
            "exact",
            "--expected-remote",
            remote,
        ]) {
            Request::VerifyRegistration(r) | Request::ArchiveResearch(r) => r,
            _ => panic!(),
        };
        assert_eq!(r.name, "任务");
        assert_eq!(r.expected_task_id.as_deref(), Some("exact"));
        assert_eq!(r.expected_remote, Some(expected));
    }
    for kind in ["super", "priority"] {
        match request(&[
            "create",
            "--name",
            "task",
            "--templatefile",
            "frozen",
            "--type",
            kind,
            "--auth-profile",
            "profile",
            "--remote",
            "--force",
            "--expected-task-id",
            "id",
        ]) {
            Request::Create(r) => {
                assert_eq!(r.task_type, kind);
                assert_eq!(r.auth_profile, "profile");
                assert!(r.target_remote && r.force);
                assert_eq!(r.expected_task_id.as_deref(), Some("id"));
            }
            _ => panic!(),
        }
    }
    match request(&[
        "generate",
        "--name",
        "task",
        "--replace",
        "--timeout",
        "42",
        "--expected-task-id",
        "id",
    ]) {
        Request::Generate(r) => {
            assert!(r.replace);
            assert_eq!(r.timeout_seconds, 42);
            assert_eq!(r.expected_task_id.as_deref(), Some("id"));
        }
        _ => panic!(),
    }
    match request(&[
        "sync",
        "--name",
        "task",
        "--timeout",
        "-999999999999999999999999999",
        "--expected-task-id",
        "id",
    ]) {
        Request::Sync(r) => assert_eq!(r.expected_task_id.as_deref(), Some("id")),
        _ => panic!(),
    }
    match request(&[
        "pause",
        "--name",
        "task",
        "--expected-task-id",
        "id",
        "--expected-launch-attempt-id",
        "launch",
    ]) {
        Request::Pause(r) => {
            assert_eq!(r.expected_task_id.as_deref(), Some("id"));
            assert_eq!(r.expected_launch_attempt_id.as_deref(), Some("launch"));
        }
        _ => panic!(),
    }
    match request(&[
        "observe-accepted",
        "--name",
        "task",
        "--expected-task-id",
        "id",
    ]) {
        Request::ObserveAccepted(r) => assert_eq!(r.expected_task_id.as_deref(), Some("id")),
        _ => panic!(),
    }
    match request(&[
        "resume",
        "--name",
        "task",
        "--include-timeout",
        "--recover-processing",
    ]) {
        Request::Resume(r) => assert!(r.include_timeout && r.recover_processing),
        _ => panic!(),
    }
    match request(&[
        "delete",
        "--name",
        "task",
        "--confirm",
        "different",
        "--force",
        "--expected-task-id",
        "id",
    ]) {
        Request::Delete(r) => {
            assert_eq!(r.confirm.as_deref(), Some("different"));
            assert_eq!(r.expected_task_id.as_deref(), Some("id"));
        }
        _ => panic!(),
    }
}

#[test]
fn start_preserves_literal_json_and_notification_fields() {
    let json = " {\"label\":\"--app-config\", \"$oid\":\"literal\", \"x\":1, \"x\":2} ";
    match request(&[
        "start",
        "--name",
        "task",
        "--config",
        json,
        "--remote",
        "--expected-task-id",
        "id",
        "--expected-launch-attempt-id",
        "launch",
        "--notify-session-id",
        "session",
        "--notify-item-key",
        "mission:task",
        "--review-seconds",
        "1801",
        "--watch-timeout",
        "3601",
    ]) {
        Request::Start(r) => {
            assert_eq!(r.config_json.as_deref(), Some(json));
            assert_eq!(r.remote, Some(true));
            assert_eq!(r.expected_task_id.as_deref(), Some("id"));
            assert_eq!(r.expected_launch_attempt_id.as_deref(), Some("launch"));
            let n = r.notification;
            assert!(matches!(n.target,Some(NotificationTarget::Session(s)) if s=="session"));
            assert_eq!(n.item_key.as_deref(), Some("mission:task"));
            assert_eq!(n.review_seconds, Some(1801));
            assert_eq!(n.watch_timeout, Some(3601));
            assert!(!n.no_notify);
        }
        _ => panic!(),
    }
    for cmd in ["start", "watch"] {
        let r = request(&[cmd, "--name", "task", "--notify-task-id", "worker"]);
        let n = match r {
            Request::Start(r) => r.notification,
            Request::Watch(r) => r.notification,
            _ => panic!(),
        };
        assert!(matches!(n.target,Some(NotificationTarget::Worker(s)) if s=="worker"));
    }
    match request(&[
        "watch",
        "--name",
        "task",
        "--replace-watcher",
        "--no-notify",
        "--notify-item-key",
        "item",
        "--review-seconds",
        "-1",
        "--watch-timeout",
        "0",
    ]) {
        Request::Watch(r) => {
            assert!(r.replace_watcher && r.notification.no_notify);
            assert!(r.notification.target.is_none());
            assert_eq!(r.notification.item_key.as_deref(), Some("item"));
            assert_eq!(r.notification.review_seconds, Some(-1));
            assert_eq!(r.notification.watch_timeout, Some(0));
        }
        _ => panic!(),
    }
    // Invalid JSON is a business/core concern, not CLI syntax/config discovery.
    match request(&[
        "start",
        "--name",
        "task",
        "--config",
        "{broken",
        "--no-remote",
        "--no-notify",
    ]) {
        Request::Start(r) => {
            assert_eq!(r.config_json.as_deref(), Some("{broken"));
            assert_eq!(r.remote, Some(false));
            assert!(r.notification.no_notify);
        }
        _ => panic!(),
    }
}

#[test]
fn inspection_paths_and_numeric_values_remain_core_owned() {
    for (flag, expected) in [("--remote", true), ("--no-remote", false)] {
        match request(&["status", "--name", "task", flag, "--inspect-items"]) {
            Request::Status(r) => {
                assert_eq!(r.remote, Some(expected));
                assert!(r.inspect_items);
            }
            _ => panic!(),
        }
    }
    match request(&[
        "results",
        "--name",
        "task",
        "--limit",
        "201",
        "--offset",
        "-1",
        "--output",
        "relative/结果.json",
    ]) {
        Request::Results(r) => {
            assert_eq!((r.limit, r.offset), (201, -1));
            assert_eq!(r.output.unwrap().to_str(), Some("relative/结果.json"));
        }
        _ => panic!(),
    }
    match request(&[
        "collect-results",
        "--name",
        "task",
        "--output",
        "/explicit/out.json",
    ]) {
        Request::CollectResults(r) => {
            assert_eq!(r.output.unwrap().to_str(), Some("/explicit/out.json"))
        }
        _ => panic!(),
    }
    match request(&[
        "priority-prune",
        "--name",
        "task",
        "--confirm",
        "not-task",
        "--sharpe",
        "NaN",
        "--fitness",
        "inf",
        "--dry-run",
    ]) {
        Request::PriorityPrune(r) => {
            assert!(r.sharpe.is_nan() && r.fitness.is_infinite() && r.dry_run);
            assert_eq!(r.confirm, "not-task");
        }
        _ => panic!(),
    }
}

#[test]
fn checked_representation_rejects_without_wrapping_or_io() {
    for args in [
        vec!["generate", "--name", "task", "--timeout", "-1"],
        vec![
            "generate",
            "--name",
            "task",
            "--timeout",
            "18446744073709551616",
        ],
        vec![
            "results",
            "--name",
            "task",
            "--limit",
            "9223372036854775808",
        ],
        vec![
            "results",
            "--name",
            "task",
            "--offset",
            "-9223372036854775809",
        ],
        vec![
            "watch",
            "--name",
            "task",
            "--watch-timeout",
            "999999999999999999999",
        ],
        vec![
            "start",
            "--name",
            "task",
            "--review-seconds",
            "999999999999999999999",
        ],
    ] {
        let i = invocation(&args); // Shared argparse-compatible parse still succeeds.
        assert!(to_request(&i).unwrap_err().contains("shared core"));
    }
    match request(&["generate", "--name", "task", "--timeout", "0"]) {
        Request::Generate(r) => assert_eq!(r.timeout_seconds, 0),
        _ => panic!(),
    }
    match request(&[
        "results",
        "--name",
        "task",
        "--offset",
        "-9223372036854775808",
    ]) {
        Request::Results(r) => assert_eq!(r.offset, i64::MIN),
        _ => panic!(),
    }
    let mut i = invocation(&["start", "--name", "task"]);
    i.options
        .insert("remote".into(), CliValue::String("false".into()));
    assert!(to_request(&i).is_err());
    i.options.remove("remote");
    assert!(to_request(&i).is_err());
}

#[test]
fn global_selection_is_explicit_prefix_only_and_does_not_steal_business_values() {
    match parse(&strings(&[
        "--app-config",
        "a.toml",
        "--app-config=b.toml",
        "start",
        "--name",
        "task",
        "--config=--app-config=literal",
    ]))
    .unwrap()
    {
        Entry::Command {
            app_config,
            invocation,
            ..
        } => {
            assert_eq!(app_config.unwrap().to_str(), Some("b.toml"));
            assert_eq!(
                invocation.options["config"],
                CliValue::String("--app-config=literal".into())
            );
        }
        _ => panic!(),
    }
    match parse(&strings(&["list"])).unwrap() {
        Entry::Command { app_config, .. } => assert!(app_config.is_none()),
        _ => panic!(),
    }
    for args in [
        vec!["--app-config"],
        vec!["--app-config="],
        vec!["--app-config", "--help"],
        vec!["--app-conf=a"],
        vec!["list", "--app-config", "a.toml"],
    ] {
        assert_eq!(parse(&strings(&args)).unwrap_err().exit_code, 2);
    }
    for args in [
        vec!["--help"],
        vec!["--app-config", "/nonexistent-fixture.toml", "--help"],
        vec!["--app-config=a", "start", "--help"],
    ] {
        match parse(&strings(&args)).unwrap() {
            Entry::Help(h) => {
                assert!(h.contains("--app-config PATH") && h.contains("--config JSON"))
            }
            _ => panic!(),
        }
    }
    // Shared parser owns abbreviations, repeats and notification mutex.
    match request(&[
        "status",
        "--na=first",
        "--name=last",
        "--remote",
        "--no-remote",
    ]) {
        Request::Status(r) => {
            assert_eq!(r.name, "last");
            assert_eq!(r.remote, Some(false));
        }
        _ => panic!(),
    }
    assert!(parse(&strings(&[
        "watch",
        "--name",
        "task",
        "--notify-task-id",
        "worker",
        "--no-notify"
    ]))
    .is_err());
}
