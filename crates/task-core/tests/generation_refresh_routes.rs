//! Independent refresh suite; does not edit the protected historical inventory.
use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{create_generate::{self as core, *}, inspection::{list_with_generation, ListRequest}, mission_lock::MissionLocks, task_store::{TaskStore, Host, CollectionSummary}};
use std::{path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicUsize, Ordering}}};
const ID: &str = "0123456789abcdef01234567";
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf);
impl Root { fn new() -> Self { let p = std::env::temp_dir().join(format!("qn-generation-refresh-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))); std::fs::create_dir_all(&p).unwrap(); Self(p) } }
impl Drop for Root { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn task() -> Document {
    let generation = doc! {"id":"g1", "phase":"writing", "producerPid":42000, "startedAt":"2026-10-01T00:00:01+00:00"};
    doc! {"_id":ObjectId::parse_str(ID).unwrap(), "name":"fixture", "status":"generating", "isRemote":false, "targetRemote":true,
        "templatefile":"frozen", "auth_profile":"user1", "taskType":"regular", "generation":&generation,
        "updatedAt":"2026-10-01T00:00:01+00:00", "events":[
            {"id":"created", "command":"create", "status":"pending", "at":"2026-10-01T00:00:00+00:00", "detail":{}},
            {"command":"generation-begin", "status":"generating", "at":"2026-10-01T00:00:01+00:00", "detail":generation}]}
}
fn field<'a>(d: &'a Document, k: &str) -> Option<&'a Bson> {
    match k.split_once('.') { Some((a,b)) => field(d.get_document(a).ok()?,b), None => d.get(k) }
}
fn set(d: &mut Document, k: &str, v: Bson) {
    match k.split_once('.') { Some((a,b)) => set(d.get_document_mut(a).unwrap(),b,v), None => {d.insert(k,v);} }
}
fn matches(d:&Document,q:&Document)->bool { q.iter().all(|(k,v)| { let a=field(d,k); match v {
    Bson::Document(op) if op.contains_key("$exists") => a.is_some()==op.get_bool("$exists").unwrap(),
    Bson::Document(op) if op.contains_key("$ne") => a!=op.get("$ne"),
    Bson::Null => a.is_none() || a==Some(v), _=>a==Some(v) } }) }
struct Store { extra_rows:Mutex<Vec<Document>>, row:Mutex<Option<Document>>, writes:Mutex<Vec<Document>>, calls:Mutex<Vec<String>>, has:Result<bool,String>, cas_patch:Mutex<Option<Document>>, after_probe:Mutex<Option<Document>>, ack_loss:bool }
impl Store { fn new() -> Self { Self {extra_rows:Mutex::new(vec![]),row:Mutex::new(Some(task())),writes:Mutex::new(vec![]),calls:Mutex::new(vec![]),has:Ok(false),cas_patch:Mutex::new(None),after_probe:Mutex::new(None),ack_loss:false} }
fn row(&self)->Document {self.row.lock().unwrap().clone().unwrap()} }
#[async_trait] impl TaskStore for Store {
 async fn list_tasks(&self,host:Host,_:bool)->Result<Vec<Document>,String> {self.calls.lock().unwrap().push(format!("list:{host:?}"));let mut rows:Vec<_>=self.row.lock().unwrap().clone().into_iter().collect();rows.extend(self.extra_rows.lock().unwrap().clone());Ok(rows)}
 async fn find_task(&self,host:Host,q:Document,_:bool)->Result<Option<Document>,String> {assert_eq!(host,Host::Local);self.calls.lock().unwrap().push("find".into());Ok(self.row.lock().unwrap().as_ref().filter(|r|matches(r,&q)).cloned())}
 async fn update_task(&self,host:Host,q:Document,u:Document)->Result<u64,String> {assert_eq!(host,Host::Local);self.writes.lock().unwrap().push(q.clone());let mut row=self.row.lock().unwrap();let row=row.as_mut().unwrap();if let Some(p)=self.cas_patch.lock().unwrap().take(){row.extend(p);}if !matches(row,&q){return Ok(0)}
 for(k,v)in u.get_document("$set").unwrap(){set(row,k,v.clone());}if !row.contains_key("events"){row.insert("events",Vec::<Bson>::new());}row.get_array_mut("events").unwrap().push(u.get_document("$push").unwrap().get("events").unwrap().clone());if self.ack_loss{Err("ack lost".into())}else{Ok(1)}}
 async fn has_rows(&self,host:Host,name:&str)->Result<bool,String>{assert_eq!(host,Host::Local);assert_eq!(name,"fixture");self.calls.lock().unwrap().push("has_rows".into());if let Some(p)=self.after_probe.lock().unwrap().take(){self.row.lock().unwrap().as_mut().unwrap().extend(p);}self.has.clone()}
 async fn collection_names(&self,_:Host)->Result<Vec<String>,String>{panic!("no inventory scan")}
 async fn summary(&self,_:Host,_:&str)->Result<CollectionSummary,String>{panic!("no candidate summary")}
 async fn rows(&self,_:Host,_:&str,_:Option<Document>)->Result<Vec<Document>,String>{panic!("no rows")}
 async fn insert_rows(&self,_:Host,_:&str,_:Vec<Document>)->Result<(),String>{panic!("no item write")}
 async fn rename_collection(&self,_:Host,_:&str,_:&str)->Result<(),String>{panic!("no rename")}
 async fn ping(&self,_:Host)->Result<(),String>{panic!("no ping")}
}
struct Runtime { observed:Result<GenerationProcesses,String>, calls:AtomicUsize }
impl Runtime {fn stopped()->Self{Self{observed:Ok(GenerationProcesses::Stopped),calls:AtomicUsize::new(0)}}}
#[async_trait] impl GenerationRuntime for Runtime {
 async fn inspect_generation(&self,_:&Path,t:&Document)->Result<GenerationProcesses,String>{assert_eq!(t.get_str("name").unwrap(),"fixture");self.calls.fetch_add(1,Ordering::Relaxed);self.observed.clone()}
 async fn creation_metadata(&self,_:&Path,_:&Document)->Result<CreationMetadata,GenerationFailure>{panic!("no create")}
 async fn generate(&self,_:&Path,_:&GenerationCommand)->Result<GeneratedOutput,GenerationFailure>{panic!("no generate")}
}
#[tokio::test] async fn empty_historical_pending_uses_original_finish_once_and_full_cas(){
 let root=Root::new();let s=Store::new();let r=Runtime::stopped();let old=s.row();
 let result=core::reconcile_generation_for_list(&root.0,&s,&r,&old).await.unwrap();assert!(result.changed);assert!(result.diagnostic.is_none());
 let row=result.task.unwrap();assert_eq!(row.get_str("status").unwrap(),"pending");let g=row.get_document("generation").unwrap();assert_eq!(g.get_str("phase").unwrap(),"generation_failed");assert_eq!(g.get_str("id").unwrap(),"g1");assert!(g.contains_key("finishedAt"));
 let writes=s.writes.lock().unwrap();assert_eq!(writes.len(),1);let q=&writes[0];for k in ["_id","name","generation","events","targetRemote","auth_profile","updatedAt"]{assert_eq!(q.get(k),old.get(k),"{k}");}assert_eq!(q.get_document("acceptedObservation").unwrap().get_bool("$exists").unwrap(),false);drop(writes);
 let again=core::reconcile_generation_for_list(&root.0,&s,&r,&row).await.unwrap();assert!(!again.changed);assert_eq!(s.writes.lock().unwrap().len(),1);assert_eq!(r.calls.load(Ordering::Relaxed),1);
 let stale=core::reconcile_generation_for_list(&root.0,&s,&r,&old).await.unwrap();assert!(!stale.changed);assert_eq!(stale.task.unwrap().get_str("status").unwrap(),"pending");assert_eq!(s.writes.lock().unwrap().len(),1);
}
#[tokio::test] async fn busy_skips_database_probe_and_process_without_writes(){
 let root=Root::new();let s=Store::new();let r=Runtime::stopped();let lock=MissionLocks::acquire(&root.0,&["fixture"],false).unwrap();
 let out=core::reconcile_generation_for_list(&root.0,&s,&r,&s.row()).await.unwrap();assert!(out.diagnostic.is_none());assert_eq!(out.task,Some(s.row()));assert!(!out.changed);assert!(s.calls.lock().unwrap().is_empty());assert_eq!(r.calls.load(Ordering::Relaxed),0);assert!(s.writes.lock().unwrap().is_empty());drop(lock);
 assert!(MissionLocks::try_acquire(&root.0,&["fixture"],false).unwrap().is_some());assert!(MissionLocks::try_acquire(&root.0.join("missing"),&["fixture"],false).is_err());
}
#[tokio::test] async fn live_orphan_owner_uncertainty_and_probe_error_never_write(){
 for observed in [Ok(GenerationProcesses::Live("orphan writer present".into())),Ok(GenerationProcesses::Unverified("owner PID alive without birth".into())),Err("AccessDenied".into())]{let root=Root::new();let s=Store::new();let old=s.row();let r=Runtime{observed,calls:AtomicUsize::new(0)};let out=core::reconcile_generation_for_list(&root.0,&s,&r,&old).await;if let Ok(v)=out{assert!(!v.changed);assert!(v.diagnostic.is_some());}assert_eq!(s.row(),old);assert!(s.writes.lock().unwrap().is_empty());assert!(!s.calls.lock().unwrap().iter().any(|c|c=="has_rows"));}
}
#[tokio::test] async fn nonempty_uncertain_and_missing_history_are_explicit_unknown(){
 for mode in 0..3{let root=Root::new();let mut s=Store::new();if mode==0{s.has=Ok(true)}else if mode==1{s.has=Err("DB probe unavailable".into())}else{s.row.lock().unwrap().as_mut().unwrap().remove("events");}let r=Runtime::stopped();let out=core::reconcile_generation_for_list(&root.0,&s,&r,&s.row()).await.unwrap();assert!(out.changed);let row=s.row();assert_eq!(row.get_str("status").unwrap(),"generation_unknown");assert_eq!(row.get_document("generation").unwrap().get_str("phase").unwrap(),"generation_unknown");assert!(row.get_str("lastError").unwrap().contains(["nonempty","DB probe unavailable","previous_status_unverified"][mode]));}
}
#[tokio::test] async fn superseded_token_and_deleted_identity_are_not_adopted(){
 let root=Root::new();let s=Store::new();let r=Runtime::stopped();let old=s.row();s.row.lock().unwrap().as_mut().unwrap().get_document_mut("generation").unwrap().insert("id","next");let out=core::reconcile_generation_for_list(&root.0,&s,&r,&old).await.unwrap();assert!(!out.changed);assert!(out.diagnostic.unwrap().contains("changed"));assert_eq!(r.calls.load(Ordering::Relaxed),0);
 s.row.lock().unwrap().as_mut().unwrap().insert("_id",ObjectId::new());let out=core::reconcile_generation_for_list(&root.0,&s,&r,&old).await.unwrap();assert!(out.task.is_none());assert!(s.writes.lock().unwrap().is_empty());
}
#[tokio::test] async fn concurrent_snapshot_event_and_cas_changes_never_overwritten(){
 for pre_finish in [false,true]{let root=Root::new();let s=Store::new();let r=Runtime::stopped();let patch=if pre_finish{doc!{"events":[]}}else{doc!{"status":"paused","launchAttemptId":"new"}};if pre_finish{*s.after_probe.lock().unwrap()=Some(patch)}else{*s.cas_patch.lock().unwrap()=Some(patch)}let out=core::reconcile_generation_for_list(&root.0,&s,&r,&s.row()).await.unwrap();assert!(!out.changed);assert!(out.diagnostic.is_some());assert!(!s.row().get_document("generation").unwrap().contains_key("finishedAt"));}
}
#[tokio::test] async fn lost_ack_is_confirmed_by_exact_finish_readback_without_retry(){let root=Root::new();let mut s=Store::new();s.ack_loss=true;let r=Runtime::stopped();let out=core::reconcile_generation_for_list(&root.0,&s,&r,&s.row()).await.unwrap();assert!(out.changed);assert_eq!(s.writes.lock().unwrap().len(),1);}
#[tokio::test] async fn malformed_phase_pid_or_token_never_dispatches_probe(){for mode in 0..4{let root=Root::new();let s=Store::new();let r=Runtime::stopped();let mut row=s.row();if mode==0{row.remove("generation");}else{let g=row.get_document_mut("generation").unwrap();match mode{1=>{g.insert("phase","preparing");},2=>{g.insert("producerPid",0);},_=>{g.insert("id","");}}}*s.row.lock().unwrap()=Some(row.clone());let out=core::reconcile_generation_for_list(&root.0,&s,&r,&row).await;assert!(out.is_err() || out.unwrap().diagnostic.is_some());assert_eq!(r.calls.load(Ordering::Relaxed),0);assert!(s.writes.lock().unwrap().is_empty());}}
#[tokio::test] async fn cli_list_uses_same_coordinator_projects_and_isolates_row_error(){let root=Root::new();let s=Store::new();let r=Runtime::stopped();let out=list_with_generation(&root.0,&s,&r,&ListRequest::default()).await;assert_eq!(out.value["ok"],true);assert_eq!(out.value["tasks"][0]["status"],"pending");assert!(out.value["tasks"][0].get("events").is_none());assert_eq!(s.writes.lock().unwrap().len(),1);
 let s=Store::new();s.extra_rows.lock().unwrap().push(doc!{"_id":ObjectId::new(),"name":"unrelated","status":"ready"});let r=Runtime{observed:Err("argv unverified".into()),calls:AtomicUsize::new(0)};let out=list_with_generation(&root.0,&s,&r,&ListRequest::default()).await;assert_eq!(out.value["ok"],true);assert_eq!(out.value["tasks"][0]["generationRefreshError"],"argv unverified");assert_eq!(out.value["tasks"][1]["name"],"unrelated");assert!(out.value["tasks"][1].get("generationRefreshError").is_none());assert!(s.writes.lock().unwrap().is_empty());
 let out=list_with_generation(&root.0,&s,&r,&ListRequest{remote:true,include_deactive:false}).await;assert_eq!(out.value["ok"],true);assert_eq!(r.calls.load(Ordering::Relaxed),1);}

