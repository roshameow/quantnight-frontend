//! A process host, not a second task scheduler. Accepted operations outlive connections.
use std::{collections::HashSet,sync::{Arc,Mutex}};
use super::{identity::{self,HostIdentity,RuntimeSelection},protocol::{Admission,HostStatus,Message,VERSION},codec,RuntimeObserver,TransportError};
use crate::task_control::TaskControl;
#[derive(Default)]
struct Inflight {draining:bool,active:u64,accepted_calls:u64,kind_counts:[u64;20],progress_subscriptions:u64,ids:HashSet<String>}
pub struct RuntimeState {pub control:Arc<TaskControl>,pub identity:HostIdentity,inflight:Mutex<Inflight>,settled:tokio::sync::Notify,progress:Option<Arc<super::progress::ProgressHub>>}
impl RuntimeState {
    pub fn new(control:Arc<TaskControl>,identity:HostIdentity)->Arc<Self>{{let progress=control.runtime_context().ok().map(super::progress::ProgressHub::new);Arc::new(Self{control,identity,inflight:Mutex::new(Inflight::default()),settled:tokio::sync::Notify::new(),progress})}}
    pub fn status(&self)->HostStatus{let state=self.inflight.lock().unwrap_or_else(|e|e.into_inner());HostStatus{identity:self.identity.clone(),draining:state.draining,active:state.active,accepted_calls:state.accepted_calls,accepted_by_kind:crate::task_request::REQUEST_KIND_NAMES.iter().enumerate().map(|(index,name)|((*name).to_string(),state.kind_counts[index])).collect(),progress_subscriptions:state.progress_subscriptions,pools:self.control.runtime_context().map(|context|context.pool_snapshot()).unwrap_or_default()}}
    pub fn drain(&self){self.inflight.lock().unwrap_or_else(|e|e.into_inner()).draining=true;self.settled.notify_waiters();}
    pub async fn wait_drained(&self){loop{let notified=self.settled.notified();if self.status().active==0{return;}notified.await;}}
    fn accept(self:&Arc<Self>,id:&str,kind:crate::task_request::RequestKind)->Result<ActiveGuard,TransportError>{
        let mut state=self.inflight.lock().unwrap_or_else(|e|e.into_inner());
        let reason=if state.draining{Some(("host_draining","runtime is draining; business not executed"))}else if !state.ids.insert(id.into()){Some(("duplicate_request","duplicate request id; original result unconfirmed, not replayed"))}else{None};
        if let Some((class,message))=reason{let mut e=TransportError::new(class,"admission",message);e.execution="not_executed".into();return Err(e);}
        state.active+=1;state.accepted_calls=state.accepted_calls.saturating_add(1);state.kind_counts[kind as usize]=state.kind_counts[kind as usize].saturating_add(1);Ok(ActiveGuard(self.clone(),id.into()))
    }
}
struct ActiveGuard(Arc<RuntimeState>,String);
impl Drop for ActiveGuard{fn drop(&mut self){let mut state=self.0.inflight.lock().unwrap_or_else(|e|e.into_inner());state.active-=1;state.ids.remove(&self.1);drop(state);self.0.settled.notify_waiters();}}
fn business_fresh(host:&HostIdentity)->Result<(),String>{
    host.selection.verify_current()?;identity::core_fresh(&host.selection)?;
    identity::cli_fresh(&host.selection,&host.cli_source)?;identity::host_image(&host.selection)?;
    identity::task_settings_fresh(&host.selection,&host.task_settings)
}
pub(crate) fn operation_observer()->(RuntimeObserver,tokio::sync::mpsc::Receiver<super::RuntimeEvent>,Arc<std::sync::atomic::AtomicU64>){
    let(events,receiver)=tokio::sync::mpsc::channel(64);
    let dropped=Arc::new(std::sync::atomic::AtomicU64::new(0));let lost=dropped.clone();
    let observer:RuntimeObserver=Arc::new(move|event|{if events.try_send(event).is_err(){lost.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}});
    (observer,receiver,dropped)
}
async fn flush_progress(stream:&mut (impl tokio::io::AsyncWrite+Unpin),receiver:&mut tokio::sync::mpsc::Receiver<super::RuntimeEvent>,dropped:&std::sync::atomic::AtomicU64)->Result<(),String>{
    while let Ok(event)=receiver.try_recv(){codec::write(stream,&Message::Event(event)).await?;}
    let lost=dropped.swap(0,std::sync::atomic::Ordering::Relaxed);
    if lost>0{codec::write(stream,&Message::Event(super::RuntimeEvent::ProgressGap{dropped:lost})).await?;}Ok(())
}
fn not_executed(class:&str,message:impl Into<String>)->Message{let mut error=TransportError::new(class,"admission",message);error.execution="not_executed".into();Message::Error(error)}

