//! Compatibility function delegates to the one core application dispatcher.
use crate::invocation::Request;
use quantnight_task_core::{task_control::TaskControl,cleanup_sync::TaskOutcome};
pub async fn dispatch(control:&TaskControl,request:&Request)->TaskOutcome {
    match quantnight_task_core::task_dispatch::dispatch(control,request,None).await {
        Ok(result)=>TaskOutcome{value:result.value,exit_code:result.exit_code},
        Err(error)=>TaskOutcome{value:serde_json::json!({"schemaVersion":1,"ok":false,"errorType":"RuntimeError","error":error}),exit_code:1},
    }
}
