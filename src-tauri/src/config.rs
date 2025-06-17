use config::{Config, ConfigError, File};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::env;
use std::path::{Path,PathBuf};


#[derive(Debug, Deserialize)]
pub struct ScriptConfig {
    pub module: Option<String>,  // Python 脚本：-m xxx
    pub command: Option<String>, // 如 "bash" 或 "python3"
    pub script: Option<String>,  // 如 shell 脚本路径
    pub args: Vec<String>,       // 参数模板
}

#[derive(Debug, Deserialize)]
pub struct PythonConfig {
    pub interpreter: String,
    pub working_dir: String,
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize)]
pub struct BashConfig {
    pub working_dir: String,
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MongoConfig {
    pub local_uri: String,
    pub remote_uri: String,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub python: PythonConfig,
    pub bash: BashConfig,
    pub mongodb: MongoConfig,
}


pub fn load_config(path: &Path) -> Result<AppConfig, ConfigError> {
    let builder = Config::builder().add_source(File::from(path));
    let cfg = builder.build()?;
    cfg.try_deserialize()
}

pub fn default_config_path() -> PathBuf {
    #[cfg(debug_assertions)]
    {
        // 开发模式下直接用 src-tauri 目录的配置
        return PathBuf::from("config.toml");
    }
    #[cfg(not(debug_assertions))]
    {
        // 获取当前可执行文件路径
        if let Ok(exe_path) = env::current_exe() {
            #[cfg(target_os = "macos")]
            {
                // macOS: app bundle 中：.app/Contents/MacOS/<binary>
                if let Some(resources_dir) = exe_path
                    .parent() // MacOS
                    .and_then(|p| p.parent()) // Contents
                    .map(|p| p.join("Resources"))
                {
                    return resources_dir.join("config.toml");
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: 和 .exe 放在同一目录（或 resource 子目录）
                return exe_path
                    .parent()
                    .map(|p| p.join("config.toml"))
                    .unwrap_or_else(|| PathBuf::from("config.toml"));
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: 通常放在 /usr/share/<app>/config.toml 或可执行文件旁边
                return exe_path
                    .parent()
                    .map(|p| p.join("config.toml"))
                    .unwrap_or_else(|| PathBuf::from("config.toml"));
            }
        }

        // fallback
        PathBuf::from("config.toml")
    }
}


pub fn replace_args(args: &[String], vars: &[(&str, &str)]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            let mut result = arg.clone();
            for (key, value) in vars {
                result = result.replace(&format!("{{{}}}", key), value);
            }
            result
        })
        .collect()
}

pub async fn run_bash_script(
    script_key: &str,
    vars: &[(&str, &str)],
    is_remote: bool,
    app_config: &AppConfig,
) -> Result<(String, String), String> {
    let script_cfg = app_config
        .bash
        .scripts
        .get(script_key)
        .ok_or(format!("未找到 {} 脚本配置", script_key))?;
    let command = script_cfg.command.as_deref().unwrap_or("bash");
    let script = script_cfg.script.as_ref().ok_or(format!("{} 缺少 script 字段", script_key))?;

    let mut args = vec![script.clone()];
    args.extend(replace_args(&script_cfg.args, vars));
    if is_remote {
        args.push("--remote".to_string());
    }

    let output = Command::new(command)
        .args(&args)
        .current_dir(&app_config.bash.working_dir)
        .output()
        .map_err(|e| format!("命令执行失败：{}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("脚本执行失败: {}\n{}", output.status, stderr));
    }

    Ok((stdout.trim().to_string(), stderr.trim().to_string()))
}

pub fn run_python_module(
    config: &AppConfig,
    script_key: &str,
    replacements: &HashMap<&str, &str>,
) -> Result<String, String> {
    let script_cfg = config.python.scripts.get(script_key)
        .ok_or(format!("配置中缺少 {} 脚本定义", script_key))?;

    let module = script_cfg.module.as_ref().ok_or("module 字段缺失")?;

    let mut final_args = vec!["-m".to_string(), module.to_string()];

    for arg in &script_cfg.args {
        if arg == "{extra_args}" {
            if let Some(extra) = replacements.get("{extra_args}") {
                final_args.extend(extra.split_whitespace().map(|s| s.to_string()));
            }
        } else {
            let mut replaced = arg.clone();
            for (key, val) in replacements {
                replaced = replaced.replace(key, val);
            }
            final_args.push(replaced);
        }
    }

    let output = Command::new(&config.python.interpreter)
        .args(&final_args)
        .current_dir(&config.python.working_dir)
        .output()
        .map_err(|e| format!("命令执行失败：{}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Python 执行失败:\nSTDERR:\n{}\nSTDOUT:\n{}", stderr, stdout);
        return Err(format!("执行失败:\n{}", stderr));
    }

    println!("任务执行成功:\n{}", String::from_utf8_lossy(&output.stdout));

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
