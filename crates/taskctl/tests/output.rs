use quantnight_task_core::{
    cleanup_sync::TaskOutcome,
    cli_args::{parse_args, CliParse},
};
use quantnight_taskctl::output::{rejected, write_outcome};
use serde_json::{json, Value};

#[test]
fn preserves_priority_false_exit_zero_and_all_exit_codes() {
    for (ok, exit_code) in [(false, 0), (true, 0), (false, 1), (false, 7), (true, 9)] {
        let outcome = TaskOutcome {
            exit_code,
            value: json!({
                "schemaVersion":1,"command":"status","task":"fixture","ok":ok,
                "note":"newlines\nremain JSON escaped","checkpoint":{"valid":false}
            }),
        };
        let mut output = Vec::new();
        assert_eq!(write_outcome(&mut output, &outcome).unwrap(), exit_code);
        assert_eq!(output.iter().filter(|b| **b == b'\n').count(), 1);
        let values: Vec<Value> = serde_json::Deserializer::from_slice(&output)
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(values, vec![outcome.value]);
    }
}
#[test]
fn bootstrap_failure_is_versioned_and_list_task_is_null() {
    for args in [vec!["list"], vec!["status", "--name", "fixture"]] {
        let args = args.into_iter().map(String::from).collect::<Vec<_>>();
        let CliParse::Command(i) = parse_args(&args).unwrap() else {
            panic!()
        };
        let r = rejected(&i, "explicit fixture failure");
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.value["schemaVersion"], 1);
        assert_eq!(r.value["command"], i.command);
        assert_eq!(r.value["task"], json!(i.name));
        assert_eq!(r.value["ok"], false);
        assert_eq!(r.value["errorType"], "RuntimeError");
        assert_eq!(r.value["error"], "explicit fixture failure");
        assert!(r.value["at"].as_str().is_some());
    }
}
#[test]
fn stdout_failure_is_not_silently_claimed_as_delivery() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(write_outcome(
        &mut Broken,
        &TaskOutcome {
            exit_code: 0,
            value: json!({"ok":true})
        }
    )
    .is_err());
}
