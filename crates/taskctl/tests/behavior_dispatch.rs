//! Actual argv parser -> invocation -> dispatch -> TaskControl -> coordinators
//! -> OriginalOperationsRuntime -> captured synthetic process wire + MemoryStore.
//! No production dispatcher/Runtime mock. Professional process bodies are fake
//! (U coverage), not real Python owner, tmux, platform or cold executable proof.
#![cfg(unix)]
#[path = "support/dispatch_fixture.rs"]
mod support;
use bson::{doc, Bson, Document};
use quantnight_task_core::task_store::Host;
use quantnight_taskctl::{parse, Request};
use serde_json::{json, Value};
use std::fs;
use support::*;

fn request_payload(wire: &Value) -> Value {
    serde_json::from_str(
        wire["argv"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
fn owner_calls(f: &Fixture, flag: &str) -> Vec<Value> {
    f.wire()
        .into_iter()
        .filter(|v| v["argv"].as_array().unwrap().iter().any(|x| x == flag))
        .collect()
}
fn bytes(rows: &[Document]) -> Vec<Vec<u8>> {
    rows.iter().map(|r| bson::to_vec(r).unwrap()).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn parsed_defaults_and_real_list_failure_are_not_route_name_assertions() {
    let f = Fixture::new().await;
    match parsed(&["start", "--name", "defaults"]) {
        Request::Start(r) => {
            assert!(r.config_json.is_none() && r.remote.is_none() && r.expected_task_id.is_none());
            assert!(r.expected_remote.is_none() && !r.notification.no_notify);
        }
        _ => panic!("wrong request type"),
    }
    match parsed(&["generate", "--name", "defaults"]) {
        Request::Generate(r) => {
            assert_eq!(r.timeout_seconds, 900);
            assert!(!r.replace);
        }
        _ => panic!(),
    }
    match parsed(&["results", "--name", "defaults"]) {
        Request::Results(r) => {
            assert_eq!((r.limit, r.offset), (20, 0));
            assert!(r.output.is_none());
        }
        _ => panic!(),
    }
    for tokens in [
        vec!["start", "--name", "x", "--remote=true"],
        vec![
            "start",
            "--name",
            "x",
            "--notify-session-id",
            "main",
            "--no-notify",
        ],
        vec!["results", "--name", "x", "--limit", "not-an-int"],
    ] {
        assert!(parse(&tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err());
    }
    let listed = f.run(&["list"]).await;
    ok(&listed);
    assert_eq!(listed.value["tasks"], json!([]));
    assert!(listed.value["task"].is_null());
    f.store.data.lock().unwrap().fail = Some("list:Remote:true".into());
    failed(&f.run(&["list", "--remote", "--include-deactive"]).await);
    assert!(f.store.data.lock().unwrap().writes.is_empty());
    assert_eq!(
        f.wire().len(),
        1,
        "only binding proof, no professional operation process"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn parsed_regular_lifecycle_both_hosts_preserves_identity_receipts_and_exact_wire() {
    for remote in [false, true] {
        let f = Fixture::new().await;
        let name = "regular_lifecycle";
        f.create_generate(name, "regular", remote).await;
        let created = f.store.task(name);
        let id = created.get_object_id("_id").unwrap().to_hex();
        assert_eq!(created.get_bool("isRemote"), Ok(remote));
        assert_eq!(created.get_bool("targetRemote"), Ok(remote));
        assert_eq!(created.get_str("runId"), Ok("mission:regular_lifecycle"));
        assert_eq!(
            created.get_document("generation").unwrap().get_str("phase"),
            Ok("complete")
        );
        let host = Host::from_remote(remote);
        assert_eq!(f.store.rows_at(host, name).len(), 3);
        if remote {
            assert_eq!(
                bytes(&f.store.rows_at(Host::Local, name)),
                bytes(&f.store.rows_at(host, name))
            );
        }
        let gen = f
            .wire()
            .into_iter()
            .find(|v| v["argv"][0] == "run")
            .unwrap();
        assert_eq!(
            gen["argv"],
            json!([
                "run",
                "-m",
                "backend.mongodb.mongo_datum",
                "--template",
                "frozen",
                "--alpha_filename",
                name,
                "--auth_profile",
                "user1"
            ])
        );
        for action in ["verify-registration", "archive-research"] {
            let result = f
                .run(&[
                    action,
                    "--name",
                    name,
                    "--expected-task-id",
                    &id,
                    "--expected-remote",
                    if remote { "true" } else { "false" },
                ])
                .await;
            ok(&result);
            if action == "verify-registration" {
                assert_eq!(result.value["registeredCount"], 3);
                assert_eq!(result.value["simulationStarted"], false);
            } else {
                assert_eq!(result.value["platformFetched"], false);
            }
        }
        let cfg = config(name);
        ok(&f
            .run(&[
                "start",
                "--name",
                name,
                "--config",
                &cfg,
                "--expected-task-id",
                &id,
                "--no-notify",
            ])
            .await);
        let running = f.store.task(name);
        let launch = running.get_str("launchAttemptId").unwrap().to_string();
        assert_eq!(running.get_str("status"), Ok("running"));
        assert_eq!(running.get_object_id("_id").unwrap().to_hex(), id);
        let launch_wire = f
            .wire()
            .into_iter()
            .find(|v| v["argv"][0] == "regular")
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(launch_wire["argv"][2].as_str().unwrap()).unwrap(),
            serde_json::from_str::<Value>(&cfg).unwrap()
        );
        assert_eq!(
            launch_wire["argv"][3],
            if remote { "--remote" } else { "false" }
        );
        let binding: Value =
            serde_json::from_str(launch_wire["binding"].as_str().unwrap()).unwrap();
        assert_eq!(binding["taskId"], id);
        assert_eq!(binding["launchAttemptId"], launch);
        assert_eq!(binding["remote"], remote);
        assert_eq!(
            launch_wire["tmux"],
            json!({"TMUX":null,"TMUX_PANE":null,"TMUX_TMPDIR":null})
        );
        let watch = f
            .run(&[
                "watch",
                "--name",
                name,
                "--notify-task-id",
                "fixture-worker",
                "--notify-item-key",
                "mission:fixture",
                "--review-seconds",
                "61",
                "--watch-timeout",
                "120",
                "--replace-watcher",
            ])
            .await;
        ok(&watch);
        assert_eq!(watch.value["simulationStarted"], false);
        assert_eq!(watch.value["notification"]["requiresLease"], true);
        assert_eq!(
            f.store
                .task(name)
                .get_document("agentNotification")
                .unwrap()
                .get_i32("reviewSeconds"),
            Ok(61)
        );
        let attach = owner_calls(&f, "--native-control")
            .into_iter()
            .map(|v| request_payload(&v))
            .find(|v| v["action"] == "attach")
            .unwrap();
        assert_eq!(attach["replace"], true);
        assert_eq!(attach["remote"], remote);
        assert_eq!(attach["spec"]["timeoutSeconds"], 120);
        let status = f.run(&["status", "--name", name]).await;
        ok(&status);
        assert_eq!(status.value["execution"]["total"], 3);
        assert_eq!(status.value["unfinished"], 3);
        assert_eq!(status.value["terminal"], false);
        ok(&f
            .run(&[
                "pause",
                "--name",
                name,
                "--expected-task-id",
                &id,
                "--expected-launch-attempt-id",
                &launch,
            ])
            .await);
        assert_eq!(f.store.task(name).get_str("status"), Ok("paused"));
        let mut rows = f.store.rows_at(host, name);
        rows[0].extend(doc! {"status":"transient_error","_not_posted_":true});
        rows[1].extend(doc!{"status":"timeout_error","acceptance":"UNKNOWN","superSlotId":"retained-slot","payloadHash":"opaque-hash","submissionAttempts":[{"outcome":"UNKNOWN"}]});
        rows[2].extend(doc!{"status":"processing","acceptance":"ACCEPTED","progressUrl":"https://fixture.invalid/simulations/original","payloadHash":"retained-hash"});
        let protected = bytes(&rows[1..]);
        f.store.put_rows(host, name, rows);
        let resumed = f
            .run(&[
                "resume",
                "--name",
                name,
                "--include-timeout",
                "--recover-processing",
            ])
            .await;
        ok(&resumed);
        assert_eq!(resumed.value["resumed"], 1);
        assert_eq!(bytes(&f.store.rows_at(host, name)[1..]), protected);
        assert_eq!(f.count("regular"), 1, "resume never launches");
        ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
        assert_eq!(f.count("regular"), 2);
        let restarted = f.store.task(name);
        assert_eq!(restarted.get_object_id("_id").unwrap().to_hex(), id);
        assert_ne!(restarted.get_str("launchAttemptId").unwrap(), launch);
        assert_eq!(
            bytes(&f.store.rows_at(host, name)[1..]),
            protected,
            "start never clears accepted/unknown"
        );
        ok(&f.run(&["pause", "--name", name]).await);
        let before = bytes(&f.store.rows_at(host, name));
        let removed = f
            .run(&["delete", "--name", name, "--expected-task-id", &id])
            .await;
        ok(&removed);
        assert_eq!(removed.value["itemsPreserved"], true);
        assert_eq!(removed.value["platformRequestsCancelled"], false);
        assert_eq!(removed.value["capacityReleased"], false);
        assert_eq!(removed.value["unresolvedItemCount"], 2);
        let archive = format!("{name}__archive_{id}");
        assert_eq!(bytes(&f.store.rows_at(host, &archive)), before);
        assert_eq!(f.store.task(name).get_str("status"), Ok("deactive"));
        assert_eq!(f.run(&["list"]).await.value["tasks"], json!([]));
        assert_eq!(
            f.run(&["list", "--include-deactive"]).await.value["tasks"][0]["name"],
            name
        );
        let receipt: Value = serde_json::from_slice(
            &fs::read(removed.value["receiptPath"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["taskId"], id);
        assert_eq!(receipt["ok"], true);
        assert_eq!(f.count("regular"), 2, "archive cannot resimulate");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn parsed_super_and_priority_local_remote_keep_specialist_semantics() {
    for kind in ["super", "priority"] {
        for remote in [false, true] {
            let f = Fixture::new().await;
            let name = "typed_lifecycle";
            f.create_generate(name, kind, remote).await;
            let cfg = if kind == "super" {
                config(name)
            } else {
                json!({"refine_templates":[],"interval":17,"extension":{"$oid":"literal","null":null}}).to_string()
            };
            ok(&f
                .run(&["start", "--name", name, "--config", &cfg, "--no-notify"])
                .await);
            let running = f.store.task(name);
            assert_eq!(running.get_bool("isRemote"), Ok(remote));
            let launched = f.wire().into_iter().find(|v| v["argv"][0] == kind).unwrap();
            let sent: Value = serde_json::from_str(launched["argv"][2].as_str().unwrap()).unwrap();
            if kind == "super" {
                assert_eq!(sent["startup_binding"]["remote"], remote);
                assert_eq!(
                    sent["startup_binding"]["taskId"],
                    running.get_object_id("_id").unwrap().to_hex()
                );
                assert_eq!(sent["literal"]["$oid"], "opaque-not-bson");
            } else {
                assert_eq!(sent["interval"], 17);
                assert_eq!(sent["refine_templates"], json!([]));
                assert_eq!(sent["extension"]["$oid"], "literal");
            }
            ok(&f.run(&["pause", "--name", name]).await);
            if kind == "super" {
                let before = f.store.task(name);
                let rows = bytes(&f.store.rows_at(Host::from_remote(remote), name));
                let observe = f.run(&["observe-accepted", "--name", name]).await;
                ok(&observe);
                assert_eq!(observe.value["simulationStarted"], false);
                assert_eq!(observe.value["taskStatus"], "paused");
                let after = f.store.task(name);
                for key in [
                    "_id",
                    "isRemote",
                    "launchAttemptId",
                    "stopConfirmedAt",
                    "startConfig",
                ] {
                    assert_eq!(before.get(key), after.get(key));
                }
                assert_eq!(
                    bytes(&f.store.rows_at(Host::from_remote(remote), name)),
                    rows
                );
                f.faults(json!({"observationUnknown":1}));
                let uncertain = f.run(&["observe-accepted", "--name", name]).await;
                failed(&uncertain);
                assert!(f
                    .store
                    .task(name)
                    .get_document("acceptedObservation")
                    .unwrap()
                    .get_str("finishedAt")
                    .is_ok());
                assert_eq!(f.store.task(name).get_str("status"), Ok("paused"));
                f.faults(json!({}));
            } else {
                let status = f
                    .run(&["status", "--name", name, "--remote", "--inspect-items"])
                    .await;
                ok(&status);
                assert_eq!(status.value["remote"], true);
                assert_eq!(status.value["inspected"], true);
                let before = f.store.task(name);
                let dry = f
                    .run(&[
                        "priority-prune",
                        "--name",
                        name,
                        "--confirm",
                        name,
                        "--sharpe",
                        "1.25",
                        "--fitness",
                        "0.75",
                        "--dry-run",
                    ])
                    .await;
                ok(&dry);
                assert_eq!(dry.value["retired"], 0);
                assert_eq!(f.store.task(name), before);
                let actual = f
                    .run(&[
                        "priority-prune",
                        "--name",
                        name,
                        "--confirm",
                        name,
                        "--sharpe",
                        "1.25",
                        "--fitness",
                        "0.75",
                    ])
                    .await;
                ok(&actual);
                assert_eq!(actual.value["retired"], 2);
                assert_eq!(actual.value["keptStates"]["UNKNOWN"], 1);
                let req = request_payload(owner_calls(&f, "--native-prune").last().unwrap());
                assert_eq!(req["sharpe"], 1.25);
                assert_eq!(req["fitness"], 0.75);
                assert_eq!(req["dryRun"], false);
                assert_eq!(req["remote"], remote);
                f.faults(json!({"priorityInvalid":true}));
                let bad = f.run(&["status", "--name", name]).await;
                assert_eq!(
                    bad.exit_code, 0,
                    "original Priority invalid checkpoint exit-zero contract"
                );
                assert_eq!(bad.value["ok"], false);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn parsed_results_paging_and_explicit_collect_use_actual_execution_rows() {
    let f = Fixture::new().await;
    let name = "result_page";
    f.create_generate(name, "regular", true).await;
    f.store
        .alter(name, doc! {"startConfig":{"tag":{"tags":[name]}}});
    let mut rows = f.store.rows_at(Host::Remote, name);
    for (i, row) in rows.iter_mut().enumerate() {
        row.extend(doc!{"status":"success","alphaId":format!("alpha-{i}"),"payload":{"must":"remain export-only"}});
    }
    f.store.put_rows(Host::Remote, name, rows);
    f.store.data.lock().unwrap().canonical = vec![doc! {"id":"alpha-0","is":{"fitness":1.2}}];
    let page = f
        .run(&["results", "--name", name, "--limit", "2", "--offset", "0"])
        .await;
    ok(&page);
    assert_eq!(page.value["pageItems"], 2);
    assert_eq!(page.value["storedOnPage"], 1);
    assert_eq!(page.value["hasMore"], true);
    assert_eq!(page.value["notInCanonicalStore"], json!(["alpha-1"]));
    assert_eq!(page.value["platformFetched"], false);
    assert!(!page.value.to_string().contains("export-only"));
    assert_eq!(
        f.run(&["results", "--name", name, "--limit", "2", "--offset", "2"])
            .await
            .value["pageItems"],
        1
    );
    let exported = f.run(&["collect-results", "--name", name]).await;
    ok(&exported);
    let expected = f.root.join(format!("reports/missions/{name}.json"));
    assert_eq!(
        exported.value["output"],
        expected.to_string_lossy().as_ref()
    );
    let full: Value = serde_json::from_slice(&fs::read(expected).unwrap()).unwrap();
    assert_eq!(full["total"], 3);
    assert_eq!(full["rows"][0]["payload"]["must"], "remain export-only");
    assert!(full["rows"][0].get("_id").is_none());
    let pagefile = f.root.join("bounded.json");
    ok(&f
        .run(&[
            "results",
            "--name",
            name,
            "--limit",
            "1",
            "--output",
            pagefile.to_str().unwrap(),
        ])
        .await);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(pagefile).unwrap()).unwrap()["limit"],
        1
    );
    failed(
        &f.run(&[
            "collect-results",
            "--name",
            name,
            "--output",
            f.root.to_str().unwrap(),
        ])
        .await,
    );
    assert!(f
        .store
        .data
        .lock()
        .unwrap()
        .calls
        .contains(&format!("page:Remote:{name}:2:2")));
    assert!(!f
        .store
        .data
        .lock()
        .unwrap()
        .calls
        .contains(&format!("page:Local:{name}:2:2")));
}

#[tokio::test(flavor = "current_thread")]
async fn all_mutating_and_evidence_routes_have_parsed_refusals_without_identity_pollution() {
    let f = Fixture::new().await;
    let name = "negative";
    f.create_generate(name, "regular", false).await;
    let wrong = "ffffffffffffffffffffffff";
    let original = f.store.task(name);
    let rows = bytes(&f.store.rows_at(Host::Local, name));
    let cases = vec![
        vec!["create", "--name", name, "--templatefile", "changed"],
        vec!["generate", "--name", name, "--expected-task-id", wrong],
        vec!["sync", "--name", name],
        vec!["start", "--name", name, "--remote", "--no-notify"],
        vec!["pause", "--name", name, "--expected-task-id", wrong],
        vec!["observe-accepted", "--name", name],
        vec!["watch", "--name", name],
        vec![
            "priority-prune",
            "--name",
            name,
            "--confirm",
            "wrong",
            "--sharpe",
            "1.25",
            "--fitness",
            "0.75",
        ],
        vec![
            "delete",
            "--name",
            name,
            "--force",
            "--expected-task-id",
            wrong,
        ],
        vec![
            "verify-registration",
            "--name",
            name,
            "--expected-task-id",
            wrong,
        ],
        vec![
            "archive-research",
            "--name",
            name,
            "--expected-remote",
            "true",
        ],
        vec!["results", "--name", name, "--limit", "201"],
    ];
    let external = f.wire().len();
    for argv in cases {
        failed(&f.run(&argv).await);
        assert_eq!(f.store.task(name), original, "{argv:?}");
        assert_eq!(bytes(&f.store.rows_at(Host::Local, name)), rows, "{argv:?}");
    }
    assert_eq!(
        f.wire().len(),
        external,
        "all rejected before original external process boundary"
    );
    // Core25 may verify-and-archive a genuinely stopped ready task. Keep the
    // force safety oracle against UNKNOWN stop evidence, not an obsolete
    // requirement that a user must already have issued pause.
    f.faults(json!({"badStop":true}));
    failed(&f.run(&["delete", "--name", name, "--force"]).await);
    assert_eq!(f.store.task(name), original);
    assert_eq!(bytes(&f.store.rows_at(Host::Local, name)), rows);
    f.faults(json!({}));
    f.store.alter(name, doc! {"status":"launch_unknown"});
    let unknown = f.store.task(name);
    failed(
        &f.run(&["resume", "--name", name, "--recover-processing"])
            .await,
    );
    assert_eq!(f.store.task(name), unknown);
    f.store.data.lock().unwrap().fail = Some("summary:Local:negative".into());
    failed(&f.run(&["status", "--name", name]).await);
    assert_eq!(f.store.task(name), unknown);
}

#[tokio::test(flavor = "current_thread")]
async fn saved_null_default_literal_and_explicit_config_survive_actual_cli_to_launch_wire() {
    for kind in ["regular", "super", "priority"] {
        let f = Fixture::new().await;
        let name = "old_fields";
        f.create_generate(name, kind, false).await;
        f.store.remove_field(name, "startConfig");
        f.store.remove_field(name, "mission_config");
        ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
        let payload: Value = serde_json::from_str(
            f.wire().iter().find(|v| v["argv"][0] == kind).unwrap()["argv"][2]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        if kind == "priority" {
            assert_eq!(payload["interval"], 600);
        } else {
            assert_eq!(
                payload["max_concurrent"],
                if kind == "super" { 3 } else { 8 }
            );
        }
        ok(&f.run(&["pause", "--name", name]).await);
        f.store.alter(
            name,
            doc! {"startConfig":Bson::Null,"mission_config":{"max_concurrent":4}},
        );
        let before = f.store.task(name);
        let calls = f.count(kind);
        if kind != "priority" {
            failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
            assert_eq!(f.store.task(name), before);
            assert_eq!(f.count(kind), calls);
        }
        failed(
            &f.run(&["start", "--name", name, "--config", "null", "--no-notify"])
                .await,
        );
        let cfg = config(name);
        ok(&f
            .run(&["start", "--name", name, "--config", &cfg, "--no-notify"])
            .await);
        let last = f
            .wire()
            .into_iter()
            .filter(|v| v["argv"][0] == kind)
            .last()
            .unwrap();
        let sent: Value = serde_json::from_str(last["argv"][2].as_str().unwrap()).unwrap();
        let explicit: Value = serde_json::from_str(&cfg).unwrap();
        for (key, value) in explicit.as_object().unwrap() {
            assert_eq!(sent[key], *value, "explicit key {key}");
        }
        assert_eq!(sent["literal"]["null"], Value::Null);
        assert_eq!(sent["literal"]["flags"], json!([false, true]));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn absence_and_postclaim_unknown_failures_recover_only_through_original_pause() {
    let f = Fixture::new().await;
    let name = "launch_unknown";
    f.create_generate(name, "regular", false).await;
    let original = f.store.task(name);
    f.faults(json!({"presence":true}));
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    assert_eq!(f.store.task(name), original);
    assert_eq!(f.count("regular"), 0);
    f.faults(json!({"launchLost":true}));
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    let unknown = f.store.task(name);
    assert_eq!(unknown.get_str("status"), Ok("launch_unknown"));
    assert_eq!(f.count("regular"), 1);
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    assert_eq!(f.count("regular"), 1);
    assert_eq!(f.store.task(name), unknown);
    f.faults(json!({"badStop":true}));
    failed(&f.run(&["pause", "--name", name]).await);
    assert_eq!(f.store.task(name), unknown);
    f.faults(json!({}));
    ok(&f.run(&["pause", "--name", name]).await);
    let stopped = f.store.task(name);
    assert_eq!(stopped.get_str("status"), Ok("paused"));
    assert_eq!(
        stopped.get("launchAttemptId"),
        unknown.get("launchAttemptId")
    );
    assert_eq!(
        stopped.get_str("stopConfirmationBasis"),
        Ok("session_absent")
    );
    assert_eq!(f.count("regular"), 1);
    assert_eq!(stopped.get("launchSettledAttemptId"), Some(&Bson::Null));
    failed(&f.run(&["start", "--name", name, "--no-notify"]).await);
    failed(&f.run(&["delete", "--name", name, "--force"]).await);
    assert_eq!(f.store.task(name), stopped);
    assert_eq!(f.count("regular"), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn partial_archive_preserves_all_rows_and_no_false_success_or_relaunch() {
    let f = Fixture::new().await;
    let name = "partial_archive";
    f.create_generate(name, "regular", true).await;
    ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
    ok(&f.run(&["pause", "--name", name]).await);
    let id = f.store.task(name).get_object_id("_id").unwrap().to_hex();
    let archive = format!("{name}__archive_{id}");
    let rows = bytes(&f.store.rows_at(Host::Remote, name));
    f.store.data.lock().unwrap().fail = Some(format!("rename:Local:{name}"));
    let result = f
        .run(&["delete", "--name", name, "--expected-task-id", &id])
        .await;
    failed(&result);
    assert_eq!(result.value["remoteDeleted"], true);
    assert_ne!(result.value["controlHidden"], true);
    assert_eq!(bytes(&f.store.rows_at(Host::Remote, &archive)), rows);
    assert_eq!(f.store.rows_at(Host::Local, name).len(), 3);
    assert_eq!(f.store.task(name).get_str("status"), Ok("deleting"));
    // Explicit same-ID retry uses the retained remote namespace, not empty
    // active rows. This is original safe retry, not --force or a new launch.
    let retried = f
        .run(&["delete", "--name", name, "--expected-task-id", &id])
        .await;
    ok(&retried);
    assert_eq!(retried.value["controlHidden"], true);
    assert_eq!(f.store.task(name).get_str("status"), Ok("deactive"));
    assert_eq!(bytes(&f.store.rows_at(Host::Remote, &archive)), rows);
    assert_eq!(f.count("regular"), 1);
    let request = request_payload(
        owner_calls(&f, "--native-archive-statistics")
            .last()
            .unwrap(),
    );
    assert_eq!(request["evidenceCollection"], archive);
    assert_eq!(
        f.store
            .data
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| **c == format!("rename:Remote:{name}"))
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn wire_owner_failures_and_wrong_watch_receipt_do_not_retry_or_claim_success() {
    let f = Fixture::new().await;
    let name = "owner_errors";
    f.create_generate(name, "regular", false).await;
    f.faults(json!({"ownerFail":"native-verify-registration"}));
    failed(&f.run(&["verify-registration", "--name", name]).await);
    assert_eq!(owner_calls(&f, "--native-verify-registration").len(), 1);
    f.faults(json!({"ownerFail":"native-archive-statistics"}));
    failed(&f.run(&["archive-research", "--name", name]).await);
    assert_eq!(owner_calls(&f, "--native-archive-statistics").len(), 1);
    f.faults(json!({}));
    ok(&f.run(&["start", "--name", name, "--no-notify"]).await);
    let before = f.store.task(name);
    f.faults(json!({"wrongWatch":true}));
    let result = f
        .run(&[
            "watch",
            "--name",
            name,
            "--notify-session-id",
            "fixture-main",
        ])
        .await;
    failed(&result);
    assert_eq!(result.value["notificationMetadataConfirmed"], false);
    assert_eq!(f.store.task(name), before);
    assert_eq!(
        owner_calls(&f, "--native-control").len(),
        2,
        "one preflight + one attach, no retries"
    );
    assert_eq!(f.count("regular"), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn resume_flags_require_typed_no_post_and_do_not_clear_accepted_or_unknown() {
    let f = Fixture::new().await;
    let name = "retry_evidence";
    f.create_generate(name, "regular", false).await;
    let mut rows = vec![
        unposted("000000000000000000000001", "transient_error"),
        unposted("000000000000000000000002", "timeout_error"),
        unposted("000000000000000000000003", "processing"),
    ];
    let mut unknown = unposted("000000000000000000000004", "timeout_error");
    unknown.insert("acceptance", "UNKNOWN");
    rows.push(unknown);
    let mut weak = unposted("000000000000000000000005", "transient_error");
    weak.insert("_not_posted_", 1);
    rows.push(weak);
    let protected = bytes(&rows[3..]);
    f.store.put_rows(Host::Local, name, rows);
    let default = f.run(&["resume", "--name", name]).await;
    ok(&default);
    assert_eq!(default.value["resumed"], 1);
    let expanded = f
        .run(&[
            "resume",
            "--name",
            name,
            "--include-timeout",
            "--recover-processing",
        ])
        .await;
    ok(&expanded);
    assert_eq!(expanded.value["resumed"], 2);
    assert_eq!(bytes(&f.store.rows_at(Host::Local, name)[3..]), protected);
    assert_eq!(f.count("regular"), 0);
}

#[path = "support/watch_status_fixture.rs"]
mod watch_status_fixture;

#[tokio::test(flavor = "current_thread")]
async fn completed_status_then_watch_preserves_business_timestamps_and_binding() {
    // Main timestamp_scope_correction#4: attach is a real metadata operation;
    // only subsequent observation is a no-write transaction. Keep completion
    // protected across both phases; do not change maintenance_ops semantics.
    for row_status in ["success", "timeout_error"] {
        let f=Fixture::new().await;
        let name="rewatch_completed";
        f.create_generate(name,"regular",false).await;
        ok(&f.run(&["start","--name",name,"--no-notify"]).await);
        let mut rows=f.store.rows_at(Host::Local,name);
        for row in &mut rows { row.insert("status",row_status); }
        f.store.put_rows(Host::Local,name,rows);
        // Completion is produced by real core, never seeded as a result DTO.
        ok(&f.run(&["status","--name",name]).await);
        let completed=f.store.task(name);
        assert_eq!(completed.get_str("status"),Ok(if row_status=="success" {"completed"}else{"completed_with_errors"}));
        for receiver in ["main-original","main-rebound"] {
            let previous=f.store.task(name);
            let write_count=f.store.data.lock().unwrap().writes.len();
            ok(&f.run(&["watch","--name",name,"--notify-session-id",receiver,"--replace-watcher"]).await);
            let watched=f.store.task(name);
            for field in ["_id","status","completedAt"] {
                assert_eq!(watched.get(field),completed.get(field),"watch changed completion field {field}");
            }
            assert_ne!(watched.get("updatedAt"),previous.get("updatedAt"));
            let mut business_before=previous.clone();let mut business_after=watched.clone();
            for field in ["agentNotification","updatedAt","events"] {
                business_before.remove(field);business_after.remove(field);
            }
            assert_eq!(business_after,business_before,"only original notification metadata may change");
            let old_events=previous.get_array("events").unwrap();let events=watched.get_array("events").unwrap();
            assert_eq!(&events[..old_events.len()],old_events.as_slice());
            assert_eq!(events.len(),old_events.len()+1);
            assert_eq!(events.last().unwrap().as_document().unwrap().get_str("command"),Ok("watch-attach"));
            assert_eq!(watched.get_document("agentNotification").unwrap()
                .get_document("target").unwrap().get_str("sessionId"),Ok(receiver));
            {
                let data=f.store.data.lock().unwrap();assert_eq!(data.writes.len(),write_count+1);
                let set=data.writes.last().unwrap().2.get_document("$set").unwrap();
                assert_eq!(set.len(),2);assert!(set.contains_key("agentNotification")&&set.contains_key("updatedAt"));
            }
            let binding=json!({"taskId":completed.get_object_id("_id").unwrap().to_hex(),
                "launchId":completed.get_str("launchAttemptId").unwrap(),"remote":false}).to_string();
            for args in [vec!["status","--name",name,"--watch-observation",binding.as_str()],vec!["status","--name",name]] {
                ok(&f.run(&args).await);
                assert_eq!(f.store.task(name),watched,"observation must preserve the post-attach snapshot");
                assert_eq!(f.store.data.lock().unwrap().writes.len(),write_count+1);
            }
        }
    }
}
