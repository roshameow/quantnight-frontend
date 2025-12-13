use std::fs;
use std::path::PathBuf;
use tauri::command;
use dirs::data_dir;

// --- Helper function to get the path to our config file ---
fn get_config_js_path() -> Result<PathBuf, String> {
    // In development, use the relative path
    #[cfg(debug_assertions)]
    {
        return Ok(PathBuf::from("../src/config.js"));
    }
    
    // In production, use the app data directory
    #[cfg(not(debug_assertions))]
    {
        let app_data = data_dir()
            .ok_or("Failed to get app data directory")?;
        let config_dir = app_data.join("quantnight");
        
        // Ensure the directory exists
        fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
        
        Ok(config_dir.join("config.js"))
    }
}

fn get_config_toml_path() -> Result<PathBuf, String> {
    // In development, use the relative path
    #[cfg(debug_assertions)]
    {
        return Ok(PathBuf::from("./config.toml"));
    }
    
    // In production, use the app data directory
    #[cfg(not(debug_assertions))]
    {
        let app_data = data_dir()
            .ok_or("Failed to get app data directory")?;
        let config_dir = app_data.join("quantnight");
        
        // Ensure the directory exists
        fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
        
        Ok(config_dir.join("config.toml"))
    }
}

// Initialize config files from default if they don't exist
fn initialize_config_files() -> Result<(), String> {
    #[cfg(not(debug_assertions))]
    {
        let config_js_path = get_config_js_path()?;
        let config_toml_path = get_config_toml_path()?;
        
        // Copy default config.js if it doesn't exist
        if !config_js_path.exists() {
            let default_content = include_str!("../resources/default_config.js");
            fs::write(&config_js_path, default_content)
                .map_err(|e| format!("Failed to write default config.js: {}", e))?;
        }
        
        // Copy default config.toml if it doesn't exist
        if !config_toml_path.exists() {
            let default_content = include_str!("../resources/config.toml");
            fs::write(&config_toml_path, default_content)
                .map_err(|e| format!("Failed to write default config.toml: {}", e))?;
        }
    }
    
    Ok(())
}

// --- Tauri Commands ---

#[command]
pub fn get_config_js_content() -> Result<String, String> {
    // Initialize config files if they don't exist
    initialize_config_files()?;
    
    let config_path = get_config_js_path()?;
    fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config.js from {:?}: {}", config_path, e))
}

#[command]
pub fn save_config_js_content(content: String) -> Result<(), String> {
    let config_path = get_config_js_path()?;
    fs::write(&config_path, content)
        .map_err(|e| format!("Failed to write to config.js at {:?}: {}", config_path, e))
}

#[command]
pub fn get_config_toml_content() -> Result<String, String> {
    // Initialize config files if they don't exist
    initialize_config_files()?;
    
    let config_path = get_config_toml_path()?;
    fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config.toml from {:?}: {}", config_path, e))
}

#[command]
pub fn save_config_toml_content(content: String) -> Result<(), String> {
    let config_path = get_config_toml_path()?;
    println!("Attempting to save config.toml to: {:?}", config_path);
    println!("Content to save (first 200 chars): {}", &content[..content.len().min(200)]);
    
    fs::write(&config_path, content)
        .map_err(|e| {
            eprintln!("Failed to write to config.toml at {:?}: {}", config_path, e);
            format!("Failed to write to config.toml: {}", e)
        })?;
    
    println!("Successfully saved config.toml");
    Ok(())
}
