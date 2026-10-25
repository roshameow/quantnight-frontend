//! Shared pure Rust source fingerprint owner. Build/runtime use identical bytes.
//! No GUI, business configuration, process spawning, Cargo invocation or watcher.
//! Cooperative admission freshness, not an immutable compiler/ABA attestation.
use sha2::{Digest, Sha256};
use std::{fs, path::{Path, PathBuf}, time::UNIX_EPOCH};

fn feed(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}
fn collect(root: &Path, directory: &Path, extensions: &[&str], paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| format!("dev source directory: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() { return Err(format!("dev source symlink not supported: {}", entry.path().display())); }
        if ty.is_dir() { collect(root, &entry.path(), extensions, paths)?; }
        else if entry.path().extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e))
            && !(extensions.contains(&"json") && entry.path().parent().and_then(|p| p.file_name()).is_some_and(|n| n == "schemas")) {
            paths.push(entry.path().strip_prefix(root).map_err(|e| e.to_string())?.to_owned());
        }
    }
    Ok(())
}

/// Bytes plus source modification identities: an edit/undo during compilation
/// must not admit a host merely because the final source bytes equal the start.
/// This is a cooperative dev freshness check, not an immutable compiler snapshot.
pub fn fingerprint(root: &Path, directories: &[&str], files: &[&str]) -> Result<String, String> {
    fingerprint_with_manifests(root, directories, files, &[])
}

/// Nonsecret Tauri capability source extensions. JSON5 is conservatively covered
/// even when the locked owner's config-json5 feature is disabled. Never gen/.
pub fn fingerprint_with_manifests(root: &Path, directories: &[&str], files: &[&str], manifests: &[&str]) -> Result<String, String> {
    let mut paths = files.iter().map(PathBuf::from).collect::<Vec<_>>();
    for dir in directories { collect(root, &root.join(dir), &["rs"], &mut paths)?; }
    for dir in manifests {
        let directory = root.join(dir);
        if directory.exists() {
            if fs::symlink_metadata(&directory).map_err(|e| e.to_string())?.file_type().is_symlink() {
                return Err("capability source directory must not be a symlink".into());
            }
            collect(root, &directory, &["json", "toml", "json5"], &mut paths)?;
        }
    }
    paths.sort(); paths.dedup();
    let mut hash = Sha256::new();
    for relative in paths {
        feed(&mut hash, relative.to_str().ok_or("non-UTF8 source path")?.as_bytes());
        let path = root.join(&relative);
        let before = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => { feed(&mut hash, b"absent"); continue; }
            Err(error) => return Err(format!("dev source metadata: {error}")),
        };
        if !before.is_file() { return Err(format!("dev input is not a regular file: {}", path.display())); }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let after = fs::metadata(&path).map_err(|e| e.to_string())?;
        let modified = before.modified().map_err(|e| e.to_string())?;
        if modified != after.modified().map_err(|e| e.to_string())? || before.len() != after.len() {
            return Err("dev sources changed during admission; wait for normal Tauri reload".into());
        }
        feed(&mut hash, b"present");
        feed(&mut hash, &modified.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos().to_le_bytes());
        feed(&mut hash, &bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn crate_fingerprint(root: &Path) -> Result<String, String> {
    fingerprint(root, &["src"], &["Cargo.toml", "build.rs", "../source_identity.rs"])
}

/// A normal Cargo build step only: sample THIS crate before its compilation.
/// Another crate's later build step cannot replace this exported linked proof.
pub fn emit_crate_identity() -> Result<(), String> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest dir")?)
        .canonicalize().map_err(|e| e.to_string())?;
    for p in ["src", "Cargo.toml", "build.rs", "../source_identity.rs"] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    println!("cargo:rustc-env=QUANTNIGHT_CRATE_SOURCE_ID={}", crate_fingerprint(&root)?);
    println!("cargo:rustc-env=QUANTNIGHT_CRATE_ROOT={}", root.display());
    for name in ["TARGET", "PROFILE", "CARGO_ENCODED_RUSTFLAGS"] {
        println!("cargo:rustc-env=QUANTNIGHT_CRATE_{name}={}", std::env::var(name).unwrap_or_default());
    }
    let mut features = std::env::vars().filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(str::to_owned)).collect::<Vec<_>>();
    features.sort();
    println!("cargo:rustc-env=QUANTNIGHT_CRATE_FEATURES={}", features.join(","));
    Ok(())
}

/// Runtime comparison used by the dev host and the real Cargo race regression.
pub fn admit_linked(root: &Path, embedded: &str) -> Result<(), String> {
    if crate_fingerprint(root)? != embedded {
        return Err(format!("linked crate source is stale: {}; fix and rerun `npm run tauri dev`", root.display()));
    }
    Ok(())
}