#[cfg(feature="test-support")]
mod facade_route {
 use super::*;
 use std::{sync::Arc,os::unix::fs::PermissionsExt};
 use quantnight_task_core::{backend_binding::validate_backend_binding,config::bash_test_support::FakeBash,task_control::TaskControl};
 fn fixture(body:&str)->FakeBash {
  let fake=FakeBash::new(":");let root=Path::new(&fake.config.env.working_dir);
  std::fs::write(root.join("credentials.yaml"),"synthetic\n").unwrap();std::fs::create_dir_all(root.join(".venv/bin")).unwrap();std::fs::create_dir_all(root.join("scripts")).unwrap();
  let python=root.join(".venv/bin/python");std::fs::write(&python,"#!/bin/sh\n[ \"$3\" = backend.mongodb.mongo_config ] || exit 97\necho '{\"schemaVersion\":1,\"command\":\"validate-task-control-binding\",\"ok\":true}'\n").unwrap();std::fs::set_permissions(&python,std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::write(root.join("scripts/check_tmux_python.sh"),body).unwrap();fake
 }
 fn observation(state:&str)->String {format!("printf '%s\\n' \"$@\" > observed.argv\nprintf '%s\\n' 'QUANTNIGHT_GENERATION_OBSERVED {{\"schemaVersion\":1,\"taskId\":\"{ID}\",\"taskName\":\"fixture\",\"generationId\":\"g1\",\"producerPid\":42000,\"remote\":false,\"state\":\"{state}\",\"reason\":\"fixture process fact\"}}'\n")}
 #[tokio::test] async fn shared_facade_and_native_cli_reach_original_runtime_local_mode_and_finish(){
  for cli in [false,true]{let fake=fixture(&observation("stopped"));let binding=validate_backend_binding(&fake.config).await.unwrap();let s=Arc::new(Store::new());s.row.lock().unwrap().as_mut().unwrap().insert("isRemote",true);let controller=TaskControl::with_store(binding,s.clone());
   if cli{assert_eq!(controller.list(&ListRequest::default()).await.value["tasks"][0]["status"],"pending");}else{assert!(controller.reconcile_generation_for_list(&s.row()).await.unwrap().changed);}
   assert_eq!(s.writes.lock().unwrap().len(),1);let argv=std::fs::read_to_string(Path::new(&fake.config.env.working_dir).join("observed.argv")).unwrap();let args:Vec<_>=argv.lines().collect();assert_eq!(&args[..4],&["fixture","false","--inspect-generation","--binding"]);let request:serde_json::Value=serde_json::from_str(args[4]).unwrap();assert_eq!(request["taskId"],ID);assert_eq!(request["generation"]["id"],"g1");
  }
 }
 #[tokio::test] async fn real_facade_original_checker_early_mode_and_leaf_fake_os_close_empty_generation(){
  let backend=std::env::var_os("QUANTNIGHT_BACKEND_SOURCE").map(PathBuf::from)
      .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../backend"));
  let source=std::fs::read_to_string(backend.join("scripts/check_tmux_python.sh")).expect("paired backend checker required");
  let fake=fixture(&source);let root=Path::new(&fake.config.env.working_dir);
  for file in ["script_config.py","script_config.sh"] {std::fs::copy(backend.join("scripts").join(file),root.join("scripts").join(file)).unwrap();}
  let python=std::env::var("QUANTNIGHT_TEST_PYTHON").unwrap_or_else(|_| "python3".into());
  // Only fake config export and OS leaves. The actual checker shell, embedded
  // main, mode parser, argv/PID logic, runtime and core finalizer all execute.
  std::fs::write(root.join(".venv/bin/python"),format!(r#"#!/bin/sh
if [ "$3" = backend.mongodb.mongo_config ]; then
 echo '{{"schemaVersion":1,"command":"validate-task-control-binding","ok":true}}'
else
 export PATH="$PWD/bin:$PATH"
 exec '{python}' "$@"
fi
"#)).unwrap();
  std::fs::create_dir_all(root.join("bin")).unwrap();
  std::fs::write(root.join("psutil.py"),"class Error(Exception): pass\nclass NoSuchProcess(Error): pass\ndef Process(pid): raise NoSuchProcess(pid)\n").unwrap();
  std::fs::write(root.join("bin/ps"),"#!/bin/sh\necho '1 0 launchd'\n").unwrap();
  for file in ["ps","tmux","ssh","uv"] {let path=root.join("bin").join(file);if file!="ps" {std::fs::write(&path,"#!/bin/sh\necho forbidden_leaf >&2\nexit 97\n").unwrap();}std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o700)).unwrap();}
  let config_path=root.join("scripts/script_config.py");
  let config_source=std::fs::read_to_string(&config_path).unwrap().replace(
      "NATIVE_TMUX_CANDIDATES = (\"/opt/homebrew/bin/tmux\", \"/usr/local/bin/tmux\", \"/usr/bin/tmux\", \"/bin/tmux\")",
      &format!("NATIVE_TMUX_CANDIDATES = ({:?},)",root.join("bin/tmux").to_str().unwrap()));
  std::fs::write(config_path,config_source).unwrap();
  let binding=validate_backend_binding(&fake.config).await.unwrap();let s=Arc::new(Store::new());
  let controller=TaskControl::with_store(binding,s.clone());
  let result=controller.reconcile_generation_for_list(&s.row()).await.unwrap();assert!(result.changed,"{:?}",result.diagnostic);
  assert_eq!(s.row().get_str("status").unwrap(),"pending");assert_eq!(s.writes.lock().unwrap().len(),1);
 }
 #[tokio::test] async fn adapter_rejects_foreign_duplicate_malformed_and_nonzero_observation(){
  for output in [observation("stopped").replace(ID,"1123456789abcdef01234567"),observation("stopped").replace("g1","foreign"),observation("stopped").replace("42000","42001"),observation("stopped").replace("false","true"),observation("unknown-state"),format!("{}{}",observation("stopped"),observation("stopped")),"echo malformed".into(),format!("{}\necho unverified >&2",observation("stopped")),format!("{}\nexit 23",observation("stopped"))]{let fake=fixture(&output);let binding=validate_backend_binding(&fake.config).await.unwrap();let s=Arc::new(Store::new());let controller=TaskControl::with_store(binding,s.clone());assert!(controller.reconcile_generation_for_list(&s.row()).await.is_err());assert!(s.writes.lock().unwrap().is_empty());assert!(!s.calls.lock().unwrap().iter().any(|c|c=="has_rows"));}
 }
}

