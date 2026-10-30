use quantnight_task_core::{
    cli_args::{self, CliInvocation, CliParse, CliUsageError, CliValue},
    create_generate::{CreateRequest, GenerateRequest},
    inspection::{
        CollectRequest, EvidenceAction, EvidenceRequest, ListRequest, ResultsRequest, StatusRequest,
    },
    maintenance_ops::{ObserveAcceptedRequest, PriorityPruneRequest, WatchRequest},
    resume::ResumeRequest,
    start::{NotificationOptions, NotificationTarget, StartRequest},
};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Entry {
    Help(String),
    Command {
        app_config: Option<PathBuf>,
        backend_root: Option<PathBuf>,
        invocation: CliInvocation,
    },
}

/// Only a prefix global is consumed here. All business tokens go unchanged to
/// the shared parser, including values that happen to contain "--app-config".
pub fn parse(args: &[String]) -> Result<Entry, CliUsageError> {
    let mut index = 0;
    let mut app_config = None;
    let mut backend_root = None;
    while let Some(arg) = args.get(index) {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (arg.as_str(), None),
        };
        if !matches!(flag, "--app-config" | "--backend-root") { break; }
        let value = if let Some(value) = inline {
            if value.is_empty() { return Err(usage("configuration selector requires a nonempty path")); }
            value
        } else {
            index += 1;
            args.get(index).filter(|value| !value.is_empty() && !value.starts_with('-'))
                .ok_or_else(|| usage("configuration selector requires PATH (use =PATH for a leading dash)"))?
        };
        if flag == "--app-config" { app_config = Some(PathBuf::from(value)); }
        else { backend_root = Some(PathBuf::from(value)); }
        index += 1;
    }
    if app_config.is_some() && backend_root.is_some() {
        return Err(usage("--app-config and --backend-root are mutually exclusive"));
    }
    match cli_args::parse_args(&args[index..])? {
        CliParse::Help { command } => Ok(Entry::Help(format!(
            "{}\nNative global (before COMMAND only):\n  --app-config PATH  Explicit existing AppConfig file.\n  --backend-root PATH  Original backend config owner; used by the Python compatibility entry.\n  --config JSON      Remains the start business configuration, NOT a file path.\nNo automatic configuration discovery or installation.\n",
            cli_args::help_text(command.as_deref())?
        ))),
        CliParse::Command(invocation) => Ok(Entry::Command { app_config, backend_root, invocation }),
    }
}

fn usage(message: &str) -> CliUsageError {
    CliUsageError {
        command: None,
        message: message.into(),
        exit_code: 2,
    }
}

#[derive(Debug)]
pub struct NamedRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
}
#[derive(Debug)]
pub struct PauseRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
    pub expected_launch_attempt_id: Option<String>,
}
#[derive(Debug)]
pub struct DeleteRequest {
    pub name: String,
    pub expected_task_id: Option<String>,
    pub confirm: Option<String>,
}

#[derive(Debug)]
pub enum Request {
    List(ListRequest),
    VerifyRegistration(EvidenceRequest),
    ArchiveResearch(EvidenceRequest),
    Create(CreateRequest),
    Generate(GenerateRequest),
    Sync(NamedRequest),
    Start(StartRequest),
    ObserveAccepted(ObserveAcceptedRequest),
    Watch(WatchRequest),
    Status(StatusRequest),
    Pause(PauseRequest),
    Resume(ResumeRequest),
    PriorityPrune(PriorityPruneRequest),
    Delete(DeleteRequest),
    Results(ResultsRequest),
    CollectResults(CollectRequest),
}

