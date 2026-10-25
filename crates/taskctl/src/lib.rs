//! CLI adaptation and shared process entry. Business policy belongs exclusively to task-core.
pub mod bootstrap;
pub mod dispatch;
pub mod invocation;
pub mod output;

pub use invocation::{parse, to_request, Entry, Request};

pub mod entry;
pub use entry::{run_os, run_os_with_admission};

/// Build-step identity exported by the actual linked library. Standalone
/// operations do not impose development GUI admission or load desktop crates.
pub const DEV_SOURCE_ID: &str = env!("QUANTNIGHT_CRATE_SOURCE_ID");
pub const DEV_CRATE_ROOT: &str = env!("QUANTNIGHT_CRATE_ROOT");
pub const DEV_TARGET: &str = env!("QUANTNIGHT_CRATE_TARGET");
pub const DEV_PROFILE: &str = env!("QUANTNIGHT_CRATE_PROFILE");
pub const DEV_FEATURES: &str = env!("QUANTNIGHT_CRATE_FEATURES");
pub const DEV_RUSTFLAGS: &str = env!("QUANTNIGHT_CRATE_CARGO_ENCODED_RUSTFLAGS");
