//! Structural owner oracle after runtime migration. The CLI must delegate once;
//! every application route must terminate at the original core facade, not IPC policy copies.
use quote::ToTokens;
use std::collections::BTreeMap;
fn tokens(value:&impl ToTokens)->String{value.to_token_stream().to_string()}
fn function(source:&str)->syn::ItemFn{syn::parse_file(source).unwrap().items.into_iter().find_map(|item|match item{syn::Item::Fn(f) if f.sig.ident=="dispatch"=>Some(f),_=>None}).unwrap()}
fn normalized(body:&str)->String{
    let mut expr=syn::parse_str::<syn::Expr>(body).unwrap();
    if let syn::Expr::Block(block)=&expr{if let [syn::Stmt::Expr(inner,None)]=block.block.stmts.as_slice(){expr=inner.clone();}}
    tokens(&expr).split_whitespace().collect::<String>().replace(",)",")").replace(",}","}")
}
fn add_pattern(pat:&syn::Pat,body:&syn::Expr,result:&mut BTreeMap<String,String>){
    match pat {
        syn::Pat::Or(or)=>for case in &or.cases{add_pattern(case,body,result)},
        syn::Pat::TupleStruct(p)=>{assert!(result.insert(p.path.segments.last().unwrap().ident.to_string(),normalized(&tokens(body))).is_none());},
        syn::Pat::Struct(p)=>{assert!(result.insert(p.path.segments.last().unwrap().ident.to_string(),normalized(&tokens(body))).is_none());},
        syn::Pat::Path(p)=>{assert!(result.insert(p.path.segments.last().unwrap().ident.to_string(),normalized(&tokens(body))).is_none());},
        _=>panic!("unrestricted route pattern is not allowed")
    }
}
fn routes(source:&str)->BTreeMap<String,String>{
    let function=function(source);let matched=function.block.stmts.iter().find_map(|stmt|match stmt{syn::Stmt::Local(local)=>local.init.as_ref().and_then(|init|match init.expr.as_ref(){syn::Expr::Match(m)=>Some(m),_=>None}),_=>None}).expect("one typed application match required");
    assert_eq!(tokens(&matched.expr),"request");let mut result=BTreeMap::new();
    for arm in &matched.arms{assert!(arm.guard.is_none());add_pattern(&arm.pat,&arm.body,&mut result);}result
}
fn expected()->BTreeMap<String,String>{
    let specs=[
        ("List","control.list(r).await"),("VerifyRegistration","control.dataset_evidence(r).await"),("ArchiveResearch","control.dataset_evidence(r).await"),
        ("Create","control.create(r,bash).await"),("Generate","control.generate(r,bash).await"),
        ("Sync","control.sync_bound(&r.name,r.expected_task_id.as_deref(),r.expected_remote,bash).await"),
        ("Start","control.start(r,bash).await"),("ObserveAccepted","control.observe_accepted(r,bash).await"),
        ("Watch","control.watch(r).await"),("Status","control.status(r).await"),
        ("Pause",r#"{let stages=observer.map(|o|Arc::new(move |e|o(RuntimeEvent::PauseStage(e))) as crate::pause::PauseObserver);
            let p=control.pause(&r.name,PauseOptions{expected_task_id:r.expected_task_id.as_deref(),expected_launch_attempt_id:r.expected_launch_attempt_id.as_deref(),expected_launch_absent:r.expected_launch_absent,expected_remote:r.expected_remote,remaining:r.remaining_ms.map(std::time::Duration::from_millis),observer:stages},r.budget,bash).await;
            return Ok(CallResult{value:p.value,exit_code:p.exit_code,confirmed_stop:p.confirmed_stop})}"#),
        ("Resume","control.resume(r,bash).await"),("PriorityPrune","control.priority_prune(r,bash).await"),
        ("Delete","control.delete_bound(&r.name,r.expected_task_id.as_deref(),r.expected_remote,r.confirm.as_deref(),bash).await"),
        ("Results","control.results(r).await"),("CollectResults","control.collect_results(r).await"),
        ("PriorityUpdate","control.update_priority(r,bash).await"),
        ("TaskById","return Ok(CallResult{value:crate::runtime::wire_document::encode(&control.task_by_id(id).await?)?,exit_code:0,confirmed_stop:None})"),
        ("GuiTasks","return Ok(CallResult{value:crate::gui_task_queries::list(control.runtime_context()?.as_ref(),control).await?,exit_code:0,confirmed_stop:None})"),
        ("TaskDiagnostics","return Ok(CallResult{value:serde_json::Value::String(crate::task_diagnostics::inspect(control.runtime_context()?.as_ref(),name,task_id.as_deref(),*remote,observer).await?),exit_code:0,confirmed_stop:None})"),
    ];specs.into_iter().map(|(name,body)|(name.into(),normalized(body))).collect()
}
fn assert_routes(source:&str){let actual=routes(source);assert_eq!(actual.len(),20);assert_eq!(actual,expected());}
fn assert_cli(source:&str){let f=function(source);assert_eq!(f.block.stmts.len(),1);let syn::Stmt::Expr(expr,_) =&f.block.stmts[0] else{panic!("one delegate expression")};assert_eq!(normalized(&tokens(expr)),normalized(r#"match quantnight_task_core::task_dispatch::dispatch(control,request,None).await {Ok(result)=>TaskOutcome{value:result.value,exit_code:result.exit_code},Err(error)=>TaskOutcome{value:serde_json::json!({"schemaVersion":1,"ok":false,"errorType":"RuntimeError","error":error}),exit_code:1}}"#));}
#[test]
fn every_route_delegates_to_the_actual_common_facade(){assert_cli(include_str!("../src/dispatch.rs"));assert_routes(include_str!("../../task-core/src/task_dispatch.rs"));}
#[test]
fn oracle_rejects_misrouting_or_changed_safety_arguments(){
    let source=include_str!("../../task-core/src/task_dispatch.rs");
    for(old,new) in [("control.status(r)","control.results(r)"),("r.budget,bash","crate::pause::StopBudget::Cli,bash"),("expected_remote:r.expected_remote","expected_remote:None"),("exit_code:p.exit_code","exit_code:0"),("r.confirm.as_deref()","None"),("confirmed_stop:p.confirmed_stop","confirmed_stop:None")]{
        assert!(source.contains(old));assert!(std::panic::catch_unwind(||assert_routes(&source.replace(old,new))).is_err(),"oracle must reject {new}");
    }
    let cli=include_str!("../src/dispatch.rs");assert!(std::panic::catch_unwind(||assert_cli(&cli.replace("exit_code:result.exit_code","exit_code:0"))).is_err());
}
