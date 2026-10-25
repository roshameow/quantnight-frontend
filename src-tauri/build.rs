#[path = "../crates/source_identity.rs"]
mod source_identity;
#[path = "src/dev_identity_inputs.rs"]
mod dev_identity_inputs;

fn main() {
    println!("cargo:rerun-if-env-changed=QUANTNIGHT_DEV_BINDING");
    println!("cargo:rerun-if-env-changed=QUANTNIGHT_TASKCTL_BIN");
    let frontend = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .parent().unwrap().canonicalize().unwrap();
    println!("cargo:rerun-if-changed={}", frontend.join(dev_identity_inputs::LOCATOR).display());
    let identity = match dev_identity_inputs::resolve_frontend(&frontend).expect("explicit enrollment required; run register-dev-host.mjs with reviewed canonical paths") {
        None => String::new(),
        Some((path, binding)) => {
            assert_eq!(frontend, binding.frontend_root, "dev binding frontend mismatch");
            let target = std::env::var("TARGET").unwrap();
            let profile = std::env::var("PROFILE").unwrap();
            assert_eq!(binding.target, target, "dev binding target mismatch");
            assert_eq!(binding.profile, profile, "dev binding profile mismatch");
            assert!(target.contains("apple-darwin"), "dev loaded-image admission is macOS-only; standalone is unchanged");
            println!("cargo:rerun-if-changed={}", std::path::Path::new(&path).display());
            for input in dev_identity_inputs::SOURCE_DIRS.iter().chain(dev_identity_inputs::SOURCE_FILES) {
                println!("cargo:rerun-if-changed={}", frontend.join(input).display());
            }
            println!("cargo:rerun-if-changed={}", frontend.join("src-tauri/capabilities").display());
            println!("cargo:rerun-if-changed={}", binding.backend_root.join(dev_identity_inputs::LOCATOR).display());
            let mut features = std::env::vars().filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(str::to_owned)).collect::<Vec<_>>();
            features.sort();
            let identity = dev_identity_inputs::BuildIdentity {
                source: dev_identity_inputs::fingerprint(&frontend).expect("dev source identity"),
                binding, binding_path: path, target, profile, features,
                rustflags: std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default(),
            };
            serde_json::to_string(&identity).unwrap()
        }
    };
    println!("cargo:rustc-env=QUANTNIGHT_DEV_BUILD_IDENTITY={identity}");
    tauri_build::build()
}
