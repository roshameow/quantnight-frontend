//! Configuration selection only. All parsing/roles remain with existing owners.
use std::{path::{Path, PathBuf}, time::Duration};
use quantnight_task_core::config::{AppConfig, load_config, run_process_private};
use serde_json::Value;

pub async fn load(app_config: Option<&Path>, backend_root: Option<&Path>) -> Result<AppConfig, String> {
    match (app_config, backend_root) {
        (Some(path), None) => load_config(path)
            .map_err(|_| "unable to load the explicit AppConfig file (check path and schema)".into()),
        (None, Some(root)) => from_backend(root).await,
        (None, None) => Err("--app-config PATH or --backend-root PATH is required; no implicit configuration is selected".into()),
        _ => Err("configuration selectors are mutually exclusive".into()),
    }
}

async fn from_backend(root: &Path) -> Result<AppConfig, String> {
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
