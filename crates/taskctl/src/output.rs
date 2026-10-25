//! Transport formatting only: never derive a business exit code from "ok".
use quantnight_task_core::{cleanup_sync::TaskOutcome, cli_args::CliInvocation};
use serde_json::json;
use std::io::{self, Write};

pub fn rejected(invocation: &CliInvocation, error: impl AsRef<str>) -> TaskOutcome {
    TaskOutcome {
        exit_code: 1,
        value: json!({
            "schemaVersion": 1, "command": invocation.command, "task": invocation.name,
            "ok": false, "at": chrono::Utc::now().to_rfc3339(),
            "errorType": "RuntimeError", "error": error.as_ref(),
        }),
    }
}

/// Serialize before writing, then one JSON document plus newline. The shared
/// outcome (including its exit code) is opaque to this transport adapter.
pub fn write_outcome(writer: &mut impl Write, outcome: &TaskOutcome) -> io::Result<i32> {
    let mut bytes = serde_json::to_vec(&outcome.value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(outcome.exit_code)
}
