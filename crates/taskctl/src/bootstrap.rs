//! Configuration selection only. All parsing/roles remain with existing owners.
use std::{path::Path};
use quantnight_task_core::config::{AppConfig, load_config};
use quantnight_task_core::backend_binding::export_backend_config;

pub async fn load(app_config: Option<&Path>, backend_root: Option<&Path>) -> Result<AppConfig, String> {
    match (app_config, backend_root) {
        (Some(path), None) => load_config(path)
            .map_err(|_| "unable to load the explicit AppConfig file (check path and schema)".into()),
        (None, Some(root)) => export_backend_config(root).await,
        (None, None) => Err("--app-config PATH or --backend-root PATH is required; no implicit configuration is selected".into()),
        _ => Err("configuration selectors are mutually exclusive".into()),
    }
}
