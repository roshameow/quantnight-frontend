//! Private owner-export regression: synthetic in-memory data only. No real
//! config, credentials, clients, network, or temporary credential files.
use quantnight_task_core::{config::AppConfig, task_store::MongoTaskStore};
use serde_json::json;

#[test]
fn minimal_original_owner_export_deserializes_without_gui_config_or_alpha_fallback() {
    let value = json!({
        "env":{"working_dir":"/synthetic/backend/root"},
        "python":{"scripts":{}}, "bash":{"scripts":{}},
        "mongodb":{"local_uri":"mongodb://synthetic.invalid", "remote_uri":"",
            "databases":{"mission":"simulation_mission", "simulation":"simulation_db", "alpha":""}}
    });
    let config: AppConfig = serde_json::from_value(value).unwrap();
    assert_eq!(config.env.working_dir, "/synthetic/backend/root");
    assert!(config.python.scripts.is_empty());
    assert!(config.bash.scripts.is_empty());
    assert!(config.mongodb.remote_uri.is_empty());
    assert!(config.mongodb.databases.alpha.is_empty());
    assert!(config.mongodb.databases.dataset.is_none());
    // Namespace validation does not invent a canonical alpha database. The
    // native results path still asks its original namespace owner separately.
    MongoTaskStore::from_config(&config).unwrap();
}

#[cfg(unix)]
mod pipe_tests {
    use quantnight_task_core::config::{run_process_private, ProcessFailureKind};
    use std::time::{Duration, Instant};
    use tokio::process::Command;

    const STDOUT_SECRET: &str = "SYNTHETIC_OWNER_SECRET";
    const STDERR_SECRET: &str = "SYNTHETIC_STDERR_SECRET";

    #[test]
    fn private_output_never_reaches_parent_stdio_on_success_or_failures() {
        // A child test process gives a real stdout/stderr boundary: --nocapture
        // ensures an accidental eprintln in the process owner cannot hide in
        // Rust's test capture. Only inert synthetic strings are involved.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "pipe_tests::private_output_probe",
                "--nocapture",
            ])
            .env("QUANTNIGHT_PRIVATE_OUTPUT_PROBE", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            output.status.success(),
            "private probe failed: {stdout}\n{stderr}"
        );
        assert!(stdout.contains("PRIVATE_PROBE_OK"));
        for token in [
            STDOUT_SECRET,
            STDERR_SECRET,
            "[bash stdout]",
            "[bash stderr]",
        ] {
            assert!(
                !stdout.contains(token),
                "private content reached parent stdout"
            );
            assert!(
                !stderr.contains(token),
                "private content reached parent stderr"
            );
        }
    }

    #[test]
    #[ignore = "executed by the parent test with a real stdio boundary"]
    fn private_output_probe() {
        assert_eq!(
            std::env::var("QUANTNIGHT_PRIVATE_OUTPUT_PROBE").unwrap(),
            "1"
        );
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let prefix =
                format!("printf '%s\\n' '{STDOUT_SECRET}'; printf '%s\\n' '{STDERR_SECRET}' >&2;");
            let mut command = Command::new("/bin/sh");
            command.args(["-c", &prefix]);
            let (out, err) = run_process_private(command, Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(out, STDOUT_SECRET);
            assert_eq!(err, STDERR_SECRET);

            for (script, kind, exit_code) in [
                (
                    format!("{prefix} exit 23"),
                    ProcessFailureKind::Exit,
                    Some(23),
                ),
                (
                    format!("{prefix} exec /bin/sleep 30"),
                    ProcessFailureKind::Timeout,
                    None,
                ),
                (
                    format!("{prefix} printf '\\377\\n'"),
                    ProcessFailureKind::Stdout,
                    None,
                ),
                (
                    format!("{prefix} printf '\\377\\n' >&2"),
                    ProcessFailureKind::Stderr,
                    None,
                ),
            ] {
                let mut command = Command::new("/bin/sh");
                command.args(["-c", &script]);
                let started = Instant::now();
                let error = run_process_private(command, Duration::from_millis(80))
                    .await
                    .unwrap_err();
                assert_eq!(error.kind, kind);
                assert_eq!(error.exit_code, exit_code);
                assert!(!error.cause.is_empty());
                assert!(error.stdout().is_empty());
                assert!(error.stderr().is_empty());
                for formatted in [error.to_string(), format!("{error:?}"), error.cause.clone()] {
                    assert!(!formatted.contains(STDOUT_SECRET));
                    assert!(!formatted.contains(STDERR_SECRET));
                }
                if kind == ProcessFailureKind::Timeout {
                    assert!(error.cause.contains("80 ms"));
                    assert!(started.elapsed() < Duration::from_secs(2));
                }
            }
            let error = run_process_private(
                Command::new("/definitely/missing/synthetic-owner"),
                Duration::from_millis(80),
            )
            .await
            .unwrap_err();
            assert_eq!(error.kind, ProcessFailureKind::Spawn);
            assert!(error.stdout().is_empty() && error.stderr().is_empty());
        });
        println!("PRIVATE_PROBE_OK");
    }
}
