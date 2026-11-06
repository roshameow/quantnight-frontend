pub mod config;
pub mod mongo_manager;
pub mod mission_lock;
pub mod lifecycle_policy;
pub mod task_store;
pub mod backend_binding;
pub mod cleanup_sync;
pub mod operations_runtime;
pub mod cli_args;
pub mod task_control;
pub mod create_generate;
pub mod pause;
pub mod start;
pub mod resume;
pub mod maintenance_ops;
pub mod inspection;
pub mod priority_update;
pub mod alpha_query;

/// Build-step identity exported by the actual linked library. Standalone
/// operations do not impose development GUI admission or load desktop crates.
pub const DEV_SOURCE_ID: &str = env!("QUANTNIGHT_CRATE_SOURCE_ID");
pub const DEV_CRATE_ROOT: &str = env!("QUANTNIGHT_CRATE_ROOT");
pub const DEV_TARGET: &str = env!("QUANTNIGHT_CRATE_TARGET");
pub const DEV_PROFILE: &str = env!("QUANTNIGHT_CRATE_PROFILE");
pub const DEV_FEATURES: &str = env!("QUANTNIGHT_CRATE_FEATURES");
pub const DEV_RUSTFLAGS: &str = env!("QUANTNIGHT_CRATE_CARGO_ENCODED_RUSTFLAGS");

pub mod task_request;
pub mod task_dispatch;
pub mod runtime;

#[path = "../../native_identity.rs"]
pub mod native_identity;

pub mod gui_task_queries;
pub mod task_progress;
pub mod task_diagnostics;

pub(crate) mod generation_owner;
