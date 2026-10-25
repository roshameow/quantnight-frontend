//! Host binding/input selection; hashing belongs to crates/source_identity.rs.
//! No business configuration, Python imports, credential hashes, or Cargo calls.
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};
#[cfg(test)] use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema_version: u32,
    pub frontend_root: PathBuf,
    pub backend_root: PathBuf,
    pub executable: PathBuf,
    pub target: String,
    pub profile: String,
}

impl Binding {
    pub fn read(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() { return Err("dev binding must be an explicit absolute path".into()); }
        let bytes = fs::read(path).map_err(|e| format!("dev binding unavailable: {e}"))?;
        let binding: Self = serde_json::from_slice(&bytes).map_err(|e| format!("invalid dev binding: {e}"))?;
        if binding.schema_version != 1 || binding.profile != "debug" || binding.target.is_empty() {
            return Err("unsupported dev binding schema/profile/target (debug only)".into());
        }
        for root in [&binding.frontend_root, &binding.backend_root] {
            if !root.is_absolute() || !root.is_dir() || root.canonicalize().map_err(|e| e.to_string())? != *root {
                return Err("dev roots must be explicit canonical directories".into());
            }
        }
        if !binding.executable.is_absolute() || binding.executable.file_name().is_none() {
            return Err("dev executable must be an explicit absolute path".into());
        }
        let parent = binding.executable.parent().ok_or("missing executable parent")?;
        if parent.canonicalize().map_err(|e| format!("dev executable directory: {e}"))? != parent {
            return Err("dev executable directory must be canonical".into());
        }
        Ok(binding)
    }
}

pub const LOCATOR: &str = ".quantnight/taskctl-selection.json";
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
enum Selection {
    Dev { schema_version: u32, binding: PathBuf },
    Standalone { schema_version: u32 },
}
fn read_selection(root: &Path) -> Result<Selection, String> {
    let path = root.join(LOCATOR);
    let bytes = fs::read(&path).map_err(|e| format!("taskctl enrollment unavailable at {}: {e}; explicitly register dev or standalone", path.display()))?;
    let value: Selection = serde_json::from_slice(&bytes).map_err(|e| format!("invalid selection registration: {e}"))?;
    match &value {
        Selection::Dev { schema_version: 1, binding } => {
            if !binding.is_absolute() || !binding.is_file() || binding.canonicalize().map_err(|e| e.to_string())? != *binding {
                return Err("registered binding must be one canonical existing file".into());
            }
        }
        Selection::Standalone { schema_version: 1 } => (),
        _ => return Err("unsupported selection schema".into()),
    }
    Ok(value)
}
/// Known manifest root only. Both locators point to the same single document;
/// environment is an agreement check, never a fallback or selection authority.
pub fn resolve_frontend(root: &Path) -> Result<Option<(PathBuf, Binding)>, String> {
    let selected = read_selection(root)?;
    match selected {
        Selection::Standalone { .. } => {
            if std::env::var_os("QUANTNIGHT_DEV_BINDING").is_some() { return Err("dev environment conflicts with standalone registration".into()); }
            Ok(None)
        }
        Selection::Dev { binding: path, .. } => {
            if std::env::var_os("QUANTNIGHT_TASKCTL_BIN").is_some() { return Err("standalone executable conflicts with registered dev mode".into()); }
            if std::env::var_os("QUANTNIGHT_DEV_BINDING").is_some_and(|env| Path::new(&env) != path) {
                return Err("dev environment conflicts with registered binding".into());
            }
            let binding = Binding::read(&path)?;
            if binding.frontend_root != root { return Err("registered frontend root mismatch".into()); }
            match read_selection(&binding.backend_root)? {
                Selection::Dev { binding: other, .. } if other == path => (),
                _ => return Err("frontend/backend locators must reference one canonical binding".into()),
            }
            Ok(Some((path, binding)))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildIdentity {
    pub binding: Binding,
    pub binding_path: PathBuf,
    pub source: String,
    pub target: String,
    pub profile: String,
    pub features: Vec<String>,
    pub rustflags: String,
}

pub const SOURCE_DIRS: &[&str] = &["src-tauri/src", "crates/task-core/src", "crates/taskctl/src"];
pub const SOURCE_FILES: &[&str] = &[
    "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs",
    "src-tauri/tauri.conf.json", "src-tauri/.taurignore",
    "crates/Cargo.toml", "crates/Cargo.lock", "crates/task-core/Cargo.toml",
    "crates/taskctl/Cargo.toml", "vite.config.js", "scripts/dev-inputs.mjs",
    "package.json", "package-lock.json", "scripts/dev-registration.mjs", "scripts/register-dev-host.mjs", ".cargo/config.toml", "src-tauri/.cargo/config.toml",
    "src-tauri/src/dev-input.stamp", "crates/source_identity.rs",
    "crates/task-core/build.rs", "crates/taskctl/build.rs",
];

pub fn fingerprint(root: &Path) -> Result<String, String> {
    crate::source_identity::fingerprint_with_manifests(root, SOURCE_DIRS, SOURCE_FILES, &["src-tauri/capabilities"])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inputs_exclude_private_outputs_but_detect_rust_add_change_unlink_and_touch() {
        let root = std::env::temp_dir().join(format!("qn-dev-input-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&root).unwrap();
        for dir in SOURCE_DIRS { fs::create_dir_all(root.join(dir)).unwrap(); }
        let initial = fingerprint(&root).unwrap();
        fs::write(root.join("credentials.yaml"), "synthetic-secret").unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        fs::write(root.join("artifacts/task.json"), "synthetic-task").unwrap();
        assert_eq!(initial, fingerprint(&root).unwrap());
        let source = root.join("crates/task-core/src/example.rs");
        fs::write(&source, "const A: u8 = 1;").unwrap();
        let added = fingerprint(&root).unwrap(); assert_ne!(initial, added);
        fs::write(&source, "const A: u8 = 2;").unwrap();
        assert_ne!(added, fingerprint(&root).unwrap());
        fs::remove_file(&source).unwrap();
        assert_eq!(initial, fingerprint(&root).unwrap());
        fs::write(root.join("crates/Cargo.lock"), "fixture-lock").unwrap();
        assert_ne!(initial, fingerprint(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
