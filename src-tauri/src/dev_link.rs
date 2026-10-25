//! Match the existing macOS Cargo dev loader route for direct host execution.
//! No library copy, package-manager search, or caller DYLD environment required.
pub fn link_args(target_os: &str, profile: &str) -> &'static [&'static str] {
    if target_os == "macos" && profile == "debug" {
        // Cargo's standard DYLD_FALLBACK_LIBRARY_PATH includes /usr/local/lib.
        // A directly exec'd GUI-host CLI must encode that same native route.
        &["-Wl,-rpath,/usr/local/lib"]
    } else {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::link_args;
    #[test]
    fn mac_dev_uses_original_native_provider() {
        assert_eq!(link_args("macos", "debug"), &["-Wl,-rpath,/usr/local/lib"]);
    }
    #[test]
    fn release_packaging_keeps_its_existing_owner() {
        assert!(link_args("macos", "release").is_empty());
    }
    #[test]
    fn other_targets_get_no_macos_loader_override() {
        assert!(link_args("linux", "debug").is_empty());
        assert!(link_args("windows", "debug").is_empty());
    }
}