/// Public for testing the invocation boundary, not a second argument parser.
/// Missing/mistyped fields reject rather than inventing defaults. Core parser
/// owns defaults; core coordinators own business validation.
pub fn to_request(i: &CliInvocation) -> Result<Request, String> {
    let a = Args(i);
    let name = || {
        i.name
            .clone()
            .ok_or_else(|| "parsed task name missing".to_string())
    };
    let expected_id = || a.text("expected_task_id");
    Ok(match i.command.as_str() {
        "list" => Request::List(ListRequest {
            include_deactive: a.flag("include_deactive")?,
            remote: a.flag("remote")?,
        }),
        "verify-registration" | "archive-research" => {
            let action = if i.command == "verify-registration" {
                EvidenceAction::VerifyRegistration
            } else {
                EvidenceAction::ArchiveResearch
            };
            let r = EvidenceRequest {
                name: name()?,
                action,
                expected_task_id: expected_id()?,
                expected_remote: match a.text("expected_remote")?.as_deref() {
                    None => None,
                    Some("true") => Some(true),
                    Some("false") => Some(false),
                    _ => return Err("invalid parsed expected_remote".into()),
                },
            };
            if i.command == "verify-registration" {
                Request::VerifyRegistration(r)
            } else {
                Request::ArchiveResearch(r)
            }
        }
        "create" => {
            let mut r = CreateRequest::new(name()?, a.required_text("templatefile")?);
            r.task_type = a.required_text("type")?;
            r.auth_profile = a.required_text("auth_profile")?;
            r.target_remote = a.flag("remote")?;
            r.force = a.flag("force")?;
            r.expected_task_id = expected_id()?;
            Request::Create(r)
        }
        "generate" => Request::Generate(GenerateRequest {
            name: name()?,
            expected_task_id: expected_id()?,
            replace: a.flag("replace")?,
            timeout_seconds: a.u64("timeout")?,
        }),
        // Original cmd_sync does not read args.timeout. Do not invent a new
        // deadline or silently change the shared coordinator's fixed budgets.
        "sync" => Request::Sync(NamedRequest {
            name: name()?,
            expected_task_id: expected_id()?,
        }),
        "start" => Request::Start(StartRequest {
            name: name()?,
            config_json: a.text("config")?,
            remote: a.optional_bool("remote")?,
            expected_task_id: expected_id()?,
            expected_remote: None,
            expected_launch_attempt_id: a.text("expected_launch_attempt_id")?,
            notification: a.notification()?,
        }),
        "observe-accepted" => Request::ObserveAccepted(ObserveAcceptedRequest {
            name: name()?,
            expected_task_id: expected_id()?,
        }),
        "watch" => Request::Watch(WatchRequest {
            name: name()?,
            notification: a.notification()?,
            replace_watcher: a.flag("replace_watcher")?,
        }),
        "status" => Request::Status(StatusRequest {
            name: name()?,
            remote: a.optional_bool("remote")?,
            inspect_items: a.flag("inspect_items")?,
            watch_observation: a.text("watch_observation")?.map(|raw|
                serde_json::from_str(&raw).map_err(|e| format!("invalid watcher observation: {e}")))
                .transpose()?,
        }),
        "pause" => Request::Pause(PauseRequest {
            name: name()?,
            expected_task_id: expected_id()?,
            expected_launch_attempt_id: a.text("expected_launch_attempt_id")?,
        }),
        "resume" => Request::Resume(ResumeRequest {
            name: name()?,
            expected_task_id: None,
            include_timeout: a.flag("include_timeout")?,
            recover_processing: a.flag("recover_processing")?,
        }),
        "priority-prune" => Request::PriorityPrune(PriorityPruneRequest {
            name: name()?,
            confirm: a.required_text("confirm")?,
            sharpe: a.float("sharpe")?,
            fitness: a.float("fitness")?,
            dry_run: a.flag("dry_run")?,
        }),
        // --force is accepted for legacy syntax, never a safety bypass.
        "delete" => Request::Delete(DeleteRequest {
            name: name()?,
            expected_task_id: expected_id()?,
            confirm: a.text("confirm")?,
        }),
        "results" => Request::Results(ResultsRequest {
            name: name()?,
            limit: a.i64("limit")?,
            offset: a.i64("offset")?,
            output: a.text("output")?.map(PathBuf::from),
        }),
        "collect-results" => Request::CollectResults(CollectRequest {
            name: name()?,
            output: a.text("output")?.map(PathBuf::from),
        }),
        _ => return Err(format!("unsupported parsed command: {}", i.command)),
    })
}

struct Args<'a>(&'a CliInvocation);
impl Args<'_> {
    fn value(&self, key: &str) -> Result<&CliValue, String> {
        self.0
            .options
            .get(key)
            .ok_or_else(|| format!("parsed option missing: {key}"))
    }
    fn bad(&self, key: &str) -> String {
        format!("invalid parsed option: {key}")
    }
    fn text(&self, key: &str) -> Result<Option<String>, String> {
        match self.value(key)? {
            CliValue::Null => Ok(None),
            CliValue::String(s) => Ok(Some(s.clone())),
            _ => Err(self.bad(key)),
        }
    }
    fn required_text(&self, key: &str) -> Result<String, String> {
        self.text(key)?.ok_or_else(|| self.bad(key))
    }
    fn optional_bool(&self, key: &str) -> Result<Option<bool>, String> {
        match self.value(key)? {
            CliValue::Null => Ok(None),
            CliValue::Bool(v) => Ok(Some(*v)),
            _ => Err(self.bad(key)),
        }
    }
    fn flag(&self, key: &str) -> Result<bool, String> {
        self.optional_bool(key)?.ok_or_else(|| self.bad(key))
    }
    fn integer(&self, key: &str) -> Result<&str, String> {
        match self.value(key)? {
            CliValue::Integer(v) => Ok(v),
            _ => Err(self.bad(key)),
        }
    }
    fn i64(&self, key: &str) -> Result<i64, String> {
        self.integer(key)?.parse().map_err(|_| {
            format!(
                "--{} exceeds the shared core i64 request representation; no operation performed",
                key.replace('_', "-")
            )
        })
    }
    fn u64(&self, key: &str) -> Result<u64, String> {
        self.integer(key)?.parse().map_err(|_| {
            format!(
                "--{} cannot be represented by the shared core u64 request; no operation performed",
                key.replace('_', "-")
            )
        })
    }
    fn optional_integer(&self, key: &str) -> Result<Option<i64>, String> {
        if matches!(self.value(key)?, CliValue::Null) {
            Ok(None)
        } else {
            self.i64(key).map(Some)
        }
    }
    fn float(&self, key: &str) -> Result<f64, String> {
        self.value(key)?.as_f64().ok_or_else(|| self.bad(key))
    }
    fn notification(&self) -> Result<NotificationOptions, String> {
        let session = self.text("notify_session_id")?;
        let worker = self.text("notify_task_id")?;
        let no_notify = self.flag("no_notify")?;
        if usize::from(session.is_some()) + usize::from(worker.is_some()) + usize::from(no_notify)
            > 1
        {
            return Err("invalid parsed notification target conflict".into());
        }
        Ok(NotificationOptions {
            target: session
                .map(NotificationTarget::Session)
                .or_else(|| worker.map(NotificationTarget::Worker)),
            no_notify,
            item_key: self.text("notify_item_key")?,
            review_seconds: self.optional_integer("review_seconds")?,
            watch_timeout: self.optional_integer("watch_timeout")?,
        })
    }
}
