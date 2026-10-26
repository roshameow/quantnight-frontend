//! Additional behavior checks through the same actual public CLI dispatcher.
#![cfg(unix)]
#[path = "support/dispatch_fixture.rs"]
mod support;
use bson::{doc, Bson};
use quantnight_task_core::task_store::Host;
use serde_json::{json, Value};
use support::*;

#[tokio::test(flavor = "current_thread")]
async fn acknowledged_state_not_assumed_when_control_claim_reply_is_lost() {
    let f = Fixture::new().await;
    let name = "lost_claim";
    f.create_generate(name, "regular", false).await;
    let original = f.store.task(name);
    let rows = f.store.rows_at(Host::Local, name);
    f.store.data.lock().unwrap().fail_after_update = true;
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    let uncertain = f.store.task(name);
    assert_eq!(uncertain.get_str("status"), Ok("launch_unknown"));
    assert_eq!(uncertain.get("_id"), original.get("_id"));
    assert_eq!(
        f.count("regular"),
        0,
        "claim reply lost: do not execute launcher"
    );
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    assert_eq!(f.store.task(name), uncertain);
    assert_eq!(f.count("regular"), 0);
    assert_eq!(f.store.rows_at(Host::Local, name), rows);
    ok(&f.run(&["pause", "--name", name]).await);
    assert_eq!(f.store.task(name).get_str("status"), Ok("paused"));
    assert_eq!(f.count("regular"), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn conflict_saved_key_precedence_and_explicit_gui_value_are_not_interchangeable() {
    for kind in ["regular", "super", "priority"] {
        let f = Fixture::new().await;
        let name = "saved_precedence";
        f.create_generate(name, kind, false).await;
        f.store.alter(name,doc!{"startConfig":{"sentinel":"start","max_concurrent":2},"mission_config":{"sentinel":"mission","max_concurrent":3},"config":{"sentinel":"legacy","max_concurrent":4}});
        ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
        let wire = f.wire().into_iter().find(|v| v["argv"][0] == kind).unwrap();
        let request: Value = serde_json::from_str(wire["argv"][2].as_str().unwrap()).unwrap();
        assert_eq!(
            request["sentinel"],
            if kind == "priority" {
                "mission"
            } else {
                "start"
            }
        );
        ok(&f.run(&["pause", "--name", name]).await);
        // This is an explicit input equivalence check, NOT executing Vue/Tauri:
        // actual UI saved-config selection still belongs to its real SFC tests.
        let explicit=json!({"sentinel":"gui-explicit","max_concurrent":1,"extension":{"$numberInt":"literal","none":null,"flag":false}}).to_string();
        ok(&f
            .run(&[
                "start",
                "--name",
                name,
                "--config",
                &explicit,
                "--no-notify",
            ])
            .await);
        let wire = f
            .wire()
            .into_iter()
            .filter(|v| v["argv"][0] == kind)
            .last()
            .unwrap();
        let request: Value = serde_json::from_str(wire["argv"][2].as_str().unwrap()).unwrap();
        assert_eq!(request["sentinel"], "gui-explicit");
        assert_eq!(request["extension"]["$numberInt"], "literal");
        assert_eq!(
            f.store
                .task(name)
                .get_document("startConfig")
                .unwrap()
                .get_document("extension")
                .unwrap()
                .get("none"),
            Some(&Bson::Null)
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn generate_replace_and_expected_id_are_carried_through_actual_generation_process() {
    let f = Fixture::new().await;
    let name = "regenerate";
    f.create_generate(name, "super", false).await;
    let id = f.store.task(name).get_object_id("_id").unwrap().to_hex();
    let first = f
        .store
        .task(name)
        .get_document("generation")
        .unwrap()
        .get_str("id")
        .unwrap()
        .to_owned();
    ok(&f
        .run(&[
            "generate",
            "--name",
            name,
            "--replace",
            "--timeout",
            "23",
            "--expected-task-id",
            &id,
        ])
        .await);
    let last = f
        .wire()
        .into_iter()
        .filter(|v| v["argv"][0] == "run")
        .last()
        .unwrap();
    assert_eq!(
        last["argv"],
        json!([
            "run",
            "-m",
            "backend.mongodb.mongo_datum",
            "--template",
            "frozen",
            "--alpha_filename",
            name,
            "--auth_profile",
            "user1",
            "--alpha_type",
            "super",
            "--replace"
        ])
    );
    let current = f.store.task(name);
    assert_eq!(current.get_object_id("_id").unwrap().to_hex(), id);
    assert_ne!(
        current
            .get_document("generation")
            .unwrap()
            .get_str("id")
            .unwrap(),
        first
    );
    assert_eq!(
        current.get_document("generation").unwrap().get_str("phase"),
        Ok("complete")
    );
    assert_eq!(f.store.rows_at(Host::Local, name).len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn frozen_core_known_no_dispatch_failure_settles_only_original_attempt_then_allows_explicit_retry(
) {
    let f = Fixture::new().await;
    let name = "no_dispatch";
    f.create_generate(name, "regular", false).await;
    let rows = f.store.rows_at(Host::Local, name);
    let id = f.store.task(name).get_object_id("_id").unwrap();
    f.faults(json!({"ownerFail":"validate-execution-binding"}));
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    let failed_task = f.store.task(name);
    assert_eq!(failed_task.get_str("status"), Ok("launch_unknown"));
    assert_eq!(
        failed_task.get("launchSettledAttemptId"),
        failed_task.get("launchAttemptId")
    );
    assert_eq!(f.count("regular"), 0);
    let event = failed_task
        .get_array("events")
        .unwrap()
        .iter()
        .filter_map(|v| v.as_document())
        .find(|e| e.get_str("command") == Ok("start-not-dispatched"))
        .unwrap();
    assert_eq!(
        event
            .get_document("detail")
            .unwrap()
            .get_bool("launcherInvoked"),
        Ok(false)
    );
    f.faults(json!({}));
    let result = f.run(&["start", "--name", name, "--no-notify"]).await;
    ok(&result);
    assert_eq!(result.value["operationSettled"], true);
    let retried = f.store.task(name);
    assert_eq!(retried.get_object_id("_id").unwrap(), id);
    assert_ne!(
        retried.get("launchAttemptId"),
        failed_task.get("launchAttemptId")
    );
    assert_eq!(f.count("regular"), 1);
    assert_eq!(f.store.rows_at(Host::Local, name), rows);
}
