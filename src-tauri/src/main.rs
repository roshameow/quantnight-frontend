#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod watcher;
mod datas;
mod mongo_manager;
mod frontend_config; // Add new module
mod file_commands; // Add file commands module
mod metrics; // Add metrics module

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::async_runtime;

use mongo_manager::MongoClients;
use crate::config::load_config;
use crate::config::default_config_path;

fn main() {
    let config_path = env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);
    println!("加载配置路径: {:?}", config_path);
    let config = load_config(&config_path).expect("配置加载失败");

    // 初始化 MongoClients 并用 Arc 包装
    let mongo_clients = async_runtime::block_on(async {
        Arc::new(MongoClients::new(&config).await.expect("MongoDB 初始化失败"))
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(mongo_clients.clone()) // 注册 Arc<MongoClients>
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
            watcher::frontend_ready,  // 确保此命令通过前端调用
            datas::get_alpha_results,
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
