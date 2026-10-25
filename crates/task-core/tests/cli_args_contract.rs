use quantnight_task_core::cli_args::{
    help_text, parse_args, CliInvocation, CliParse, CliValue, CLI_HELP_EXIT_CODE,
    CLI_USAGE_EXIT_CODE,
};

fn parse(args: &[&str]) -> Result<CliParse, quantnight_task_core::cli_args::CliUsageError> {
    parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}
fn command(args: &[&str]) -> CliInvocation {
    match parse(args).unwrap() {
        CliParse::Command(v) => v,
        other => panic!("expected invocation, got {other:?}"),
    }
}

#[test]
fn all_sixteen_commands_and_same_schema_help() {
    let names = [
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
    ];
    let root = help_text(None).unwrap();
    for name in names {
        assert!(root.contains(name));
        assert_eq!(
            parse(&[name, "--help"]).unwrap(),
            CliParse::Help {
                command: Some(name.into())
            }
        );
        assert!(help_text(Some(name)).unwrap().contains("--name") || name == "list");
    }
    assert_eq!(CLI_HELP_EXIT_CODE, 0);
    assert!(help_text(Some("not-a-command")).is_err());
}

#[test]
fn defaults_are_parser_defaults_not_handler_defaults() {
    let c = command(&["create", "--name", "n", "--templatefile", "t"]);
    assert_eq!(c.options["type"].as_str(), Some("regular"));
    assert_eq!(c.options["auth_profile"].as_str(), Some("user1"));
    assert_eq!(c.options["remote"].as_bool(), Some(false));
    for verb in ["start", "watch"] {
        let c = command(&[verb, "--name", "n"]);
        assert_eq!(c.options["watch_timeout"], CliValue::Null);
        assert_eq!(c.options["review_seconds"], CliValue::Null);
        assert_eq!(c.options["notify_session_id"], CliValue::Null);
        assert_eq!(c.options["no_notify"].as_bool(), Some(false));
    }
    for verb in ["generate", "sync"] {
        assert_eq!(
            command(&[verb, "--name", "n"]).options["timeout"].as_i64(),
            Some(900)
        );
    }
    let r = command(&["results", "--name", "n"]);
    assert_eq!(r.options["limit"].as_i64(), Some(20));
    assert_eq!(r.options["offset"].as_i64(), Some(0));
}

#[test]
fn config_is_opaque_business_payload_not_app_path() {
    let c = command(&[
        "start",
        "--name",
        "",
        "--config={not json",
        "--expected-task-id=not-objectid",
    ]);
    assert_eq!(c.name.as_deref(), Some(""));
    assert_eq!(c.options["config"].as_str(), Some("{not json"));
    assert_eq!(c.options["expected_task_id"].as_str(), Some("not-objectid"));
    assert_eq!(
        parse(&["--config", "some-path", "start"])
            .unwrap_err()
            .exit_code,
        2
    );
    assert!(parse(&["status", "--name", "n", "--config", "{}"]).is_err());
}

#[test]
fn remote_three_states_and_last_option_wins() {
    for verb in ["start", "status"] {
        assert_eq!(
            command(&[verb, "--name", "n"]).options["remote"],
            CliValue::Null
        );
        assert_eq!(
            command(&[verb, "--name", "n", "--remote", "--no-remote"]).options["remote"].as_bool(),
            Some(false)
        );
        assert_eq!(
            command(&[verb, "--name", "n", "--no-remote", "--remote"]).options["remote"].as_bool(),
            Some(true)
        );
    }
    assert!(parse(&["list", "--no-remote"]).is_err());
    assert!(parse(&["start", "--name", "n", "--remote=false"]).is_err());
}

