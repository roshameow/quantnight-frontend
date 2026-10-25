//! One cold-start configuration proof against the ORIGINAL Python config owner.
//! GUI operations subsequently check a cheap in-memory file fingerprint; they
//! do not start Python for every button. No URI discovery, alternate parser,
//! configuration cache file, client factory, or persistent service is added.
use std::{path::{Path, PathBuf}, time::Duration};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use crate::config::{AppConfig, run_process_private};

pub async fn export_backend_config(root: &Path) -> Result<AppConfig, String> {
    let root = root.canonicalize().map_err(|_| "explicit backend root unavailable")?;
    let mut command = tokio::process::Command::new(root.join(".venv/bin/python"));
    command.args(["-B", "-m", "backend.mongodb.mongo_config", "--export-task-config"])
        .arg(&root).current_dir(&root).env("PYTHONPATH", &root)
        .env_remove("TMUX").env_remove("TMUX_PANE").env_remove("TMUX_TMPDIR");
    // The result contains private endpoints. Do not use the diagnostic-logging
    // transport wrapper, print it, embed it in errors, or write a temp config.
    let (stdout, _) = run_process_private(command, Duration::from_secs(8)).await.map_err(|error| error.to_string())?;
    let mut receipt: Value = serde_json::from_str(&stdout)
        .map_err(|_| "original configuration owner returned invalid private JSON")?;
    if receipt.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || receipt.get("command").and_then(Value::as_str) != Some("export-task-config")
        || receipt.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("original configuration owner did not confirm native bootstrap".into());
    }
    let result = receipt.get_mut("result").ok_or("original configuration export missing")?.take();
    let config: AppConfig = serde_json::from_value(result)
        .map_err(|_| "original configuration export has invalid AppConfig schema")?;
    let configured_root = PathBuf::from(&config.env.working_dir).canonicalize()
        .map_err(|_| "exported backend root unavailable")?;
    if root != configured_root { return Err("original configuration export changed backend root".into()); }
    Ok(config)
}

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn source_digest(root: &Path) -> Result<String, String> {
    std::fs::read(root.join("credentials.yaml")).map(|content| digest(&content))
        .map_err(|_| "backend configuration unavailable; no alternative configuration used".into())
}

/// Desktop TOML selects root/UI settings, not effective task transport. Keep
/// this checked merge in the binding owner; never publish the headless export
/// as a desktop AppConfig or weaken the direct validator for legacy endpoints.
pub(crate) async fn resolve_gui_binding(mut config: AppConfig) -> Result<(AppConfig, BackendBinding), String> {
    let root = Path::new(&config.env.working_dir).canonicalize()
        .map_err(|_| "backend working directory unavailable")?;
    let before = source_digest(&root)?;
    let exported = export_backend_config(&root).await?;
    if source_digest(&root)? != before {
        return Err("backend configuration changed during export; restart application with a stable configuration".into());
    }
    if config.mongodb.databases.mission != exported.mongodb.databases.mission
        || config.mongodb.databases.simulation != exported.mongodb.databases.simulation {
        return Err("desktop task namespaces conflict with backend authority; check mission/simulation settings before restarting".into());
    }
    config.env.working_dir = root.to_string_lossy().into_owned();
    config.mongodb.local_uri = exported.mongodb.local_uri;
    config.mongodb.remote_uri = exported.mongodb.remote_uri;
    let binding = validate_backend_binding(&config).await?;
    // Include the gap between export and validation's initial fingerprint.
    if *binding.credentials_digest.read().map_err(|_| "configuration proof lock unavailable")? != before {
        return Err("backend configuration changed between export and validation; restart application".into());
    }
    binding.verify_current()?;
    Ok((config, binding))
}

pub struct BackendBinding {
    root: PathBuf,
    credentials_digest: std::sync::RwLock<String>,
    refresh: tokio::sync::Mutex<()>,
    request: Value,
}
impl BackendBinding {
    pub fn root(&self) -> &Path { &self.root }
    /// Only opaque endpoint fingerprints, never URI credentials. Passed to
    /// original research helpers so they can recheck before using their store.
    pub fn request(&self) -> &Value { &self.request }
    pub fn verify_current(&self) -> Result<(), String> {
        let current = source_digest(&self.root)?;
        if current != *self.credentials_digest.read().map_err(|_| "configuration proof lock unavailable")? {
            return Err("backend configuration changed; rebind the original application configuration before task mutation".into());
        }
        Ok(())
    }
    /// Revalidate only when the original file changes. Unrelated SMTP/AI edits
    /// must not become a permanent task-wide hold. One shared refresh owner,
    /// one 8s deadline (including lock wait), no retry or client recreation.
    pub async fn refresh_if_changed(&self) -> Result<(), String> {
        if self.verify_current().is_ok() { return Ok(()); }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        let _owner = tokio::time::timeout_at(deadline, self.refresh.lock()).await
            .map_err(|_| "configuration proof refresh deadline expired")?;
        if self.verify_current().is_ok() { return Ok(()); }
        let before = source_digest(&self.root)?;
        let budget = deadline.saturating_duration_since(tokio::time::Instant::now());
        check_owner(&self.root, &self.request, budget).await?;
        let after = source_digest(&self.root)?;
        if before != after {
            return Err("backend configuration changed during owner validation; task operation refused".into());
        }
        *self.credentials_digest.write().map_err(|_| "configuration proof lock unavailable")? = after;
        Ok(())
    }

}

