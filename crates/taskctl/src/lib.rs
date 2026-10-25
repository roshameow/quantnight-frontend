//! IO-free CLI adaptation. Business policy belongs exclusively to task-core.
pub mod bootstrap;
pub mod dispatch;
pub mod invocation;
pub mod output;

pub use invocation::{parse, to_request, Entry, Request};
