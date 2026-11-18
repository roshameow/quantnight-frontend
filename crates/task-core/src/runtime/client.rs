//! All adapters use this client. No business replay, service spawn, or database fallback.
use super::{identity::{self,RuntimeSelection},protocol::{Admission,CallContext,HostStatus,Message},codec,CallResult,RuntimeObserver,TransportError};
#[derive(Clone)]
pub struct RuntimeClient {selection:RuntimeSelection,mode:Admission,task_settings:Option<String>}
pub struct ProgressSubscription { pub(crate) task:Option<tokio::task::JoinHandle<()>> }
impl ProgressSubscription {pub fn is_alive(&self)->bool{self.task.as_ref().is_some_and(|task|!task.is_finished())}}
impl Drop for ProgressSubscription {fn drop(&mut self){if let Some(task)=self.task.take(){task.abort();}}}
impl RuntimeClient {
    /// Construct a client for the explicit selection. The operation itself opens and
    /// attests its one connection; a discarded probe cannot admit a later Call.
    pub fn business(selection: &RuntimeSelection) -> Self {
        Self { selection: selection.clone(), mode: Admission::Business, task_settings: None }
    }
    pub fn management(selection: &RuntimeSelection) -> Self {
        Self { mode: Admission::Management, ..Self::business(selection) }
    }
    pub async fn connect_gui(selection: &RuntimeSelection, task_settings: &str) -> Result<Self, TransportError> {
        let client = Self { task_settings: Some(task_settings.into()), ..Self::business(selection) };
        #[cfg(target_os = "macos")]
        { client.open().await?; Ok(client) }
        #[cfg(not(target_os = "macos"))]
        { let _ = client; Err(Self::unsupported()) }
    }
    #[cfg(not(target_os = "macos"))]
    fn unsupported() -> TransportError {
        TransportError::new("unsupported_platform", "connect", "runtime enrollment requires macOS; legacy mode is unchanged")
    }
    #[cfg(unix)]
    async fn open(&self) -> Result<(tokio::net::UnixStream, String), TransportError> {
        #[cfg(not(target_os = "macos"))]
        { Err(Self::unsupported()) }
        #[cfg(target_os = "macos")]
        {
            use super::protocol::{Hello, VERSION};
            use std::time::Duration;
            use tokio::{net::UnixStream, time::{timeout_at, Instant}};
            let deadline = Instant::now() + Duration::from_secs(10);
            let paths = self.selection.paths().map_err(identity::error)?;
            identity::check_socket(&self.selection)
                .map_err(|e| TransportError::new("runtime_socket", "connect", e))?;
            let stream = timeout_at(deadline, UnixStream::connect(paths.socket)).await
                .map_err(|_| TransportError::new("startup_timeout", "connect", "registered runtime socket connection exceeded the 10s startup budget; no request sent"))?
                .map_err(|e| TransportError::new("connect", "connect", format!("registered runtime socket connection failed: {e}; no direct fallback")))?;
            let admission = identity::client_preflight(&stream, &self.selection)
                .map_err(|e| TransportError::new("identity_mismatch", "hello", e))?;
            let hello = Hello { mode: self.mode.clone(), protocol: VERSION, selection: self.selection.clone(),
                core_source: crate::DEV_SOURCE_ID.into(), task_settings: self.task_settings.clone() };
            let (stream, host, call_id) = timeout_at(deadline, Self::hello(stream, hello)).await
                .map_err(|_| TransportError::new("startup_timeout", "hello", "registered runtime did not finish its handshake within the 10s startup budget; no request sent"))??;
            identity::client_postflight(&stream, &self.selection, &host, &admission, self.mode == Admission::Business)
                .map_err(identity::error)?;
            Ok((stream, call_id))
        }
    }
    #[cfg(any(target_os = "macos", all(test, unix, feature = "test-support")))]
    pub(crate) async fn hello(mut stream: tokio::net::UnixStream, hello: super::protocol::Hello)
        -> Result<(tokio::net::UnixStream, super::HostIdentity, String), TransportError>
    {
        let lost = |e| TransportError::new("connection_lost", "hello", e);
        codec::write_preamble(&mut stream).await.map_err(lost)?;
        codec::write(&mut stream, &Message::Hello(hello)).await.map_err(lost)?;
        codec::read_preamble(&mut stream).await.map_err(|e| match e {
            codec::PreambleError::Unavailable => lost("registered runtime ended the connection before its handshake; host startup or exit not confirmed".into()),
            codec::PreambleError::Mismatch => TransportError::new("protocol_mismatch", "hello", e.to_string()),
        })?;
        match codec::read(&mut stream).await.map_err(lost)? {
            Message::Ready { identity, call_id } => Ok((stream, identity, call_id)),
            Message::Error(mut e) => {
                // No Call was sent and this connection has not attested a Ready peer.
                e.execution = "not_sent".into();
                e.request_id = None;
                e.host_generation = None;
                Err(e)
            }
            _ => Err(TransportError::new("protocol_mismatch", "hello", "runtime did not return checked ready identity")),
        }
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
        let uncertain = |class: &str, message: String, generation: Option<String>| {
            let mut e = TransportError::new(class, "call", message);
            e.execution = "unconfirmed".into();
            e.request_id = Some(context.request_id.clone());
            e.host_generation = generation;
            e
        };
        let mut accepted_generation = None;
        codec::write_owned(&mut stream, Message::Call { context: context.clone(), request }).await
            .map_err(|e| uncertain("connection_lost", e, None))?;
        loop {
            match codec::read(&mut stream).await
                .map_err(|e| uncertain("connection_lost", e, accepted_generation.clone()))? {
                Message::Accepted { id, host_generation } if id == context.request_id => {
                    accepted_generation = Some(host_generation);
                }
                Message::Event(event) => {
                    if let Some(callback) = &observer { callback(event); }
                    else if let super::RuntimeEvent::ProgressGap { dropped } = event {
                        eprintln!("taskctl: {dropped} process updates coalesced; original completion receipt remains authoritative");
                    }
                }
                Message::Completed { id, result } if id == context.request_id => return Ok(result),
                Message::Error(mut e) => {
                    if accepted_generation.is_some() || e.execution != "not_executed" {
                        e.execution = "unconfirmed".into();
                    }
                    e.request_id = Some(context.request_id.clone());
                    e.host_generation = accepted_generation;
                    return Err(e);
                }
                _ => return Err(uncertain("protocol_mismatch", "unexpected runtime response; operation result unconfirmed".into(), accepted_generation)),
            }
        }
    }
    #[cfg(unix)]
    pub async fn management_status(&self)->Result<HostStatus,TransportError>{self.manage(Message::ManagementStatus).await}
    #[cfg(unix)]
    pub async fn drain(&self,expected_host_generation:&str)->Result<HostStatus,TransportError>{self.manage(Message::Drain{expected_host_generation:expected_host_generation.into()}).await}
    #[cfg(unix)]
    async fn manage(&self,message:Message)->Result<HostStatus,TransportError>{
        if self.mode!=Admission::Management{return Err(TransportError::new("identity_mismatch","management","management admission required"));}
        let (stream, _) = self.open().await?;
        Self::management_exchange(stream, message).await
    }
    #[cfg(unix)]
    pub(crate) async fn management_exchange(mut stream: tokio::net::UnixStream, message: Message) -> Result<HostStatus, TransportError> {
        let unconfirmed = |class: &str, message| {
            let mut e = TransportError::new(class, "management", message);
            e.execution = "unconfirmed".into();
            e
        };
        codec::write(&mut stream, &message).await.map_err(|e| unconfirmed("connection_lost", e))?;
        match codec::read(&mut stream).await.map_err(|e| unconfirmed("connection_lost", e))? {
            Message::HostStatus(status) => Ok(status),
            Message::Error(e) => Err(e),
            _ => Err(unconfirmed("protocol_mismatch", "unexpected management reply".into())),
        }
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
