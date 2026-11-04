//! The single application dispatcher. Adapters never implement storage/lifecycle policy.
use crate::{task_control::TaskControl,task_request::TaskRequest,cleanup_sync::TaskOutcome,
    runtime::{CallResult,RuntimeObserver,RuntimeEvent},pause::PauseOptions};
use std::sync::Arc;
pub async fn dispatch(control:&TaskControl, request:&TaskRequest, observer:Option<RuntimeObserver>)->Result<CallResult,String> {
    let bash=observer.clone().map(|o|Arc::new(move |e|o(RuntimeEvent::Bash(e))) as crate::config::BashScriptObserver);
    let result:TaskOutcome=match request {
        TaskRequest::List(r)=>control.list(r).await,
        TaskRequest::VerifyRegistration(r)|TaskRequest::ArchiveResearch(r)=>control.dataset_evidence(r).await,
        TaskRequest::Create(r)=>control.create(r,bash).await,
        TaskRequest::Generate(r)=>control.generate(r,bash).await,
        TaskRequest::Sync(r)=>control.sync_bound(&r.name,r.expected_task_id.as_deref(),r.expected_remote,bash).await,
        TaskRequest::Start(r)=>control.start(r,bash).await,
        TaskRequest::ObserveAccepted(r)=>control.observe_accepted(r,bash).await,
        TaskRequest::Watch(r)=>control.watch(r).await,
        TaskRequest::Status(r)=>control.status(r).await,
        TaskRequest::Pause(r)=>{
            let stages=observer.map(|o|Arc::new(move |e|o(RuntimeEvent::PauseStage(e))) as crate::pause::PauseObserver);
            let p=control.pause(&r.name,PauseOptions{expected_task_id:r.expected_task_id.as_deref(),
                expected_launch_attempt_id:r.expected_launch_attempt_id.as_deref(),expected_launch_absent:r.expected_launch_absent,
                expected_remote:r.expected_remote,remaining:r.remaining_ms.map(std::time::Duration::from_millis),observer:stages},r.budget,bash).await;
            return Ok(CallResult{value:p.value,exit_code:p.exit_code,confirmed_stop:p.confirmed_stop})
        },
        TaskRequest::Resume(r)=>control.resume(r,bash).await,
        TaskRequest::PriorityPrune(r)=>control.priority_prune(r,bash).await,
        TaskRequest::Delete(r)=>control.delete_bound(&r.name,r.expected_task_id.as_deref(),r.expected_remote,r.confirm.as_deref(),bash).await,
        TaskRequest::Results(r)=>control.results(r).await,
        TaskRequest::CollectResults(r)=>control.collect_results(r).await,
        TaskRequest::PriorityUpdate(r)=>control.update_priority(r,bash).await,
        TaskRequest::TaskById{id}=>return Ok(CallResult{value:crate::runtime::wire_document::encode(&control.task_by_id(id).await?)?,exit_code:0,confirmed_stop:None}),
        TaskRequest::GuiTasks=>return Ok(CallResult{value:crate::gui_task_queries::list(control.runtime_context()?.as_ref(),control).await?,exit_code:0,confirmed_stop:None}),
        TaskRequest::TaskDiagnostics{name,task_id,remote}=>return Ok(CallResult{value:serde_json::Value::String(crate::task_diagnostics::inspect(control.runtime_context()?.as_ref(),name,task_id.as_deref(),*remote,observer).await?),exit_code:0,confirmed_stop:None}),
    };
    Ok(CallResult{value:result.value,exit_code:result.exit_code,confirmed_stop:None})
}