#[cfg(unix)]
pub async fn serve_connection(mut stream:tokio::net::UnixStream,state:Arc<RuntimeState>)->Result<(),String>{
    identity::peer_uid(&stream)?;
    let hello=tokio::time::timeout(std::time::Duration::from_secs(10),async{
        codec::read_preamble(&mut stream).await?;match codec::read(&mut stream).await?{Message::Hello(hello)=>Ok::<_,String>(hello),_=>Err("missing IPC hello".into())}
    }).await.map_err(|_|"hello deadline expired")??;
    codec::write_preamble(&mut stream).await?;
    if hello.protocol!=VERSION || !identity::management_selection_matches(&hello.selection,&state.identity.selection){codec::write(&mut stream,&not_executed("identity_mismatch","runtime UID/backend/selection/protocol mismatch")).await?;return Ok(());}
    if hello.mode==Admission::Business {
        let fresh=business_fresh(&state.identity);
        if hello.task_settings.as_ref().is_some_and(|settings|settings!=&state.identity.task_settings){return codec::write(&mut stream,&not_executed("identity_mismatch","GUI task script settings differ from selected runtime AppConfig; select the original AppConfig and safe reload, no fallback")).await;}
        if hello.selection!=state.identity.selection || hello.core_source!=state.identity.core_source || fresh.is_err(){codec::write(&mut stream,&not_executed("source_stale","runtime business generation is stale; manage/drain original host without replay")).await?;return Ok(());}
    }
    let call_id=identity::nonce()?;
    codec::write(&mut stream,&Message::Ready{identity:state.identity.clone(),call_id:call_id.clone()}).await?;
    let message=codec::read(&mut stream).await?;
    if hello.mode==Admission::Management {
        let reply=match message{Message::ManagementStatus=>Message::HostStatus(state.status()),Message::Drain{expected_host_generation} if expected_host_generation==state.identity.generation=>{state.drain();Message::HostStatus(state.status())},_=>not_executed("identity_mismatch","management command/generation mismatch")};
        return codec::write(&mut stream,&reply).await;
    }
    // Admission belongs to the complete request, not only an earlier hello.
    if business_fresh(&state.identity).is_err(){return codec::write(&mut stream,&not_executed("source_stale","runtime generation changed before request admission; not executed")).await;}
    execute_bound(stream,state,message,&call_id).await
}
#[cfg(unix)]
pub(crate) async fn execute_bound(mut stream:tokio::net::UnixStream,state:Arc<RuntimeState>,message:Message,call_id:&str)->Result<(),String>{
    if let Message::Call{context,..}=&message {if context.request_id!=call_id{return codec::write(&mut stream,&not_executed("identity_mismatch","call id is not bound to this ready connection; original frame not replayed")).await;}}
    execute_message(stream,state,message).await
}
#[cfg(unix)]
pub(crate) async fn execute_message(mut stream:tokio::net::UnixStream,state:Arc<RuntimeState>,message:Message)->Result<(),String>{
    if state.status().draining && matches!(message,Message::Subscribe|Message::Refresh){return codec::write(&mut stream,&not_executed("host_draining","runtime is draining; observation request not accepted")).await;}
    match message {
        Message::Call{context,request}=>{
            if let Err(mut e)=context.validate(){e.execution="not_executed".into();return codec::write(&mut stream,&Message::Error(e)).await;}
            let guard=match state.accept(&context.request_id,request.kind()){Ok(guard)=>guard,Err(e)=>return codec::write(&mut stream,&Message::Error(e)).await};
            let id=context.request_id.clone();let operation_state=state.clone();
            let(observer,mut event_rx,dropped)=operation_observer();
            let (done,result)=tokio::sync::oneshot::channel();
            // This task owns its guard and original core independently of the writer.
            // Dropping a connection/JoinHandle does not abort Tokio's spawned task.
            tokio::spawn(async move{
                let _guard=guard;let mut request=request;request.resolve_output(&context.caller_cwd);
                if let (crate::task_request::TaskRequest::Pause(pause),Some(deadline))=(&mut request,context.pause_deadline_ns){
                    pause.remaining_ms=Some(deadline.saturating_sub(identity::monotonic_ns().unwrap_or(u64::MAX))/1_000_000);
                }
                let remaining=context.diagnostic_deadline_ns.map(|deadline|std::time::Duration::from_nanos(deadline.saturating_sub(identity::monotonic_ns().unwrap_or(u64::MAX))));
                let outcome=crate::task_diagnostics::with_call_budget(remaining,crate::operations_runtime::with_environment(Some(context.operational_env),crate::task_dispatch::dispatch(&operation_state.control,&request,Some(observer)))).await;
                let _=done.send(outcome);
            });
            let(mut reader,mut writer)=stream.into_split();
            let exchange=async {
            codec::write(&mut writer,&Message::Accepted{id:id.clone(),host_generation:state.identity.generation.clone()}).await?;
            tokio::pin!(result);
            loop{
                let lost=dropped.swap(0,std::sync::atomic::Ordering::Relaxed);
                if lost>0{codec::write(&mut writer,&Message::Event(super::RuntimeEvent::ProgressGap{dropped:lost})).await?;}
                tokio::select!{
                event=event_rx.recv()=>{if let Some(event)=event{codec::write(&mut writer,&Message::Event(event)).await?;}else{break;}},
                outcome=&mut result=>{flush_progress(&mut writer,&mut event_rx,&dropped).await?;let reply=match outcome{Ok(Ok(result))=>Message::Completed{id,result},Ok(Err(error))=>{let mut e=TransportError::new("application","dispatch",error);e.execution="unconfirmed".into();Message::Error(e)},Err(_)=>{let mut e=TransportError::new("connection_lost","operation","original operation interrupted; result unconfirmed");e.execution="unconfirmed".into();Message::Error(e)}};return codec::write_owned(&mut writer,reply).await;}
            }}
            // All event senders can drop just before the oneshot becomes ready.
            flush_progress(&mut writer,&mut event_rx,&dropped).await?;
            let reply=match result.await{Ok(Ok(result))=>Message::Completed{id,result},Ok(Err(error))=>{let mut e=TransportError::new("application","dispatch",error);e.execution="unconfirmed".into();Message::Error(e)},Err(_)=>{let mut e=TransportError::new("application","operation","operation result unconfirmed; inspect original business receipts");e.execution="unconfirmed".into();Message::Error(e)}};
            codec::write_owned(&mut writer,reply).await
            };
            tokio::pin!(exchange);let mut unexpected=[0_u8;1];
            // EOF ends observation only. The accepted original task above is
            // detached and keeps its own core/lock/receipt owner until finish.
            tokio::select!{reply=&mut exchange=>reply,_=tokio::io::AsyncReadExt::read(&mut reader,&mut unexpected)=>Ok(())}

        },
        Message::Subscribe=>{
            let hub=state.progress.as_ref().ok_or("checked task progress context unavailable")?;
            let(mut events,_lease)=match hub.subscribe().await{Ok(pair)=>pair,Err(e)=>return codec::write(&mut stream,&Message::Error(e)).await};
            {let mut counts=state.inflight.lock().unwrap_or_else(|error|error.into_inner());counts.progress_subscriptions=counts.progress_subscriptions.saturating_add(1);}
            codec::write(&mut stream,&Message::Subscribed).await?;
            loop {tokio::select!{
                event=events.recv()=>match event{Ok(event)=>codec::write(&mut stream,&Message::Event(event)).await?,Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped))=>{codec::write(&mut stream,&Message::Event(super::RuntimeEvent::ProgressGap{dropped})).await?;hub.refresh().await},Err(_)=>break},
                _=stream.readable()=>{let mut byte=[0];match stream.try_read(&mut byte){Ok(0)=>break,Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{},_=>break}}
            }}Ok(())
        },
        Message::Refresh=>{if let Some(hub)=&state.progress{hub.refresh().await;}codec::write(&mut stream,&Message::Refreshed).await},
        _=>codec::write(&mut stream,&not_executed("protocol_mismatch","business request expected")).await,
    }
}
#[cfg(unix)]
pub async fn serve(listener:tokio::net::UnixListener,state:Arc<RuntimeState>)->Result<(),String>{
    loop{let (stream,_)=listener.accept().await.map_err(|_|"runtime accept failed")?;let state=state.clone();tokio::spawn(async move{let _=serve_connection(stream,state).await;});}
}
#[cfg(target_os="macos")]
fn activated_listener(selection:&RuntimeSelection)->Result<(tokio::net::UnixListener,Option<tokio::net::UnixStream>),String>{
    use std::os::fd::FromRawFd;
    extern "C"{fn launch_activate_socket(name:*const libc::c_char,fds:*mut *mut libc::c_int,count:*mut libc::size_t)->libc::c_int;}
    let mut fds=std::ptr::null_mut();let mut count=0;
    if unsafe{launch_activate_socket(b"TaskRuntime\0".as_ptr().cast(),&mut fds,&mut count)}!=0{return Err("runtime-host requires its original enrolled launchd socket; no private spawn/bind fallback".into());}
    let descriptors=unsafe{std::slice::from_raw_parts(fds,count)}.to_vec();unsafe{libc::free(fds.cast())};
    if descriptors.len()!=1{for fd in descriptors{unsafe{libc::close(fd);}}return Err("launchd runtime requires exactly one socket descriptor".into());}
    let listener=unsafe{std::os::unix::net::UnixListener::from_raw_fd(descriptors[0])};
    checked_inherited_listener(listener,&selection.paths()?.socket)
}
#[cfg(target_os="macos")]
fn checked_inherited_listener(listener:std::os::unix::net::UnixListener,expected:&std::path::Path)->Result<(tokio::net::UnixListener,Option<tokio::net::UnixStream>),String>{
    use std::os::fd::AsRawFd;
    let address=listener.local_addr().map_err(|_|"launchd listener identity unavailable")?;
    if !address.as_pathname().is_some_and(|actual|unix_endpoint_matches(actual,expected)){return Err("launchd listener pathname differs from enrollment".into());}
    unsafe{libc::fcntl(listener.as_raw_fd(),libc::F_SETFD,libc::FD_CLOEXEC);}
    let mut kind=0;let mut size=std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    if unsafe{libc::getsockopt(listener.as_raw_fd(),libc::SOL_SOCKET,libc::SO_TYPE,(&mut kind as *mut libc::c_int).cast(),&mut size)}!=0 || kind!=libc::SOCK_STREAM{return Err("launchd descriptor must be a Unix stream listener".into());}
    listener.set_nonblocking(true).map_err(|_|"runtime listener nonblocking setup failed")?;
    // Darwin does not implement SO_ACCEPTCONN (ENOPROTOOPT even after listen).
    // A real nonblocking accept validates the listener without changing its
    // backlog. Preserve a queued first connection; never probe and discard it.
    let first=match listener.accept(){
        Ok((stream,_))=>{stream.set_nonblocking(true).map_err(|_|"runtime first connection nonblocking setup failed")?;Some(tokio::net::UnixStream::from_std(stream).map_err(|_|"runtime first connection unavailable")?)},
        Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>None,
        Err(_)=>return Err("launchd descriptor must be an accepting Unix stream listener".into()),
    };
    let listener=tokio::net::UnixListener::from_std(listener).map_err(|_|"runtime listener unavailable")?;Ok((listener,first))
}
#[cfg(target_os="macos")]
fn unix_endpoint_matches(actual:&std::path::Path,expected:&std::path::Path)->bool{
    use std::{ffi::OsStr,os::unix::ffi::OsStrExt,path::Path};
    // launchd can return a full-size sockaddr_un; Rust's local_addr may retain
    // its trailing NUL padding. Only the C pathname terminator/padding is
    // decoded, not case/canonical path aliases or bytes after a nonzero suffix.
    let bytes=actual.as_os_str().as_bytes();let end=bytes.iter().position(|byte|*byte==0).unwrap_or(bytes.len());
    bytes[end..].iter().all(|byte|*byte==0) && Path::new(OsStr::from_bytes(&bytes[..end]))==expected
}
#[cfg(target_os="macos")]
fn serve_retained_connection(first:Option<tokio::net::UnixStream>,state:Arc<RuntimeState>)->Option<tokio::task::JoinHandle<()>>{
    first.map(|stream|tokio::spawn(async move{let _=serve_connection(stream,state).await;}))
}
#[cfg(target_os="macos")]
pub async fn run_host(selection:RuntimeSelection,cli_source:&str)->Result<(),String>{
    use std::os::{fd::AsRawFd,unix::fs::OpenOptionsExt};
    let current=std::env::current_exe().and_then(|p|p.canonicalize()).map_err(|_|"host executable unavailable")?;
    if current!=selection.executable{return Err("runtime-host must be the exact registered independent taskctl executable".into());}
    selection.verify_current()?;identity::core_fresh(&selection)?;let image=identity::host_image(&selection)?;
    let paths=selection.paths()?;identity::check_directory(&paths.directory)?;identity::check_socket(&selection)?;
    if paths.lock.exists(){identity::checked_read(&paths.lock)?;}
    let lock=std::fs::OpenOptions::new().create(true).append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&paths.lock).map_err(|_|"runtime host lock unavailable")?;
    if unsafe{libc::flock(lock.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB)}!=0{return Err("another host owns this runtime binding".into());}
    let (listener,first)=activated_listener(&selection)?;
    let config=identity::load_host_config(&selection).await?;
    let native_owner=identity::process_identity(std::process::id())?;let instance=identity::nonce()?;
    let mut control=TaskControl::connect(&config).await?;
    control.attach_generation_owner(crate::generation_owner::HostedGenerationOwner::new(native_owner.clone(),&instance));
    let control=Arc::new(control);
    let host=HostIdentity{selection,uid:identity::uid(),owner:native_owner,generation:instance,image,core_source:crate::DEV_SOURCE_ID.into(),cli_source:cli_source.into(),mission_database:config.mongodb.databases.mission.clone(),simulation_database:config.mongodb.databases.simulation.clone(),task_settings:crate::backend_binding::task_settings_fingerprint(&config)?,ready:true};
    identity::publish_ready(&host)?;let state=RuntimeState::new(control,host);
    let mut signal=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|_|"host termination signal unavailable")?;
    serve_retained_connection(first,state.clone());
    tokio::select!{result=serve(listener,state.clone())=>result,_=signal.recv()=>{state.drain();state.wait_drained().await;Ok(())}}
}
#[cfg(not(target_os="macos"))]
pub async fn run_host(_:RuntimeSelection,_:&str)->Result<(),String>{Err("runtime host requires macOS; legacy standalone CLI remains available".into())}


