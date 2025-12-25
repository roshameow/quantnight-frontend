use serde_json::Value;
use tauri::command;

#[command]
pub async fn save_dialog(default_path: String, filters: Vec<Value>) -> Result<Option<String>, String> {
    // 在Tauri 2.x中，我们需要使用不同的方法
    // 由于没有直接的dialog API，我们将使用桌面作为默认路径
    use std::env;
    use std::path::PathBuf;
    
    // 获取桌面路径
    let desktop_path = if cfg!(target_os = "macos") {
        if let Ok(home) = env::var("HOME") {
            PathBuf::from(home).join("Desktop")
        } else {
            PathBuf::from("~/Desktop")
        }
    } else if cfg!(target_os = "windows") {
        if let Ok(app_data) = env::var("USERPROFILE") {
            PathBuf::from(app_data).join("Desktop")
        } else {
            PathBuf::from("~/Desktop")
        }
    } else {
        // Linux和其他系统
        if let Ok(home) = env::var("HOME") {
            PathBuf::from(home).join("Desktop")
        } else {
            PathBuf::from("~/Desktop")
        }
    };
    
    // 在桌面路径下创建文件
    let file_path = desktop_path.join(&default_path);
    Ok(Some(file_path.to_string_lossy().to_string()))
}

#[command]
pub async fn write_file(path: String, contents: String) -> Result<(), String> {
    use std::fs;
    
    fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(())
}