//! Shared typed application calls, not a second business parser.
use serde::{Serialize, Deserialize};
use crate::{create_generate::{CreateRequest,GenerateRequest}, inspection::{ListRequest,EvidenceRequest,StatusRequest,ResultsRequest,CollectRequest},
    start::StartRequest, maintenance_ops::{ObserveAcceptedRequest,WatchRequest,PriorityPruneRequest},
    resume::ResumeRequest, priority_update::UpdatePriorityRequest, pause::StopBudget};
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct NamedRequest { pub name:String, pub expected_task_id:Option<String>, pub expected_remote:Option<bool> }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct DeleteRequest { pub name:String, pub expected_task_id:Option<String>, pub expected_remote:Option<bool>, pub confirm:Option<String> }
pub type PauseCallerBudget = StopBudget;
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct PauseRequest {
    pub name:String, pub expected_task_id:Option<String>, pub expected_remote:Option<bool>,
    pub expected_launch_attempt_id:Option<String>, pub expected_launch_absent:bool,
    pub remaining_ms:Option<u64>, pub budget:PauseCallerBudget,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="operation",content="arguments",deny_unknown_fields)]
pub enum TaskRequest {
    List(ListRequest), VerifyRegistration(EvidenceRequest), ArchiveResearch(EvidenceRequest),
    Create(CreateRequest), Generate(GenerateRequest), Sync(NamedRequest), Start(StartRequest),
    ObserveAccepted(ObserveAcceptedRequest), Watch(WatchRequest), Status(StatusRequest),
    Pause(PauseRequest), Resume(ResumeRequest), PriorityPrune(PriorityPruneRequest),
    Delete(DeleteRequest), Results(ResultsRequest), CollectResults(CollectRequest),
    PriorityUpdate(UpdatePriorityRequest), TaskById { id:String },
    GuiTasks, TaskDiagnostics { name:String, task_id:Option<String>, remote:bool },
}
impl TaskRequest {
    pub fn resolve_output(&mut self, cwd:&std::path::Path) {
        let path = match self { Self::Results(r)=>&mut r.output, Self::CollectResults(r)=>&mut r.output, _=>return };
        if let Some(p)=path { if p.is_relative() { *p=cwd.join(&*p); } }
    }
}

#[derive(Clone,Copy,Debug)]
#[repr(usize)]
pub(crate) enum RequestKind {List,VerifyRegistration,ArchiveResearch,Create,Generate,Sync,Start,ObserveAccepted,Watch,Status,Pause,Resume,PriorityPrune,Delete,Results,CollectResults,PriorityUpdate,TaskById,GuiTasks,TaskDiagnostics}
pub(crate) const REQUEST_KIND_NAMES:[&str;20]=["List","VerifyRegistration","ArchiveResearch","Create","Generate","Sync","Start","ObserveAccepted","Watch","Status","Pause","Resume","PriorityPrune","Delete","Results","CollectResults","PriorityUpdate","TaskById","GuiTasks","TaskDiagnostics"];
impl TaskRequest {pub(crate) fn kind(&self)->RequestKind{match self{Self::List(_)=>RequestKind::List,Self::VerifyRegistration(_)=>RequestKind::VerifyRegistration,Self::ArchiveResearch(_)=>RequestKind::ArchiveResearch,Self::Create(_)=>RequestKind::Create,Self::Generate(_)=>RequestKind::Generate,Self::Sync(_)=>RequestKind::Sync,Self::Start(_)=>RequestKind::Start,Self::ObserveAccepted(_)=>RequestKind::ObserveAccepted,Self::Watch(_)=>RequestKind::Watch,Self::Status(_)=>RequestKind::Status,Self::Pause(_)=>RequestKind::Pause,Self::Resume(_)=>RequestKind::Resume,Self::PriorityPrune(_)=>RequestKind::PriorityPrune,Self::Delete(_)=>RequestKind::Delete,Self::Results(_)=>RequestKind::Results,Self::CollectResults(_)=>RequestKind::CollectResults,Self::PriorityUpdate(_)=>RequestKind::PriorityUpdate,Self::TaskById{..}=>RequestKind::TaskById,Self::GuiTasks=>RequestKind::GuiTasks,Self::TaskDiagnostics{..}=>RequestKind::TaskDiagnostics}}}
