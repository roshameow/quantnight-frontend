use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use config::{Config, ConfigError, File};
use dirs::data_dir;
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
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BashConfig {
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EnvConfig {
    pub working_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseNames {
    pub mission: String,
    pub simulation: String,
    pub alpha: String,
    pub dataset: Option<String>,
    pub dataset_collection: Option<String>,
    pub datafield: Option<String>,
    pub datafield_collection: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MongoConfig {
    pub local_uri: String,
    pub remote_uri: String,
    pub databases: DatabaseNames,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub env: EnvConfig,
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
        // 在生产环境中，使用用户数据目录
        if let Some(app_data) = get_user_data_dir() {
            let config_dir = app_data.join("quantnight");
            
            // 确保目录存在
            if let Err(_) = std::fs::create_dir_all(&config_dir) {
                // 如果创建失败，回退到旧逻辑
                return fallback_config_path();
            }
            
            let config_path = config_dir.join("config.toml");
            
            // 如果用户配置文件不存在，从默认配置复制
            if !config_path.exists() {
                if let Some(default_path) = get_default_config_path() {
                    if let Ok(default_content) = std::fs::read_to_string(&default_path) {
                        let _ = std::fs::write(&config_path, default_content);
                    }
                }
            }
            
            return config_path;
        }
        
        // 回退到旧逻辑
        fallback_config_path()
    }
}

// 获取用户数据目录
fn get_user_data_dir() -> Option<PathBuf> {
    data_dir()
}

// 获取默认配置文件路径（打包在应用中的）
fn get_default_config_path() -> Option<PathBuf> {
    #[cfg(not(debug_assertions))]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            #[cfg(target_os = "macos")]
            {
                // macOS: app bundle 中：.app/Contents/Resources/resources/config.toml
                if let Some(resources_dir) = exe_path
                    .parent() // MacOS
                    .and_then(|p| p.parent()) // Contents
                    .map(|p| p.join("Resources").join("resources"))
                {
                    return Some(resources_dir.join("config.toml"));
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: 和 .exe 放在同一目录的 resources 子目录
                return exe_path
                    .parent()
                    .and_then(|p| Some(p.join("resources").join("config.toml")));
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: 通常放在 /usr/share/<app>/resources/config.toml 或可执行文件旁边的 resources 目录
                return exe_path
                    .parent()
                    .and_then(|p| Some(p.join("resources").join("config.toml")));
            }
        }
    }
    
    None
}

// 回退到旧的配置路径逻辑
fn fallback_config_path() -> PathBuf {
    #[cfg(not(debug_assertions))]
    {
        // 获取当前可执行文件路径
        if let Ok(exe_path) = std::env::current_exe() {
            #[cfg(target_os = "macos")]
            {
                // macOS: app bundle 中：.app/Contents/MacOS/<binary>
                if let Some(resources_dir) = exe_path
                    .parent() // MacOS
                    .and_then(|p| p.parent()) // Contents
                    .map(|p| p.join("Resources").join("resources"))
                {
                    return resources_dir.join("config.toml");
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: 和 .exe 放在同一目录的 resources 子目录
                return exe_path
                    .parent()
                    .and_then(|p| Some(p.join("resources").join("config.toml")))
                    .unwrap_or_else(|| PathBuf::from("config.toml"));
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: 通常放在 /usr/share/<app>/resources/config.toml 或可执行文件旁边的 resources 目录
                return exe_path
                    .parent()
                    .and_then(|p| Some(p.join("resources").join("config.toml")))
                    .unwrap_or_else(|| PathBuf::from("config.toml"));
            }
        }
    }

    // fallback
    PathBuf::from("config.toml")
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

/// Observations are diagnostics only; callers must validate any stdout protocol.
#[derive(Debug)]
pub enum BashScriptEvent {
    Config,
    Launch,
    Wait,
    Stdout(String),
}

pub type BashScriptObserver = std::sync::Arc<dyn Fn(BashScriptEvent) + Send + Sync>;

/// The original entry point retains its signature and five-minute budget.
pub async fn run_bash_script(
    script_key: &str,
    vars: &[(&str, &str)],
    is_remote: bool,
    app_config: &AppConfig,
) -> Result<(String, String), String> {
    run_bash_script_observed(script_key, vars, is_remote, app_config,
        Duration::from_secs(300), false, None).await
}

// A read-only probe owns a fresh process group containing ONLY its diagnostic
// shell/SSH descendants. Never enable this for a stop/start/research operation.
// Drop also covers cancellation while waiting on stdout inherited by a child.
struct DiagnosticGroup(Option<u32>);
impl Drop for DiagnosticGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()).status();
        }
    }
}

