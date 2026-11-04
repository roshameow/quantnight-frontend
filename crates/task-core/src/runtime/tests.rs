//! Offline real Unix codec/peer UID -> shared dispatcher -> original TaskControl -> lowest fake storage.
//! No live launchd/Mongo/platform and no whole-business success mock.
use super::*;
use crate::{task_store::{TaskStore,Host,CollectionSummary},backend_binding::validate_backend_binding,config::bash_test_support::FakeBash};
use bson::{doc,Document};
use std::{sync::{Arc,atomic::{AtomicUsize,Ordering}},path::Path};
use protocol::{Message,CallContext,Admission,Hello};
use identity::{RuntimeSelection,ConfigurationSelector,HostIdentity};
use tokio::net::UnixStream;
use std::os::unix::fs::PermissionsExt;
struct Store {calls:AtomicUsize,entered:tokio::sync::Notify,gate:tokio::sync::Notify}
#[async_trait::async_trait]
impl TaskStore for Store {
    async fn find_task(&self,_:Host,filter:Document,_:bool)->Result<Option<Document>,String>{
        self.calls.fetch_add(1,Ordering::SeqCst);self.entered.notify_one();self.gate.notified().await;
        Ok(Some(doc!{"_id":filter.get_object_id("_id").unwrap(),"name":"original-task","value":9007199254740993_i64}))
    }
    async fn update_task(&self,_:Host,_:Document,_:Document)->Result<u64,String>{panic!("unexpected mutation")}
    async fn collection_names(&self,_:Host)->Result<Vec<String>,String>{panic!("unexpected read")}
    async fn summary(&self,_:Host,_:&str)->Result<CollectionSummary,String>{panic!("unexpected read")}
    async fn has_rows(&self,_:Host,_:&str)->Result<bool,String>{panic!("unexpected read")}
    async fn rows(&self,_:Host,_:&str,_:Option<Document>)->Result<Vec<Document>,String>{panic!("unexpected read")}
    async fn insert_rows(&self,_:Host,_:&str,_:Vec<Document>)->Result<(),String>{panic!("unexpected mutation")}
    async fn rename_collection(&self,_:Host,_:&str,_:&str)->Result<(),String>{panic!("unexpected mutation")}
    async fn ping(&self,_:Host)->Result<(),String>{panic!("unexpected IO")}
}
async fn fixture()->(FakeBash,Arc<server::RuntimeState>,Arc<Store>){
    let fake=FakeBash::new(":");let root=Path::new(&fake.config.env.working_dir);
    std::fs::write(root.join("credentials.yaml"),"synthetic only").unwrap();std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    let python=root.join(".venv/bin/python");std::fs::write(&python,"#!/bin/sh\nprintf '%s\\n' '{\"schemaVersion\":1,\"command\":\"validate-task-control-binding\",\"ok\":true}'\n").unwrap();std::fs::set_permissions(python,std::fs::Permissions::from_mode(0o700)).unwrap();
    let binding=validate_backend_binding(&fake.config).await.unwrap();
    let store=Arc::new(Store{calls:AtomicUsize::new(0),entered:tokio::sync::Notify::new(),gate:tokio::sync::Notify::new()});
    let control=Arc::new(crate::task_control::TaskControl::with_store(binding,store.clone()));
    let selection=RuntimeSelection{binding_path:root.join("binding.json"),backend_root:root.into(),frontend_root:root.into(),executable:std::env::current_exe().unwrap(),configuration:ConfigurationSelector{kind:"backend_root".into(),path:root.into()},protocol_version:1,binding_revision:"old".into(),target:crate::DEV_TARGET.into(),profile:crate::DEV_PROFILE.into()};
    let host=HostIdentity{selection,uid:identity::uid(),owner:identity::process_identity(std::process::id()).unwrap(),generation:"original-host".into(),image:crate::native_identity::loaded().unwrap(),core_source:"old".into(),cli_source:"old".into(),mission_database:"simulation_mission".into(),simulation_database:"simulation_db".into(),task_settings:"old".into(),ready:true};
    (fake,server::RuntimeState::new(control,host),store)
}
// The activation fixture reuses this original lowest-storage seam, never DB.
pub(super) async fn activation_fixture()->(FakeBash,Arc<server::RuntimeState>){let(fake,state,_store)=fixture().await;(fake,state)}
fn call()->(CallContext,crate::task_request::TaskRequest){(CallContext{request_id:identity::nonce().unwrap(),caller_cwd:std::env::current_dir().unwrap(),pause_deadline_ns:None,diagnostic_deadline_ns:None,operational_env:crate::operations_runtime::OperationalEnvironment::default()},crate::task_request::TaskRequest::TaskById{id:"0123456789abcdef01234567".into()})}
async fn admitted_pair(state:Arc<server::RuntimeState>)->UnixStream{
    let(client,mut host)=UnixStream::pair().unwrap();identity::peer_uid(&client).unwrap();identity::peer_uid(&host).unwrap();
    tokio::spawn(async move{let message=codec::read(&mut host).await.unwrap();let _=server::execute_message(host,state,message).await;});client
}
#[tokio::test]
async fn original_core_result_is_lossless_and_connection_loss_never_replays(){
    let(_root,state,store)=fixture().await;let(context,request)=call();
    let client=admitted_pair(state.clone()).await;
    let task=tokio::spawn(client::RuntimeClient::exchange(client,context,request,None));
    store.entered.notified().await;assert_eq!(state.status().active,1);store.gate.notify_one();
    let result=task.await.unwrap().unwrap();let document=wire_document::decode(result.value).unwrap();
    assert_eq!(document.get_i64("value").unwrap(),9007199254740993);assert_eq!(store.calls.load(Ordering::SeqCst),1);
    let(context,request)=call();let client=admitted_pair(state.clone()).await;
    let task=tokio::spawn(client::RuntimeClient::exchange(client,context,request,None));
    store.entered.notified().await;task.abort();let _=task.await;assert_eq!(state.status().active,1);
    state.drain();assert_eq!(state.status().active,1);store.gate.notify_one();state.wait_drained().await;
    assert_eq!(store.calls.load(Ordering::SeqCst),2);assert_eq!(state.status().active,0);
    assert_eq!(state.status().accepted_by_kind["TaskById"],2);assert_eq!(state.status().accepted_by_kind["GuiTasks"],0);assert_eq!(state.status().accepted_by_kind.len(),20);
    let(context,request)=call();let result=client::RuntimeClient::exchange(admitted_pair(state.clone()).await,context,request,None).await.unwrap_err();
    assert_eq!(result.class,"host_draining");assert_eq!(result.execution,"not_executed");assert_eq!(store.calls.load(Ordering::SeqCst),2);
}
#[tokio::test]
async fn duplicate_id_not_dispatched_and_partial_reply_is_unconfirmed(){
    let(_root,state,store)=fixture().await;let(context,request)=call();let id=context.clone();let req=request.clone();
    let first=tokio::spawn(client::RuntimeClient::exchange(admitted_pair(state.clone()).await,context,request,None));store.entered.notified().await;
    let second=client::RuntimeClient::exchange(admitted_pair(state.clone()).await,id,req,None).await.unwrap_err();assert_eq!(second.class,"duplicate_request");assert_eq!(store.calls.load(Ordering::SeqCst),1);
    store.gate.notify_one();first.await.unwrap().unwrap();
    let(client,mut fake)=UnixStream::pair().unwrap();let(context,request)=call();tokio::spawn(async move{let _=codec::read(&mut fake).await;});
    let error=client::RuntimeClient::exchange(client,context,request,None).await.unwrap_err();assert_eq!(error.execution,"unconfirmed");assert_eq!(error.class,"connection_lost");
}
#[tokio::test]
async fn stale_host_management_works_but_business_rejects_before_original_core(){
    let(_root,state,store)=fixture().await;
    for mode in [Admission::Management,Admission::Business]{
        let(client,host)=UnixStream::pair().unwrap();let s=state.clone();tokio::spawn(async move{let _=server::serve_connection(host,s).await;});let mut client=client;
        codec::write_preamble(&mut client).await.unwrap();codec::write(&mut client,&Message::Hello(Hello{mode:mode.clone(),protocol:1,selection:state.identity.selection.clone(),core_source:"new".into(),task_settings:None})).await.unwrap();codec::read_preamble(&mut client).await.unwrap();
        match codec::read(&mut client).await.unwrap(){
            Message::Ready{identity,..}=>{assert_eq!(mode,Admission::Management);assert_eq!(identity.generation,"original-host");codec::write(&mut client,&Message::Drain{expected_host_generation:identity.generation}).await.unwrap();match codec::read(&mut client).await.unwrap(){Message::HostStatus(s)=>assert!(s.draining),_=>panic!("missing drain facts")}},
            Message::Error(e)=>{assert_eq!(mode,Admission::Business);assert_eq!(e.class,"source_stale");},_=>panic!("wrong hello")
        }
    }
    assert_eq!(store.calls.load(Ordering::SeqCst),0);
}
#[tokio::test]
async fn chunked_large_message_has_no_small_total_cap_and_bad_frame_fails(){
    let(input,mut output)=tokio::io::duplex(4096);let body="x".repeat(3*1024*1024);let expected=body.len();
    let writer=tokio::spawn(async move{codec::write(&mut output,&Message::Completed{id:"large".into(),result:CallResult{value:serde_json::json!(body),exit_code:0,confirmed_stop:None}}).await.unwrap();});
    let mut input=input;match codec::read(&mut input).await.unwrap(){Message::Completed{result,..}=>assert_eq!(result.value.as_str().unwrap().len(),expected),_=>panic!("wrong message")};writer.await.unwrap();
    use tokio::io::AsyncWriteExt;let(mut writer,mut reader)=tokio::io::duplex(16);writer.write_u32((codec::CHUNK+1) as u32).await.unwrap();assert!(codec::read(&mut reader).await.unwrap_err().contains("physical"));
}
#[test]
fn bson_business_document_keys_and_large_integers_are_not_extjson_interpreted(){
    let mut request=crate::create_generate::CreateRequest::new("test","test");let document=doc!{"opaque":{"$date":"business-key"},"integer":9007199254740993_i64,"when":bson::DateTime::from_millis(33),"id":bson::oid::ObjectId::new()};request.mission_config=Some(document.clone());
    let bytes=serde_json::to_vec(&request).unwrap();let decoded:crate::create_generate::CreateRequest=serde_json::from_slice(&bytes).unwrap();assert_eq!(decoded.mission_config.unwrap(),document);
}

