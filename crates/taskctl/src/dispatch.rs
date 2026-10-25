//! All routes terminate at the same application facade as the desktop.
//! No storage, network, process, retry or lifecycle implementation lives here.
use crate::invocation::Request;
use quantnight_task_core::{
    cleanup_sync::TaskOutcome,
    pause::{PauseOptions, StopBudget},
    task_control::TaskControl,
};

pub async fn dispatch(control: &TaskControl, request: &Request) -> TaskOutcome {
    match request {
        Request::List(r) => control.list(r).await,
        Request::VerifyRegistration(r) => control.dataset_evidence(r).await,
        Request::ArchiveResearch(r) => control.dataset_evidence(r).await,
        Request::Create(r) => control.create(r, None).await,
        Request::Generate(r) => control.generate(r, None).await,
        Request::Sync(r) => {
            control
                .sync(&r.name, r.expected_task_id.as_deref(), None)
                .await
        }
        Request::Start(r) => control.start(r, None).await,
        Request::ObserveAccepted(r) => control.observe_accepted(r, None).await,
        Request::Watch(r) => control.watch(r).await,
        Request::Status(r) => control.status(r).await,
        Request::Pause(r) => {
            let result = control
                .pause(
                    &r.name,
                    PauseOptions {
                        expected_task_id: r.expected_task_id.as_deref(),
                        expected_launch_attempt_id: r.expected_launch_attempt_id.as_deref(),
                        expected_remote: None,
                        observer: None,
                    },
                    StopBudget::Cli,
                    None,
                )
                .await;
            TaskOutcome {
                value: result.value,
                exit_code: result.exit_code,
            }
        }
        Request::Resume(r) => control.resume(r, None).await,
        Request::PriorityPrune(r) => control.priority_prune(r, None).await,
        Request::Delete(r) => {
            control
                .delete(
                    &r.name,
                    r.expected_task_id.as_deref(),
                    r.confirm.as_deref(),
                    None,
                )
                .await
        }
        Request::Results(r) => control.results(r).await,
        Request::CollectResults(r) => control.collect_results(r).await,
    }
}