/// Same configuration, argv construction, transport and readers as the legacy
/// entry point. The deadline includes pipe draining, not just child.wait().
pub async fn run_bash_script_observed(
    script_key: &str,
    vars: &[(&str, &str)],
    is_remote: bool,
    app_config: &AppConfig,
    budget: Duration,
    diagnostic: bool,
    observer: Option<BashScriptObserver>,
) -> Result<(String, String), String> {
    run_bash_script_observed_with_env(script_key, vars, is_remote, app_config, budget,
        diagnostic, observer, &[]).await
}

pub(crate) async fn run_bash_script_observed_with_env(
    script_key: &str, vars: &[(&str, &str)], is_remote: bool, app_config: &AppConfig,
    budget: Duration, diagnostic: bool, observer: Option<BashScriptObserver>,
    environment: &[(&str, &str)],
) -> Result<(String, String), String> {
    let observe = |event| { if let Some(callback) = &observer { callback(event); } };
    observe(BashScriptEvent::Config);
    let script_cfg = app_config.bash.scripts.get(script_key)
        .ok_or_else(|| format!("未找到 {} 脚本配置", script_key))?;
    let command = script_cfg.command.as_deref().unwrap_or("bash");
    let script = script_cfg.script.as_ref()
        .ok_or_else(|| format!("{} 缺少 script 字段", script_key))?;
    let mut args = vec![script.clone()];
    args.extend(replace_args(&script_cfg.args, vars));
    if is_remote { args.push("--remote".to_string()); }
    let quoted_args: Vec<String> = args.iter().map(|arg| format!("'{}'", arg)).collect();
    println!("Executing Bash command: {} {}", command, quoted_args.join(" "));

    let mut command = TokioCommand::new(command);
    command.args(&args).current_dir(&app_config.env.working_dir)
        .envs(environment.iter().copied());
    run_process_observed(command, budget, diagnostic, observer).await
}

// Atomic subprocess transport shared by the existing shell runner and local
// taskctl CLI. No task lifecycle, config resolution or tmux selection here.
pub(crate) async fn run_process_observed(
    mut command: TokioCommand,
    budget: Duration,
    diagnostic: bool,
    observer: Option<BashScriptObserver>,
) -> Result<(String, String), String> {
    let observe = |event| { if let Some(callback) = &observer { callback(event); } };
    observe(BashScriptEvent::Launch);
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    if diagnostic { command.process_group(0); }
    let mut child = command.spawn().map_err(|e| format!("启动命令失败：{}", e))?;
    let _diagnostic_group = DiagnosticGroup(if diagnostic { child.id() } else { None });
    let stdout = child.stdout.take().ok_or("无法获取 stdout pipe")?;
    let stderr = child.stderr.take().ok_or("无法获取 stderr pipe")?;
    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();
    observe(BashScriptEvent::Wait);
    // No detached reader tasks/channels: cancellation cannot leave a reader
    // alive indefinitely, and partial stderr survives timeout/read/wait errors.
    let execution = timeout(budget, async {
        tokio::try_join!(
            async { child.wait().await.map_err(|e| format!("等待子进程失败: {e}")) },
            async {
                while let Some(line) = stdout_reader.next_line().await
                    .map_err(|e| format!("读取 stdout 失败: {e}"))? {
                    println!("[bash stdout] {}", line);
                    observe(BashScriptEvent::Stdout(line.clone()));
                    stdout_lines.push(line);
                }
                Ok::<_, String>(())
            },
            async {
                while let Some(line) = stderr_reader.next_line().await
                    .map_err(|e| format!("读取 stderr 失败: {e}"))? {
                    eprintln!("[bash stderr] {}", line);
                    stderr_lines.push(line);
                }
                Ok::<_, String>(())
            }
        )
    }).await;
    let result = match execution {
        Ok(Ok((status, _, _))) if status.success() =>
            return Ok((stdout_lines.join("\n"), stderr_lines.join("\n"))),
        Ok(Ok((status, _, _))) => format!("脚本退出非 0: {:?}", status.code()),
        Ok(Err(error)) => error,
        Err(_) => format!("Bash 脚本执行超时（预算 {} ms）", budget.as_millis()),
    };
    // Only this child PID (and for diagnostic=true its private group) is owned.
    // No task-name lookup, tmux kill, job cancellation or retry is performed.
    let _ = child.start_kill();
    let _ = timeout(Duration::from_millis(250), child.wait()).await;
    Err(format!("{result}\nstderr: {:?}\nstdout: {:?}", stderr_lines, stdout_lines))
}