#[tokio::test]
async fn transport_wait_has_no_blanket_thirty_second_operation_deadline(){
    let(_root,state,store)=fixture().await;tokio::time::pause();let(context,request)=call();
    let waiter=tokio::spawn(client::RuntimeClient::exchange(admitted_pair(state.clone()).await,context,request,None));
    store.entered.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(901)).await;
    assert!(!waiter.is_finished(),"transport must not cancel accepted core work at 30s/900s");
    assert_eq!(state.status().active,1);store.gate.notify_one();waiter.await.unwrap().unwrap();
    state.wait_drained().await;assert_eq!(store.calls.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn prior_call_frame_rejected_on_new_connection_without_completed_id_history(){
    let(_root,state,store)=fixture().await;let(context,request)=call();let previous_id=context.request_id.clone();
    let current_id=identity::nonce().unwrap();assert_ne!(previous_id,current_id);
    let(client,mut host)=UnixStream::pair().unwrap();let s=state.clone();tokio::spawn(async move{let message=codec::read(&mut host).await.unwrap();server::execute_bound(host,s,message,&current_id).await.unwrap();});
    let error=client::RuntimeClient::exchange(client,context,request,None).await.unwrap_err();
    assert_eq!(error.class,"identity_mismatch");assert_eq!(error.execution,"not_executed");assert_eq!(store.calls.load(Ordering::SeqCst),0);
    assert_eq!(state.status().active,0);
}

#[tokio::test]
async fn burst_progress_is_bounded_and_final_original_stop_proof_uses_separate_completed_message(){
    let(observer,mut receiver,dropped)=server::operation_observer();
    for n in 0..1000{observer(RuntimeEvent::Bash(crate::config::BashScriptEvent::Stdout(format!("synthetic-stage-{n}"))));}
    assert_eq!(receiver.len(),64);assert_eq!(dropped.load(Ordering::Relaxed),936);
    let proof=crate::pause::ConfirmedTaskStop{confirmed:true,task_id:"0123456789abcdef01234567".into(),task_name:"exact".into(),is_remote:true,stop_confirmed_at:9007199254740993,confirmation_basis:crate::pause::StopConfirmationBasis::SessionAbsent};
    let message=Message::Completed{id:"original-call".into(),result:CallResult{value:serde_json::json!({"ok":true,"command":"pause","task":"exact"}),exit_code:0,confirmed_stop:Some(proof)}};
    let(mut writer,mut reader)=tokio::io::duplex(128);
    let send=tokio::spawn(async move{codec::write(&mut writer,&message).await.unwrap();});
    // Slow event consumption never occupies this independent completion path.
    match codec::read(&mut reader).await.unwrap(){Message::Completed{result,..}=>{let stop=result.confirmed_stop.unwrap();assert!(stop.confirmed);assert!(stop.is_remote);assert_eq!(stop.task_name,"exact");assert_eq!(stop.stop_confirmed_at,9007199254740993);assert_eq!(stop.confirmation_basis,crate::pause::StopConfirmationBasis::SessionAbsent);},_=>panic!("missing original completion")};
    send.await.unwrap();drop(observer);let mut count=0;while receiver.recv().await.is_some(){count+=1;}assert_eq!(count,64);
}

#[tokio::test]
async fn owned_chunk_serializer_preserves_large_legal_result_and_releases_on_disconnect(){
    let(a,mut b)=tokio::io::duplex(128);let data="legal".repeat(1024*1024);let length=data.len();
    let send=tokio::spawn(async move{codec::write_owned(&mut b,Message::Completed{id:"owned".into(),result:CallResult{value:serde_json::json!(data),exit_code:0,confirmed_stop:None}}).await});
    let mut a=a;match codec::read(&mut a).await.unwrap(){Message::Completed{result,..}=>assert_eq!(result.value.as_str().unwrap().len(),length),_=>panic!("wrong response")};send.await.unwrap().unwrap();
    let(reader,mut writer)=tokio::io::duplex(64);drop(reader);
    let result=codec::write_owned(&mut writer,Message::Completed{id:"closed".into(),result:CallResult{value:serde_json::json!("x".repeat(1024*1024)),exit_code:0,confirmed_stop:None}}).await;
    assert!(result.is_err());
}
