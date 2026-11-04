use quantnight_task_core::task_control::TaskControl;
use crate::{
    dispatch::dispatch,
    output::{rejected, write_outcome},
    parse, to_request, Entry,
};
use std::io::{self, Write};

/// Run the original CLI with OS arguments excluding argv[0].
/// Standalone callers have no host-admission requirement.
pub fn run_os(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    run_os_with_admission(args, |_, _| Ok(()))
}

/// Desktop dev hosts inject admission after syntax/typed conversion, before
/// constructing Tokio or reading any business configuration. Help and usage
/// never invoke the hook. No desktop types/dependencies cross this boundary.
pub fn run_os_with_admission(
    args: impl IntoIterator<Item = std::ffi::OsString>,
    admit: impl FnOnce(Option<&std::path::Path>, Option<&std::path::Path>) -> Result<(), String>,
) -> i32 {
    // Decode and parse before constructing a runtime, loading configuration or
    // invoking the facade. Even invalid argv must not panic on non-UTF8 input.
    let args = match args.into_iter()
        .map(|v| v.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(args) => args,
        Err(_) => {
            eprintln!("taskctl: arguments must be valid UTF-8");
            return 2;
        }
    };
    if let Some(code) = crate::runtime_entry::run(&args) { return code; }
    let (app_config, backend_root, invocation) = match parse(&args) {
        Ok(Entry::Help(text)) => {
            if let Err(error) = io::stdout().lock().write_all(text.as_bytes()) {
                eprintln!("taskctl: stdout: {error}");
                return 1;
            }
            return 0;
        }
        Err(error) => {
            eprintln!("taskctl: {error}");
            return error.exit_code;
        }
        Ok(Entry::Command {
            app_config,
            backend_root,
            invocation,
        }) => (app_config, backend_root, invocation),
    };

    let outcome = (|| {
        let request = to_request(&invocation)?;
        admit(app_config.as_deref(), backend_root.as_deref())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("unable to initialize runtime: {error}"))?;
        runtime.block_on(async {
            use quantnight_task_core::runtime::{RuntimeSelection,RuntimeClient,CallContext};
            if let Some(root)=backend_root.as_deref() {
                if let Some(selection)=RuntimeSelection::for_backend(root)? {
                    crate::runtime_entry::admit_selected_cli(&selection)?;
                    let result=async {let client=RuntimeClient::connect(&selection).await?;client.call(CallContext::current()?,request.clone(),None).await}.await;
                    return Ok(match result {Ok(result)=>quantnight_task_core::cleanup_sync::TaskOutcome{value:result.value,exit_code:result.exit_code},Err(error)=>{
                        let mut outcome=rejected(&invocation,error.to_string());outcome.value["transport"]=serde_json::to_value(error).unwrap_or(serde_json::Value::Null);outcome
                    }});
                }
            }
            let config = crate::bootstrap::load(app_config.as_deref(), backend_root.as_deref()).await?;
            if let Some(selection)=RuntimeSelection::for_backend(std::path::Path::new(&config.env.working_dir))? {
                crate::runtime_entry::admit_selected_cli(&selection)?;
                let selected=app_config.as_deref().and_then(|path|path.canonicalize().ok());
                if selection.configuration.kind!="app_config" || selected.as_ref()!=Some(&selection.configuration.path){return Err("explicit AppConfig differs from enrolled runtime selector; no direct fallback".into());}
                let result=async{let client=RuntimeClient::connect(&selection).await?;client.call(CallContext::current()?,request.clone(),None).await}.await;
                return Ok(match result{Ok(result)=>quantnight_task_core::cleanup_sync::TaskOutcome{value:result.value,exit_code:result.exit_code},Err(error)=>{let mut outcome=rejected(&invocation,error.to_string());outcome.value["transport"]=serde_json::to_value(error).unwrap_or(serde_json::Value::Null);outcome}});
            }
            let control = TaskControl::connect(&config).await?;
            Ok::<_, String>(dispatch(&control, &request).await)
        })
    })()
    .unwrap_or_else(|error: String| rejected(&invocation, error));

    match write_outcome(&mut io::stdout().lock(), &outcome) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("taskctl: stdout: {error}");
            1
        }
    }
}
