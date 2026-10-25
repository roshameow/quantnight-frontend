use async_trait::async_trait;
use bson::{doc, oid::ObjectId, Bson, Document};
use quantnight_task_core::{priority_update::*, start::{StartRuntime, StartCommand, LaunchOutput},
    task_store::{TaskStore, Host, CollectionSummary}};
use std::{path::Path, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

fn matches(row: &Document, query: &Document) -> bool {
    query.iter().all(|(key, value)| {
        if let Bson::Document(op) = value {
            if let Ok(exists) = op.get_bool("$exists") { return row.contains_key(key) == exists; }
            if let Some(other) = op.get("$ne") { return row.get(key) != Some(other); }
        }
        row.get(key) == Some(value) || (*value == Bson::Null && !row.contains_key(key))
    })
}
struct Store { row: Mutex<Document>, writes: AtomicU64 }
#[async_trait]
impl TaskStore for Store {
    async fn find_task(&self, host: Host, q: Document, _: bool) -> Result<Option<Document>, String> {
        assert_eq!(host, Host::Local); let row = self.row.lock().unwrap();
        Ok(matches(&row, &q).then(|| row.clone()))
    }
    async fn update_task(&self, host: Host, q: Document, change: Document) -> Result<u64, String> {
        assert_eq!(host, Host::Local); self.writes.fetch_add(1, Ordering::SeqCst);
        let mut row=self.row.lock().unwrap(); if !matches(&row,&q) { return Ok(0); }
        let set=change.get_document("$set").unwrap();
        for key in ["status","isRemote","targetRemote","launchAttemptId","stopConfirmedAt"] {
            assert!(!set.contains_key(key), "config publication must not write {key}");
        }
        for (key,value) in set { row.insert(key,value.clone()); } Ok(1)
    }
    async fn collection_names(&self,_:Host)->Result<Vec<String>,String>{panic!("no item IO")}
    async fn summary(&self,_:Host,_:&str)->Result<CollectionSummary,String>{panic!("no item IO")}
    async fn has_rows(&self,_:Host,_:&str)->Result<bool,String>{panic!("no item IO")}
    async fn rows(&self,_:Host,_:&str,_:Option<Document>)->Result<Vec<Document>,String>{panic!("no item IO")}
    async fn insert_rows(&self,_:Host,_:&str,_:Vec<Document>)->Result<(),String>{panic!("no item IO")}
    async fn rename_collection(&self,_:Host,_:&str,_:&str)->Result<(),String>{panic!("no item IO")}
    async fn ping(&self,_:Host)->Result<(),String>{panic!("no ping")}
}
struct Runtime { store:Arc<Store>, mode:&'static str, published:AtomicU64 }
#[async_trait]
impl StartRuntime for Runtime {
    async fn priority_config(&self,_:&Path,_:&Document,c:Option<Document>)->Result<Document,String>{
        if self.mode=="validation-error" { return Err("original validator rejected config".into()); }
        if self.mode=="changed-before-publish" { self.store.row.lock().unwrap().insert("updatedAt","changed"); }
        let mut c=c.unwrap(); if self.mode=="drops-user-scope" { c.remove("custom"); } Ok(c)
    }
    async fn validate_super_config(&self,_:&Path,_:&Document)->Result<(),String>{panic!("no super")}
    async fn bind_regular_source(&self,_:&Path,_:&Document,_:&Document)->Result<Document,String>{panic!("no regular")}
    async fn verify_priority_extension(&self,_:&Path,_:&Document,_:u64,_:u64)->Result<Document,String>{panic!("no extension")}
    async fn continuation(&self,_:&Path,_:&Document,_:bool)->Result<Option<Document>,String>{panic!("no continuation")}
    async fn preflight_notification(&self,_:&Path,_:&Document)->Result<(),String>{panic!("no notification")}
    async fn assert_consumer_absent(&self,_:&Path,_:&Document,_:bool)->Result<(),String>{panic!("publish is not start/stop")}
    async fn validate_execution_binding(&self,_:&Path,_:&Document,_:bool)->Result<(),String>{panic!("no launch")}
    async fn launch(&self,_:&Path,_:&StartCommand)->Result<LaunchOutput,String>{panic!("never launch")}
    async fn attach_watcher(&self,_:&Path,_:&Document,_:bool,_:&Document)->Result<Document,String>{panic!("no watcher")}
}
#[async_trait]
impl PriorityUpdateRuntime for Runtime {
    async fn publish_priority_config(&self,_:&Path,_:&Document,c:&Document,remote:bool)->Result<(),String>{
        assert!(!remote); assert_eq!(c.get_document("custom").unwrap().get_i32("keep").unwrap(),7);
        self.published.fetch_add(1,Ordering::SeqCst);
        if self.mode=="publish-unknown" { return Err("publisher response lost".into()); }
        if self.mode=="replacement-after-publish" { self.store.row.lock().unwrap().insert("_id",ObjectId::new()); }
        Ok(())
    }
}
static NEXT:AtomicU64=AtomicU64::new(0);
struct Fixture { root:std::path::PathBuf, store:Arc<Store>, request:UpdatePriorityRequest }
impl Fixture {
    fn new(status:&str)->Self {
        let root=std::env::temp_dir().join(format!("qn-priority-update-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));
        std::fs::create_dir_all(&root).unwrap(); let id=ObjectId::new();
        let row=doc!{"_id":id,"name":"fixture","taskType":"priority","auth_profile":"user1","status":status,
            "isRemote":false,"targetRemote":true,"updatedAt":"original","launchAttemptId":"old-launch","stopConfirmedAt":bson::DateTime::from_millis(1)};
        Self { root,store:Arc::new(Store{row:Mutex::new(row),writes:AtomicU64::new(0)}),
            request:UpdatePriorityRequest{name:"fixture".into(),config_json:r#"{"custom":{"keep":7}}"#.into(),remote:Some(false),expected_task_id:Some(id.to_hex())} }
    }
    fn runtime(&self,mode:&'static str)->Runtime {Runtime{store:self.store.clone(),mode,published:AtomicU64::new(0)}}
}
impl Drop for Fixture { fn drop(&mut self){std::fs::remove_dir_all(&self.root).unwrap();} }
#[tokio::test]
async fn config_publication_never_asserts_running_or_changes_host_or_launch() {
    for status in ["paused","running","pending"] {
        let f=Fixture::new(status); let before=f.store.row.lock().unwrap().clone(); let runtime=f.runtime("ok");
        let result=update_priority(&f.root,f.store.as_ref(),&runtime,&f.request).await;
        assert_eq!(result.exit_code,0); assert_eq!(result.value["taskStatus"],status);
        assert_eq!(result.value["configPublished"],true); assert_eq!(result.value["adoptionVerified"],false);
        let after=f.store.row.lock().unwrap();
        for key in ["status","isRemote","targetRemote","launchAttemptId","stopConfirmedAt"] {assert_eq!(after.get(key),before.get(key));}
        assert_eq!(runtime.published.load(Ordering::SeqCst),1);
    }
}
#[tokio::test]
async fn identity_host_observer_busy_and_validator_guards_precede_publication() {
    for mode in ["id","host","observer","busy","validation-error","changed-before-publish","drops-user-scope"] {
        let mut f=Fixture::new("paused");
        match mode {"id"=>f.request.expected_task_id=Some(ObjectId::new().to_hex()),"host"=>f.request.remote=Some(true),
            "observer"=>{f.store.row.lock().unwrap().insert("acceptedObservation",doc!{"id":"open"});},
            "busy"=>{f.store.row.lock().unwrap().insert("status","launch_unknown");},_=>{}}
        let runtime=f.runtime(mode);
        let result=update_priority(&f.root,f.store.as_ref(),&runtime,&f.request).await;
        assert_eq!(result.exit_code,1,"{mode}"); assert_eq!(runtime.published.load(Ordering::SeqCst),0,"{mode}");
        assert_eq!(f.store.writes.load(Ordering::SeqCst),0,"{mode}");
    }
}
#[tokio::test]
async fn unknown_publication_and_lost_control_cas_never_retry_or_adopt_replacement() {
    for mode in ["publish-unknown","replacement-after-publish"] {
        let f=Fixture::new("paused"); let runtime=f.runtime(mode);
        let result=update_priority(&f.root,f.store.as_ref(),&runtime,&f.request).await;
        assert_eq!(result.exit_code,1); assert_eq!(runtime.published.load(Ordering::SeqCst),1);
        assert_eq!(result.value["adoptionVerified"],false);
        if mode=="publish-unknown" {assert!(result.value["configPublished"].is_null());}
        else {assert_eq!(result.value["configPublished"],true);}
        assert!(f.store.row.lock().unwrap().get("mission_config").is_none());
    }
}
