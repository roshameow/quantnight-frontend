use serde_json::Value;
use tauri::{command, AppHandle};
use tauri_plugin_dialog::DialogExt;

#[command]
pub async fn save_dialog(app: AppHandle, default_path: String, _filters: Vec<Value>) -> Result<Option<String>, String> {
    let file_path = app.dialog()
        .file()
        .set_file_name(&default_path)
        .add_filter("JSON", &["json"])
        .blocking_save_file();
        
    Ok(file_path.map(|p| p.to_string()))
}

#[command]
pub async fn write_file(path: String, contents: String) -> Result<(), String> {
    use std::fs;
    
    fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(())
}