#[tokio::test] async fn history_failure_outcome_is_not_misread_as_pending_and_does_not_expand_replay(){
 for mode in 0..4 {let root=Root::new();let s=Store::new();let r=Runtime::stopped();{
  let mut row=s.row.lock().unwrap();let t=row.as_mut().unwrap();let events=t.get_array_mut("events").unwrap();
  match mode {
   0=>{events[0].as_document_mut().unwrap().insert("status","ready");},
   1=>{events.insert(1,Bson::Document(doc!{"command":"unknown-event","status":"pending","at":"2026-10-01T00:00:00+00:00","detail":{}}));},
   2=>{events[0].as_document_mut().unwrap().insert("at","2026-10-02T00:00:00+00:00");},
   _=>{events.insert(1,Bson::Document(doc!{"command":"generation-finish","status":"generation_failed","at":"2026-10-01T00:00:00+00:00","detail":{"generationId":"old"}}));}
  }
 }let out=core::reconcile_generation_for_list(&root.0,&s,&r,&s.row()).await.unwrap();assert!(out.changed);assert_eq!(s.row().get_str("status").unwrap(),"generation_unknown");assert!(s.row().get_str("lastError").unwrap().contains("previous_status_unverified"));}
}

// Sanitized history is emitted by the actual watch coordinator/update writer,
// not an invented event schema. No real watcher/notification/process is run.
mod watch_history_review {
    use super::*;
    use quantnight_task_core::{maintenance_ops::{self, MaintenanceRuntime, WatchRequest,
        ObservationCommand, ObservationOutput, PriorityPruneCommand}, start::{NotificationOptions, NotificationTarget}};
    struct WatchLeaf;
    #[async_trait]
    impl MaintenanceRuntime for WatchLeaf {
        async fn prepare_watch(&self, _: &Path, task: &Document, remote: bool, _: &Document) -> Result<Document,String> {
            let generation=task.get_document("generation").unwrap();
            Ok(doc!{"missionId":task.get_object_id("_id").unwrap().to_hex(),"launchId":"legacy","remote":remote,
                "generationId":generation.get_str("id").unwrap(),"generationPhase":generation.get_str("phase").unwrap()})
        }
        async fn attach_watcher(&self, _: &Path, task: &Document, remote: bool, spec: &Document, replace: bool) -> Result<Document,String> {
            let mut receipt=spec.clone();
            receipt.extend(doc!{"missionId":task.get_object_id("_id").unwrap().to_hex(),"launchId":"legacy","remote":remote,
                "generationId":"g1","generationPhase":"writing","producerPid":43,
                "producerMarkerPath":"sanitized-watcher.lock","checkpointPath":"sanitized-watch.json",
                "requiresLease":spec.get_document("target").unwrap().contains_key("taskId"),"reused":!replace});
            if replace { receipt.insert("replacedProducer",doc!{"pid":42,"processCreatedAt":123.5,
                "target":{"sessionId":"sanitized-main"},"itemKey":"mission:fixture"}); }
            Ok(receipt)
        }
        async fn assert_consumer_absent(&self,_:&Path,_:&Document,_:bool)->Result<(),String>{panic!("no consumer inspection")}
        async fn observe_accepted(&self,_:&Path,_:&ObservationCommand)->Result<ObservationOutput,String>{panic!("no observation")}
        async fn priority_prune(&self,_:&Path,_:&Document,_:&PriorityPruneCommand)->Result<Document,String>{panic!("no prune")}
    }
    async fn attach_twice(root:&Root, store:&Store) {
        for replace in [false,true] {
            let request=WatchRequest{name:"fixture".into(),replace_watcher:replace,notification:NotificationOptions{
                target:Some(if replace { NotificationTarget::Worker("sanitized-worker".into()) }
                    else { NotificationTarget::Session("sanitized-main".into()) }),..Default::default()}};
            let result=maintenance_ops::watch(&root.0,store,&WatchLeaf,&request).await;
            assert_eq!(result.value["ok"],true,"{}",result.value);
            assert_eq!(result.value["simulationStarted"],false);
        }
        let row=store.row();let events=row.get_array("events").unwrap();assert_eq!(events.len(),4);
        assert_eq!(events[0].as_document().unwrap().get_str("status"),Ok("pending"));
        for event in &events[2..] {
            let event=event.as_document().unwrap();assert_eq!(event.get_str("command"),Ok("watch-attach"));
            assert_eq!(event.get_str("status"),Ok("ok"));assert!(!event.get_str("id").unwrap().is_empty());
        }
        assert_eq!(row.get_str("status"),Ok("generating"));
        store.writes.lock().unwrap().clear();store.calls.lock().unwrap().clear();
    }
    #[tokio::test]
    async fn original_watch_attach_and_replacement_after_begin_restore_empty_dead_pending() {
        for remote in [false,true] {
            let root=Root::new();let store=Store::new();store.row.lock().unwrap().as_mut().unwrap().insert("isRemote",remote);
            attach_twice(&root,&store).await;
            let before=store.row();let runtime=Runtime::stopped();
            let out=core::reconcile_generation_for_list(&root.0,&store,&runtime,&before).await.unwrap();
            assert!(out.changed);assert!(out.diagnostic.is_none());let after=out.task.unwrap();
            assert_eq!(after.get_str("status"),Ok("pending"));
            assert_eq!(after.get_document("generation").unwrap().get_str("phase"),Ok("generation_failed"));
            assert_eq!(after.get("agentNotification"),before.get("agentNotification"));
            assert_eq!(&after.get_array("events").unwrap()[..4],before.get_array("events").unwrap());
            assert_eq!(store.writes.lock().unwrap().len(),1);
            let repeat=core::reconcile_generation_for_list(&root.0,&store,&runtime,&after).await.unwrap();
            assert!(!repeat.changed);assert_eq!(store.writes.lock().unwrap().len(),1);
        }
    }
    #[tokio::test]
    async fn foreign_malformed_or_state_changing_watch_histories_remain_explicit_unknown() {
        for mode in 0..10 {
            let root=Root::new();let store=Store::new();attach_twice(&root,&store).await;
            { let mut guard=store.row.lock().unwrap();let row=guard.as_mut().unwrap();
              let event=row.get_array_mut("events").unwrap()[3].as_document_mut().unwrap();
              match mode {
                0=>{event.get_document_mut("detail").unwrap().insert("missionId","1123456789abcdef01234567");},
                1=>{event.get_document_mut("detail").unwrap().insert("generationId","foreign");},
                2=>{event.get_document_mut("detail").unwrap().insert("remote",true);},
                3=>{event.insert("status","paused");},
                4=>{event.insert("command","pause");},
                5=>{event.insert("detail","malformed");},
                6=>{event.get_document_mut("detail").unwrap().insert("generationPhase","preparing");},
                7=>{event.insert("at","2026-09-01T00:00:00+00:00");},
                8=>{event.get_document_mut("detail").unwrap().insert("launchId","foreign-launch");},
                _=>{event.get_document_mut("detail").unwrap().insert("target",doc!{"foreign":"not-a-route"});}
              }
            }
            let out=core::reconcile_generation_for_list(&root.0,&store,&Runtime::stopped(),&store.row()).await.unwrap();
            assert!(out.changed);let row=out.task.unwrap();assert_eq!(row.get_str("status"),Ok("generation_unknown"),"mode {mode}");
            assert!(row.get_str("lastError").unwrap().contains("previous_status_unverified"));
        }
    }
}