/// Configuration selection remains with the existing TOML loader and backend
/// MongoConfig. The latter is authoritative for runner endpoints/namespaces.
/// This is a bootstrap check, not part of frequent GUI progress refreshes.
pub async fn validate_backend_binding(config: &AppConfig) -> Result<BackendBinding, String> {
    let root = Path::new(&config.env.working_dir).canonicalize()
        .map_err(|_| "backend working directory unavailable".to_string())?;
    let content = source_digest(&root)?;
    let request = json!({
        "schemaVersion": 1, "root": root.to_string_lossy(),
        "localUriSha256": digest(config.mongodb.local_uri.as_bytes()),
        // Null is an explicit absent-role claim, not permission to skip the
        // comparison. The authoritative owner must confirm its remote is also
        // absent; any populated endpoint mismatch remains a binding failure.
        "remoteUriSha256": if config.mongodb.remote_uri.trim().is_empty() {
            None
        } else { Some(digest(config.mongodb.remote_uri.as_bytes())) },
        "missionDatabase": config.mongodb.databases.mission,
        "simulationDatabase": config.mongodb.databases.simulation,
    });
    check_owner(&root, &request, Duration::from_secs(8)).await?;
    let binding = BackendBinding { root, credentials_digest: std::sync::RwLock::new(content), refresh: tokio::sync::Mutex::new(()), request };
    // Reject a change between the pre-read and the owner's actual parse.
    binding.verify_current()?;
    Ok(binding)
}

async fn check_owner(root: &Path, request: &Value, budget: Duration) -> Result<(), String> {
    let mut command = Command::new(root.join(".venv/bin/python"));
    command.args(["-B", "-m", "backend.mongodb.mongo_config", "--validate-task-control-binding"])
        .arg(serde_json::to_string(&request).map_err(|e| e.to_string())?)
        .current_dir(&root).env("PYTHONPATH", &root)
        .env_remove("TMUX").env_remove("TMUX_PANE").env_remove("TMUX_TMPDIR");
    let (out, _) = run_process_private(command, budget).await.map_err(|error| error.to_string())?;
    let receipt: Value = serde_json::from_str(&out)
        .map_err(|_| "backend configuration check returned no valid JSON receipt".to_string())?;
    if receipt.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || receipt.get("command").and_then(Value::as_str) != Some("validate-task-control-binding")
        || receipt.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("backend configuration check did not confirm this task binding".into());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::bash_test_support::FakeBash;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(source: &str) -> FakeBash {
        let fake = FakeBash::new(":");
        let root = Path::new(&fake.config.env.working_dir);
        std::fs::write(root.join("credentials.yaml"), "synthetic fixture only\n").unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let path = root.join(".venv/bin/python");
        std::fs::write(&path, format!("#!/bin/sh\n{source}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        fake
    }
    const OK: &str = r#"printf '%s\n' '{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}'"#;

    #[tokio::test]
    async fn one_bootstrap_then_file_checks_without_more_python() {
        let fake = fixture(&format!("echo called >> counter\n{OK}"));
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        binding.verify_current().unwrap();
        binding.verify_current().unwrap();
        assert_eq!(std::fs::read_to_string(binding.root().join("counter")).unwrap(), "called\n");
        assert!(!binding.request().to_string().contains("mongodb://"));
        std::fs::write(binding.root().join("credentials.yaml"), "changed\n").unwrap();
        assert!(binding.verify_current().unwrap_err().contains("changed"));
    }
    #[tokio::test]
    async fn unrelated_file_edit_revalidates_once_without_a_permanent_global_hold() {
        let fake = fixture(&format!("echo called >> counter\n{OK}"));
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        std::fs::write(binding.root().join("credentials.yaml"), "same endpoints, unrelated setting changed\n").unwrap();
        let (a, b) = tokio::join!(binding.refresh_if_changed(), binding.refresh_if_changed());
        a.unwrap(); b.unwrap();
        binding.refresh_if_changed().await.unwrap();
        assert_eq!(std::fs::read_to_string(binding.root().join("counter")).unwrap(), "called\ncalled\n");
        binding.verify_current().unwrap();
        std::fs::write(binding.root().join("credentials.yaml"), "different endpoint\n").unwrap();
        std::fs::write(binding.root().join(".venv/bin/python"), "#!/bin/sh\nexit 23\n").unwrap();
        assert!(binding.refresh_if_changed().await.is_err());
        assert!(binding.verify_current().is_err(), "failed validation must not adopt new configuration");
    }
    #[tokio::test]
    async fn changed_during_owner_check_is_not_adopted() {
        let fake = fixture(&format!("echo changed > credentials.yaml\n{OK}"));
        assert!(validate_backend_binding(&fake.config).await.err().unwrap().contains("changed"));
    }
    #[tokio::test]
    async fn absent_failed_or_malformed_proof_never_falls_back() {
        for source in [":", "echo '{}'; exit 0", "echo broken", "exit 23",
            "echo '{\"schemaVersion\":1,\"command\":\"other\",\"ok\":true}'"] {
            let fake = fixture(source);
            assert!(validate_backend_binding(&fake.config).await.is_err());
        }
        let fake = fixture(OK);
        std::fs::remove_file(Path::new(&fake.config.env.working_dir).join("credentials.yaml")).unwrap();
        assert!(validate_backend_binding(&fake.config).await.is_err());
    }
}
