//! Lightweight slash-command grammar shared by the TUI and other hosts.
//!
//! A slash command is a control-plane message, not a prompt for the model.
//! Keeping the grammar here (rather than in the terminal event loop) lets the
//! TUI, a future GUI, and headless adapters make the same routing decision.
//! The parser intentionally does not execute anything and never performs
//! shell expansion: callers decide how a parsed command is authorized.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum UTF-8 bytes accepted for one slash command.
pub const MAX_SLASH_INPUT_BYTES: usize = 16 * 1024;

/// Wire/log schema version for the typed command representation.  This is
/// independent from the execution-blueprint version so hosts can evolve the
/// command grammar without rewriting old blueprint records.
pub const SLASH_SCHEMA_VERSION: u16 = 1;

/// The owner that should receive a parsed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlashRoute {
    /// A local interactive operation (view state, configuration, or the
    /// first-class b3ehive concepts owned by zenpi).
    Local,
    /// A b3ehive runtime operation.  The parser only records its arguments;
    /// it must not start a competition or loop by itself.
    Runtime,
}

/// A user-facing command recognized at the beginning of an input buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum SlashCommand {
    /// Show command help, optionally narrowed to one command name.
    Help {
        topic: Option<String>,
    },
    /// Submit a b3ehive goal to the host's first-class goal owner.
    Goal {
        instruction: String,
    },
    /// Query the active model when omitted, or request a model change.
    Model {
        name: Option<String>,
    },
    /// Operate on the first-class blueprint owner.
    Blueprint {
        action: BlueprintAction,
    },
    /// Read or transform a learning target through the first-class learn
    /// owner.  An omitted target asks the host for the current learn status.
    Learn {
        target: Option<String>,
    },
    /// Display recent session entries.
    History {
        limit: Option<usize>,
    },
    /// Display the current agent/runtime status.
    Status,
    /// Clear the visible transcript without deleting the durable journal.
    Clear,
    /// Ask the active request to stop.
    Cancel,
    /// Leave the interactive host.
    Exit,
    /// A command understood by the b3ehive runtime, but intentionally not
    /// interpreted by zenpi's local core.
    Compete {
        args: Vec<String>,
    },
    Loop {
        args: Vec<String>,
    },
}

/// Operations supported by `/blueprint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintAction {
    /// Show the active blueprint (the default for `/blueprint`).
    Show,
    /// Report checklist/worker progress.
    Status,
    /// Validate a blueprint file or the configured default.
    Validate { path: Option<String> },
    /// Ask the runtime host to execute one bounded blueprint target.
    Run { target: String },
    /// Open a blueprint path in the host's resource view.
    Open { path: String },
}

impl SlashCommand {
    /// Return the routing boundary for this command.
    pub const fn route(&self) -> SlashRoute {
        match self {
            Self::Compete { .. } | Self::Loop { .. } => SlashRoute::Runtime,
            _ => SlashRoute::Local,
        }
    }

    /// Canonical command spelling, useful for status messages and telemetry.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Help { .. } => "help",
            Self::Goal { .. } => "goal",
            Self::Model { .. } => "model",
            Self::Blueprint { .. } => "blueprint",
            Self::Learn { .. } => "learn",
            Self::History { .. } => "history",
            Self::Status => "status",
            Self::Clear => "clear",
            Self::Cancel => "cancel",
            Self::Exit => "exit",
            Self::Compete { .. } => "compete",
            Self::Loop { .. } => "loop",
        }
    }

    /// Whether this command belongs to zenpi's first-class control surface
    /// rather than being ordinary prompt text or a delegated runtime call.
    pub const fn is_first_class(&self) -> bool {
        matches!(
            self,
            Self::Goal { .. } | Self::Model { .. } | Self::Blueprint { .. } | Self::Learn { .. }
        )
    }

    /// Whether execution must be handed to the b3ehive runtime.
    pub const fn is_runtime(&self) -> bool {
        matches!(self.route(), SlashRoute::Runtime)
    }
}

