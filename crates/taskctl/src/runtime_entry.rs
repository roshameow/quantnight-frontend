//! Runtime management adaptation only. All schema/path/IPC/identity lives in core.
use std::path::{Path,PathBuf};
use quantnight_task_core::runtime::{identity,RuntimeSelection,RuntimeClient};
fn failure(command:&str,error:impl AsRef<str>)->serde_json::Value{serde_json::json!({"schemaVersion":1,"command":command,"ok":false,"errorType":"RuntimeError","error":error.as_ref()})}
pub fn run(input:&[String])->Option<i32>{
    // The original qn shim injects its exact --backend-root prefix. Management
    // still requires explicit --binding; this prefix is only an agreement check.
    let (args,shim_root)=if input.first().map(String::as_str)==Some("--backend-root") && input.get(2).is_some_and(|command|matches!(command.as_str(),"runtime-plan"|"runtime-status"|"runtime-drain"|"runtime-host")) {
        (&input[2..],Some(PathBuf::from(&input[1])))
    } else { (input,None) };
    let command=args.first()?.as_str();if !matches!(command,"runtime-plan"|"runtime-status"|"runtime-drain"|"runtime-host"){return None;}
    if args.iter().skip(1).any(|a|matches!(a.as_str(),"--help"|"-h")){
        println!("taskctl {command} --binding /canonical/binding.json{}",match command{"runtime-plan"=>" [--runtime-executable /explicit/independent/taskctl] [--app-config /canonical/original/config.toml] [--metadata-only]","runtime-drain"=>" --generation EXACT","runtime-status"=>" [--stopped-observation]",_=>""});return Some(0);
    }
    let parsed=(||{
        let(mut binding,mut executable,mut generation,mut app_config,mut metadata_only,mut stopped_observation)=(None,None,None,None,false,false);let mut i=1;
        while i<args.len(){let flag=args[i].as_str();i+=1;if flag=="--stopped-observation" && command=="runtime-status" && !stopped_observation{stopped_observation=true;continue;}if flag=="--metadata-only" && command=="runtime-plan" && !metadata_only{metadata_only=true;continue;}let value=args.get(i).ok_or("runtime option requires a value")?;i+=1;
            match flag{"--binding" if binding.is_none()=>binding=Some(PathBuf::from(value)),"--runtime-executable" if command=="runtime-plan" && executable.is_none()=>executable=Some(PathBuf::from(value)),"--app-config" if command=="runtime-plan" && app_config.is_none()=>app_config=Some(PathBuf::from(value)),"--generation" if command=="runtime-drain" && generation.is_none()=>generation=Some(value.clone()),_=>return Err("invalid or duplicate runtime option")}}
        let binding=binding.ok_or("--binding is required")?;if !binding.is_absolute(){return Err("--binding must be absolute");}
        if command=="runtime-drain" && generation.is_none(){return Err("runtime-drain requires --generation EXACT");}
        Ok((binding,executable,generation,app_config,metadata_only,stopped_observation))
    })();
    let(binding,executable,generation,app_config,metadata_only,stopped_observation)=match parsed{Ok(parsed)=>parsed,Err(e)=>{println!("{}",failure(command,e));return Some(2);}};
    let result=(||->Result<serde_json::Value,String>{
        if let Some(root)=&shim_root {let bound=identity::registration::Binding::from_bytes(&identity::checked_read(&binding)?)?;
            if root.canonicalize().map_err(|_|"original shim backend root unavailable")?!=bound.backend_root{return Err("original shim backend differs from explicit management binding".into());}}
        if command=="runtime-plan"{
            let mut plan=identity::plan_with_config(&binding,executable.as_deref(),app_config.as_deref())?;
            if !metadata_only {let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|_|"plan validation runtime unavailable")?;runtime.block_on(identity::validate_plan(&mut plan))?;}
            return serde_json::to_value(plan).map_err(|_|"runtime plan serialization failed".into());
        }
        let selection=RuntimeSelection::from_binding(&binding)?;
        if stopped_observation {return Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true,"observation":identity::stopped_observation(&selection)?}));}
        let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|_|"runtime initialization failed")?;
        runtime.block_on(async{
            if command=="runtime-host"{
                admit_selected_cli(&selection)?;
                quantnight_task_core::runtime::server::run_host(selection,crate::DEV_SOURCE_ID).await?;
                return Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true}));
            }
            let client=RuntimeClient::connect_management(&selection).await.map_err(|e|e.to_string())?;
            let status=if command=="runtime-status"{client.management_status().await}else{client.drain(generation.as_deref().unwrap_or("")).await}.map_err(|e|e.to_string())?;
            Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true,"status":status}))
        })
    })();
    match result{Ok(value)=>{println!("{value}");Some(0)},Err(e)=>{println!("{}",failure(command,e));Some(1)}}
}
#[allow(dead_code)]fn _path_type(_: &Path){}

/// Native business-client freshness uses the already shared build identity;
/// management deliberately remains available to compatible stale generations.
pub(crate) fn admit_selected_cli(selection:&RuntimeSelection)->Result<(),String>{
    if Path::new(crate::DEV_CRATE_ROOT)!=selection.frontend_root.join("crates/taskctl") || crate::DEV_TARGET!=selection.target || crate::DEV_PROFILE!=selection.profile{return Err("linked CLI checkout/target/profile differs from selected runtime".into());}
    crate::source_identity::admit_linked(&selection.frontend_root.join("crates/taskctl"),crate::DEV_SOURCE_ID)
}
