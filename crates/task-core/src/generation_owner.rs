//! Original generation business-owner lifetime, not a transport/job/result journal.
//! Retains active exact task/generation scopes only; no completed IDs or client claims.
use std::{collections::HashSet,sync::{Arc,Mutex}};
use bson::{Document,Bson,doc,oid::ObjectId};
use crate::native_identity::{ProcessIdentity,process_identity};
pub(crate) struct HostedGenerationOwner {native:ProcessIdentity,tag:Document,active:Mutex<HashSet<(String,String)>>}
pub(crate) struct GenerationScope {owner:Arc<HostedGenerationOwner>,key:(String,String)}
impl HostedGenerationOwner {
    pub(crate) fn new(native:ProcessIdentity,instance:&str)->Arc<Self>{Arc::new(Self{
        tag:doc!{"schemaVersion":1,"kind":"task_core_host","instanceId":instance,"pid":i64::from(native.pid),
            "startedSeconds":native.started_seconds as i64,"startedMicros":native.started_micros as i64},
        native,active:Mutex::new(HashSet::new())})}
    pub(crate) fn enter(self:&Arc<Self>,task:ObjectId,generation:&str)->Result<GenerationScope,String>{
        if generation.is_empty() || process_identity(std::process::id()).ok().as_ref()!=Some(&self.native){return Err("generation business owner native instance unverified".into());}
        let key=(task.to_hex(),generation.to_owned());
        if !self.active.lock().map_err(|_|"generation scope registry unavailable")?.insert(key.clone()){return Err("same generation scope already active".into());}
        Ok(GenerationScope{owner:self.clone(),key})
    }
    pub(crate) fn tag(&self)->Document{self.tag.clone()}
    /// Some(false) is only current-instance operation absence, NOT writer death,
    /// generation failure/completion, or authority to bypass the original CAS.
    pub(crate) fn scope_active(&self,task:&Document)->Option<bool>{
        if process_identity(std::process::id()).ok().as_ref()!=Some(&self.native){return None;}
        let id=task.get_object_id("_id").ok()?;let generation=task.get_document("generation").ok()?;
        if generation.get_document("controlOwner").ok()!=Some(&self.tag){return None;}
        let pid=match generation.get("producerPid")?{Bson::Int32(pid)=>i64::from(*pid),Bson::Int64(pid)=>*pid,_=>return None};
        if pid!=i64::from(self.native.pid){return None;}
        let token=generation.get_str("id").ok()?;if token.is_empty(){return None;}
        Some(self.active.lock().ok()?.contains(&(id.to_hex(),token.to_string())))
    }
}
impl Drop for GenerationScope {fn drop(&mut self){if let Ok(mut active)=self.owner.active.lock(){active.remove(&self.key);}}}
tokio::task_local!{static CURRENT:Arc<HostedGenerationOwner>;}
pub(crate) fn current()->Option<Arc<HostedGenerationOwner>>{CURRENT.try_with(Arc::clone).ok()}
pub(crate) async fn scope<T>(owner:Option<Arc<HostedGenerationOwner>>,future:impl std::future::Future<Output=T>)->T{if let Some(owner)=owner{CURRENT.scope(owner,future).await}else{future.await}}

#[cfg(all(test,target_os="macos"))]
mod tests {
    use super::*;
    #[test]
    fn same_host_two_generations_have_independent_scopes_without_completed_history(){
        let owner=HostedGenerationOwner::new(process_identity(std::process::id()).unwrap(),"business-instance");
        let(a,b)=(ObjectId::new(),ObjectId::new());let first=owner.enter(a,"generation-a").unwrap();let second=owner.enter(b,"generation-b").unwrap();
        let task=|id,token|doc!{"_id":id,"generation":{"id":token,"phase":"writing","producerPid":std::process::id() as i64,"controlOwner":owner.tag()}};
        assert_eq!(owner.scope_active(&task(a,"generation-a")),Some(true));drop(second);
        assert_eq!(owner.scope_active(&task(b,"generation-b")),Some(false));assert_eq!(owner.scope_active(&task(a,"generation-a")),Some(true));
        let mut foreign=task(b,"generation-b");foreign.get_document_mut("generation").unwrap().get_document_mut("controlOwner").unwrap().insert("instanceId","other-instance");assert_eq!(owner.scope_active(&foreign),None);
        foreign=task(b,"generation-b");foreign.get_document_mut("generation").unwrap().get_document_mut("controlOwner").unwrap().insert("startedMicros",-1);assert_eq!(owner.scope_active(&foreign),None);
        foreign=task(b,"generation-b");foreign.get_document_mut("generation").unwrap().remove("controlOwner");assert_eq!(owner.scope_active(&foreign),None);
        foreign=task(b,"generation-b");foreign.get_document_mut("generation").unwrap().get_document_mut("controlOwner").unwrap().remove("startedSeconds");assert_eq!(owner.scope_active(&foreign),None);
        drop(first);assert!(owner.active.lock().unwrap().is_empty());
    }
    #[test]
    fn unhealthy_registry_cannot_prove_generation_owner_inactive(){
        let owner=HostedGenerationOwner::new(process_identity(std::process::id()).unwrap(),"same-instance");let row=doc!{"_id":ObjectId::new(),"generation":{"id":"generation","producerPid":std::process::id() as i64,"controlOwner":owner.tag()}};
        let poisoned=owner.clone();let _=std::thread::spawn(move||{let _guard=poisoned.active.lock().unwrap();panic!("synthetic registry poison");}).join();
        assert_eq!(owner.scope_active(&row),None);assert!(owner.enter(ObjectId::new(),"new").is_err());
    }

}