#[cfg(all(test,feature="test-support",target_os="macos"))]
mod activation_tests {
    use super::*;
    use std::{fs,path::{Path,PathBuf},process::Command,os::{fd::{AsRawFd,FromRawFd,IntoRawFd},unix::{ffi::OsStrExt,fs::PermissionsExt}}};
    use super::super::protocol::Hello;
    fn acceptconn_fact(fd:libc::c_int)->serde_json::Value{
        let mut accepting=0;let mut size=std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        let rc=unsafe{libc::getsockopt(fd,libc::SOL_SOCKET,libc::SO_ACCEPTCONN,(&mut accepting as *mut libc::c_int).cast(),&mut size)};
        if rc==0{serde_json::json!({"supported":true,"accepting":accepting})}else{let error=std::io::Error::last_os_error();assert_eq!(error.raw_os_error(),Some(libc::ENOPROTOOPT));serde_json::json!({"supported":false,"errno":error.raw_os_error()})}
    }
    fn bound_unlistening(path:&Path)->std::os::unix::net::UnixListener{
        let fd=unsafe{libc::socket(libc::AF_UNIX,libc::SOCK_STREAM,0)};assert!(fd>=0);
        let owned=unsafe{std::os::unix::net::UnixListener::from_raw_fd(fd)};
        let mut address:libc::sockaddr_un=unsafe{std::mem::zeroed()};let bytes=path.as_os_str().as_bytes();assert!(bytes.len()+1<address.sun_path.len());
        address.sun_len=(2+bytes.len()+1) as u8;address.sun_family=libc::AF_UNIX as u8;
        for(target,source)in address.sun_path.iter_mut().zip(bytes){*target=*source as libc::c_char;}
        assert_eq!(unsafe{libc::bind(fd,(&address as *const libc::sockaddr_un).cast(),address.sun_len as libc::socklen_t)},0);owned
    }
    #[tokio::test]
    async fn native_accept_checks_reject_unlistening_wrong_path_type_and_preserve_empty_listener(){
        use std::ffi::OsStr;
        let endpoint=Path::new("/fixture/selected.sock");
        assert!(unix_endpoint_matches(Path::new(OsStr::from_bytes(b"/fixture/selected.sock\0\0\0")),endpoint));
        assert!(!unix_endpoint_matches(Path::new(OsStr::from_bytes(b"/fixture/selected.sock\0other")),endpoint));
        assert!(!unix_endpoint_matches(Path::new(OsStr::from_bytes(b"/fixture/other.sock\0\0")),endpoint));
        assert!(!unix_endpoint_matches(Path::new(OsStr::from_bytes(b"\0\0")),endpoint));
        assert!(!unix_endpoint_matches(Path::new(OsStr::from_bytes(b"\0abstract-name")),endpoint));
        let root=Path::new("/tmp").join(format!("qn-fd-{}",&identity::nonce().unwrap()[..12]));fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();
        let path=root.join("bound");let bound=bound_unlistening(&path);let before=acceptconn_fact(bound.as_raw_fd());
        let error=checked_inherited_listener(bound,&path).err().expect("bound non-listening stream must fail");assert!(error.contains("accepting"));
        let path=root.join("listening");let listener=std::os::unix::net::UnixListener::bind(&path).unwrap();let after=acceptconn_fact(listener.as_raw_fd());
        let(listener,first)=checked_inherited_listener(listener,&path).unwrap();assert!(first.is_none(),"empty real listener returns WouldBlock, not a fabricated accepted stream");drop(listener);
        let path=root.join("wrong-path");let listener=std::os::unix::net::UnixListener::bind(&path).unwrap();assert!(checked_inherited_listener(listener,&root.join("other")).err().unwrap().contains("pathname"));
        let path=root.join("datagram");let datagram=std::os::unix::net::UnixDatagram::bind(&path).unwrap();let fd=datagram.into_raw_fd();let wrong=unsafe{std::os::unix::net::UnixListener::from_raw_fd(fd)};assert!(checked_inherited_listener(wrong,&path).err().unwrap().contains("stream"));
        eprintln!("darwin_acceptconn_actual_facts before={before} after={after}; nonlistener rejected; real empty listener accepted via actual accept WouldBlock");fs::remove_dir_all(root).unwrap();
    }
    struct LaunchFixture {root:PathBuf,slot:PathBuf,domain:String,registered:bool}
    impl LaunchFixture {
        fn cleanup(&mut self){if self.registered{let out=Command::new("/bin/launchctl").args(["bootout",&self.domain]).output();if let Err(error)=out{eprintln!("isolated activation fixture cleanup error: {error}");}self.registered=false;}let _=fs::remove_dir_all(&self.slot);let _=fs::remove_dir_all(&self.root);}
    }
    impl Drop for LaunchFixture{fn drop(&mut self){self.cleanup();}}
    fn xml(value:&str)->String{value.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
    async fn raw_hello(stream:&mut tokio::net::UnixStream,selection:&RuntimeSelection,mode:Admission,core:&str,protocol:u32)->Message{
        codec::write_preamble(stream).await.unwrap();codec::write(stream,&Message::Hello(Hello{mode,protocol,selection:selection.clone(),core_source:core.into(),task_settings:None})).await.unwrap();codec::read_preamble(stream).await.unwrap();codec::read(stream).await.unwrap()
    }
    const CHILD_ENV:&str="QUANTNIGHT_ISOLATED_ACTIVATION_SELECTION";
    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn real_launchd_inherited_listener_retains_first_management_connection(){
        if let Some(input)=std::env::var_os(CHILD_ENV){
            let selection:RuntimeSelection=serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(20),async{
                identity::check_socket(&selection).unwrap();
                // The production launch_activate_socket function, not bind or
                // an injected activation result, receives this actual fd.
                let(listener,first)=activated_listener(&selection).unwrap();assert!(first.is_some(),"socket-demand triggering connection must be retained");
                for fd in [listener.as_raw_fd(),first.as_ref().unwrap().as_raw_fd()]{
                    let descriptor_flags=unsafe{libc::fcntl(fd,libc::F_GETFD)};let status_flags=unsafe{libc::fcntl(fd,libc::F_GETFL)};
                    assert!(descriptor_flags>=0 && status_flags>=0);assert_ne!(descriptor_flags&libc::FD_CLOEXEC,0,"actual inherited/prefetched descriptor must be close-on-exec");assert_ne!(status_flags&libc::O_NONBLOCK,0,"actual inherited/prefetched descriptor must be nonblocking");
                }
                let(listener,empty)=checked_inherited_listener(listener.into_std().unwrap(),&selection.paths().unwrap().socket).unwrap();assert!(empty.is_none(),"no second connection is queued before the first reply");
                let(_files,fixture_state)=super::super::tests::activation_fixture().await;let mut host=fixture_state.identity.clone();host.selection=selection;identity::publish_ready(&host).unwrap();let state=RuntimeState::new(fixture_state.control.clone(),host);
                serve_retained_connection(first,state.clone()).unwrap().await.unwrap();
                for _ in 0..3{let(stream,_)=listener.accept().await.unwrap();serve_connection(stream,state.clone()).await.unwrap();}
                assert_eq!(state.status().active,0);assert_eq!(state.status().accepted_calls,0,"management/source-negative fixture must not dispatch a business operation");
            }).await.expect("bounded isolated activation child");return;
        }
        assert!(identity::uid()>0,"isolated user launchd fixture requires the existing non-root user");
        let root=Path::new("/tmp").join(format!("qn-la-{}",&identity::nonce().unwrap()[..12]));fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
        let selection=RuntimeSelection{binding_path:root.join("fixture-binding.json"),backend_root:root.clone(),frontend_root:root.clone(),executable:std::env::current_exe().unwrap(),configuration:identity::ConfigurationSelector{kind:"backend_root".into(),path:root.clone()},protocol_version:VERSION,binding_revision:"isolated-fixture".into(),target:crate::DEV_TARGET.into(),profile:crate::DEV_PROFILE.into()};
        let paths=selection.paths().unwrap();fs::create_dir_all(paths.directory.parent().unwrap()).unwrap();fs::create_dir(&paths.directory).unwrap();fs::set_permissions(&paths.directory,fs::Permissions::from_mode(0o700)).unwrap();
        let label=format!("com.quantnight.test.activation.{}",identity::nonce().unwrap());let domain=format!("gui/{}/{}",identity::uid(),label);let mut fixture=LaunchFixture{root:root.clone(),slot:paths.directory.clone(),domain:domain.clone(),registered:false};
        let input=root.join("selection.json");fs::write(&input,serde_json::to_vec(&selection).unwrap()).unwrap();let plist=root.join("agent.plist");
        let body=format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>--exact</string><string>runtime::server::activation_tests::real_launchd_inherited_listener_retains_first_management_connection</string><string>--nocapture</string></array><key>WorkingDirectory</key><string>{}</string><key>EnvironmentVariables</key><dict><key>{CHILD_ENV}</key><string>{}</string><key>PATH</key><string>/usr/bin:/bin</string></dict><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string><key>Sockets</key><dict><key>TaskRuntime</key><dict><key>SockFamily</key><string>Unix</string><key>SockType</key><string>stream</string><key>SockPathName</key><string>{}</string><key>SockPathMode</key><integer>384</integer></dict></dict><key>ExitTimeOut</key><integer>5</integer></dict></plist>"#,label,xml(selection.executable.to_str().unwrap()),xml(root.to_str().unwrap()),xml(input.to_str().unwrap()),xml(root.join("stdout").to_str().unwrap()),xml(root.join("stderr").to_str().unwrap()),xml(paths.socket.to_str().unwrap()));
        fs::write(&plist,body).unwrap();let bootstrap=Command::new("/bin/launchctl").args(["bootstrap",&format!("gui/{}",identity::uid())]).arg(&plist).output().unwrap();assert!(bootstrap.status.success(),"isolated bootstrap: {}",String::from_utf8_lossy(&bootstrap.stderr));fixture.registered=true;
        // bootstrap may publish the private socket before chmod/listen finish.
        // Fixture readiness only: no Hello/Call bytes or accepted operation is
        // sent/replayed before the first successful connection. Production
        // startup/retry policy and its original budgets are not changed.
        let mut first=tokio::time::timeout(std::time::Duration::from_secs(5),async{loop{
            match tokio::net::UnixStream::connect(&paths.socket).await{
                Ok(stream)=>break stream,
                Err(error) if matches!(error.kind(),std::io::ErrorKind::ConnectionRefused|std::io::ErrorKind::PermissionDenied|std::io::ErrorKind::NotFound)=>tokio::time::sleep(std::time::Duration::from_millis(10)).await,
                Err(error)=>panic!("isolated fixture connect failed: {error}"),
            }
        }}).await.expect("bounded bootstrap fixture socket readiness");identity::check_socket(&selection).unwrap();let first_reply=tokio::time::timeout(std::time::Duration::from_secs(20),raw_hello(&mut first,&selection,Admission::Management,"stale-client",VERSION)).await.unwrap_or_else(|error|panic!("first activation hello {error}; child stdout={:?}; stderr={:?}",fs::read_to_string(root.join("stdout")),fs::read_to_string(root.join("stderr"))));
        let host=match first_reply{Message::Ready{identity,..}=>identity,other=>panic!("first management hello: {other:?}")};identity::peer_host(&first,&host).unwrap();let native=crate::native_identity::attest_process(&host.owner).unwrap();assert_eq!(native.uid,identity::uid());assert_eq!(native.identity,host.owner);assert_eq!(native.executable,selection.executable);
        codec::write(&mut first,&Message::ManagementStatus).await.unwrap();match codec::read(&mut first).await.unwrap(){Message::HostStatus(status)=>{assert_eq!(status.identity,host);assert_eq!(status.accepted_calls,0);},other=>panic!("management result: {other:?}")};{use tokio::io::AsyncReadExt;let mut byte=[0_u8;1];assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(5),first.read(&mut byte)).await.unwrap().unwrap(),0,"first management reply is complete and delivered only once before EOF");}drop(first);
        let mut second=tokio::net::UnixStream::connect(&paths.socket).await.unwrap();assert!(matches!(raw_hello(&mut second,&selection,Admission::Management,"stale-client",VERSION).await,Message::Ready{..}));codec::write(&mut second,&Message::ManagementStatus).await.unwrap();assert!(matches!(codec::read(&mut second).await.unwrap(),Message::HostStatus(_)));drop(second);
        let mut stale=tokio::net::UnixStream::connect(&paths.socket).await.unwrap();match raw_hello(&mut stale,&selection,Admission::Business,"wrong-linked-source",VERSION).await{Message::Error(error)=>{assert_eq!(error.class,"source_stale");assert_eq!(error.execution,"not_executed");},other=>panic!("stale source: {other:?}")};drop(stale);
        let mut wrong=tokio::net::UnixStream::connect(&paths.socket).await.unwrap();match raw_hello(&mut wrong,&selection,Admission::Management,"stale-client",VERSION+1).await{Message::Error(error)=>{assert_eq!(error.class,"identity_mismatch");assert_eq!(error.execution,"not_executed");},other=>panic!("wrong protocol: {other:?}")};drop(wrong);
        let report=serde_json::json!({"fixture":"real user launchd→production activated_listener→retained first original serve_connection/codec→ManagementStatus","label":label,"socket":paths.socket,"uid":identity::uid(),"host_pid":host.owner.pid,"first_connection_retained":true,"prefetched_and_listener_cloexec_nonblocking_verified":true,"first_complete_reply_then_EOF":true,"second_management":true,"stale_source_rejected":true,"wrong_protocol_rejected":true,"business_dispatched":0});
        fixture.cleanup();assert!(!paths.directory.exists());assert!(!root.exists());let absent=Command::new("/bin/launchctl").args(["print",&domain]).output().unwrap();assert!(!absent.status.success(),"private fixture job must be removed");
        let mut report=report;report["job_directory_socket_ready_cleanup_verified"]=true.into();eprintln!("actual_activated_listener_fixture {report}");if let Some(path)=std::env::var_os("QUANTNIGHT_ACTIVATION_TEST_REPORT"){fs::write(path,serde_json::to_vec_pretty(&report).unwrap()).unwrap();}
    }
}