pub async fn run_python_command(
    config: &AppConfig,
    script_key: &str,
    replacements: &HashMap<&str, &str>,
) -> Result<String, String> {
    let script_cfg = config
        .python
        .scripts
        .get(script_key)
        .ok_or_else(|| format!("配置中缺少 {} 脚本定义", script_key))?;

    let command = script_cfg
        .command
        .as_ref()
        .ok_or_else(|| format!("脚本 {} 缺少 command 配置", script_key))?;

    // 构造参数：如果是 python 则添加 -u 确保 unbuffered 输出，如果是 uv 则不添加
    let mut final_args = Vec::new();
    if command.contains("python") {
        final_args.push("-u".to_string());
    }

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

    let quoted_args: Vec<String> = final_args.iter().map(|arg| format!("'{}'", arg)).collect();
    println!("Executing Python command: {} {}", command, quoted_args.join(" "));

    // 启动子进程，并管道 stdout/stderr
    let mut child = TokioCommand::new(command)
        .args(&final_args)
        .current_dir(&config.env.working_dir)
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

    // Ordinary list generation may take several minutes; match taskctl's
    // existing 15-minute default rather than treating it as a short request.
    let deadline_seconds = if script_key == "generate_list" { 900 } else { 300 };
    let wait_future = child.wait();
    let status = match timeout(Duration::from_secs(deadline_seconds), wait_future).await {
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
#[cfg(test)]
pub(crate) mod bash_test_support {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    // Lowest-layer fake only: real TOML loader, argv, bash process, both pipe
    // readers, wait and deadline remain under the original config owner.
    pub struct FakeBash {
        pub config: AppConfig,
        directory: PathBuf,
    }
    impl FakeBash {
        pub fn new(source: &str) -> Self {
            let directory = std::env::temp_dir().join(format!("quantnight-native-{}-{}",
                std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("fake.sh"), source).unwrap();
            let quoted_dir = serde_json::to_string(directory.to_str().unwrap()).unwrap();
            let mut toml = format!(r#"
[env]
working_dir = {quoted_dir}
[python.scripts]
[mongodb]
local_uri = "mongodb://offline.invalid"
remote_uri = "mongodb://offline.invalid"
[mongodb.databases]
mission = "unused"
simulation = "unused"
alpha = "unused"
"#);
            for key in ["probe", "check_task_status", "pause_task"] {
                toml.push_str(&format!(r#"
[bash.scripts.{key}]
command = "/bin/bash"
script = "fake.sh"
args = ["{{task_name}}"]
"#));
            }
            let path = directory.join("config.toml");
            std::fs::write(&path, toml).unwrap();
            Self { config: load_config(&path).unwrap(), directory }
        }
    }
    impl Drop for FakeBash {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.directory); }
    }
}

#[cfg(test)]
mod bash_runner_tests {
    use super::*;
    use super::bash_test_support::FakeBash;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn original_entry_point_retains_argv_and_both_streams() {
        let fake = FakeBash::new("printf '%s\\n' \"$@\"; echo 原stderr >&2");
        let (out, err) = run_bash_script("probe", &[("task_name", "exact-name")], true, &fake.config).await.unwrap();
        assert_eq!(out, "exact-name\n--remote");
        assert_eq!(err, "原stderr");
    }

    #[tokio::test]
    async fn real_runner_streams_stdout_before_exit_and_preserves_timeout_stderr() {
        let fake = FakeBash::new("echo stage-line; echo detailed-timeout-cause >&2; exec /bin/sleep 30");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let observer: BashScriptObserver = Arc::new(move |event| { let _ = tx.send(event); });
        let config = fake.config.clone();
        let started = std::time::Instant::now();
        let run = tokio::spawn(async move { run_bash_script_observed("probe", &[], false,
            &config, Duration::from_millis(200), true, Some(observer)).await });
        assert!(matches!(rx.recv().await, Some(BashScriptEvent::Config)));
        assert!(matches!(rx.recv().await, Some(BashScriptEvent::Launch)));
        assert!(matches!(rx.recv().await, Some(BashScriptEvent::Wait)));
        let event = timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert!(matches!(event, BashScriptEvent::Stdout(ref line) if line == "stage-line"));
        assert!(!run.is_finished(), "stdout must be observed while script is still running");
        let error = run.await.unwrap().unwrap_err();
        assert!(error.contains("200 ms") && error.contains("detailed-timeout-cause"), "{error}");
        assert_eq!(error.lines().next(), Some("Bash 脚本执行超时（预算 200 ms）"));
        assert!(!error.contains('不'));
        assert!(error.contains("stage-line"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn nonzero_empty_and_configuration_launch_errors_keep_actual_causes() {
        let fake = FakeBash::new("echo partial; echo precise-error >&2; exit 23");
        let error = run_bash_script("probe", &[], false, &fake.config).await.unwrap_err();
        assert!(error.contains("23") && error.contains("precise-error") && error.contains("partial"));
        let fake = FakeBash::new(":");
        assert_eq!(run_bash_script("probe", &[], false, &fake.config).await.unwrap(), ("".into(), "".into()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let observer: BashScriptObserver = Arc::new(move |event| recorded.lock().unwrap().push(format!("{event:?}")));
        let error = run_bash_script_observed("absent", &[], false, &fake.config,
            Duration::from_secs(1), true, Some(observer.clone())).await.unwrap_err();
        assert!(error.contains("未找到 absent"));
        assert_eq!(*seen.lock().unwrap(), vec!["Config"]);
        let mut config = fake.config.clone();
        config.bash.scripts.get_mut("probe").unwrap().command = Some("/definitely/not/a/command".into());
        let error = run_bash_script_observed("probe", &[], false, &config,
            Duration::from_secs(1), true, Some(observer)).await.unwrap_err();
        assert!(error.contains("启动命令失败"));
        assert_eq!(*seen.lock().unwrap(), vec!["Config", "Config", "Launch"]);
        let fake = FakeBash::new("printf '\\377\\n'");
        let error = run_bash_script("probe", &[], false, &fake.config).await.unwrap_err();
        assert!(error.contains("读取 stdout 失败"), "{error}");
    }

    #[tokio::test]
    async fn total_deadline_includes_inherited_pipes_and_cleans_only_diagnostic_group() {
        let fake = FakeBash::new("echo pipe-child; /bin/sleep 30 & echo $! > diagnostic-child.pid; echo error-before-parent-exit >&2; exit 0");
        // An unrelated process must survive even though this diagnostic's
        // process group (including the child holding the pipe) is cleaned up.
        let mut unrelated = TokioCommand::new("/bin/sleep").arg("30").kill_on_drop(true).spawn().unwrap();
        let started = std::time::Instant::now();
        let result = run_bash_script_observed("probe", &[], false, &fake.config,
            Duration::from_millis(100), true, None).await.unwrap_err();
        assert!(result.contains("超时") && result.contains("error-before-parent-exit"), "{result}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(unrelated.try_wait().unwrap().is_none());
        let pid = std::fs::read_to_string(Path::new(&fake.config.env.working_dir).join("diagnostic-child.pid")).unwrap();
        let state = std::process::Command::new("/bin/ps").args(["-p", pid.trim(), "-o", "stat="])
            .output().unwrap();
        let state = String::from_utf8_lossy(&state.stdout);
        assert!(state.trim().is_empty() || state.trim().starts_with('Z'),
            "diagnostic descendant must be exited, not an orphaned live probe: {state}");
        unrelated.kill().await.unwrap();
    }
}
