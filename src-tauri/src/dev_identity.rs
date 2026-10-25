//! Dev-host admission only. No business routing, build runner, or retry policy.
use crate::{dev_identity_inputs::{Binding, BuildIdentity, fingerprint}, dev_image};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, path::{Path, PathBuf}, sync::OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ProcessIdentity { pid: u32, started_seconds: u64, started_micros: u64 }
#[cfg(target_os = "macos")]
fn process_identity(pid: u32) -> Result<ProcessIdentity, String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: libproc receives a writable buffer with the documented size.
    let result = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size) };
    if result != size { return Err("dev GUI process identity is unavailable/stale".into()); }
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid || info.pbi_status == 5 { return Err("dev GUI process is not live".into()); }
    Ok(ProcessIdentity { pid, started_seconds: info.pbi_start_tvsec, started_micros: info.pbi_start_tvusec })
}
#[cfg(not(target_os = "macos"))]
fn process_identity(_: u32) -> Result<ProcessIdentity, String> { Err("dev host process identity requires macOS".into()) }

// A dev-host lock is not a mission mutator. Keep it outside the existing mission
// namespace; use the same nonblocking OS flock discipline (no unlink/retries).
struct HostLock(File);
impl HostLock {
    fn try_acquire(root: &Path) -> Result<Option<Self>, String> {
        let directory = root.join(".quantnight");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let file = OpenOptions::new().create(true).append(true).open(directory.join("dev-host.lock"))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)] {
            use std::os::fd::AsRawFd;
            // SAFETY: this File owns the live descriptor; flock stores no pointer.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock { return Ok(None); }
                return Err(format!("dev host lock: {error}"));
            }
            Ok(Some(Self(file)))
        }
        #[cfg(not(unix))] { let _ = file; Err("dev host requires native flock".into()) }
    }
}
impl Drop for HostLock {
    fn drop(&mut self) {
        // Closing the owned descriptor releases flock. Never unlink its inode.
        let _ = &self.0;
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema_version: u32,
    ready: bool,
    owner: ProcessIdentity,
    binding: Binding,
    source: String,
    image: [u8; 16],
}
struct Host {
    build: BuildIdentity,
    image: [u8; 16],
    owner: ProcessIdentity,
    _lock: HostLock,
}
static GUI: OnceLock<Host> = OnceLock::new();

fn embedded() -> Result<BuildIdentity, String> {
    // Existing native test binary owns fixture build metadata, never a normal
    // executable endpoint. All resolution/freshness/locking/entry paths remain real.
    #[cfg(test)]
    if let Ok(value) = std::env::var("QN_TEST_EMBEDDED_IDENTITY") {
        return serde_json::from_str(&value).map_err(|e| e.to_string());
    }
    let value = env!("QUANTNIGHT_DEV_BUILD_IDENTITY");
    if value.is_empty() { return Err("this GUI host was not built with an explicit dev binding; standalone selection is separate".into()); }
    serde_json::from_str(value).map_err(|e| format!("invalid embedded dev identity: {e}"))
}
fn current_binding(build: &BuildIdentity) -> Result<Binding, String> {
    let (path, binding) = crate::dev_identity_inputs::resolve_frontend(&build.binding.frontend_root)?
        .ok_or("loaded dev host conflicts with standalone registration")?;
    if path != build.binding_path { return Err("registered binding path differs from loaded host".into()); }
    Ok(binding)
}
fn linked_fresh(build: &BuildIdentity) -> Result<(), String> {
    for (name, source, root, target, profile) in [
        ("task-core", quantnight_task_core::DEV_SOURCE_ID, quantnight_task_core::DEV_CRATE_ROOT,
            quantnight_task_core::DEV_TARGET, quantnight_task_core::DEV_PROFILE),
        ("taskctl", quantnight_taskctl::DEV_SOURCE_ID, quantnight_taskctl::DEV_CRATE_ROOT,
            quantnight_taskctl::DEV_TARGET, quantnight_taskctl::DEV_PROFILE),
    ] {
        let expected = build.binding.frontend_root.join("crates").join(name);
        if Path::new(root) != expected || target != build.target || profile != build.profile {
            return Err(format!("linked {name} root/target/profile mismatch; fix and rerun `npm run tauri dev`"));
        }
        crate::source_identity::admit_linked(&expected, source)?;
    }
    Ok(())
}
fn fresh(build: &BuildIdentity) -> Result<[u8; 16], String> {
    if current_binding(build)? != build.binding { return Err("dev binding differs from the loaded host; use normal npm run tauri dev".into()); }
    let current = std::env::current_exe().map_err(|e| e.to_string())?.canonicalize().map_err(|e| e.to_string())?;
    if current != build.binding.executable { return Err("dev executable path differs from explicit binding".into()); }
    if fingerprint(&build.binding.frontend_root)? != build.source {
        return Err("dev Rust inputs changed or build is incomplete; wait for a successful normal Tauri reload; if startup stopped, fix and rerun `npm run tauri dev` (no task retry/build performed)".into());
    }
    linked_fresh(build)?;
    dev_image::ensure_current(&build.binding.executable)
}
fn record_path(root: &Path) -> PathBuf { root.join(".quantnight/dev-host-ready.json") }
impl Host {
    fn publish(&self, ready: bool) -> Result<(), String> {
        let record = Ready { schema_version: 1, ready, owner: self.owner.clone(),
            binding: self.build.binding.clone(), source: self.build.source.clone(), image: self.image };
        let path = record_path(&self.build.binding.backend_root);
        let temp = path.with_extension(format!("{}.tmp", self.owner.pid));
        fs::write(&temp, serde_json::to_vec(&record).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        fs::rename(&temp, &path).map_err(|e| e.to_string())
    }
}

/// Called before normal GUI config I/O. A configured dev host always fails
/// closed; an ordinary unbound GUI build retains its original startup route.
pub fn initialize_gui() -> Result<(), String> {
    if env!("QUANTNIGHT_DEV_BUILD_IDENTITY").is_empty() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("manifest root unavailable")?;
        if crate::dev_identity_inputs::resolve_frontend(root)?.is_some() { return Err("registered dev mode but GUI is not dev-bound; rerun npm run tauri dev".into()); }
        return Ok(());
    }
    let build = embedded()?;
    let image = fresh(&build)?;
    let lock = HostLock::try_acquire(&build.binding.backend_root)?.ok_or("another live GUI owns this dev binding")?;
    let host = Host { build, image, owner: process_identity(std::process::id())?, _lock: lock };
    host.publish(false)?;
    GUI.set(host).map_err(|_| "dev GUI initialized twice".to_string())
}

/// Original GUI AppConfig is the only business-config/root owner. No second
/// TOML parser or private binding is introduced by the watcher/build/shim.
pub fn verify_gui_root(root: &Path) -> Result<(), String> {
    if let Some(host) = GUI.get() {
        if root.canonicalize().map_err(|e| e.to_string())? != host.build.binding.backend_root {
            return Err("original AppConfig env.working_dir differs from explicit dev backend binding".into());
        }
    }
    Ok(())
}
pub fn ready() -> Result<(), String> {
    if let Some(host) = GUI.get() { fresh(&host.build)?; host.publish(true)?; }
    Ok(())
}
pub fn admit_gui() -> Result<(), String> {
    if let Some(host) = GUI.get() { fresh(&host.build)?; }
    else if !env!("QUANTNIGHT_DEV_BUILD_IDENTITY").is_empty() { return Err("dev GUI is not initialized".into()); }
    Ok(())
}

/// Library hook: only called after CLI syntax/typed conversion succeeds, before
/// Tokio, Python MongoConfig, AppConfig or DB access. Standalone never calls it.
pub fn admit_cli(app_config: Option<&Path>, backend_root: Option<&Path>) -> Result<(), String> {
    if app_config.is_some() { return Err("dev taskctl requires its explicit backend root, not a separate AppConfig".into()); }
    let build = embedded()?;
    let backend = backend_root.ok_or("dev taskctl requires --backend-root from the original shim")?
        .canonicalize().map_err(|e| e.to_string())?;
    if backend != build.binding.backend_root { return Err("shim backend root differs from dev binding".into()); }
    let image = fresh(&build)?;
    if let Some(_closed_gui_lock) = HostLock::try_acquire(&backend)? {
        // No conflicting live GUI: a fresh last-built host is allowed. A stale
        // record from an exited/crashed GUI is not a global standalone barrier.
        return Ok(());
    }
    let record: Ready = serde_json::from_slice(&fs::read(record_path(&backend)).map_err(|e| format!("live dev GUI has no ready record: {e}"))?)
        .map_err(|e| format!("invalid dev GUI ready record: {e}"))?;
    admit_ready(&record, &build, image)
}

fn admit_ready(record: &Ready, build: &BuildIdentity, image: [u8; 16]) -> Result<(), String> {
    if record.schema_version != 1 || !record.ready || record.binding != build.binding || record.source != build.source || record.image != image {
        return Err("live GUI has a different/unready loaded generation; wait for normal Tauri reload; if stopped, fix and rerun `npm run tauri dev`".into());
    }
    if process_identity(record.owner.pid)? != record.owner { return Err("dev GUI ready owner is stale/reused".into()); }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn actual_native_owner_and_image_generation_checks_no_fake_pid() {
        let root = std::env::temp_dir().join(format!("qn-native-owner-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(root.clone());
        let build = BuildIdentity {
            binding: Binding { schema_version: 1, frontend_root: root.clone(), backend_root: root.clone(),
                executable: std::env::current_exe().unwrap(), target: "aarch64-apple-darwin".into(), profile: "debug".into() },
            binding_path: root.join("binding.json"),
            source: "fixture-current-source".into(), target: "aarch64-apple-darwin".into(), profile: "debug".into(),
            features: vec![], rustflags: String::new(),
        };
        let image = dev_image::loaded().unwrap();
        let mut record = Ready { schema_version: 1, ready: true, owner: process_identity(std::process::id()).unwrap(),
            binding: build.binding.clone(), source: build.source.clone(), image };
        let held = HostLock::try_acquire(&root).unwrap().unwrap();
        assert!(HostLock::try_acquire(&root).unwrap().is_none());
        admit_ready(&record, &build, image).unwrap();
        record.owner.started_micros += 1;
        assert!(admit_ready(&record, &build, image).unwrap_err().contains("stale/reused"));
        record.owner = process_identity(std::process::id()).unwrap();
        record.ready = false;
        assert!(admit_ready(&record, &build, image).is_err());
        record.ready = true;
        // A real second compiled image, not a fabricated UUID or fake PID.
        fs::write(root.join("new.rs"), "fn main() {}" ).unwrap();
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        assert!(std::process::Command::new(rustc).arg(root.join("new.rs")).arg("-o").arg(root.join("new-host"))
            .status().unwrap().success());
        let new_image = dev_image::disk(&root.join("new-host")).unwrap();
        assert_ne!(image, new_image);
        assert!(admit_ready(&record, &build, new_image).unwrap_err().contains("different/unready"));
        drop(held);
        assert!(HostLock::try_acquire(&root).unwrap().is_some());
    }
    #[test]
    fn actual_linked_core_and_cli_identities_match_current_candidate() {
        if env!("QUANTNIGHT_DEV_BUILD_IDENTITY").is_empty() { return; }
        linked_fresh(&embedded().unwrap()).unwrap();
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "dev_admission_tests.rs"]
mod admission_chain_tests;
