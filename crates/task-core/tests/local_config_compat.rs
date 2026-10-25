//! All fixtures are synthetic. No test constructs a Mongo Client, connects to
//! a server, reads application configuration, or invokes the real Python owner.
#![cfg(feature = "test-support")]
use bson::doc;
use quantnight_task_core::{
    config::AppConfig,
    mongo_manager::{
        local_config_test_support::{without_client_construction, FakeClients},
        MongoClients,
    },
    task_control::TaskControl,
    task_store::{Host, MongoTaskStore, TaskStore},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn config() -> AppConfig {
    // The SAME config crate/deserializer as load_config; no config filesystem IO.
    config::Config::builder()
        .add_source(config::File::from_str(
            r#"
[env]
working_dir = "/synthetic/not-accessed"
[python.scripts]
[bash.scripts]
[mongodb]
local_uri = "not-a-real-uri-and-must-not-be-parsed"
[mongodb.databases]
mission = "simulation_mission"
simulation = "simulation_db"
alpha = "alpha_db"
"#,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap()
}

#[test]
fn missing_remote_deserializes_as_absent_not_local_and_namespaces_stay_required() {
    let mut config = config();
    assert!(config.mongodb.remote_uri.is_empty());
    assert_ne!(config.mongodb.remote_uri, config.mongodb.local_uri);
    MongoTaskStore::from_config(&config).unwrap();
    config.mongodb.databases.mission.clear();
    assert!(MongoTaskStore::from_config(&config)
        .err()
        .unwrap()
        .contains("no default database fallback"));
}

#[tokio::test]
async fn lazy_configured_roles_only_build_when_requested_and_never_rebuild() {
    let mut config = config();
    config.mongodb.remote_uri = "remote-opaque-synthetic-value".into();
    let pools = FakeClients::<String>::from_config(&config);
    let calls = AtomicUsize::new(0);
    let build = |uri: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(Arc::new(uri)) }
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let local = pools.client_with(false, build).await.unwrap();
    assert_eq!(local.as_str(), config.mongodb.local_uri);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let same = pools.client_with(false, build).await.unwrap();
    assert!(Arc::ptr_eq(local, same));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let remote = pools.client_with(true, build).await.unwrap();
    assert_eq!(remote.as_str(), config.mongodb.remote_uri);
    assert!(!Arc::ptr_eq(local, remote));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn absent_remote_never_invokes_factory_or_selects_an_initialized_local_pool() {
    for absent in ["", " \n\t "] {
        let mut config = config();
        config.mongodb.remote_uri = absent.into();
        let pools = FakeClients::<String>::from_config(&config);
        let local = pools
            .client_with(false, |uri| async { Ok(Arc::new(uri)) })
            .await
            .unwrap();
        assert_eq!(local.as_str(), config.mongodb.local_uri);
        let calls = AtomicUsize::new(0);
        let result = pools
            .client_with(true, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err("factory must not run".into()) }
            })
            .await;
        assert!(result
            .unwrap_err()
            .contains("remote MongoDB role is unconfigured"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn concurrent_access_is_one_initialization_and_failed_attempt_is_not_retried() {
    let mut config = config();
    config.mongodb.remote_uri = "opaque-remote".into();
    let pools = FakeClients::<String>::from_config(&config);
    let calls = AtomicUsize::new(0);
    let build = |uri| {
        calls.fetch_add(1, Ordering::SeqCst);
        async {
            tokio::task::yield_now().await;
            Ok(Arc::new(uri))
        }
    };
    let (a, b) = tokio::join!(
        pools.client_with(false, build),
        pools.client_with(false, build)
    );
    assert!(Arc::ptr_eq(a.unwrap(), b.unwrap()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for _ in 0..2 {
        let error = pools
            .client_with(true, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err("synthetic initialization failure".into()) }
            })
            .await
            .unwrap_err();
        assert_eq!(error, "synthetic initialization failure");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn old_full_pool_injection_keeps_exact_arcs_without_building_anything() {
    let local = Arc::new("original GUI local".to_string());
    let remote = Arc::new("original GUI remote".to_string());
    let pools = FakeClients::from_existing(local.clone(), remote.clone());
    for (is_remote, expected) in [(false, &local), (true, &remote)] {
        let actual = pools
            .client_with(is_remote, |_| async { panic!("must reuse GUI pool") })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(actual, expected));
    }
}

// Type-check the real GUI source surface without manufacturing real clients.
#[allow(dead_code)]
async fn gui_public_surface_stays_source_compatible(
    config: &AppConfig,
    local: Arc<mongodb::Client>,
    remote: Arc<mongodb::Client>,
) {
    let clients = Arc::new(MongoClients {
        local: local.clone(),
        remote: remote.clone(),
    });
    let _: Arc<mongodb::Client> = clients.local.clone();
    let _: Arc<mongodb::Client> = clients.remote.clone();
    let _ = MongoTaskStore::new(clients.clone(), config);
    let _ = TaskControl::with_existing_clients(config, clients).await;
}

#[tokio::test]
async fn actual_store_remote_reads_and_writes_fail_before_any_client_or_query() {
    without_client_construction(async {
        let store = MongoTaskStore::from_config(&config()).unwrap();
        let remote = Host::Remote;
        let errors = vec![
            store.ping(remote).await.unwrap_err(),
            store.list_tasks(remote, false).await.unwrap_err(),
            store.find_task(remote, doc! {}, false).await.unwrap_err(),
            store.find_tasks(remote, doc! {}).await.unwrap_err(),
            store.collection_names(remote).await.unwrap_err(),
            store.summary(remote, "synthetic").await.unwrap_err(),
            store.has_rows(remote, "synthetic").await.unwrap_err(),
            store
                .consumer_capability_exists(remote, "synthetic")
                .await
                .unwrap_err(),
            store.rows(remote, "synthetic", None).await.unwrap_err(),
            store
                .result_items_page(remote, "synthetic", 20, 0)
                .await
                .unwrap_err(),
            store
                .collect_result_items(remote, "synthetic")
                .await
                .unwrap_err(),
            store.insert_task(remote, doc! {}).await.unwrap_err(),
            store
                .update_task(remote, doc! {}, doc! {})
                .await
                .unwrap_err(),
            store
                .update_item(remote, "synthetic", doc! {}, doc! {})
                .await
                .unwrap_err(),
            store
                .update_items(remote, "synthetic", doc! {}, doc! {})
                .await
                .unwrap_err(),
            store
                .insert_rows(remote, "synthetic", vec![doc! {}])
                .await
                .unwrap_err(),
            store
                .rename_collection(remote, "synthetic", "archive")
                .await
                .unwrap_err(),
        ];
        for error in errors {
            assert_eq!(
                error,
                "remote MongoDB role is unconfigured; operation unavailable (no fallback)"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn old_full_factory_requires_remote_before_constructing_even_local() {
    without_client_construction(async {
        // Local sentinel is invalid: the absent-role guard MUST run before the
        // shared build_client, so neither ClientOptions nor Client is constructed.
        assert_eq!(
            MongoClients::new(&config())
                .await
                .err()
                .unwrap()
                .to_string(),
            "remote MongoDB role is unconfigured; operation unavailable (no fallback)"
        );
    })
    .await;
}

#[tokio::test]
#[should_panic(expected = "offline bootstrap attempted Mongo client construction")]
async fn offline_factory_sentinel_fails_before_uri_parsing_or_client_construction() {
    let mut config = config();
    config.mongodb.remote_uri = "also-not-a-real-uri".into();
    without_client_construction(MongoClients::new(&config))
        .await
        .ok();
}

#[cfg(unix)]
mod binding {
    use super::*;
    use quantnight_task_core::{
        backend_binding::validate_backend_binding, config::bash_test_support::FakeBash,
        inspection::ListRequest,
    };
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{os::unix::fs::PermissionsExt, path::Path};

    fn sha(value: &str) -> String {
        format!("{:x}", Sha256::digest(value.as_bytes()))
    }
    // Lowest subprocess fixture accepts only the exact authoritative synthetic
    // binding. This is not a test of the real Python owner (Main owns that).
    fn owner_fixture(remote: Option<&str>) -> FakeBash {
        let mut fake = FakeBash::new(":");
        fake.config.mongodb.local_uri = "mongodb://synthetic-local.invalid".into();
        fake.config.mongodb.remote_uri = remote.unwrap_or("").into();
        fake.config.mongodb.databases.mission = "simulation_mission".into();
        fake.config.mongodb.databases.simulation = "simulation_db".into();
        let root = Path::new(&fake.config.env.working_dir)
            .canonicalize()
            .unwrap();
        std::fs::write(
            root.join("credentials.yaml"),
            "synthetic owner configuration\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let expected = json!({"schemaVersion":1,"root":root.to_string_lossy(),
            "localUriSha256":sha(&fake.config.mongodb.local_uri),
            "remoteUriSha256":remote.map(sha),
            "missionDatabase":"simulation_mission","simulationDatabase":"simulation_db"});
        let expected = expected.to_string().replace('\'', "'\"'\"'");
        let source = format!("#!/bin/sh\necho called >> counter\nprintf '%s' \"$5\" > request.json\nif [ \"$5\" = '{expected}' ]; then\n  printf '%s\\n' '{{\"schemaVersion\":1,\"command\":\"validate-task-control-binding\",\"ok\":true}}'\nelse\n  exit 23\nfi\n");
        let executable = root.join(".venv/bin/python");
        std::fs::write(&executable, source).unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        fake
    }

    #[tokio::test]
    async fn both_absent_accepts_explicit_null_keeps_opaque_proof_and_refreshes_once() {
        let fake = owner_fixture(None);
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        assert!(binding.request()["remoteUriSha256"].is_null());
        assert_eq!(
            binding.request()["localUriSha256"],
            sha(&fake.config.mongodb.local_uri)
        );
        assert!(!binding.request().to_string().contains("mongodb://"));
        assert_eq!(
            binding.root(),
            Path::new(&fake.config.env.working_dir)
                .canonicalize()
                .unwrap()
        );
        let original = binding.request().clone();
        std::fs::write(
            binding.root().join("credentials.yaml"),
            "unrelated setting edit\n",
        )
        .unwrap();
        let (a, b) = tokio::join!(binding.refresh_if_changed(), binding.refresh_if_changed());
        a.unwrap();
        b.unwrap();
        binding.refresh_if_changed().await.unwrap();
        assert_eq!(binding.request(), &original);
        assert_eq!(
            std::fs::read_to_string(binding.root().join("counter")).unwrap(),
            "called\ncalled\n"
        );
    }

    #[tokio::test]
    async fn populated_remote_agrees_but_mismatch_or_one_sided_absence_fails() {
        let mut fake = owner_fixture(Some("mongodb://synthetic-remote.invalid"));
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        assert_eq!(
            binding.request()["remoteUriSha256"],
            sha(&fake.config.mongodb.remote_uri)
        );
        fake.config.mongodb.remote_uri = "mongodb://different-remote.invalid".into();
        assert!(validate_backend_binding(&fake.config).await.is_err());
        fake.config.mongodb.remote_uri.clear();
        assert!(
            validate_backend_binding(&fake.config).await.is_err(),
            "null cannot skip a configured backend remote"
        );
        let mut fake = owner_fixture(None);
        fake.config.mongodb.remote_uri = "mongodb://synthetic-remote.invalid".into();
        assert!(validate_backend_binding(&fake.config).await.is_err());
        fake.config.mongodb.remote_uri.clear();
        fake.config.mongodb.databases.simulation = "other_namespace".into();
        assert!(validate_backend_binding(&fake.config).await.is_err());
    }

    #[tokio::test]
    async fn native_bootstrap_is_lazy_and_remote_facade_operation_reports_unavailable() {
        without_client_construction(async {
            let fake = owner_fixture(None);
            // No Mongo client is needed for connect; the remote list also fails
            // before any factory/query and cannot silently become a local list.
            let control = TaskControl::connect(&fake.config).await.unwrap();
            let outcome = control
                .list(&ListRequest {
                    remote: true,
                    include_deactive: false,
                })
                .await;
            assert_eq!(outcome.exit_code, 1);
            assert_eq!(outcome.value["ok"], false);
            assert!(outcome.value["error"]
                .as_str()
                .unwrap()
                .contains("remote MongoDB role is unconfigured"));
            let request: Value = serde_json::from_str(
                &std::fs::read_to_string(control.root().join("request.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(request["remoteUriSha256"], Value::Null);
            assert_eq!(
                std::fs::read_to_string(control.root().join("counter")).unwrap(),
                "called\n"
            );
        })
        .await;
    }
}
