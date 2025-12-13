use std::fs;
use std::path::PathBuf;
use tauri::command;

// --- Helper function to create default config if it doesn't exist ---
fn create_default_config_if_missing() {
    let config_js_path = get_config_js_path();
    let config_toml_path = get_config_toml_path();
    
    // Create default config.js if it doesn't exist
    if !config_js_path.exists() {
        let default_js_content = r#"export const AppConfig = {
  "paths": {
    "superTemplatePath": "./templates/super",
    "templatePath": "./templates/regular",
    "priorityTemplatePath": "./templates/priority"
  },
  "dataFilterOptions": [
    {
      "label": "alpha_submitted_zzz",
      "value": "alpha_submitted_zzz"
    },
    {
      "label": "alpha_results",
      "value": "alpha_results"
    },
    {
      "label": "alpha_submitted",
      "value": "alpha_submitted"
    }
  ]
};"#;
        if let Err(e) = fs::write(&config_js_path, default_js_content) {
            eprintln!("Failed to create default config.js: {}", e);
        } else {
            println!("Created default config.js at {:?}", config_js_path);
        }
    }
    
    // Create default config.toml if it doesn't exist
    if !config_toml_path.exists() {
        let default_toml_content = r#"[mongodb]
local_uri = "mongodb://localhost:27017"
remote_uri = "mongodb+srv://username:password@cluster.mongodb.net/dbname?retryWrites=true&w=majority"

  [mongodb.databases]
  mission = "simulation_mission"
  simulation = "simulation_db"
  alpha = "alpha_db"

[python]
interpreter = "python3"
working_dir = "./scripts"
scripts = { }

[bash]
working_dir = "./scripts"

[bash.scripts.sync_remote_task]
command = "bash"
script = "backend/mongodb/sync_mission_list.sh"
args = [ "{alpha_mission_list}" ]

[bash.scripts.start_task]
command = "bash"
script = "./scripts/start_task.sh"
args = [ "{task_name}", "{config}" ]

[bash.scripts.start_super_task]
command = "bash"
script = "./scripts/start_super_task.sh"
args = [ "{task_name}", "{config}" ]

[bash.scripts.start_priority_task]
command = "bash"
script = "./scripts/start_priority_task.sh"
args = [ "{task_name}", "{config}" ]

[bash.scripts.update_priority_task]
command = "bash"
script = "./scripts/update_priority_task.sh"
args = [ "{task_name}", "{config}" ]

[bash.scripts.check_task_status]
command = "bash"
script = "./scripts/check_tmux_python.sh"
args = [ "{task_name}" ]

[bash.scripts.pause_task]
command = "bash"
script = "./scripts/pause_task.sh"
args = [ "{task_name}" ]"#;
        if let Err(e) = fs::write(&config_toml_path, default_toml_content) {
            eprintln!("Failed to create default config.toml: {}", e);
        } else {
            println!("Created default config.toml at {:?}", config_toml_path);
        }
    }
}

// --- Helper function to get the path to our config file ---
fn get_config_js_path() -> PathBuf {
    // Try multiple possible locations for config.js
    let paths = vec![
        "../src/config.js",           // Development path
        "./config.js",                // Production path (bundled resource)
        "../config.js",               // Alternative production path
    ];
    
    for path in paths {
        let path_buf = PathBuf::from(path);
        if path_buf.exists() {
            return path_buf;
        }
    }
    
    // Fallback to development path if none exist
    PathBuf::from("../src/config.js")
}

fn get_config_toml_path() -> PathBuf {
    // Try multiple possible locations for config.toml
    let paths = vec![
        "./config.toml",              // Development and production path
        "config.toml",               // Alternative production path
    ];
    
    for path in paths {
        let path_buf = PathBuf::from(path);
        if path_buf.exists() {
            return path_buf;
        }
    }
    
    // Fallback to default path if none exist
    PathBuf::from("./config.toml")
}

// --- Tauri Commands ---

#[command]
pub fn get_config_js_content() -> Result<String, String> {
    create_default_config_if_missing();
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
    create_default_config_if_missing();
    fs::read_to_string(get_config_toml_path())
        .map_err(|e| format!("Failed to read config.toml: {}", e))
}

#[command]
pub fn save_config_toml_content(content: String) -> Result<(), String> {
    let config_path = get_config_toml_path();
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
