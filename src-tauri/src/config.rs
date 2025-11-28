use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use config::{Config, ConfigError, File};
use futures_util::future::try_join;
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command as TokioCommand,
    time::timeout,
};


#[derive(Debug, Deserialize, Clone)]
pub struct ScriptConfig {
    pub module: Option<String>,  // Python 脚本：-m xxx
    pub command: Option<String>, // 如 "bash" 或 "python3"
    pub script: Option<String>,  // 如 shell 脚本路径
    pub args: Vec<String>,       // 参数模板
}

#[derive(Debug, Deserialize, Clone)]
pub struct PythonConfig {
    pub interpreter: String,
    pub working_dir: String,
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BashConfig {
    pub working_dir: String,
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseNames {
    pub mission: String,
    pub simulation: String,
    pub alpha: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MongoConfig {
    pub local_uri: String,
    pub remote_uri: String,
    pub databases: DatabaseNames,
}

#[derive(Debug, Deserialize, Clone)]
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
    let script = script_cfg
        .script
        .as_ref()
        .ok_or(format!("{} 缺少 script 字段", script_key))?;

    let mut args = vec![script.clone()];

    args.extend(replace_args(&script_cfg.args, vars));
    if is_remote {
        args.push("--remote".to_string());
    }

    // 启动异步进程并管道 stdout/stderr
    let mut child = TokioCommand::new(command)
        .args(&args)
        .current_dir(&app_config.bash.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动命令失败：{}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("无法获取 stdout pipe")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("无法获取 stderr pipe")?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let (out_accum_tx, mut out_accum_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (err_accum_tx, mut err_accum_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let out_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            println!("[bash stdout] {}", line);
            let _ = out_accum_tx.send(line);
        }
    });

    let err_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            eprintln!("[bash stderr] {}", line);
            let _ = err_accum_tx.send(line);
        }
    });

    // 等待退出，带超时
    let status = match timeout(Duration::from_secs(300), child.wait()).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("等待子进程失败: {}", e)),
        Err(_) => {
            let _ = child.kill().await;
            return Err("Bash 脚本执行超时，已终止".to_string());
        }
    };

    let _ = try_join(out_task, err_task).await;

    // 收集输出
    let mut stdout_lines = Vec::new();
    while let Ok(line) = out_accum_rx.try_recv() {
        stdout_lines.push(line);
    }
    let mut stderr_lines = Vec::new();
    while let Ok(line) = err_accum_rx.try_recv() {
        stderr_lines.push(line);
    }

    if !status.success() {
        return Err(format!(
            "脚本退出非 0: {:?}\nstderr: {:?}\nstdout: {:?}",
            status.code(),
            stderr_lines,
            stdout_lines
        ));
    }

    Ok((stdout_lines.join("\n"), stderr_lines.join("\n")))
}

pub async fn run_python_module(
    config: &AppConfig,
    script_key: &str,
    replacements: &HashMap<&str, &str>,
) -> Result<String, String> {
    let script_cfg = config
        .python
        .scripts
        .get(script_key)
        .ok_or_else(|| format!("配置中缺少 {} 脚本定义", script_key))?;

    let module = script_cfg
        .module
        .as_ref()
        .ok_or("module 字段缺失")?;

    // 构造参数：-u 确保 unbuffered 输出、-m module
    let mut final_args = vec!["-u".to_string(), "-m".to_string(), module.to_string()];

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

    // 启动子进程，并管道 stdout/stderr
    let mut child = TokioCommand::new(&config.python.interpreter)
        .args(&final_args)
        .current_dir(&config.python.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 Python 进程失败：{}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("无法获取 stdout pipe")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("无法获取 stderr pipe")?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    // 用 channel 累积输出（也可以直接拼接在闭包里）
    let (out_accum_tx, mut out_accum_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (err_accum_tx, mut err_accum_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 读取 stdout
    let out_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            println!("[python stdout] {}", line);
            let _ = out_accum_tx.send(line);
        }
    });

    // 读取 stderr
    let err_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            eprintln!("[python stderr] {}", line);
            let _ = err_accum_tx.send(line);
        }
    });

    // 可选：加个超时，比如 5 分钟
    let wait_future = child.wait();
    let status = match timeout(Duration::from_secs(300), wait_future).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return Err(format!("等待子进程失败: {}", e));
        }
        Err(_) => {
            // 超时，尝试 kill
            let _ = child.kill().await;
            return Err("Python 脚本执行超时，已终止".to_string());
        }
    };

    // 等两个输出 reader 任务都完成
    let _ = try_join(out_task, err_task).await;

    // 收集 accumulated 输出
    let mut stdout_lines = Vec::new();
    while let Ok(line) = out_accum_rx.try_recv() {
        stdout_lines.push(line);
    }
    let mut stderr_lines = Vec::new();
    while let Ok(line) = err_accum_rx.try_recv() {
        stderr_lines.push(line);
    }

    if !status.success() {
        return Err(format!(
            "Python 脚本退出非 0: {:?}\nstderr: {:?}\nstdout: {:?}",
            status.code(),
            stderr_lines,
            stdout_lines
        ));
    }

    // 返回 stdout（也可以返回 summary.to_string() 依据上层需要）
    Ok(stdout_lines.join("\n"))
}