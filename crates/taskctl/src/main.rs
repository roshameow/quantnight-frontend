use quantnight_task_core::task_control::TaskControl;
use quantnight_taskctl::{
    dispatch::dispatch,
    output::{rejected, write_outcome},
    parse, to_request, Entry,
};
use std::io::{self, Write};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    // Decode and parse before constructing a runtime, loading configuration or
    // invoking the facade. Even invalid argv must not panic on non-UTF8 input.
    let args = match std::env::args_os()
        .skip(1)
        .map(|v| v.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(args) => args,
        Err(_) => {
            eprintln!("taskctl: arguments must be valid UTF-8");
            return 2;
        }
    };
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("unable to initialize runtime: {error}"))?;
        runtime.block_on(async {
            let config = quantnight_taskctl::bootstrap::load(app_config.as_deref(), backend_root.as_deref()).await?;
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
