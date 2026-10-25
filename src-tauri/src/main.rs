#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
#[path = "../../crates/source_identity.rs"]
mod source_identity;
mod dev_identity_inputs;
mod dev_image;
mod dev_identity;
mod config;
mod watcher;
mod datas;
mod mongo_manager;
mod frontend_config; // Add new module
mod file_commands; // Add file commands module
mod metrics; // Add metrics module

use std::env;
use std::path::PathBuf;

use tauri::async_runtime;

use quantnight_task_core::task_control::TaskControl;
use crate::config::load_config;
use crate::config::default_config_path;

fn main() {
    // Exact first OS argument only, before either GUI loop or any GUI I/O.
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("taskctl")) {
        std::process::exit(quantnight_taskctl::run_os_with_admission(args, dev_identity::admit_cli));
    }
    if let Err(error) = dev_identity::initialize_gui() {
        eprintln!("dev GUI admission failed: {error}. Fix the cause and rerun `npm run tauri dev`.");
        std::process::exit(1);
    }
    let config_path = env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);
    println!("加载配置路径: {:?}", config_path);
    let config = load_config(&config_path).expect("配置加载失败");

    if let Err(error) = dev_identity::verify_gui_root(std::path::Path::new(&config.env.working_dir)) {
        eprintln!("dev backend binding mismatch: {error}. Fix the cause and rerun `npm run tauri dev`.");
        std::process::exit(1);
    }

    // TOML selects backend root and UI settings. Backend MongoConfig is the
    // sole endpoint authority for ALL shared reads/watchers and mutations.
    // Refuse the entire GUI on failure: never show an old-URI task view as a
    // controllable new context. Recovery is a normal user restart, not replay.
    let (config, mongo_clients, control) = match async_runtime::block_on(
        TaskControl::bootstrap_gui(config)
    ).and_then(|context| context.into_parts()) {
        Ok(context) => context,
        Err(error) => {
            eprintln!("Application unavailable: backend task configuration authority failed: {error}. Check configuration and restart normally; no task operation was retried.");
            std::process::exit(1);
        }
    };

    // Fixed, non-secret operator notice: never log endpoints or AppConfig.
    eprintln!("MongoDB endpoint authority: backend MongoConfig supplies all shared GUI connections. TOML still selects the backend root, UI database/collection names and scripts; legacy TOML URI values are not used for transport. Changes require a normal restart, not a task retry.");

    tauri::Builder::default()
        .setup(|_| { dev_identity::ready().map_err(|e| format!("{e}; fix and rerun `npm run tauri dev`"))?; Ok(()) })
        .plugin(tauri_plugin_dialog::init())
        .manage(mongo_clients.clone()) // 注册 Arc<MongoClients>
        .manage(commands::TaskControlState::from_verified(control))
        .manage(config)
        .invoke_handler(tauri::generate_handler![
            commands::create_task,
            commands::generate_list,
            commands::update_remote_status,
            commands::read_latest_py_file,
            commands::sync_remote_task,
            commands::get_all_tasks,
            commands::delete_task,
            commands::start_task,
            commands::start_super_task,
            commands::start_priority_task,
            commands::update_priority_task,
            commands::pause_task,
            commands::check_task_status,
            watcher::frontend_ready,
            watcher::refresh_task_progress,
            datas::get_alpha_results,
            datas::get_alpha_count,
            datas::get_datasets,
            datas::get_datafields,
            datas::get_pnl_by_id,
            datas::get_submission_stats,
            datas::compute_correlation,
            datas::search_alpha_in_all_collections,
            frontend_config::get_config_js_content, // Add new command
            frontend_config::save_config_js_content, // Add new command
            frontend_config::get_config_toml_content,
            frontend_config::save_config_toml_content,
            commands::get_button_mappings,
            commands::save_button_mappings,
            commands::restart_app,
            file_commands::save_dialog, // Add file dialog command
            file_commands::write_file, // Add write file command
            metrics::calculate_pnl_metrics, // Add metrics calculation command
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    quantnight_frontend_lib::run();
}
