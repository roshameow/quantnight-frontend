// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod commands;
mod config;
mod watcher;
mod datas;
mod mongo_manager;

use tauri::{Manager};
use tauri::async_runtime;
use mongo_manager::MongoClients;
use crate::config::load_config;


fn main() {
    let config = load_config().expect("配置加载失败");
    let mongo_clients = async_runtime::block_on(async {
        MongoClients::new(&config).await.expect("MongoDB 初始化失败")
    });

    tauri::Builder::default()
        .manage(mongo_clients) // ✅ 注册给 Tauri 的 State 系统
        .manage(load_config().expect("配置加载失败"))
        .invoke_handler(tauri::generate_handler![
            commands::create_task,
            commands::generate_list,
            commands::update_task,
            commands::read_latest_py_file,
            commands::sync_remote_task,
            commands::get_all_tasks,
            commands::delete_task,
            commands::start_task,
            commands::start_super_task,
            commands::pause_task,
            watcher::frontend_ready,
            datas::get_alpha_results,
            datas::get_pnl_by_id,
            datas::compute_correlation,

            // ...其他命令
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            let _clients = app.state::<MongoClients>().inner().clone(); // ✅ 提前 clone 出来

            tauri::async_runtime::spawn(async move {
                if let Err(e) = watcher::start_all_task_watchers(app_handle, _clients).await {
                    eprintln!("Failed to start task watchers: {:?}", e);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    quantnight_frontend_lib::run()
}
