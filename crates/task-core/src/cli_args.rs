//! Pure taskctl argument parsing. No configuration, filesystem, DB or process IO.
//! `Command` means parsed arguments, never a successful business operation.
//! The schema preserves argparse defaults; handler defaults (e.g. watcher
//! cadence) deliberately remain None here. Only create/generate/sync/delete
//! add an optional --expected-task-id for exact GUI identity binding.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const CLI_HELP_EXIT_CODE: i32 = 0;
pub const CLI_USAGE_EXIT_CODE: i32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum CliValue {
    Null,
    Bool(bool),
    String(String),
    /// Canonical decimal, preserving Python int values beyond i64/u64.
    Integer(String),
    /// Includes NaN/Inf accepted by argparse; business finite/range gates stay downstream.
    Float(f64),
}
impl CliValue {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Self::Integer(v) = self {
            v.parse().ok()
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Float(v) = self {
            Some(*v)
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct CliInvocation {
    pub command: String,
    pub name: Option<String>,
    /// argparse destination names (underscores), including defaults; excludes name/func.
    pub options: BTreeMap<String, CliValue>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum CliParse {
    Help { command: Option<String> },
    Command(CliInvocation),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliUsageError {
    pub command: Option<String>,
    pub message: String,
    pub exit_code: i32,
}
impl fmt::Display for CliUsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for CliUsageError {}
fn usage(command: Option<&str>, message: impl Into<String>) -> CliUsageError {
    CliUsageError {
        command: command.map(str::to_owned),
        message: message.into(),
        exit_code: CLI_USAGE_EXIT_CODE,
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Integer,
    Float,
    Flag,
    BooleanOptional,
    Choice(&'static [&'static str]),
}
#[derive(Clone, Copy)]
enum DefaultValue {
    Null,
    False,
    Text(&'static str),
    Integer(&'static str),
}
impl DefaultValue {
    fn value(self) -> CliValue {
        match self {
            Self::Null => CliValue::Null,
            Self::False => CliValue::Bool(false),
            Self::Text(s) => CliValue::String(s.into()),
            Self::Integer(s) => CliValue::Integer(s.into()),
        }
    }
}
#[derive(Clone, Copy)]
struct ArgSpec {
    long: &'static str,
    dest: &'static str,
    kind: Kind,
    default: DefaultValue,
    required: bool,
    notification_group: bool,
}
const fn arg(
    long: &'static str,
    dest: &'static str,
    kind: Kind,
    default: DefaultValue,
    required: bool,
) -> ArgSpec {
    ArgSpec {
        long,
        dest,
        kind,
        default,
        required,
        notification_group: false,
    }
}
const fn notify(
    long: &'static str,
    dest: &'static str,
    kind: Kind,
    default: DefaultValue,
) -> ArgSpec {
    ArgSpec {
        long,
        dest,
        kind,
        default,
        required: false,
        notification_group: true,
    }
}
const NAME: ArgSpec = arg("--name", "name", Kind::Text, DefaultValue::Null, true);
const ID: ArgSpec = arg(
    "--expected-task-id",
    "expected_task_id",
    Kind::Text,
    DefaultValue::Null,
    false,
);
const LAUNCH: ArgSpec = arg(
    "--expected-launch-attempt-id",
    "expected_launch_attempt_id",
    Kind::Text,
    DefaultValue::Null,
    false,
);
const REMOTE_FLAG: ArgSpec = arg("--remote", "remote", Kind::Flag, DefaultValue::False, false);
const REMOTE_OPTION: ArgSpec = arg(
    "--remote",
    "remote",
    Kind::BooleanOptional,
    DefaultValue::Null,
    false,
);
const FORCE: ArgSpec = arg("--force", "force", Kind::Flag, DefaultValue::False, false);
const TIMEOUT: ArgSpec = arg(
    "--timeout",
    "timeout",
    Kind::Integer,
    DefaultValue::Integer("900"),
    false,
);
const OUTPUT: ArgSpec = arg("--output", "output", Kind::Text, DefaultValue::Null, false);
const SESSION: ArgSpec = notify(
    "--notify-session-id",
    "notify_session_id",
    Kind::Text,
    DefaultValue::Null,
);
const WORKER: ArgSpec = notify(
    "--notify-task-id",
    "notify_task_id",
    Kind::Text,
    DefaultValue::Null,
);
const NO_NOTIFY: ArgSpec = notify("--no-notify", "no_notify", Kind::Flag, DefaultValue::False);
const ITEM: ArgSpec = arg(
    "--notify-item-key",
    "notify_item_key",
    Kind::Text,
    DefaultValue::Null,
    false,
);
const REVIEW: ArgSpec = arg(
    "--review-seconds",
    "review_seconds",
    Kind::Integer,
    DefaultValue::Null,
    false,
);
const WATCH_TIMEOUT: ArgSpec = arg(
    "--watch-timeout",
    "watch_timeout",
    Kind::Integer,
    DefaultValue::Null,
    false,
);
const EXPECTED_REMOTE: ArgSpec = arg(
    "--expected-remote",
    "expected_remote",
    Kind::Choice(&["true", "false"]),
    DefaultValue::Null,
    false,
);
struct CommandSpec {
    name: &'static str,
    args: &'static [ArgSpec],
}

// One definition per command. Reusable descriptors do not create a second
// parser branch; required/default/choice/mutex checks all consume this table.
const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "list",
        args: &[
            arg(
                "--include-deactive",
                "include_deactive",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
            REMOTE_FLAG,
        ],
    },
    CommandSpec {
        name: "verify-registration",
        args: &[NAME, ID, EXPECTED_REMOTE],
    },
    CommandSpec {
        name: "archive-research",
        args: &[NAME, ID, EXPECTED_REMOTE],
    },
    CommandSpec {
        name: "create",
        args: &[
            NAME,
            arg(
                "--templatefile",
                "templatefile",
                Kind::Text,
                DefaultValue::Null,
                true,
            ),
            arg(
                "--type",
                "type",
                Kind::Choice(&["regular", "super", "priority"]),
                DefaultValue::Text("regular"),
                false,
            ),
            arg(
                "--auth-profile",
                "auth_profile",
                Kind::Text,
                DefaultValue::Text("user1"),
                false,
            ),
            REMOTE_FLAG,
            FORCE,
            ID,
        ],
    },
    CommandSpec {
        name: "generate",
        args: &[
            NAME,
            arg(
                "--replace",
                "replace",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
            TIMEOUT,
            ID,
        ],
    },
    CommandSpec {
        name: "sync",
        args: &[NAME, TIMEOUT, ID],
    },
    CommandSpec {
        name: "start",
        args: &[
            NAME,
            arg("--config", "config", Kind::Text, DefaultValue::Null, false),
            REMOTE_OPTION,
            ID,
            LAUNCH,
            SESSION,
            WORKER,
            NO_NOTIFY,
            ITEM,
            REVIEW,
            WATCH_TIMEOUT,
        ],
    },
    CommandSpec {
        name: "observe-accepted",
        args: &[NAME, ID],
    },
    CommandSpec {
        name: "watch",
        args: &[
            NAME,
            arg(
                "--replace-watcher",
                "replace_watcher",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
            SESSION,
            WORKER,
            NO_NOTIFY,
            ITEM,
            REVIEW,
            WATCH_TIMEOUT,
        ],
    },
    CommandSpec {
        name: "status",
        args: &[
            NAME,
            REMOTE_OPTION,
            arg(
                "--inspect-items",
                "inspect_items",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
        ],
    },
    CommandSpec {
        name: "pause",
        args: &[NAME, ID, LAUNCH],
    },
    CommandSpec {
        name: "resume",
        args: &[
            NAME,
            arg(
                "--include-timeout",
                "include_timeout",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
            arg(
                "--recover-processing",
                "recover_processing",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
        ],
    },
    CommandSpec {
        name: "priority-prune",
        args: &[
            NAME,
            arg("--sharpe", "sharpe", Kind::Float, DefaultValue::Null, true),
            arg(
                "--fitness",
                "fitness",
                Kind::Float,
                DefaultValue::Null,
                true,
            ),
            arg("--confirm", "confirm", Kind::Text, DefaultValue::Null, true),
            arg(
                "--dry-run",
                "dry_run",
                Kind::Flag,
                DefaultValue::False,
                false,
            ),
        ],
    },
    CommandSpec {
        name: "delete",
        args: &[
            NAME,
            arg(
                "--confirm",
                "confirm",
                Kind::Text,
                DefaultValue::Null,
                false,
            ),
            FORCE,
            ID,
        ],
    },
    CommandSpec {
        name: "results",
        args: &[
            NAME,
            arg(
                "--limit",
                "limit",
                Kind::Integer,
                DefaultValue::Integer("20"),
                false,
            ),
            arg(
                "--offset",
                "offset",
                Kind::Integer,
                DefaultValue::Integer("0"),
                false,
            ),
            OUTPUT,
        ],
    },
    CommandSpec {
        name: "collect-results",
        args: &[NAME, OUTPUT],
    },
];

#[derive(Clone)]
enum Token {
    Value,
    End,
    Unknown,
    Help {
        explicit: Option<String>,
    },
    Option {
        index: usize,
        negative: bool,
        explicit: Option<String>,
    },
}

fn classify(token: &str, spec: Option<&CommandSpec>) -> Result<Token, CliUsageError> {
    let command = spec.map(|s| s.name);
    if token == "--" {
        return Ok(Token::End);
    }
    if token == "-" || !token.starts_with('-') {
        return Ok(Token::Value);
    }
    let (prefix, explicit) = token
        .split_once('=')
        .map_or((token, None), |(p, v)| (p, Some(v.to_owned())));
    if prefix == "-h" || prefix == "--help" {
        return Ok(Token::Help { explicit });
    }
    // argparse's sole short option -h exits before interpreting an attached
    // short-option tail (-hh/-hx/-help); an explicit -h=value remains an error.
    if token.starts_with("-h") && !token.starts_with("--") {
        return Ok(Token::Help { explicit: None });
    }
    let options = spec.map_or(&[][..], |s| s.args);
    let mut candidates = vec![];
    if prefix.starts_with("--") {
        if "--help".starts_with(prefix) {
            candidates.push((None, false, "--help"));
        }
        for (index, option) in options.iter().enumerate() {
            if option.long == prefix {
                return Ok(Token::Option {
                    index,
                    negative: false,
                    explicit,
                });
            }
            if matches!(option.kind, Kind::BooleanOptional) && prefix == "--no-remote" {
                return Ok(Token::Option {
                    index,
                    negative: true,
                    explicit,
                });
            }
            if option.long.starts_with(prefix) {
                candidates.push((Some(index), false, option.long));
            }
            if matches!(option.kind, Kind::BooleanOptional) && "--no-remote".starts_with(prefix) {
                candidates.push((Some(index), true, "--no-remote"));
            }
        }
    }
    if candidates.len() > 1 {
        return Err(usage(
            command,
            format!(
                "ambiguous option: {prefix} could match {}",
                candidates
                    .iter()
                    .map(|c| c.2)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    if let Some((index, negative, _)) = candidates.pop() {
        return Ok(match index {
            Some(index) => Token::Option {
                index,
                negative,
                explicit,
            },
            None => Token::Help { explicit },
        });
    }
    // argparse does not treat -1e2/-inf/-1_0 as negative-number values. An
    // embedded ASCII space, however, makes an unmatched option a plain value.
    if negative_number(token) || token.contains(' ') {
        Ok(Token::Value)
    } else {
        Ok(Token::Unknown)
    }
}

/// Python 3.11 / Unicode14 decimal-digit blocks used by the existing interpreter.
fn decimal_digit(c: char) -> Option<u8> {
    const ZEROES: &[u32] = &[
        0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
        0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
        0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
        0xff10, 0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0,
        0x11650, 0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x16a60, 0x16ac0,
        0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e950, 0x1fbf0,
    ];
    ZEROES.iter().find_map(|zero| {
        (c as u32)
            .checked_sub(*zero)
            .filter(|n| *n < 10)
            .map(|n| n as u8)
    })
}
fn negative_number(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('-') else {
        return false;
    };
    // Python regex $ also permits one final newline.
    let rest = rest.strip_suffix('\n').unwrap_or(rest);
    if let Some((a, b)) = rest.split_once('.') {
        !b.is_empty()
            && a.chars().all(|c| decimal_digit(c).is_some())
            && b.chars().all(|c| decimal_digit(c).is_some())
    } else {
        !rest.is_empty() && rest.chars().all(|c| decimal_digit(c).is_some())
    }
}
fn numeric_text(value: &str) -> Option<String> {
    let trimmed = value.trim_matches(|c: char| {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
            || (!c.is_ascii() && c.is_whitespace())
    });
    let normalized: String = trimmed
        .chars()
        .map(|c| decimal_digit(c).map_or(c, |d| char::from(b'0' + d)))
        .collect();
    let bytes = normalized.as_bytes();
    for (i, c) in bytes.iter().enumerate() {
        if *c == b'_'
            && (i == 0
                || i + 1 == bytes.len()
                || !bytes[i - 1].is_ascii_digit()
                || !bytes[i + 1].is_ascii_digit())
        {
            return None;
        }
    }
    Some(normalized.replace('_', ""))
}
fn integer(value: &str) -> Option<String> {
    let v = numeric_text(value)?;
    let negative = v.starts_with('-');
    let digits = v.strip_prefix(['-', '+']).unwrap_or(&v);
    // The original CPython process uses its default decimal conversion guard.
    if digits.is_empty() || digits.len() > 4300 || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        Some("0".into())
    } else {
        Some(format!("{}{}", if negative { "-" } else { "" }, digits))
    }
}
fn floating(value: &str) -> Option<f64> {
    let v = numeric_text(value)?;
    let lower = v.to_ascii_lowercase();
    match lower.as_str() {
        "nan" | "+nan" => return Some(f64::NAN),
        "-nan" => return Some(-f64::NAN),
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        _ => (),
    }
    // Rust's parser also recognizes NaN/inf; restrict the remaining alphabet
    // to Python decimal float syntax before delegating IEEE rounding/overflow.
    if v.is_empty()
        || !v
            .bytes()
            .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c))
    {
        return None;
    }
    v.parse().ok()
}
fn parse_value(spec: &ArgSpec, value: String, command: &str) -> Result<CliValue, CliUsageError> {
    let invalid = |kind| {
        usage(
            Some(command),
            format!("argument {}: invalid {kind} value: {value:?}", spec.long),
        )
    };
    match spec.kind {
        Kind::Text => Ok(CliValue::String(value)),
        Kind::Choice(choices) if choices.contains(&value.as_str()) => Ok(CliValue::String(value)),
        Kind::Choice(choices) => Err(usage(
            Some(command),
            format!(
                "argument {}: invalid choice: {value:?} (choose from {})",
                spec.long,
                choices.join(", ")
            ),
        )),
        Kind::Integer => integer(&value)
            .map(CliValue::Integer)
            .ok_or_else(|| invalid("int")),
        Kind::Float => floating(&value)
            .map(CliValue::Float)
            .ok_or_else(|| invalid("float")),
        _ => unreachable!("flags have no value parser"),
    }
}

fn parse_command(spec: &CommandSpec, args: &[String]) -> Result<CliParse, CliUsageError> {
    // argparse resolves ambiguous abbreviations in an initial scan, even if a
    // help token precedes them. Conversion/mutex/required errors happen later.
    let mut ended = false;
    let tokens = args
        .iter()
        .map(|s| {
            if ended {
                return Ok(Token::Value);
            }
            let t = classify(s, Some(spec))?;
            if matches!(t, Token::End) {
                ended = true;
            }
            Ok(t)
        })
        .collect::<Result<Vec<_>, CliUsageError>>()?;
    let mut options: BTreeMap<String, CliValue> = spec
        .args
        .iter()
        .map(|s| (s.dest.into(), s.default.value()))
        .collect();
    let mut seen = BTreeSet::new();
    let mut notification: Option<&str> = None;
    let mut extras = vec![];
    let mut i = 0;
    while i < args.len() {
        match tokens[i].clone() {
            Token::Help {
                explicit: Some(value),
            } => {
                return Err(usage(
                    Some(spec.name),
                    format!("argument -h/--help: ignored explicit argument {value:?}"),
                ))
            }
            Token::Help { explicit: None } => {
                return Ok(CliParse::Help {
                    command: Some(spec.name.into()),
                })
            }
            Token::Option {
                index,
                negative,
                explicit,
            } => {
                let option = &spec.args[index];
                let value = if matches!(option.kind, Kind::Flag | Kind::BooleanOptional) {
                    if let Some(value) = explicit {
                        return Err(usage(
                            Some(spec.name),
                            format!(
                                "argument {}: ignored explicit argument {value:?}",
                                option.long
                            ),
                        ));
                    }
                    CliValue::Bool(!negative)
                } else {
                    let raw = if let Some(value) = explicit {
                        value
                    } else {
                        if i + 1 >= args.len() || !matches!(tokens[i + 1], Token::Value) {
                            return Err(usage(
                                Some(spec.name),
                                format!("argument {}: expected one argument", option.long),
                            ));
                        }
                        i += 1;
                        args[i].clone()
                    };
                    parse_value(option, raw, spec.name)?
                };
                if option.notification_group {
                    if let Some(previous) = notification {
                        if previous != option.long {
                            return Err(usage(
                                Some(spec.name),
                                format!(
                                    "argument {}: not allowed with argument {previous}",
                                    option.long
                                ),
                            ));
                        }
                    }
                    notification = Some(option.long);
                }
                seen.insert(option.dest);
                options.insert(option.dest.into(), value);
            }
            _ => extras.push(args[i].as_str()),
        }
        i += 1;
    }
    let missing: Vec<_> = spec
        .args
        .iter()
        .filter(|s| s.required && !seen.contains(s.dest))
        .map(|s| s.long)
        .collect();
    if !missing.is_empty() {
        return Err(usage(
            Some(spec.name),
            format!(
                "the following arguments are required: {}",
                missing.join(", ")
            ),
        ));
    }
    if !extras.is_empty() {
        return Err(usage(
            None,
            format!("unrecognized arguments: {}", extras.join(" ")),
        ));
    }
    let name = match options.remove("name") {
        Some(CliValue::String(value)) => Some(value),
        _ => None,
    };
    Ok(CliParse::Command(CliInvocation {
        command: spec.name.into(),
        name,
        options,
    }))
}

/// Parse arguments WITHOUT argv[0]. No handlers are called; config is an opaque
/// start payload, not a path. Names/IDs/ranges/JSON remain business validation.
pub fn parse_args(args: &[String]) -> Result<CliParse, CliUsageError> {
    let mut extras = vec![];
    for (i, token) in args.iter().enumerate() {
        match classify(token, None)? {
            Token::Help { explicit: None } => return Ok(CliParse::Help { command: None }),
            Token::Help {
                explicit: Some(value),
            } => {
                return Err(usage(
                    None,
                    format!("argument -h/--help: ignored explicit argument {value:?}"),
                ))
            }
            Token::Unknown => extras.push(token.as_str()),
            _ => {
                let spec = COMMANDS.iter().find(|s| s.name == token).ok_or_else(|| {
                    usage(None, format!("argument command: invalid choice: {token:?}"))
                })?;
                let result = parse_command(spec, &args[i + 1..])?;
                if matches!(result, CliParse::Help { .. }) || extras.is_empty() {
                    return Ok(result);
                }
                return Err(usage(
                    None,
                    format!("unrecognized arguments: {}", extras.join(" ")),
                ));
            }
        }
    }
    Err(usage(None, "the following arguments are required: command"))
}

/// Pure help rendering from the SAME schema. No argv/process/config lookup.
/// Text layout is not an argparse version-specific byte compatibility promise.
pub fn help_text(command: Option<&str>) -> Result<String, CliUsageError> {
    let mut out = String::from(
        "taskctl: QuantNight task lifecycle arguments\n-h, --help  show help (exit 0)\n",
    );
    if let Some(command) = command {
        let spec = COMMANDS
            .iter()
            .find(|s| s.name == command)
            .ok_or_else(|| usage(None, format!("unknown command: {command}")))?;
        out.push_str(&format!("command: {command}\n"));
        for arg in spec.args {
            let aliases = if matches!(arg.kind, Kind::BooleanOptional) {
                " / --no-remote"
            } else {
                ""
            };
            out.push_str(&format!(
                "{}{}{}; default={:?}{}\n",
                arg.long,
                aliases,
                if arg.required { " (required)" } else { "" },
                arg.default.value(),
                if arg.notification_group {
                    "; exclusive notification target"
                } else {
                    ""
                }
            ));
            if let Kind::Choice(choices) = arg.kind {
                out.push_str(&format!("  choices: {}\n", choices.join(", ")));
            }
        }
    } else {
        out.push_str("commands:\n");
        for spec in COMMANDS {
            out.push_str(&format!("  {}\n", spec.name));
        }
    }
    Ok(out)
}