/// A compact command catalogue for help text and completion.  Keeping this
/// as static data avoids pulling a command-line framework into the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub route: SlashRoute,
    pub usage: &'static str,
    pub summary: &'static str,
}

const NO_ALIASES: &[&str] = &[];
const HELP_ALIASES: &[&str] = &["?"];
const BLUEPRINT_ALIASES: &[&str] = &["bp"];
const EXIT_ALIASES: &[&str] = &["quit", "q"];

/// Commands exposed by the current control plane.  `compete` and `loop` are
/// deliberately listed so clients can complete them, while their route keeps
/// execution in the b3ehive runtime rather than the local agent core.
pub const COMMAND_SPECS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        name: "help",
        aliases: HELP_ALIASES,
        route: SlashRoute::Local,
        usage: "/help [command]",
        summary: "show slash-command help",
    },
    SlashCommandSpec {
        name: "goal",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/goal <instruction>",
        summary: "create or update the active b3ehive goal",
    },
    SlashCommandSpec {
        name: "model",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/model [name]",
        summary: "show or select the provider model",
    },
    SlashCommandSpec {
        name: "blueprint",
        aliases: BLUEPRINT_ALIASES,
        route: SlashRoute::Local,
        usage: "/blueprint [show|status|validate|run|open]",
        summary: "inspect and control the first-class blueprint",
    },
    SlashCommandSpec {
        name: "learn",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/learn [target]",
        summary: "inspect or run a first-class learn target",
    },
    SlashCommandSpec {
        name: "history",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/history [count]",
        summary: "show recent session history",
    },
    SlashCommandSpec {
        name: "status",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/status",
        summary: "show agent and runtime status",
    },
    SlashCommandSpec {
        name: "clear",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/clear",
        summary: "clear the visible transcript",
    },
    SlashCommandSpec {
        name: "cancel",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/cancel",
        summary: "cancel the active request",
    },
    SlashCommandSpec {
        name: "exit",
        aliases: EXIT_ALIASES,
        route: SlashRoute::Local,
        usage: "/exit",
        summary: "leave the interactive session",
    },
    SlashCommandSpec {
        name: "compete",
        aliases: NO_ALIASES,
        route: SlashRoute::Runtime,
        usage: "/compete <args...>",
        summary: "delegate proposal competition to b3ehive runtime",
    },
    SlashCommandSpec {
        name: "loop",
        aliases: NO_ALIASES,
        route: SlashRoute::Runtime,
        usage: "/loop <args...>",
        summary: "delegate the bounded loop to b3ehive runtime",
    },
];

