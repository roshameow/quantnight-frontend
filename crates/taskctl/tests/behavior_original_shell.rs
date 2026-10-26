//! X/O extension of the same real CLI dispatcher chain. Original start/check/
//! pause/config/source-registration functions run; only TaskStore/Mongo storage
//! and OS tmux/ps/ssh/uv interpreter launching are fake. No runner is executed.
#![cfg(unix)]
#[path = "support/dispatch_fixture.rs"]
mod support;
use bson::{doc, Bson};
use quantnight_task_core::task_store::Host;
use serde_json::{json, Value};
use std::fs;
use support::*;

fn tmux_calls(f: &Fixture, command: &str) -> usize {
    f.os_trace()
        .iter()
        .filter(|v| v["kind"] == "tmux" && v["argv"][2] == command)
        .count()
}
fn assert_original_owner_entry(f: &Fixture, module: &str) {
    assert!(
        f.owner_trace()
            .iter()
            .any(|v| v["argv"].as_array().unwrap().iter().any(|a| a == module)),
        "missing original {module}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn actual_dispatch_original_regular_shell_local_remote_start_pause_restart() {
    for remote in [false, true] {
        let mut f = Fixture::new().await;
        let name = "job";
        f.create_generate(name, "regular", remote).await;
        let id = f.store.task(name).get_object_id("_id").unwrap().to_hex();
        let before = f.store.rows_at(Host::from_remote(remote), name);
        f.install_original_owners(None).await;
        let full = config(name);
        ok(&f
            .run(&["start", "--name", name, "--config", &full, "--no-notify"])
            .await);
        let running = f.store.task(name);
        let context = running.get_document("tmuxContext").unwrap();
        assert_eq!(
            context.get_str("binary").unwrap(),
            f.root.join("native-tmux").to_str().unwrap()
        );
        assert_eq!(context.get_str("socketName"), Ok("default"));
        assert_eq!(context.get_str("taskId"), Ok(id.as_str()));
        assert_eq!(context.get_bool("remote"), Ok(remote));
        assert_eq!(context.get_str("sessionId"), Ok("$4"));
        assert_original_owner_entry(&f, "scripts.script_config");
        assert_original_owner_entry(&f, "scripts.source_task_registration");
        assert_original_owner_entry(&f, "backend.mongodb.mongo_config");
        assert_eq!(tmux_calls(&f, "new-session"), 1);
        assert!(f.os_trace().iter().any(|v| v["kind"] == "ps"));
        assert_eq!(f.os_trace().iter().any(|v| v["kind"] == "ssh"), remote);
        // Actual source identity injection ran before launch, not wire.py True.
        if !remote {
            let published: Value =
                serde_json::from_slice(&fs::read(f.root.join("tmp/tmp_config_job.json")).unwrap())
                    .unwrap();
            assert_eq!(published["source_task_id"], id);
            assert_eq!(published["source_remote"], false);
            assert_eq!(
                published["literal"],
                serde_json::from_str::<Value>(&full).unwrap()["literal"]
            );
        } else {
            let script = fs::read_to_string(f.root.join("tmp/run_job.sh")).unwrap();
            assert!(
                script.contains("--dist remote") && script.contains("scripts.alpha_simulate_param")
            );
        }
        ok(&f
            .run(&["pause", "--name", name, "--expected-task-id", &id])
            .await);
        let stopped = f.store.task(name);
        assert_eq!(stopped.get_str("status"), Ok("paused"));
        assert_eq!(
            stopped.get_str("stopConfirmationBasis"),
            Ok("runners_exited")
        );
        assert_eq!(tmux_calls(&f, "if-shell"), 1);
        assert!(!f.root.join("os_state.json").exists());
        assert_eq!(f.store.rows_at(Host::from_remote(remote), name), before);
        ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
        let next = f.store.task(name);
        assert_eq!(next.get_object_id("_id").unwrap().to_hex(), id);
        assert_ne!(next.get("launchAttemptId"), running.get("launchAttemptId"));
        assert_ne!(
            next.get_document("tmuxContext")
                .unwrap()
                .get("sessionCreatedAt"),
            context.get("sessionCreatedAt")
        );
        assert_eq!(next.get("stopConfirmedAt"), Some(&Bson::Null));
        assert_eq!(tmux_calls(&f, "new-session"), 2);
        assert_eq!(f.store.rows_at(Host::from_remote(remote), name), before);
        // No real tmux or runner exists. Fixture Drop removes synthetic state;
        // a third business stop is not needed to clean an OS consumer.
        assert!(!f.os_trace().iter().any(|v| v["kind"] == "forbidden-tmux"));
        assert!(!f.root.join("VIOLATIONS").exists());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn actual_original_checker_unknown_or_ssh_error_never_claims_or_launches() {
    for remote in [false, true] {
        let mut f = Fixture::new().await;
        let name = "job";
        f.create_generate(name, "regular", remote).await;
        f.install_original_owners(None).await;
        let original = f.store.task(name);
        for fault in [json!({"unknown":true}), json!({"denied":true})] {
            f.os_faults(fault);
            failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
            assert_eq!(f.store.task(name), original);
            assert_eq!(tmux_calls(&f, "new-session"), 0);
            assert_eq!(tmux_calls(&f, "if-shell"), 0);
            assert!(!f.root.join("tmp/tmp_config_job.json").exists());
        }
        if remote {
            f.os_faults(json!({"sshFail":true}));
            failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
            assert_eq!(f.store.task(name), original);
            assert_eq!(tmux_calls(&f, "new-session"), 0);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn actual_original_pause_wrong_birth_and_old_same_name_shell_never_signal_or_overwrite() {
    for remote in [false, true] {
        let mut f = Fixture::new().await;
        let name = "job";
        f.create_generate(name, "regular", remote).await;
        f.install_original_owners(None).await;
        ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
        let task = f.store.task(name);
        let mut context = task.get_document("tmuxContext").unwrap().clone();
        let born = context.get_i64("sessionCreatedAt").unwrap();
        context.insert("sessionCreatedAt", born + 1000);
        f.store.alter(name, doc! {"tmuxContext":context});
        let invalid = f.store.task(name);
        failed(&f.run(&["pause", "--name", name]).await);
        assert_eq!(f.store.task(name), invalid);
        assert_eq!(tmux_calls(&f, "if-shell"), 0);
        assert!(f.root.join("os_state.json").exists());
        f.store.alter(
            name,
            doc! {"tmuxContext":task.get_document("tmuxContext").unwrap()},
        );
        // Unknown same-name residue must still be refused. Core25 permits
        // independently OWNED residue cleanup; test that separately below.
        f.store.alter(name, doc! {"tmuxContext":Bson::Null});
        let before = f.store.task(name);
        for fault in [json!({"shell":true}), json!({"dead":true})] {
            f.os_faults(fault);
            failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
            assert_eq!(f.store.task(name), before);
            assert_eq!(tmux_calls(&f, "new-session"), 1);
            assert_eq!(tmux_calls(&f, "if-shell"), 0);
        }
        f.os_faults(json!({}));
        // Exact absent session is the independent V2 recovery basis, not proof
        // of historical PIDs. The original shell/checker determines this fact.
        fs::remove_file(f.root.join("os_state.json")).unwrap();
        ok(&f.run(&["pause", "--name", name]).await);
        assert_eq!(
            f.store.task(name).get_str("stopConfirmationBasis"),
            Ok("session_absent")
        );
        assert_eq!(tmux_calls(&f, "if-shell"), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn frozen_core_owned_shell_or_dead_residue_cleanup_preserves_receipts_before_new_launch() {
    for (remote, fault) in [(false, json!({"shell":true})), (true, json!({"dead":true}))] {
        let mut f = Fixture::new().await;
        let name = "job";
        f.create_generate(name, "regular", remote).await;
        let host = Host::from_remote(remote);
        let mut rows = f.store.rows_at(host, name);
        rows[0].extend(doc!{"status":"processing","acceptance":"ACCEPTED","progressUrl":"https://fixture.invalid/original","payloadHash":"keep-hash"});
        rows[1].extend(doc!{"status":"timeout_error","acceptance":"UNKNOWN","superSlotId":"keep-slot","submissionAttempts":[{"outcome":"UNKNOWN"}]});
        f.store.put_rows(host, name, rows.clone());
        f.install_original_owners(None).await;
        let started = f
            .run(&[
                "start",
                "--name",
                name,
                "--config",
                &config(name),
                "--no-notify",
            ])
            .await;
        ok(&started);
        assert_eq!(started.value["operationSettled"], true);
        let original = f.store.task(name);
        let id = original.get_object_id("_id").unwrap();
        assert_eq!(
            original.get("launchSettledAttemptId"),
            original.get("launchAttemptId")
        );
        f.os_faults(fault);
        let restarted = f.run(&["start", "--name", name, "--no-notify"]).await;
        ok(&restarted);
        assert_eq!(restarted.value["operationSettled"], true);
        let next = f.store.task(name);
        assert_eq!(next.get_object_id("_id").unwrap(), id);
        assert_ne!(next.get("launchAttemptId"), original.get("launchAttemptId"));
        assert_eq!(
            next.get("launchSettledAttemptId"),
            next.get("launchAttemptId")
        );
        assert_eq!(next.get_str("status"), Ok("running"));
        assert_eq!(
            tmux_calls(&f, "if-shell"),
            1,
            "one original owned-residue stop, no second stop"
        );
        assert_eq!(tmux_calls(&f, "new-session"), 2);
        assert_eq!(
            f.store.rows_at(host, name),
            rows,
            "accepted/UNKNOWN/hash/slot unchanged through auto-cleanup"
        );
        assert!(next
            .get_array("events")
            .unwrap()
            .iter()
            .any(|v| v.as_document().and_then(|d| d.get_str("command").ok()) == Some("pause")));
        assert!(!f.root.join("VIOLATIONS").exists());
    }
}
