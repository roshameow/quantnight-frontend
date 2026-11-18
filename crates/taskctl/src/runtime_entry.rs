//! Runtime management adaptation only. All schema/path/IPC/identity lives in core.
use std::path::{Path,PathBuf};
use quantnight_task_core::runtime::{identity,RuntimeSelection,RuntimeClient,TransportError};
fn failure(command:&str,error:impl AsRef<str>)->serde_json::Value{serde_json::json!({"schemaVersion":1,"command":command,"ok":false,"errorType":"RuntimeError","error":error.as_ref()})}
pub fn run(input:&[String])->Option<i32>{
    // The original qn shim injects its exact --backend-root prefix. Management
    // still requires explicit --binding; this prefix is only an agreement check.
    let (args,shim_root)=if input.first().map(String::as_str)==Some("--backend-root") && input.get(2).is_some_and(|command|matches!(command.as_str(),"runtime-plan"|"runtime-status"|"runtime-drain"|"runtime-host")) {
        (&input[2..],Some(PathBuf::from(&input[1])))
    } else { (input,None) };
    let command=args.first()?.as_str();if !matches!(command,"runtime-plan"|"runtime-status"|"runtime-drain"|"runtime-host"){return None;}
    if args.iter().skip(1).any(|a|matches!(a.as_str(),"--help"|"-h")){
        print!("{}", management_help(command)); return Some(0);
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
    let result=(||->Result<serde_json::Value, ManagementError>{
        if let Some(root)=&shim_root {let bound=identity::registration::Binding::from_bytes(&identity::checked_read(&binding)?)?;
            if root.canonicalize().map_err(|_|"original shim backend root unavailable")?!=bound.backend_root{return Err("original shim backend differs from explicit management binding".into());}}
        if command=="runtime-plan"{
            let mut plan=identity::plan_with_config(&binding,executable.as_deref(),app_config.as_deref())?;
            if !metadata_only {let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|_|"plan validation runtime unavailable")?;runtime.block_on(identity::validate_plan(&mut plan))?;}
            return serde_json::to_value(plan).map_err(|_| "runtime plan serialization failed".into());
        }
        let selection=RuntimeSelection::from_binding(&binding)?;
        if stopped_observation {return Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true,"observation":identity::stopped_observation(&selection)?}));}
        let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|_|"runtime initialization failed")?;
        runtime.block_on(async{
            if command=="runtime-host"{
                // An exact but source-stale binary can host compatible management.
                check_selected_cli_origin(&selection)?;
                quantnight_task_core::runtime::server::run_host(selection,crate::DEV_SOURCE_ID).await?;
                return Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true}));
            }
            let client = RuntimeClient::management(&selection);
            let status=if command=="runtime-status"{client.management_status().await}else{client.drain(generation.as_deref().unwrap_or("")).await}?;
            Ok(serde_json::json!({"schemaVersion":1,"command":command,"ok":true,"status":status}))
        })
    })();
    match result {
        Ok(value) => { println!("{value}"); Some(0) }
        Err(error) => {
            let mut value = match error {
                ManagementError::Local(message) => failure(command, message),
                ManagementError::Transport(error) => {
                    let mut value = failure(command, error.to_string());
                    value["transport"] = serde_json::json!(error);
                    value
                }
            };
            if command == "runtime-host" {
                // The sockets may never reach Hello when bootstrap itself fails.
                // Keep its original safe cause available to the service owner.
                eprintln!("taskctl runtime-host: {}", value["error"].as_str().unwrap_or("startup failed"));
                value["stage"] = "runtime-host".into();
            }
            println!("{value}"); Some(1)
        }
    }
}

enum ManagementError {
    Local(String),
    Transport(TransportError),
}
impl From<String> for ManagementError { fn from(error: String) -> Self { Self::Local(error) } }
impl From<&str> for ManagementError { fn from(error: &str) -> Self { Self::Local(error.into()) } }
impl From<TransportError> for ManagementError { fn from(error: TransportError) -> Self { Self::Transport(error) } }

/// Metadata-only origin check used by both business clients and runtime-host.
/// Source freshness is still mandatory at business admission.
pub(crate) fn check_selected_cli_origin(selection: &RuntimeSelection) -> Result<(), String> {
    if Path::new(crate::DEV_CRATE_ROOT) != selection.frontend_root.join("crates/taskctl")
        || crate::DEV_TARGET != selection.target || crate::DEV_PROFILE != selection.profile {
        return Err("linked CLI checkout/target/profile differs from selected runtime".into());
    }
    Ok(())
}

fn management_help(command: &str) -> String {
    let (args, summary) = match command {
        "runtime-plan" => (" [--runtime-executable PATH] [--app-config PATH] [--metadata-only]",
            "Inspect the selected runtime or plan a candidate executable/configuration. By default, validate the original business configuration without contacting MongoDB. --metadata-only skips that validation. Does not write or activate enrollment."),
        "runtime-status" => (" [--stopped-observation]",
            "Inspect the selected host identity, drain flag and active calls. --stopped-observation instead checks the stopped slot without activating the host or reclaiming the socket."),
        "runtime-drain" => (" --generation EXACT",
            "Refuse new business on the exact host generation from runtime-status. Accepted operations finish; this does not stop or reload the host or stop simulation workers."),
        "runtime-host" => ("", "Internal launchd entry point for the selected executable and inherited socket. Do not invoke as a standalone service."),
        _ => unreachable!("known management command"),
    };
    format!("Usage: taskctl {command} --binding /canonical/binding.json{args}\n\n{summary}\n")
}
