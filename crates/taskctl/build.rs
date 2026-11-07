#[path = "../source_identity.rs"]
mod source_identity;
fn main() {
    source_identity::emit_crate_identity().expect("crate source identity unavailable");
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/local/lib");
    }
}
