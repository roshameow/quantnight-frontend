use std::fs;
use std::path::PathBuf;
use tauri::command;

// --- Helper function to get the path to our config file ---
fn get_config_js_path() -> PathBuf {
    // This path is relative to the `src-tauri` directory where the binary runs
    PathBuf::from("../src/config.js")
}

fn get_config_toml_path() -> PathBuf {
    // This path is relative to the `src-tauri` directory
    PathBuf::from("./config.toml")
}

// --- Tauri Commands ---

#[command]
pub fn get_config_js_content() -> Result<String, String> {
    fs::read_to_string(get_config_js_path())
        .map_err(|e| format!("Failed to read config.js: {}", e))
}

#[command]
pub fn save_config_js_content(content: String) -> Result<(), String> {
    fs::write(get_config_js_path(), content)
        .map_err(|e| format!("Failed to write to config.js: {}", e))
}

#[command]
pub fn get_config_toml_content() -> Result<String, String> {
    fs::read_to_string(get_config_toml_path())
        .map_err(|e| format!("Failed to read config.toml: {}", e))
}

#[command]
pub fn save_config_toml_content(content: String) -> Result<(), String> {
    fs::write(get_config_toml_path(), content)
        .map_err(|e| format!("Failed to write to config.toml: {}", e))
}
