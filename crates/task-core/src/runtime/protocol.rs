//! One protocol for GUI, CLI, subscriptions, and lifecycle management.
use serde::{Serialize,Deserialize};
use super::{CallResult,RuntimeEvent,TransportError,identity::{HostIdentity,RuntimeSelection}};
use crate::task_request::TaskRequest;
pub const VERSION:u32=1;
pub const MAGIC:&[u8;8]=b"QNRT0001";
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub enum Admission {Business,Management}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Hello {pub mode:Admission,pub protocol:u32,pub selection:RuntimeSelection,pub core_source:String,#[serde(default)] pub task_settings:Option<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CallContext {pub request_id:String,pub caller_cwd:std::path::PathBuf,#[serde(default)] pub pause_deadline_ns:Option<u64>,#[serde(default)] pub diagnostic_deadline_ns:Option<u64>,#[serde(default)] pub operational_env:crate::operations_runtime::OperationalEnvironment}
impl CallContext {
    pub fn current()->Result<Self,TransportError>{Ok(Self{
        request_id:super::identity::nonce().map_err(|e|TransportError::new("identity_mismatch","request_id",e))?,
        caller_cwd:std::env::current_dir().map_err(|_|TransportError::new("identity_mismatch","caller","caller cwd unavailable"))?,pause_deadline_ns:None,diagnostic_deadline_ns:None,operational_env:crate::operations_runtime::OperationalEnvironment::capture().map_err(|e|TransportError::new("identity_mismatch","execution_context",e))?})}
    pub fn validate(&self)->Result<(),TransportError>{
        self.operational_env.validate().map_err(|e|TransportError::new("identity_mismatch","execution_context",e))?;
        if self.request_id.len()!=32 || !self.request_id.bytes().all(|v|v.is_ascii_hexdigit()) || !self.caller_cwd.is_absolute(){
            return Err(TransportError::new("identity_mismatch","request","invalid request id/caller directory"));
        }Ok(())
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct HostStatus {pub identity:HostIdentity,pub draining:bool,pub active:u64,pub accepted_calls:u64,pub accepted_by_kind:std::collections::BTreeMap<String,u64>,pub progress_subscriptions:u64,pub pools:crate::mongo_manager::PoolSnapshot}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="message",content="data",deny_unknown_fields)]
pub enum Message {
    Hello(Hello),Ready{identity:HostIdentity,call_id:String},Call{context:CallContext,request:TaskRequest},
    Accepted{id:String,host_generation:String},Event(RuntimeEvent),
    Completed{id:String,result:CallResult},Error(TransportError),
    Subscribe,Subscribed,Refresh,Refreshed,ManagementStatus,Drain{expected_host_generation:String},HostStatus(HostStatus),
}
