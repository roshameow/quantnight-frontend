#[path = "../source_identity.rs"]
mod source_identity;
fn main() {
    source_identity::emit_crate_identity().expect("crate source identity unavailable");
}