/// Look up a command specification by canonical name or alias.  The lookup
/// is case-insensitive and does not require a leading slash.
pub fn spec(name: &str) -> Option<&'static SlashCommandSpec> {
    let name = name.trim().strip_prefix('/').unwrap_or(name.trim());
    COMMAND_SPECS.iter().find(|candidate| {
        candidate.name.eq_ignore_ascii_case(name)
            || candidate
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SlashError {
    #[error("slash command is empty; type /help for available commands")]
    Empty,
    #[error("slash command exceeds {MAX_SLASH_INPUT_BYTES} bytes")]
    TooLong,
    #[error("slash command contains an unsupported control character")]
    ControlCharacter,
    #[error("unterminated quoted argument")]
    UnterminatedQuote,
    #[error("trailing escape in slash command")]
    TrailingEscape,
    #[error("unknown slash command `/{0}`; type /help")]
    UnknownCommand(String),
    #[error("/{command} requires an argument")]
    MissingArgument { command: &'static str },
    #[error("/{command} received an unexpected argument")]
    UnexpectedArgument { command: &'static str },
    #[error("/history count must be a positive integer")]
    InvalidHistoryLimit,
    #[error("/blueprint has unknown action `/{action}`")]
    UnknownBlueprintAction { action: String },
}

/// Classification result for an input buffer before it reaches the model.
///
/// Hosts should call [`route_input`] at their input boundary.  A `Slash`
/// value is control-plane data and must be handled by the host; only a
/// `Prompt` value is eligible for provider submission.  Keeping this small
/// enum next to the parser makes it difficult for one transport to silently
/// treat an unknown slash command as ordinary model text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputRoute {
    Prompt(String),
    Slash(SlashCommand),
}

/// Classify one user input without executing it.
pub fn route_input(input: &str) -> Result<InputRoute, SlashError> {
    match parse(input)? {
        Some(command) => Ok(InputRoute::Slash(command)),
        None => Ok(InputRoute::Prompt(input.to_owned())),
    }
}

/// Parse an input buffer.
///
/// `Ok(None)` means the input is ordinary model text.  A leading slash after
/// optional whitespace is always treated as a control command; malformed or
/// unknown commands return an error instead of silently reaching the model.
pub fn parse(input: &str) -> Result<Option<SlashCommand>, SlashError> {
    if input.trim().is_empty() {
        return Ok(None);
    }
    if input.len() > MAX_SLASH_INPUT_BYTES {
        return if input.trim_start().starts_with('/') {
            Err(SlashError::TooLong)
        } else {
            Ok(None)
        };
    }
    let trimmed = input.trim_start_matches(char::is_whitespace);
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    validate_controls(trimmed)?;
    let body = trimmed[1..].trim();
    if body.is_empty() {
        return Err(SlashError::Empty);
    }
    let tokens = tokenize(body)?;
    let command = tokens.first().ok_or(SlashError::Empty)?;
    let command_lower = command.to_ascii_lowercase();
    let args = &tokens[1..];
    let parsed = match command_lower.as_str() {
        "help" | "?" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "help" });
            }
            SlashCommand::Help {
                topic: args.first().cloned(),
            }
        }
        "goal" => SlashCommand::Goal {
            instruction: required_join(args, "goal")?,
        },
        "model" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "model" });
            }
            SlashCommand::Model {
                name: args.first().cloned(),
            }
        }
        "blueprint" | "bp" => SlashCommand::Blueprint {
            action: parse_blueprint(args)?,
        },
        "learn" => SlashCommand::Learn {
            target: optional_join(args),
        },
        "history" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "history" });
            }
            let limit = match args.first() {
                None => None, // No count means the host's default history window.
                Some(value) => {
                    let value = value
                        .parse::<usize>()
                        .map_err(|_| SlashError::InvalidHistoryLimit)?;
                    if value == 0 {
                        return Err(SlashError::InvalidHistoryLimit);
                    }
                    Some(value)
                }
            };
            SlashCommand::History { limit }
        }
        "status" => unit_command(args, "status", SlashCommand::Status)?,
        "clear" => unit_command(args, "clear", SlashCommand::Clear)?,
        "cancel" => unit_command(args, "cancel", SlashCommand::Cancel)?,
        "exit" | "quit" | "q" => unit_command(args, "exit", SlashCommand::Exit)?,
        "compete" => SlashCommand::Compete {
            args: args.to_vec(),
        },
        "loop" => SlashCommand::Loop {
            args: args.to_vec(),
        },
        other => return Err(SlashError::UnknownCommand(other.to_owned())),
    };
    Ok(Some(parsed))
}

/// Return canonical command names beginning with `prefix`, for a tiny TUI
/// completion menu.  The leading slash is optional.
pub fn complete(prefix: &str) -> Vec<&'static str> {
    let prefix = prefix.trim_start();
    let prefix = prefix.strip_prefix('/').unwrap_or(prefix);
    let prefix = prefix.to_ascii_lowercase();
    COMMAND_SPECS
        .iter()
        .filter(|spec| {
            spec.name.starts_with(&prefix)
                || spec.aliases.iter().any(|alias| alias.starts_with(&prefix))
        })
        .map(|spec| spec.name)
        .collect()
}