#[test]
fn notification_mutual_exclusion_and_same_target_overwrite() {
    for verb in ["start", "watch"] {
        assert!(parse(&[
            verb,
            "--name",
            "n",
            "--notify-task-id",
            "worker",
            "--no-notify"
        ])
        .is_err());
        assert!(parse(&[
            verb,
            "--name",
            "n",
            "--notify-session-id",
            "main",
            "--notify-task-id",
            "worker"
        ])
        .is_err());
        assert_eq!(
            command(&[
                verb,
                "--name",
                "n",
                "--notify-task-id",
                "a",
                "--notify-task-id=b"
            ])
            .options["notify_task_id"]
                .as_str(),
            Some("b")
        );
        assert_eq!(
            command(&[verb, "--name", "n", "--no-notify", "--no-notify"]).options["no_notify"]
                .as_bool(),
            Some(true)
        );
    }
}

#[test]
fn explicit_identity_extensions_do_not_remove_old_arguments() {
    for verb in ["create", "generate", "sync", "delete"] {
        let mut args = vec![verb, "--name", "n", "--expected-task-id=id"];
        if verb == "create" {
            args.extend(["--templatefile", "t"]);
        }
        assert_eq!(
            command(&args).options["expected_task_id"].as_str(),
            Some("id")
        );
    }
    for verb in ["start", "pause"] {
        let c = command(&[
            verb,
            "--name",
            "n",
            "--expected-task-id",
            "id",
            "--expected-launch-attempt-id",
            "launch",
        ]);
        assert_eq!(
            c.options["expected_launch_attempt_id"].as_str(),
            Some("launch")
        );
    }
    assert!(parse(&["results", "--name", "n", "--expected-task-id=id"]).is_err());
}

#[test]
fn integer_precision_and_business_ranges_remain_separate() {
    let c = command(&[
        "results",
        "--name",
        "n",
        "--limit=9223372036854775808",
        "--offset=-0",
    ]);
    assert_eq!(
        c.options["limit"],
        CliValue::Integer("9223372036854775808".into())
    );
    assert_eq!(c.options["limit"].as_i64(), None);
    assert_eq!(c.options["offset"].as_i64(), Some(0));
    assert_eq!(
        command(&["generate", "--name", "n", "--timeout= +٠١_٢ "]).options["timeout"].as_i64(),
        Some(12)
    );
    assert_eq!(
        command(&["results", "--name", "n", "--limit=-100"]).options["limit"].as_i64(),
        Some(-100)
    );
    assert!(parse(&["results", "--name", "n", "--limit=1.0"]).is_err());
}

#[test]
fn floats_keep_nonfinite_values_for_original_business_validation() {
    let c = command(&[
        "priority-prune",
        "--name",
        "n",
        "--sharpe=nan",
        "--fitness=-inf",
        "--confirm",
        "wrong-name",
    ]);
    assert!(c.options["sharpe"].as_f64().unwrap().is_nan());
    assert_eq!(c.options["fitness"].as_f64(), Some(f64::NEG_INFINITY));
    assert!(parse(&[
        "priority-prune",
        "--name",
        "n",
        "--sharpe",
        "-1e2",
        "--fitness",
        "0",
        "--confirm",
        "n"
    ])
    .is_err());
    assert_eq!(
        command(&[
            "priority-prune",
            "--name",
            "n",
            "--sharpe=-1e2",
            "--fitness",
            "0",
            "--confirm",
            "n"
        ])
        .options["sharpe"]
            .as_f64(),
        Some(-100.0)
    );
}

#[test]
fn help_order_ambiguity_unknown_and_required_match_argparse() {
    assert_eq!(
        parse(&["--unknown", "--help"]).unwrap(),
        CliParse::Help { command: None }
    );
    assert_eq!(
        parse(&["start", "--help", "--name"]).unwrap(),
        CliParse::Help {
            command: Some("start".into())
        }
    );
    assert!(parse(&["start", "--name", "--help"]).is_err());
    assert!(parse(&["start", "--help", "--n"]).is_err());
    assert!(parse(&["list", "--"]).is_err());
    for args in [
        vec![],
        vec!["bogus"],
        vec!["create", "--name", "n"],
        vec!["list", "--unknown"],
    ] {
        let e = parse(&args).unwrap_err();
        assert_eq!(e.exit_code, CLI_USAGE_EXIT_CODE);
        assert!(!e.to_string().is_empty());
    }
}
