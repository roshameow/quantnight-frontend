//! Runtime selection and native identity. One local authority; no Mongo endpoints or credentials.
use std::{fs::{self,File,OpenOptions},io::Read,path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
#[path="../../../runtime_registration.rs"]
pub mod registration;
pub use registration::{Binding,RuntimeEnrollment,ConfigurationSelector};
use crate::native_identity as image;
pub use crate::native_identity::{ProcessIdentity,process_identity};
#[path="../../../source_identity.rs"]
mod source_identity;
use super::{TransportError,protocol::VERSION};
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSelection {
    pub binding_path:PathBuf,pub backend_root:PathBuf,pub frontend_root:PathBuf,
    pub executable:PathBuf,pub configuration:ConfigurationSelector,pub protocol_version:u32,pub binding_revision:String,pub target:String,pub profile:String,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub struct HostIdentity {
    pub selection:RuntimeSelection,pub uid:u32,pub owner:ProcessIdentity,pub generation:String,
    pub image:[u8;16],pub core_source:String,pub cli_source:String,
    pub mission_database:String,pub simulation_database:String,pub task_settings:String,pub ready:bool,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub struct RuntimePaths {pub directory:PathBuf,pub socket:PathBuf,pub ready:PathBuf,pub lock:PathBuf,pub label:String,pub launch_agent:PathBuf}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct RuntimePlan {pub schema_version:u32,pub command:String,pub ok:bool,pub binding:Binding,pub selection:RuntimeSelection,pub paths:RuntimePaths,pub plist:String,#[serde(default)] pub business_validated:bool}
fn hash(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
pub fn uid()->u32{#[cfg(unix)]{unsafe{libc::geteuid()}} #[cfg(not(unix))]{0}}
pub fn nonce()->Result<String,String>{let mut v=[0;16];File::open("/dev/urandom").and_then(|mut f|f.read_exact(&mut v)).map_err(|_|"OS randomness unavailable")?;Ok(v.iter().map(|b|format!("{b:02x}")).collect())}
pub fn checked_read(path:&Path)->Result<Vec<u8>,String>{
    let before=fs::symlink_metadata(path).map_err(|_|"registered identity file unavailable")?;
    if !before.is_file(){return Err("identity slot must be a regular non-symlink file".into());}
    let mut file=File::open(path).map_err(|_|"identity slot unavailable")?;let opened=file.metadata().map_err(|_|"identity file metadata unavailable")?;
    #[cfg(unix)]{use std::os::unix::fs::MetadataExt;if before.dev()!=opened.dev() || before.ino()!=opened.ino() || opened.uid()!=uid() || opened.mode()&0o022!=0{return Err("identity file owner/mode/inode mismatch".into());}}
    let mut bytes=Vec::new();file.read_to_end(&mut bytes).map_err(|_|"identity read failed")?;Ok(bytes)
}
impl RuntimeSelection {
    pub fn from_binding(path:&Path)->Result<Self,String>{
        let binding=Binding::from_bytes(&checked_read(path)?)?;Self::with_binding(path,&binding)
    }
    fn with_binding(path:&Path,binding:&Binding)->Result<Self,String>{
        if !path.is_absolute() || path.canonicalize().map_err(|_|"binding path unavailable")?!=path{return Err("runtime binding must be canonical".into());}
        checked_read(path)?;
        let runtime=binding.runtime.as_ref().ok_or("runtime is not explicitly enrolled")?;
        for root in [&binding.frontend_root,&binding.backend_root]{
            let locator:serde_json::Value=serde_json::from_slice(&checked_read(&root.join(".quantnight/taskctl-selection.json"))?).map_err(|_|"invalid selection locator")?;
            if locator["schema_version"]!=1 || locator["mode"]!="dev" || locator["binding"].as_str()!=path.to_str(){return Err("two root selections must reference this exact binding".into());}
        }
        Ok(Self{binding_path:path.into(),backend_root:binding.backend_root.clone(),frontend_root:binding.frontend_root.clone(),executable:runtime.executable.clone(),configuration:runtime.configuration.clone(),protocol_version:runtime.protocol_version,binding_revision:hash(&serde_json::to_vec(binding).map_err(|_|"binding encoding failed")?),target:binding.target.clone(),profile:binding.profile.clone()})
    }
    pub fn for_backend(root:&Path)->Result<Option<Self>,String>{
        let locator=root.join(".quantnight/taskctl-selection.json");
        match fs::symlink_metadata(&locator){Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err("selection metadata unavailable".into()),Ok(_)=>{}}
        let value:serde_json::Value=serde_json::from_slice(&checked_read(&locator)?).map_err(|_|"invalid taskctl selection")?;
        if value["mode"]=="standalone"{return Ok(None);}
        let path=Path::new(value["binding"].as_str().ok_or("missing registered binding")?);
        let binding=Binding::from_bytes(&checked_read(path)?)?;
        if root.canonicalize().map_err(|_|"backend unavailable")?!=binding.backend_root{return Err("backend selection mismatch".into());}
        if binding.runtime.is_none(){return Ok(None);}Self::from_binding(path).map(Some)
    }
    pub fn paths(&self)->Result<RuntimePaths,String>{
        let home=dirs::home_dir().ok_or("user home unavailable")?;
        let key=hash(format!("{}\0{}\0{}\0{}\0{}",uid(),self.binding_path.display(),self.backend_root.display(),self.configuration.kind,self.configuration.path.display()).as_bytes());
        let directory=home.join("Library/Application Support/QuantNight/runtime").join(&key[..16]);let socket=directory.join("task.sock");
        if socket.as_os_str().len()>=104{return Err("registered Unix socket pathname exceeds macOS limit".into());}
        let label=format!("com.quantnight.task-runtime.{}",&key[..16]);
        Ok(RuntimePaths{ready:directory.join("ready.json"),lock:directory.join("host.lock"),launch_agent:home.join("Library/LaunchAgents").join(format!("{label}.plist")),directory,socket,label})
    }
    pub fn verify_current(&self)->Result<(),String>{if Self::from_binding(&self.binding_path)?!=*self{return Err("runtime enrollment changed; explicit safe re-adoption required".into());}Ok(())}
}
pub fn plan(binding_path:&Path, executable:Option<&Path>)->Result<RuntimePlan,String>{plan_with_config(binding_path,executable,None)}
pub fn plan_with_config(binding_path:&Path, executable:Option<&Path>,app_config:Option<&Path>)->Result<RuntimePlan,String>{
    let mut binding=Binding::from_bytes(&checked_read(binding_path)?)?;
    if let Some(exe)=executable{registration::check_executable_path(exe)?;let configuration=binding.runtime.as_ref().map(|runtime|runtime.configuration.clone()).unwrap_or_else(||ConfigurationSelector{kind:"backend_root".into(),path:binding.backend_root.clone()});binding.schema_version=2;binding.runtime=Some(RuntimeEnrollment{executable:exe.into(),configuration,protocol_version:VERSION});}
    if let Some(path)=app_config {let runtime=binding.runtime.as_mut().ok_or("--app-config plan needs an explicit runtime executable/enrollment")?;runtime.configuration=ConfigurationSelector{kind:"app_config".into(),path:path.into()};}
    let binding=Binding::from_bytes(&serde_json::to_vec(&binding).map_err(|_|"runtime binding plan encoding failed")?)?;
    let selection=RuntimeSelection::with_binding(binding_path,&binding)?;let paths=selection.paths()?;
    fn xml(v:&Path)->String{v.to_string_lossy().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
    let plist=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>runtime-host</string><string>--binding</string><string>{}</string></array><key>WorkingDirectory</key><string>{}</string><key>Sockets</key><dict><key>TaskRuntime</key><dict><key>SockFamily</key><string>Unix</string><key>SockType</key><string>stream</string><key>SockPathName</key><string>{}</string><key>SockPathMode</key><integer>384</integer></dict></dict><key>ExitTimeOut</key><integer>30</integer></dict></plist>\n",paths.label,xml(&selection.executable),xml(binding_path),xml(&selection.backend_root),xml(&paths.socket));
    Ok(RuntimePlan{schema_version:1,command:"runtime-plan".into(),ok:true,binding,selection,paths,plist,business_validated:false})
}
pub fn check_directory(path:&Path)->Result<(),String>{check_directory_owned(path,uid())}
fn check_directory_owned(path:&Path,expected_uid:u32)->Result<(),String>{
    let invalid="runtime directory and ancestors must be canonical non-symlink directories";
    if !path.is_absolute() || path.components().any(|part|matches!(part,std::path::Component::ParentDir)){return Err(invalid.into());}
    // Preserve Path's original CurDir normalization. Do not lowercase or
    // compare realpath spelling: APFS may resolve an existing non-symlink
    // directory through a legal case alias. ParentDir remains noncanonical.
    let mut prefix=PathBuf::new();let mut leaf=None;
    for part in path.components(){
        prefix.push(part.as_os_str());
        // Build each prefix without a trailing slash: lstat("link/") can
        // follow a directory symlink. Every actual ancestor and leaf must be
        // a directory according to lstat, not the followed target's metadata.
        let meta=fs::symlink_metadata(&prefix).map_err(|_|"runtime directory unavailable; explicit enrollment/setup required")?;
        if !meta.is_dir(){return Err(invalid.into());}leaf=Some(meta);
    }
    let meta=leaf.ok_or(invalid)?;
    #[cfg(unix)]{use std::os::unix::fs::MetadataExt;if meta.uid()!=expected_uid || meta.mode()&0o777!=0o700{return Err("runtime directory requires current-user owner/mode0700".into());}}Ok(())
}
pub fn check_socket(selection:&RuntimeSelection)->Result<(),String>{check_socket_path(&selection.paths()?,uid())}
fn check_socket_path(paths:&RuntimePaths,expected_uid:u32)->Result<(),String>{
    check_directory(&paths.directory)?;
    let meta=fs::symlink_metadata(&paths.socket).map_err(|_|"runtime socket unavailable; do not fall back to direct database access")?;
    #[cfg(unix)]{use std::os::unix::fs::{MetadataExt,FileTypeExt};if !meta.file_type().is_socket() || meta.uid()!=expected_uid || meta.mode()&0o777!=0o600{return Err("runtime socket type/owner/mode mismatch".into());}}Ok(())
}
#[cfg(any(target_os="macos",target_os="freebsd",target_os="openbsd",target_os="netbsd",target_os="dragonfly"))]
pub fn peer_uid(stream:&tokio::net::UnixStream)->Result<(),String>{use std::os::fd::AsRawFd;let(mut user,mut group)=(0,0);if unsafe{libc::getpeereid(stream.as_raw_fd(),&mut user,&mut group)}!=0 || user!=uid(){return Err("Unix socket peer UID mismatch".into());}Ok(())}
#[cfg(target_os="linux")]
pub fn peer_uid(stream:&tokio::net::UnixStream)->Result<(),String>{
    use std::os::fd::AsRawFd;let mut credential=unsafe{std::mem::zeroed::<libc::ucred>()};let mut size=std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe{libc::getsockopt(stream.as_raw_fd(),libc::SOL_SOCKET,libc::SO_PEERCRED,(&mut credential as *mut libc::ucred).cast(),&mut size)}!=0 || credential.uid!=uid(){return Err("Unix socket peer UID mismatch".into());}Ok(())
}
pub fn core_fresh(selection:&RuntimeSelection)->Result<(),String>{
    if Path::new(crate::DEV_CRATE_ROOT)!=selection.frontend_root.join("crates/task-core") || crate::DEV_TARGET!=selection.target || crate::DEV_PROFILE!=selection.profile{return Err("linked core checkout/target/profile differs from explicit runtime selection".into());}
    source_identity::admit_linked(&selection.frontend_root.join("crates/task-core"),crate::DEV_SOURCE_ID)
}
pub fn cli_fresh(selection:&RuntimeSelection,embedded:&str)->Result<(),String>{source_identity::admit_linked(&selection.frontend_root.join("crates/taskctl"),embedded)}
#[cfg(target_os="macos")]
pub fn peer_host(stream:&tokio::net::UnixStream,host:&HostIdentity)->Result<(),String>{if peer_pid(stream)?!=host.owner.pid{return Err("runtime socket peer PID differs from native ready owner".into());}Ok(())}
#[cfg(all(unix,not(target_os="macos")))]
pub fn peer_host(_: &tokio::net::UnixStream,_:&HostIdentity)->Result<(),String>{Err("runtime native peer process check requires macOS".into())}
pub fn host_image(selection:&RuntimeSelection)->Result<[u8;16],String>{image::ensure_current(&selection.executable)}
pub fn management_selection_matches(a:&RuntimeSelection,b:&RuntimeSelection)->bool{a.binding_path==b.binding_path && a.backend_root==b.backend_root && a.frontend_root==b.frontend_root && a.configuration==b.configuration && a.protocol_version==b.protocol_version}
pub fn validate_host(identity:&HostIdentity,selection:&RuntimeSelection,business:bool)->Result<(),String>{
    if (if business { &identity.selection!=selection } else { !management_selection_matches(&identity.selection,selection) }) || identity.uid!=uid() || !identity.ready || process_identity(identity.owner.pid)?!=identity.owner{return Err("runtime ready identity/owner/selection mismatch".into());}
    let ready:HostIdentity=serde_json::from_slice(&checked_read(&selection.paths()?.ready)?).map_err(|_|"runtime ready record invalid")?;
    if ready!=*identity{return Err("runtime handshake differs from native ready record".into());}
    if business {selection.verify_current()?;core_fresh(selection)?;if !task_settings_fresh(selection,&identity.task_settings).is_ok() || identity.core_source!=crate::DEV_SOURCE_ID || image::disk(&selection.executable)?!=identity.image{return Err("runtime loaded/source generation is stale; drain and reload through the original owner".into());}}
    Ok(())
}
pub fn publish_ready(identity:&HostIdentity)->Result<(),String>{
    let paths=identity.selection.paths()?;check_directory(&paths.directory)?;
    if paths.ready.exists(){checked_read(&paths.ready)?;}
    let temp=paths.directory.join(format!("ready-{}.tmp",nonce()?));
    let mut options=OpenOptions::new();options.create_new(true).write(true);
    #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(&temp).map_err(|_|"ready publication unavailable")?;
    use std::io::Write;file.write_all(&serde_json::to_vec(identity).map_err(|_|"ready encoding failed")?).map_err(|_|"ready write failed")?;file.sync_all().map_err(|_|"ready sync failed")?;
    fs::rename(&temp,&paths.ready).map_err(|_|"ready publication failed".into())
}
pub fn error(e:String)->TransportError{TransportError::new("identity_mismatch","identity",e)}

#[cfg(unix)]
pub fn monotonic_ns()->Result<u64,String>{let mut time=std::mem::MaybeUninit::<libc::timespec>::uninit();if unsafe{libc::clock_gettime(libc::CLOCK_MONOTONIC,time.as_mut_ptr())}!=0{return Err("native monotonic clock unavailable".into());}let t=unsafe{time.assume_init()};Ok((t.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(t.tv_nsec as u64))}

pub fn validate_configuration_source(selection:&RuntimeSelection)->Result<(),String>{
    if selection.configuration.kind=="app_config" {
        let path=&selection.configuration.path;
        if !path.is_file() || path.canonicalize().map_err(|_|"selected original AppConfig unavailable")?!=*path{return Err("selected original AppConfig must exist at its registered canonical path".into());}
    } Ok(())
}
pub async fn validate_plan(plan:&mut RuntimePlan)->Result<(),String>{load_host_config(&plan.selection).await?;plan.business_validated=true;Ok(())}
pub async fn load_host_config(selection:&RuntimeSelection)->Result<crate::config::AppConfig,String>{
    validate_configuration_source(selection)?;
    let config=match selection.configuration.kind.as_str(){
        "backend_root"=>crate::backend_binding::export_backend_config(&selection.backend_root).await?,
        "app_config"=>{let input=crate::config::load_config(&selection.configuration.path).map_err(|_|"selected original AppConfig unavailable/invalid")?;
            crate::backend_binding::resolve_gui_binding(input).await?.0},
        _=>return Err("unsupported selected runtime configuration".into()),
    };
    if Path::new(&config.env.working_dir).canonicalize().map_err(|_|"configured runtime backend unavailable")?!=selection.backend_root{return Err("selected runtime AppConfig backend differs from original binding".into());}Ok(config)
}
pub fn task_settings_fresh(selection:&RuntimeSelection,loaded:&str)->Result<(),String>{
    validate_configuration_source(selection)?;
    if selection.configuration.kind=="app_config"{
        let(root,fingerprint)=crate::backend_binding::task_settings_from_source(&selection.configuration.path)?;
        if root!=selection.backend_root{return Err("selected AppConfig root changed; explicit rebind required".into());}
        if fingerprint!=loaded{return Err("original task settings changed; explicit safe reload required".into());}
    }
    // backend_root has no AppConfig task scripts; do not invent defaults.
    // Its transport/config freshness remains with the original BackendBinding.
    Ok(())
}

/// Direction matters: root cached listener credentials are not server process authority.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct PeerAdmission { activation_pending:bool }
#[cfg(target_os="macos")]
fn peer_pid(stream:&tokio::net::UnixStream)->Result<u32,String>{
    use std::os::fd::AsRawFd;let mut pid:libc::pid_t=0;let mut size=std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    if unsafe{libc::getsockopt(stream.as_raw_fd(),0,libc::LOCAL_PEERPID,(&mut pid as *mut libc::pid_t).cast(),&mut size)}!=0 || pid<1{return Err("native Unix peer PID unavailable".into());}Ok(pid as u32)
}
fn classify_listener(user:u32,group:u32,current:u32,enrolled_activation:bool,initial_peer_pid:u32)->Result<PeerAdmission,String>{
    if initial_peer_pid<1{return Err("native initial peer PID unavailable".into());}
    if user==current{return Ok(PeerAdmission{activation_pending:false});}
    if user==0 && group==0 && current!=0 && enrolled_activation{return Ok(PeerAdmission{activation_pending:true});}
    Err("Unix listener credentials do not belong to current user or the exact enrolled launchd activation".into())
}
#[cfg(target_os="macos")]
pub fn client_preflight(stream:&tokio::net::UnixStream,selection:&RuntimeSelection)->Result<PeerAdmission,String>{
    use std::os::fd::AsRawFd;
    check_socket(selection)?;selection.verify_current()?;
    let(mut user,mut group)=(0,0);if unsafe{libc::getpeereid(stream.as_raw_fd(),&mut user,&mut group)}!=0{return Err("Unix listener credentials unavailable".into());}
    if user==uid(){return classify_listener(user,group,uid(),false,peer_pid(stream)?);}
    // Cold PID1 and already-accepted warm hostPID are both pending only. No
    // Call/operational environment is sent until native post-Ready attestation.
    if user!=0 || group!=0{return classify_listener(user,group,uid(),false,peer_pid(stream)?);}
    let paths=selection.paths()?;
    if paths.launch_agent.parent().ok_or("LaunchAgent parent missing")?.canonicalize().map_err(|_|"enrolled LaunchAgent parent unavailable")?!=paths.launch_agent.parent().unwrap(){return Err("enrolled LaunchAgent path is not canonical".into());}
    let installed=checked_read(&paths.launch_agent)?;
    if installed!=plan(&selection.binding_path,None)?.plist.as_bytes(){return Err("root listener is not the exact generated enrolled LaunchAgent".into());}
    // Reading peerPID here is an availability check, NOT authorization/PID1 gate.
    let initial_peer_pid=peer_pid(stream)?;
    classify_listener(user,group,uid(),true,initial_peer_pid)
}
#[cfg(all(unix,not(target_os="macos")))]
pub fn client_preflight(_:&tokio::net::UnixStream,_:&RuntimeSelection)->Result<PeerAdmission,String>{Err("runtime activation identity requires macOS".into())}
fn validate_attestation(peer:u32,host:&HostIdentity,attested:&crate::native_identity::ProcessAttestation,admission:&PeerAdmission,current:u32)->Result<(),String>{
    if peer!=host.owner.pid || attested.identity!=host.owner || attested.uid!=current || !attested.executable.is_absolute() || attested.executable!=host.selection.executable{return Err("runtime peer PID/birth/actual owner/executable differs from registered ready host".into());}
    if admission.activation_pending && attested.parent_pid!=1{return Err("cached-root listener did not resolve to the enrolled user launchd child".into());}
    Ok(())
}
#[cfg(target_os="macos")]
pub fn client_postflight(stream:&tokio::net::UnixStream,selection:&RuntimeSelection,host:&HostIdentity,admission:&PeerAdmission,business:bool)->Result<(),String>{
    check_socket(selection)?;
    let peer=peer_pid(stream)?;
    let attested=crate::native_identity::attest_process(&host.owner)?;
    validate_attestation(peer,host,&attested,admission,uid())?;
    validate_host(host,selection,business)
}
#[cfg(all(unix,not(target_os="macos")))]
pub fn client_postflight(_:&tokio::net::UnixStream,_:&RuntimeSelection,_:&HostIdentity,_:&PeerAdmission,_:bool)->Result<(),String>{Err("runtime activation identity requires macOS".into())}


/// Non-activating management observations are NOT authenticated live-host
/// status, a business terminal receipt, or permission to unlink outside the
/// original installer binding/deployment locks.
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub struct SlotSnapshot {pub dev:String,pub ino:String,pub uid:u32,pub mode:u32,pub ctime_seconds:String,pub ctime_nanos:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct StoppedObservation {pub reclaimable:bool,pub reason:String,pub selection:RuntimeSelection,pub identity:HostIdentity,pub socket:SlotSnapshot,pub ready:SlotSnapshot}
#[cfg(target_os="macos")]
fn slot_snapshot(path:&Path,socket:bool)->Result<SlotSnapshot,String>{
    use std::os::unix::fs::{MetadataExt,FileTypeExt};let meta=fs::symlink_metadata(path).map_err(|_|"selected stale slot metadata unavailable")?;
    if (if socket{!meta.file_type().is_socket()}else{!meta.is_file()}) || meta.uid()!=uid() || meta.mode()&0o777!=0o600{return Err("selected stale socket/ready requires original type/current UID/mode0600".into());}
    Ok(SlotSnapshot{dev:meta.dev().to_string(),ino:meta.ino().to_string(),uid:meta.uid(),mode:meta.mode()&0o777,ctime_seconds:meta.ctime().to_string(),ctime_nanos:meta.ctime_nsec().to_string()})
}
#[cfg(target_os="macos")]
fn require_job_unloaded(selection:&RuntimeSelection)->Result<(),String>{
    let label=selection.paths()?.label;let target=format!("gui/{}/{label}",uid());
    let output=std::process::Command::new("/bin/launchctl").args(["print",&target]).output().map_err(|_|"launchd unloaded observation unavailable")?;
    if output.status.success() || !String::from_utf8_lossy(&output.stderr).contains("Could not find service"){return Err("selected launchd job not authoritatively unloaded; no socket probe/reclaim".into());}Ok(())
}
#[cfg(target_os="macos")]
fn owner_esrch(owner:&ProcessIdentity)->Result<bool,String>{
    if owner.pid<2 || owner.pid>i32::MAX as u32 || owner.started_seconds==0 || owner.started_micros>=1_000_000{return Err("selected original ready native owner metadata invalid".into());}
    if unsafe{libc::kill(owner.pid as i32,0)}==0{return Ok(false);}
    if std::io::Error::last_os_error().raw_os_error()==Some(libc::ESRCH){Ok(true)}else{Err("selected ready native owner absence is not confirmed ESRCH".into())}
}
#[cfg(target_os="macos")]
fn refused_socket_without_bytes(path:&Path)->Result<bool,String>{
    use std::os::{fd::{FromRawFd,AsRawFd},unix::ffi::OsStrExt};
    let fd=unsafe{libc::socket(libc::AF_UNIX,libc::SOCK_STREAM,0)};if fd<0{return Err("selected socket observation unavailable".into());}
    let stream=unsafe{std::os::unix::net::UnixStream::from_raw_fd(fd)};
    let flags=unsafe{libc::fcntl(stream.as_raw_fd(),libc::F_GETFD)};
    if flags<0 || unsafe{libc::fcntl(fd,libc::F_SETFD,flags|libc::FD_CLOEXEC)}<0{return Err("socket observation descriptor setup failed".into());}
    stream.set_nonblocking(true).map_err(|_|"socket observation nonblocking setup failed")?;
    let mut address:libc::sockaddr_un=unsafe{std::mem::zeroed()};let bytes=path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.contains(&0) || bytes.len()>=address.sun_path.len(){return Err("invalid selected Unix observation pathname".into());}
    address.sun_family=libc::AF_UNIX as u8;address.sun_len=(2+bytes.len()+1) as u8;
    for(to,from)in address.sun_path.iter_mut().zip(bytes){*to=*from as libc::c_char;}
    if unsafe{libc::connect(fd,(&address as *const libc::sockaddr_un).cast(),address.sun_len as libc::socklen_t)}==0{return Ok(false);}
    match std::io::Error::last_os_error().raw_os_error(){Some(libc::ECONNREFUSED)=>Ok(true),Some(libc::EINPROGRESS|libc::EAGAIN|libc::EALREADY)=>Ok(false),_=>Err("selected socket probe was not confirmed ECONNREFUSED; no reclaim".into())}
}
#[cfg(target_os="macos")]
pub fn stopped_observation(selection:&RuntimeSelection)->Result<StoppedObservation,String>{stopped_observation_checked(selection,||require_job_unloaded(selection))}
#[cfg(target_os="macos")]
fn stopped_observation_checked(selection:&RuntimeSelection,mut unloaded:impl FnMut()->Result<(),String>)->Result<StoppedObservation,String>{
    selection.verify_current()?;let paths=selection.paths()?;check_directory(&paths.directory)?;unloaded()?;
    let socket=slot_snapshot(&paths.socket,true)?;let ready=slot_snapshot(&paths.ready,false)?;
    let identity:HostIdentity=serde_json::from_slice(&checked_read(&paths.ready)?).map_err(|_|"selected original ready record invalid")?;
    if identity.selection!=*selection || identity.uid!=uid() || !identity.ready || identity.generation.is_empty(){return Err("selected original ready identity/selection mismatch".into());}
    if !owner_esrch(&identity.owner)?{return Ok(StoppedObservation{reclaimable:false,reason:"owner_present".into(),selection:selection.clone(),identity,socket,ready});}
    unloaded()?;let refused=refused_socket_without_bytes(&paths.socket)?;
    unloaded()?;selection.verify_current()?;check_directory(&paths.directory)?;
    if socket!=slot_snapshot(&paths.socket,true)? || ready!=slot_snapshot(&paths.ready,false)? || !owner_esrch(&identity.owner)?{return Err("selected stale observation changed; no reclaim".into());}
    Ok(StoppedObservation{reclaimable:refused,reason:if refused{"confirmed_stale"}else{"socket_live_or_pending"}.into(),selection:selection.clone(),identity,socket,ready})
}
#[cfg(not(target_os="macos"))]
pub fn stopped_observation(_:&RuntimeSelection)->Result<StoppedObservation,String>{Err("stopped runtime observation requires original macOS owner".into())}

#[cfg(all(test,target_os="macos"))]
mod activation_tests {
    use super::*;
    fn host()->HostIdentity{
        let owner=process_identity(std::process::id()).unwrap();let attested=crate::native_identity::attest_process(&owner).unwrap();
        HostIdentity{selection:RuntimeSelection{binding_path:"/fixture/selected-binding.json".into(),backend_root:"/fixture/backend".into(),frontend_root:"/fixture/frontend".into(),executable:attested.executable,configuration:ConfigurationSelector{kind:"backend_root".into(),path:"/fixture/backend".into()},protocol_version:1,binding_revision:"fixture".into(),target:crate::DEV_TARGET.into(),profile:crate::DEV_PROFILE.into()},uid:uid(),owner,generation:"fixture".into(),image:crate::native_identity::loaded().unwrap(),core_source:crate::DEV_SOURCE_ID.into(),cli_source:"fixture".into(),mission_database:"simulation_mission".into(),simulation_database:"simulation_db".into(),task_settings:"fixture".into(),ready:true}
    }
    #[test]
    fn cached_root_is_pending_for_cold_and_warm_not_authority_and_bad_native_identity_fails(){
        let host=host();let native=crate::native_identity::attest_process(&host.owner).unwrap();
        // Initial peer PID deliberately does not decide admission: XNU can
        // report either PID1 or accepting hostPID before this branch runs.
        for initial_pid in [1,host.owner.pid]{let pending=classify_listener(0,0,uid(),true,initial_pid).unwrap();assert!(pending.activation_pending);let mut launchd_child=native.clone();launchd_child.parent_pid=1;validate_attestation(host.owner.pid,&host,&launchd_child,&pending,uid()).unwrap();
            for mutate in 0..5{let mut wrong=launchd_child.clone();match mutate{0=>wrong.uid=0,1=>wrong.parent_pid=2,2=>wrong.identity.started_micros=wrong.identity.started_micros.wrapping_add(1),3=>wrong.executable="/another/service/taskctl".into(),_=>wrong.identity.pid=wrong.identity.pid.wrapping_add(1)}assert!(validate_attestation(host.owner.pid,&host,&wrong,&pending,uid()).is_err());}
            assert!(validate_attestation(1,&host,&launchd_child,&pending,uid()).is_err());
        }
        assert!(classify_listener(0,0,uid(),false,1).is_err());assert!(classify_listener(0,20,uid(),true,1).is_err());assert!(classify_listener(uid()+1,0,uid(),true,host.owner.pid).is_err());
        let ordinary=classify_listener(uid(),20,uid(),false,host.owner.pid).unwrap();validate_attestation(host.owner.pid,&host,&native,&ordinary,uid()).unwrap();
        let mut other=host.selection.clone();other.binding_path="/another/service/binding.json".into();assert!(validate_host(&host,&other,false).is_err());
    }
    #[tokio::test]
    async fn real_socket_post_ready_pid_and_native_uid_birth_executable_are_required(){
        let host=host();let(client,server)=tokio::net::UnixStream::pair().unwrap();peer_uid(&server).unwrap();peer_host(&client,&host).unwrap();
        let mut forged=host.clone();forged.owner.pid=1;assert!(peer_host(&client,&forged).is_err());
        let actual=crate::native_identity::attest_process(&host.owner).unwrap();validate_attestation(peer_pid(&client).unwrap(),&host,&actual,&PeerAdmission{activation_pending:false},uid()).unwrap();
    }
    #[test]
    fn private_socket_type_owner_and_modes_are_not_weakened_for_activation(){
        use std::os::unix::fs::PermissionsExt;
        let directory=std::path::Path::new("/tmp").join(format!("qn-s-{}",&nonce().unwrap()[..12]));fs::create_dir(&directory).unwrap();let directory=directory.canonicalize().unwrap();fs::set_permissions(&directory,fs::Permissions::from_mode(0o700)).unwrap();
        let socket=directory.join("s");let listener=std::os::unix::net::UnixListener::bind(&socket).unwrap();fs::set_permissions(&socket,fs::Permissions::from_mode(0o600)).unwrap();
        let paths=RuntimePaths{directory:directory.clone(),socket:socket.clone(),ready:directory.join("r"),lock:directory.join("l"),label:"fixture".into(),launch_agent:directory.join("agent")};
        check_socket_path(&paths,uid()).unwrap();assert!(check_socket_path(&paths,uid()+1).is_err());
        fs::set_permissions(&socket,fs::Permissions::from_mode(0o666)).unwrap();assert!(check_socket_path(&paths,uid()).is_err());fs::set_permissions(&socket,fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&directory,fs::Permissions::from_mode(0o755)).unwrap();assert!(check_socket_path(&paths,uid()).is_err());fs::set_permissions(&directory,fs::Permissions::from_mode(0o700)).unwrap();
        drop(listener);fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn prospective_plan_retains_selected_appconfig_and_configuration_paths_have_distinct_slots(){
        let root=std::path::Path::new("/tmp").join(format!("qn-plan-{}",&nonce().unwrap()[..12]));
        fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();
        let front=root.join("front");let back=root.join("back");fs::create_dir_all(front.join("crates/target/debug")).unwrap();fs::create_dir_all(back.join(".quantnight")).unwrap();fs::create_dir_all(front.join(".quantnight")).unwrap();
        let first=back.join("first.toml");let second=back.join("second.toml");fs::write(&first,"# synthetic-private-body-not-IPC").unwrap();fs::write(&second,"# other").unwrap();
        let binding_path=back.join("binding.json");let executable=front.join("crates/target/debug/taskctl");
        let binding=Binding{schema_version:2,frontend_root:front.clone(),backend_root:back.clone(),executable:front.join("missing-gui-target/debug/gui"),target:crate::DEV_TARGET.into(),profile:"debug".into(),runtime:Some(RuntimeEnrollment{executable:executable.clone(),configuration:ConfigurationSelector{kind:"app_config".into(),path:first.clone()},protocol_version:1})};
        let bytes=serde_json::to_vec(&binding).unwrap();fs::write(&binding_path,&bytes).unwrap();
        let locator=serde_json::json!({"schema_version":1,"mode":"dev","binding":binding_path});for target in [&front,&back]{fs::write(target.join(".quantnight/taskctl-selection.json"),serde_json::to_vec(&locator).unwrap()).unwrap();}
        let preserved=plan_with_config(&binding_path,Some(&executable),None).unwrap();assert_eq!(preserved.selection.configuration.path,first);
        let changed=plan_with_config(&binding_path,None,Some(&second)).unwrap();assert_ne!(preserved.paths.label,changed.paths.label);assert_ne!(preserved.paths.socket,changed.paths.socket);
        assert_eq!(fs::read(&binding_path).unwrap(),bytes,"prospective plan must not change selection");assert!(!serde_json::to_string(&preserved).unwrap().contains("synthetic-private-body-not-IPC"));
        fs::remove_file(&first).unwrap();
        let registered=RuntimeSelection::from_binding(&binding_path).unwrap();assert_eq!(registered.configuration.path,first);
        let mut metadata=plan(&binding_path,None).unwrap();assert!(!metadata.business_validated);
        let validation=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(validate_plan(&mut metadata));
        assert!(validation.is_err());assert!(!metadata.business_validated);assert!(task_settings_fresh(&registered,"old-selected-settings").is_err());
        fs::write(&first,"[invalid TOML").unwrap();
        assert!(RuntimeSelection::from_binding(&binding_path).is_ok());
        let validation=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(validate_plan(&mut metadata));
        assert!(validation.is_err());assert_eq!(fs::read(&binding_path).unwrap(),bytes);
        fs::remove_dir_all(root).unwrap();
    }

    struct DirectoryFixture(PathBuf);
    impl DirectoryFixture {
        fn new()->Self{let path=Path::new("/tmp").join(format!("qn-case-{}",&nonce().unwrap()[..12]));fs::create_dir(&path).unwrap();Self(path.canonicalize().unwrap())}
        fn actual(&self)->PathBuf{self.0.join("Library/Application Support/quantnight/runtime/slot")}
        fn requested(&self)->PathBuf{self.0.join("Library/Application Support/QuantNight/runtime/slot")}
        fn prepare(&self){use std::os::unix::fs::PermissionsExt;fs::create_dir_all(self.actual()).unwrap();fs::set_permissions(self.actual(),fs::Permissions::from_mode(0o700)).unwrap();}
    }
    impl Drop for DirectoryFixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    #[test]
    fn directory_case_alias_keeps_real_symlink_dot_owner_and_mode_protections(){
        use std::os::unix::fs::{symlink,PermissionsExt};
        let fixture=DirectoryFixture::new();fixture.prepare();let actual=fixture.actual();check_directory(&actual).unwrap();
        assert!(check_directory_owned(&actual,uid()+1).unwrap_err().contains("owner/mode0700"));
        for mode in [0o755,0o770,0o600]{fs::set_permissions(&actual,fs::Permissions::from_mode(mode)).unwrap();assert!(check_directory(&actual).unwrap_err().contains("owner/mode0700"));}
        fs::set_permissions(&actual,fs::Permissions::from_mode(0o700)).unwrap();
        assert!(check_directory(Path::new("relative/runtime")).is_err());
        for suffix in ["/./slot","/slot/.","/./slot/./","//slot//."]{
            let raw=PathBuf::from(format!("{}{suffix}",actual.parent().unwrap().display()));
            let metadata=fs::symlink_metadata(&raw).unwrap();
            let legacy_accepts=metadata.is_dir() && raw.canonicalize().unwrap()==raw;
            assert!(legacy_accepts,"actual old expression must accept CurDir: {raw:?}");
            assert_eq!(check_directory(&raw).is_ok(),legacy_accepts,"preserve original CurDir behavior");
        }
        let parent=PathBuf::from(format!("{}/slot/../slot",actual.parent().unwrap().display()));
        let metadata=fs::symlink_metadata(&parent).unwrap();let legacy_accepts=metadata.is_dir() && parent.canonicalize().unwrap()==parent;
        assert!(!legacy_accepts,"actual old expression must reject ParentDir");assert_eq!(check_directory(&parent).is_ok(),legacy_accepts);
        eprintln!("legacy_path_eq_fixture: actual old expression accepts embedded/trailing CurDir; rejects ParentDir; new check matches both");
        let leaf=fixture.0.join("leaf-link");symlink(&actual,&leaf).unwrap();
        for path in [leaf.clone(),PathBuf::from(format!("{}/",leaf.display())),PathBuf::from(format!("{}///",leaf.display())),PathBuf::from(format!("{}/.",leaf.display()))]{assert!(check_directory(&path).is_err(),"leaf symlink accepted: {path:?}");}
        let ancestor=fixture.0.join("ancestor-link");symlink(actual.parent().unwrap(),&ancestor).unwrap();assert!(check_directory(&ancestor.join("slot")).is_err());
        let file=fixture.0.join("not-directory");fs::write(&file,"fixture").unwrap();assert!(check_directory(&file).is_err());assert!(check_directory(&file.join("slot")).is_err());
        assert!(check_directory(&fixture.0.join("absent")).is_err());check_directory(&actual).unwrap();
    }
    #[tokio::test]
    async fn real_case_alias_socket_and_original_client_exchange_use_os_directory_facts(){
        use std::os::unix::fs::{MetadataExt,PermissionsExt};
        use super::super::{client::RuntimeClient,codec,protocol::{CallContext,Message}};
        let fixture=DirectoryFixture::new();fixture.prepare();let actual=fixture.actual();let requested=fixture.requested();
        let directory=match fs::symlink_metadata(&requested){
            Ok(alias)=>{let physical=fs::symlink_metadata(&actual).unwrap();assert_eq!((alias.dev(),alias.ino()),(physical.dev(),physical.ino()),"case lookup must denote the same real inode");assert_ne!(requested.canonicalize().unwrap().as_os_str(),requested.as_os_str(),"fixture must reproduce the former realpath-spelling rejection");check_directory(&requested).unwrap();eprintln!("case_alias_fixture: OS confirms insensitive lookup/same inode; requested QuantNight resolves to actual quantnight");requested},
            Err(error)=>{assert_eq!(error.kind(),std::io::ErrorKind::NotFound);assert!(check_directory(&requested).is_err());eprintln!("case_alias_fixture: OS is case-sensitive; distinct absent alias rejected, exact physical spelling exercised");actual.clone()},
        };
        let listener=tokio::net::UnixListener::bind(actual.join("s")).unwrap();fs::set_permissions(actual.join("s"),fs::Permissions::from_mode(0o600)).unwrap();
        let paths=RuntimePaths{socket:directory.join("s"),ready:directory.join("r"),lock:directory.join("l"),label:"fixture".into(),launch_agent:directory.join("agent"),directory};
        check_socket_path(&paths,uid()).unwrap();assert!(check_socket_path(&paths,uid()+1).is_err());
        let client=tokio::net::UnixStream::connect(&paths.socket).await.unwrap();let(mut server,_)=listener.accept().await.unwrap();peer_uid(&client).unwrap();peer_uid(&server).unwrap();let host=host();peer_host(&client,&host).unwrap();
        let attested=crate::native_identity::attest_process(&host.owner).unwrap();validate_attestation(peer_pid(&client).unwrap(),&host,&attested,&PeerAdmission{activation_pending:false},uid()).unwrap();
        let context=CallContext::current().unwrap();let expected=context.request_id.clone();let request=crate::task_request::TaskRequest::TaskById{id:"0123456789abcdef01234567".into()};
        // Exercise the original client/codec on the actual aliased pathname.
        // The lowest peer deliberately closes after observing the one Call:
        // no whole-business success mock, real service, DB or automatic replay.
        let exchange=tokio::spawn(RuntimeClient::exchange(client,context,request,None));
        match codec::read(&mut server).await.unwrap(){Message::Call{context,request:crate::task_request::TaskRequest::TaskById{id}}=>{assert_eq!(context.request_id,expected);assert_eq!(id,"0123456789abcdef01234567");},other=>panic!("unexpected client frame: {other:?}")};
        drop(server);let error=exchange.await.unwrap().unwrap_err();assert_eq!(error.execution,"unconfirmed");assert_eq!(error.class,"connection_lost");drop(listener);
    }

    #[test]
    fn stopped_observation_uses_original_ready_exact_selection_esrch_and_refused_inode_not_unknown(){
        use std::os::unix::fs::{PermissionsExt,symlink};
        let root=Path::new("/tmp").join(format!("qn-stale-{}",&nonce().unwrap()[..10]));fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();
        let front=root.join("front");let back=root.join("back");fs::create_dir_all(front.join("crates/target/debug")).unwrap();fs::create_dir_all(front.join("src-tauri/target/debug")).unwrap();fs::create_dir_all(front.join(".quantnight")).unwrap();fs::create_dir_all(back.join(".quantnight")).unwrap();
        let binding_path=root.join("binding.json");let binding=Binding{schema_version:2,frontend_root:front.clone(),backend_root:back.clone(),executable:front.join("src-tauri/target/debug/gui"),target:crate::DEV_TARGET.into(),profile:"debug".into(),runtime:Some(RuntimeEnrollment{executable:front.join("crates/target/debug/taskctl"),configuration:ConfigurationSelector{kind:"backend_root".into(),path:back},protocol_version:1})};
        fs::write(&binding_path,serde_json::to_vec(&binding).unwrap()).unwrap();for dir in [&binding.frontend_root,&binding.backend_root]{fs::write(dir.join(".quantnight/taskctl-selection.json"),serde_json::to_vec(&serde_json::json!({"schema_version":1,"mode":"dev","binding":binding_path})).unwrap()).unwrap();}
        let selection=RuntimeSelection::from_binding(&binding_path).unwrap();let paths=selection.paths().unwrap();fs::create_dir_all(paths.directory.parent().unwrap()).unwrap();fs::create_dir(&paths.directory).unwrap();fs::set_permissions(&paths.directory,fs::Permissions::from_mode(0o700)).unwrap();
        struct Cleanup(PathBuf,PathBuf);impl Drop for Cleanup{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);let _=fs::remove_dir_all(&self.1);}}let _clean=Cleanup(root,paths.directory.clone());
        let listener=std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();fs::set_permissions(&paths.socket,fs::Permissions::from_mode(0o600)).unwrap();drop(listener);
        let mut child=std::process::Command::new("/bin/cat").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).spawn().unwrap();let dead=process_identity(child.id()).unwrap();drop(child.stdin.take());assert!(child.wait().unwrap().success());
        let mut ready=host();ready.selection=selection.clone();ready.owner=dead;fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();fs::set_permissions(&paths.ready,fs::Permissions::from_mode(0o600)).unwrap();
        let observed=stopped_observation_checked(&selection,||Ok(())).unwrap();assert!(observed.reclaimable);assert_eq!(observed.reason,"confirmed_stale");assert_eq!(observed.identity,ready);assert_eq!(observed.socket.mode,0o600);
        assert!(stopped_observation_checked(&selection,||Err("not unloaded".into())).is_err());
        ready.owner=process_identity(std::process::id()).unwrap();fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(!stopped_observation_checked(&selection,||Ok(())).unwrap().reclaimable);
        ready.owner.started_micros=ready.owner.started_micros.wrapping_add(1);fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(!stopped_observation_checked(&selection,||Ok(())).unwrap().reclaimable,"reused/mismatched live PID never unlocks stale recovery");
        ready.owner=observed.identity.owner.clone();ready.owner.started_micros=1_000_000;fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());
        ready=observed.identity.clone();ready.selection.binding_revision="wrong-selection".into();fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());
        ready=observed.identity.clone();ready.uid=uid()+1;fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());
        ready=observed.identity.clone();ready.generation.clear();fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());
        ready=observed.identity;fs::write(&paths.ready,serde_json::to_vec(&ready).unwrap()).unwrap();fs::set_permissions(&paths.ready,fs::Permissions::from_mode(0o644)).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());fs::set_permissions(&paths.ready,fs::Permissions::from_mode(0o600)).unwrap();fs::set_permissions(&paths.socket,fs::Permissions::from_mode(0o666)).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());fs::set_permissions(&paths.socket,fs::Permissions::from_mode(0o600)).unwrap();
        fs::remove_file(&paths.socket).unwrap();let live=std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();fs::set_permissions(&paths.socket,fs::Permissions::from_mode(0o600)).unwrap();assert!(!stopped_observation_checked(&selection,||Ok(())).unwrap().reclaimable);drop(live);
        fs::remove_file(&paths.socket).unwrap();symlink(&paths.ready,&paths.socket).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());fs::remove_file(&paths.socket).unwrap();
        let listener=std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();fs::set_permissions(&paths.socket,fs::Permissions::from_mode(0o600)).unwrap();drop(listener);fs::write(&paths.ready,"bad JSON").unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());fs::remove_file(&paths.ready).unwrap();assert!(stopped_observation_checked(&selection,||Ok(())).is_err());
    }

    #[test]
    fn stopped_observation_unix_path_capacity_102_103_allowed_104_rejected(){
        use std::os::unix::{ffi::OsStrExt,fs::PermissionsExt};
        let root=Path::new("/tmp").join(format!("qn-ss01-{}",&nonce().unwrap()[..10]));fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
        struct Cleanup(PathBuf);impl Drop for Cleanup{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}let _clean=Cleanup(root.clone());
        for length in [102_usize,103]{
            let name="s".repeat(length-root.as_os_str().as_bytes().len()-1);let socket=root.join(name);assert_eq!(socket.as_os_str().as_bytes().len(),length);
            let listener=std::os::unix::net::UnixListener::bind(&socket).unwrap();drop(listener);
            assert!(refused_socket_without_bytes(&socket).unwrap(),"original valid {length}-byte selected pathname must reach actual ECONNREFUSED probe");
        }
        let oversized=root.join("s".repeat(104-root.as_os_str().as_bytes().len()-1));assert_eq!(oversized.as_os_str().as_bytes().len(),104);
        for invalid in [oversized,PathBuf::new(),PathBuf::from(std::ffi::OsStr::from_bytes(b"/private/tmp/ss01\0suffix"))]{assert!(refused_socket_without_bytes(&invalid).unwrap_err().contains("invalid selected Unix observation pathname"));}
    }

}