/// Render a compact, terminal-friendly help response without depending on a
/// UI widget or a command-line parser. Unknown topics return None so a host
/// can show a fail-closed error.
pub fn help(topic: Option<&str>) -> Option<String> {
    match topic {
        Some(topic) => {
            let spec = spec(topic)?;
            Some(format!(
                "{} - {}\n{}",
                spec.usage,
                spec.summary,
                route_label(spec.route)
            ))
        }
        None => Some(
            COMMAND_SPECS
                .iter()
                .map(|spec| format!("{:<44} {}", spec.usage, spec.summary))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

const fn route_label(route: SlashRoute) -> &'static str {
    match route {
        SlashRoute::Local => "route: local",
        SlashRoute::Runtime => "route: b3ehive runtime",
    }
}

fn validate_controls(input: &str) -> Result<(), SlashError> {
    if input
        .chars()
        .any(|character| character != '\n' && character != '\t' && character.is_control())
    {
        return Err(SlashError::ControlCharacter);
    }
    Ok(())
}

fn required_join(args: &[String], command: &'static str) -> Result<String, SlashError> {
    let value = optional_join(args).unwrap_or_default();
    if value.trim().is_empty() {
        return Err(SlashError::MissingArgument { command });
    }
    Ok(value)
}

fn optional_join(args: &[String]) -> Option<String> {
    (!args.is_empty()).then(|| args.join(" "))
}

fn unit_command(
    args: &[String],
    command: &'static str,
    value: SlashCommand,
) -> Result<SlashCommand, SlashError> {
    if args.is_empty() {
        Ok(value)
    } else {
        Err(SlashError::UnexpectedArgument { command })
    }
}

fn parse_blueprint(args: &[String]) -> Result<BlueprintAction, SlashError> {
    let Some(action) = args.first() else {
        return Ok(BlueprintAction::Show);
    };
    let action_lower = action.to_ascii_lowercase();
    match action_lower.as_str() {
        "show" | "list" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Show)
        }
        "status" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Status)
        }
        "validate" => {
            if args.len() > 2 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Validate {
                path: args.get(1).cloned(),
            })
        }
        "run" => {
            if args.len() < 2 {
                return Err(SlashError::MissingArgument {
                    command: "blueprint run",
                });
            }
            if args.len() > 2 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Run {
                target: args[1].clone(),
            })
        }
        "open" => {
            if args.len() < 2 {
                return Err(SlashError::MissingArgument {
                    command: "blueprint open",
                });
            }
            if args.len() > 2 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Open {
                path: args[1].clone(),
            })
        }
        _ => Err(SlashError::UnknownBlueprintAction {
            action: action.clone(),
        }),
    }
}

fn tokenize(input: &str) -> Result<Vec<String>, SlashError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut token_started = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            token_started = true;
            continue;
        }
        if character == '\\' {
            escaped = true;
            token_started = true;
            continue;
        }
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                current.push(character);
            }
            token_started = true;
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                token_started = true;
            }
            character if character.is_whitespace() => {
                if token_started {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            _ => {
                current.push(character);
                token_started = true;
            }
        }
    }
    if escaped {
        return Err(SlashError::TrailingEscape);
    }
    if quote.is_some() {
        return Err(SlashError::UnterminatedQuote);
    }
    if token_started {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_is_not_a_command() {
        assert_eq!(parse("explain /goal syntax").unwrap(), None);
    }

    #[test]
    fn first_class_commands_have_local_route() {
        assert_eq!(
            parse("/goal ship it").unwrap().unwrap().route(),
            SlashRoute::Local
        );
        assert_eq!(
            parse("/blueprint status").unwrap().unwrap().route(),
            SlashRoute::Local
        );
        assert_eq!(
            parse("/learn src/core.rs").unwrap().unwrap().route(),
            SlashRoute::Local
        );
    }

    #[test]
    fn runtime_commands_are_not_executed_locally() {
        let command = parse("/compete --workers 2").unwrap().unwrap();
        assert_eq!(command.route(), SlashRoute::Runtime);
        assert_eq!(command.name(), "compete");
    }

    #[test]
    fn quotes_and_escapes_are_decoded_without_shell_expansion() {
        assert_eq!(
            parse(r#"/goal "read my file" path\ with\ spaces"#)
                .unwrap()
                .unwrap(),
            SlashCommand::Goal {
                instruction: "read my file path with spaces".into()
            }
        );
    }
}
