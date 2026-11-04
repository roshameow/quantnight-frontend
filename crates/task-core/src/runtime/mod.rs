//! Public local task runtime. No durable request journal, replay, HTTP, or alternate DB factory.
pub mod wire_document;
pub mod protocol;
pub mod codec;
pub mod identity;
pub mod client;
pub mod server;
mod progress;
pub use client::{RuntimeClient,ProgressSubscription};
pub use identity::{RuntimeSelection,HostIdentity,RuntimePaths,RuntimePlan};
pub use protocol::{CallContext,HostStatus};
use std::sync::Arc;
use serde::{Serialize,Deserialize};
use crate::{config::AppConfig,backend_binding::BackendBinding,mongo_manager::ClientSlots,task_control::TaskControl,task_request::TaskRequest};
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CallResult { pub value:serde_json::Value, pub exit_code:i32, #[serde(default,skip_serializing_if="Option::is_none")] pub confirmed_stop:Option<crate::pause::ConfirmedTaskStop> }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub enum RuntimeEvent { Bash(crate::config::BashScriptEvent), PauseStage(crate::pause::PauseStage), GuiProgress{name:String,payload:serde_json::Value}, ProgressGap{dropped:u64} }
pub type RuntimeObserver=Arc<dyn Fn(RuntimeEvent)+Send+Sync>;
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TransportError {pub class:String,pub stage:String,pub execution:String,pub request_id:Option<String>,pub host_generation:Option<String>,pub safe_message:String}
impl TransportError {
    pub fn new(class:&str,stage:&str,message:impl Into<String>)->Self{Self{class:class.into(),stage:stage.into(),execution:"not_sent".into(),request_id:None,host_generation:None,safe_message:message.into()}}
}
impl std::fmt::Display for TransportError {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{if self.class=="application"{write!(f,"{}",self.safe_message)}else{write!(f,"{} ({}; execution={})",self.safe_message,self.class,self.execution)}}}
impl std::error::Error for TransportError{}
pub struct TaskRuntimeContext {config:AppConfig,binding:Arc<BackendBinding>,clients:Arc<ClientSlots<mongodb::Client>>,gui_queries:crate::gui_task_queries::QueryState}
impl TaskRuntimeContext {
    pub(crate) fn new(config:AppConfig,binding:Arc<BackendBinding>,clients:Arc<ClientSlots<mongodb::Client>>)->Self{Self{config,binding,clients,gui_queries:Default::default()}}
    pub fn pool_snapshot(&self)->crate::mongo_manager::PoolSnapshot {self.clients.snapshot()}
    pub(crate) fn gui_queries(&self)->&crate::gui_task_queries::QueryState{&self.gui_queries}
    pub(crate) fn config(&self)->&AppConfig{&self.config}
    pub(crate) async fn client(&self,remote:bool)->Result<Arc<mongodb::Client>,String>{self.binding.refresh_if_changed().await?;self.clients.client(remote).await.cloned()}
    pub(crate) async fn verify_current(&self)->Result<(),String>{self.binding.refresh_if_changed().await}
}
#[derive(Clone)]
pub struct TaskEndpoint {kind:Arc<EndpointKind>}
enum EndpointKind {Embedded{control:Arc<TaskControl>,progress:Arc<progress::ProgressHub>},Runtime(RuntimeClient)}
impl TaskEndpoint {
    pub fn embedded(control:Arc<TaskControl>,context:Arc<TaskRuntimeContext>)->Self{Self{kind:Arc::new(EndpointKind::Embedded{control,progress:progress::ProgressHub::new(context)})}}
    pub fn runtime(client:RuntimeClient)->Self{Self{kind:Arc::new(EndpointKind::Runtime(client))}}
    pub async fn execute(&self,mut request:TaskRequest,observer:Option<RuntimeObserver>)->Result<CallResult,TransportError>{
        let cwd=std::env::current_dir().map_err(|_|TransportError::new("identity_mismatch","caller","caller cwd unavailable"))?;
        request.resolve_output(&cwd);
        match self.kind.as_ref(){
            EndpointKind::Embedded{control,..}=>crate::operations_runtime::with_environment(Some(crate::operations_runtime::OperationalEnvironment::capture().map_err(|e|TransportError::new("identity_mismatch","execution_context",e))?),crate::task_dispatch::dispatch(control,&request,observer)).await.map_err(|e|{let mut error=TransportError::new("application","dispatch",e);error.execution="unconfirmed".into();error}),
            EndpointKind::Runtime(client)=>client.call(CallContext::current()?,request,observer).await,
        }
    }
    pub async fn subscribe_progress(&self,sink:RuntimeObserver)->Result<ProgressSubscription,TransportError>{
        match self.kind.as_ref(){EndpointKind::Runtime(client)=>client.subscribe_progress(sink).await,
            EndpointKind::Embedded{progress,..}=>progress.client_subscription(sink).await}
    }
    pub async fn refresh_progress(&self)->Result<(),TransportError>{
        match self.kind.as_ref(){EndpointKind::Runtime(client)=>client.refresh_progress().await,
            EndpointKind::Embedded{progress,..}=>{progress.refresh().await;Ok(())}}
    }
}

#[cfg(all(test,target_os="macos"))]
mod tests;

/// Fix transport mode once at GUI startup, preserving original non-task data settings/pools.
/// Runtime enrollment is checked before connecting; failures cannot select embedded mode.
pub async fn bootstrap_gui(config:AppConfig)->Result<(AppConfig,Arc<crate::mongo_manager::MongoClients>,TaskEndpoint),String>{
    let root=std::path::Path::new(&config.env.working_dir);
    if let Some(selection)=RuntimeSelection::for_backend(root)?{
        let (config,clients)=TaskControl::bootstrap_gui_data(config).await?;
        let settings=crate::backend_binding::task_settings_fingerprint(&config)?;
        let client=RuntimeClient::connect_gui(&selection,&settings).await.map_err(|e|e.to_string())?;
        Ok((config,clients,TaskEndpoint::runtime(client)))
    } else {
        let(config,clients,control)=TaskControl::bootstrap_gui(config).await?.into_parts()?;
        let context=control.runtime_context()?;
        Ok((config,clients,TaskEndpoint::embedded(Arc::new(control),context)))
    }
}
