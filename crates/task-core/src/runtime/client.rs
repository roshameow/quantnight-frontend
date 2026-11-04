//! All adapters use this client. No business replay, service spawn, or database fallback.
use super::{identity::{self,RuntimeSelection},protocol::{Admission,CallContext,Hello,HostStatus,Message,VERSION},codec,CallResult,RuntimeObserver,TransportError};
#[derive(Clone)]
pub struct RuntimeClient {selection:RuntimeSelection,mode:Admission,task_settings:Option<String>}
pub struct ProgressSubscription { pub(crate) task:Option<tokio::task::JoinHandle<()>> }
impl ProgressSubscription {pub fn is_alive(&self)->bool{self.task.as_ref().is_some_and(|task|!task.is_finished())}}
impl Drop for ProgressSubscription {fn drop(&mut self){if let Some(task)=self.task.take(){task.abort();}}}
impl RuntimeClient {
    pub async fn connect(selection:&RuntimeSelection)->Result<Self,TransportError>{Self::connect_mode(selection,Admission::Business).await}
    pub async fn connect_gui(selection:&RuntimeSelection,task_settings:&str)->Result<Self,TransportError>{
        #[cfg(not(target_os="macos"))] return Err(TransportError::new("unsupported_platform","connect","runtime enrollment requires macOS; legacy GUI remains available"));
        #[cfg(target_os="macos")]
        {let client=Self{selection:selection.clone(),mode:Admission::Business,task_settings:Some(task_settings.into())};
        #[cfg(unix)] client.open().await?;
        #[cfg(not(unix))] return Err(TransportError::new("unsupported_platform","connect","runtime requires macOS"));
        Ok(client)}
    }
    pub async fn connect_management(selection:&RuntimeSelection)->Result<Self,TransportError>{Self::connect_mode(selection,Admission::Management).await}
    async fn connect_mode(selection:&RuntimeSelection,mode:Admission)->Result<Self,TransportError>{
        #[cfg(not(target_os="macos"))] return Err(TransportError::new("unsupported_platform","connect","runtime enrollment requires macOS; legacy mode is unchanged"));
        #[cfg(target_os="macos")] {let client=Self{selection:selection.clone(),mode,task_settings:None};client.open().await?;Ok(client)}
    }
    #[cfg(unix)]
    async fn open(&self)->Result<(tokio::net::UnixStream,String),TransportError>{
        // The only startup deadline. Never applies to an accepted business call.
        tokio::time::timeout(std::time::Duration::from_secs(10),async{
            identity::check_socket(&self.selection).map_err(identity::error)?;
            let mut stream=tokio::net::UnixStream::connect(self.selection.paths().map_err(identity::error)?.socket).await.map_err(|_|TransportError::new("connect","connect","registered runtime socket connection failed; no direct fallback"))?;
            let admission=identity::client_preflight(&stream,&self.selection).map_err(|e|TransportError::new("peer_uid","hello",e))?;
            codec::write_preamble(&mut stream).await.map_err(|e|TransportError::new("protocol_mismatch","hello",e))?;
            codec::write(&mut stream,&Message::Hello(Hello{mode:self.mode.clone(),protocol:VERSION,selection:self.selection.clone(),core_source:crate::DEV_SOURCE_ID.into(),task_settings:self.task_settings.clone()})).await.map_err(|e|TransportError::new("codec","hello",e))?;
            codec::read_preamble(&mut stream).await.map_err(|e|TransportError::new("protocol_mismatch","hello",e))?;
            let call_id=match codec::read(&mut stream).await.map_err(|e|TransportError::new("connection_lost","hello",e))?{
                Message::Ready{identity:host,call_id}=>{identity::client_postflight(&stream,&self.selection,&host,&admission,self.mode==Admission::Business).map_err(identity::error)?;call_id},
                Message::Error(e)=>return Err(e),_=>return Err(TransportError::new("protocol_mismatch","hello","runtime did not return checked ready identity")),
            };Ok((stream,call_id))
        }).await.map_err(|_|TransportError::new("startup_timeout","hello","runtime startup/hello exceeded 10s; no business request sent"))?
    }
    #[cfg(unix)]
    pub async fn call(&self,mut context:CallContext,mut request:crate::task_request::TaskRequest,observer:Option<RuntimeObserver>)->Result<CallResult,TransportError>{
        if self.mode!=Admission::Business{return Err(TransportError::new("identity_mismatch","admission","management connection cannot execute business"));}
        context.validate()?;request.resolve_output(&context.caller_cwd);
        if let crate::task_request::TaskRequest::Pause(pause)=&request {
            let budget=pause.remaining_ms.unwrap_or(pause.budget.seconds()*1000).min(pause.budget.seconds()*1000);
            context.pause_deadline_ns=Some(identity::monotonic_ns().map_err(identity::error)?.saturating_add(budget.saturating_mul(1_000_000)));
        }
        if matches!(&request,crate::task_request::TaskRequest::TaskDiagnostics{..}) {
            context.diagnostic_deadline_ns=Some(identity::monotonic_ns().map_err(identity::error)?.saturating_add(crate::task_diagnostics::RUNTIME_BUDGET.as_nanos() as u64));
        }
        let (stream,call_id)=self.open().await?;
        context.request_id=call_id;context.validate()?;
        Self::exchange(stream,context,request,observer).await
    }
    /// One admitted socket is the lowest transport seam; production and tests use the same exchange.
    #[cfg(unix)]
    pub(crate) async fn exchange(mut stream:tokio::net::UnixStream,context:CallContext,request:crate::task_request::TaskRequest,observer:Option<RuntimeObserver>)->Result<CallResult,TransportError>{
        let uncertain=|class:&str,message:String|{let mut e=TransportError::new(class,"call",message);e.execution="unconfirmed".into();e.request_id=Some(context.request_id.clone());e};
        codec::write_owned(&mut stream,Message::Call{context:context.clone(),request}).await.map_err(|e|uncertain("connection_lost",e))?;
        loop {match codec::read(&mut stream).await.map_err(|e|uncertain("connection_lost",e))? {
            Message::Accepted{id,..} if id==context.request_id=>{},
            Message::Event(event)=>{if let Some(callback)=&observer{callback(event);}else if let super::RuntimeEvent::ProgressGap{dropped}=event{eprintln!("taskctl: {dropped} process updates coalesced; original completion receipt remains authoritative");}},
            Message::Completed{id,result} if id==context.request_id=>return Ok(result),
            Message::Error(mut e)=>{if e.execution!="not_executed"{e.execution="unconfirmed".into();}e.request_id=Some(context.request_id.clone());return Err(e);},
            _=>return Err(uncertain("protocol_mismatch","unexpected runtime response; operation result unconfirmed".into())),
        }}
    }
    #[cfg(unix)]
    pub async fn management_status(&self)->Result<HostStatus,TransportError>{self.manage(Message::ManagementStatus).await}
    #[cfg(unix)]
    pub async fn drain(&self,expected_host_generation:&str)->Result<HostStatus,TransportError>{self.manage(Message::Drain{expected_host_generation:expected_host_generation.into()}).await}
    #[cfg(unix)]
    async fn manage(&self,message:Message)->Result<HostStatus,TransportError>{
        if self.mode!=Admission::Management{return Err(TransportError::new("identity_mismatch","management","management admission required"));}
        let(mut stream,_)=self.open().await?;codec::write(&mut stream,&message).await.map_err(|e|TransportError::new("connection_lost","management",e))?;
        match codec::read(&mut stream).await.map_err(|e|TransportError::new("connection_lost","management",e))?{Message::HostStatus(status)=>Ok(status),Message::Error(e)=>Err(e),_=>Err(TransportError::new("protocol_mismatch","management","unexpected management reply"))}
    }
    #[cfg(unix)]
    pub async fn subscribe_progress(&self,sink:RuntimeObserver)->Result<ProgressSubscription,TransportError>{
        if self.mode!=Admission::Business{return Err(TransportError::new("identity_mismatch","subscribe","business admission required"));}
        let(mut stream,_)=self.open().await?;codec::write(&mut stream,&Message::Subscribe).await.map_err(|e|TransportError::new("connection_lost","subscribe",e))?;
        match codec::read(&mut stream).await.map_err(|e|TransportError::new("connection_lost","subscribe",e))?{Message::Subscribed=>{},Message::Error(e)=>return Err(e),_=>return Err(TransportError::new("protocol_mismatch","subscribe","subscription acknowledgement missing"))}
        Ok(ProgressSubscription{task:Some(tokio::spawn(async move{loop{match codec::read(&mut stream).await{Ok(Message::Event(event))=>sink(event),_=>{sink(super::RuntimeEvent::GuiProgress{name:"task-progress-refresh-error".into(),payload:serde_json::json!({"scope":"all","error":"runtime progress connection ended; previous task data retained, no operation replayed","updated_at":chrono::Utc::now().timestamp_millis()})});break;}}}}))})
    }
    #[cfg(unix)]
    pub async fn refresh_progress(&self)->Result<(),TransportError>{
        let(mut stream,_)=self.open().await?;codec::write(&mut stream,&Message::Refresh).await.map_err(|e|TransportError::new("connection_lost","refresh",e))?;
        match codec::read(&mut stream).await.map_err(|e|TransportError::new("connection_lost","refresh",e))?{Message::Refreshed=>Ok(()),Message::Error(e)=>Err(e),_=>Err(TransportError::new("protocol_mismatch","refresh","refresh acknowledgement missing"))}
    }
}

#[cfg(not(unix))]
impl RuntimeClient {
    pub async fn call(&self,_:CallContext,_:crate::task_request::TaskRequest,_:Option<RuntimeObserver>)->Result<CallResult,TransportError>{Err(TransportError::new("unsupported_platform","call","runtime requires macOS; legacy entry remains available"))}
    pub async fn management_status(&self)->Result<HostStatus,TransportError>{Err(TransportError::new("unsupported_platform","management","runtime requires macOS"))}
    pub async fn drain(&self,_:&str)->Result<HostStatus,TransportError>{self.management_status().await}
    pub async fn subscribe_progress(&self,_:RuntimeObserver)->Result<ProgressSubscription,TransportError>{Err(TransportError::new("unsupported_platform","progress","runtime requires macOS"))}
    pub async fn refresh_progress(&self)->Result<(),TransportError>{Err(TransportError::new("unsupported_platform","progress","runtime requires macOS"))}
}
