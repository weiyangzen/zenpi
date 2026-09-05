//! Lightweight slash-command grammar shared by the TUI and other hosts.
//!
//! A slash command is a control-plane message, not a prompt for the model.
//! Keeping the grammar here (rather than in the terminal event loop) lets the
//! TUI, a future GUI, and headless adapters make the same routing decision.
//! The parser intentionally does not execute anything and never performs
//! shell expansion: callers decide how a parsed command is authorized.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::layout::{PaneId, TabId};

/// Maximum UTF-8 bytes accepted for one slash command.
pub const MAX_SLASH_INPUT_BYTES: usize = 16 * 1024;
/// Maximum raw UTF-8 bytes accepted for an explicit user-shell input.
pub const MAX_USER_SHELL_INPUT_BYTES: usize = 16 * 1024;

/// Keep retention parsing bounded before a host touches the filesystem.
/// Session directories are intentionally capped below this value as well;
/// larger policies are almost certainly an accidental destructive request.
pub const MAX_SESSION_GC_RETAIN_NEWEST: usize = 4096;
pub const MAX_SESSION_GC_AGE_SECONDS: u64 = u64::MAX / 1_000;

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
    /// Persist one validated Goal JSON document in the session's domain
    /// store.  Keeping this as a distinct variant prevents an input such as
    /// `putative migration` from being mistaken for a persistence request.
    GoalPut {
        path: String,
    },
    /// Propose a plan without pretending that it has been persisted or run.
    Plan {
        instruction: Option<String>,
    },
    /// Query the active model when omitted, or request a model change.
    Model {
        name: Option<String>,
    },
    /// List the bounded model/profile catalog visible to the host.
    Models,
    /// Run redacted local configuration and runtime diagnostics.
    Doctor,
    /// Operate on the first-class blueprint owner.
    Blueprint {
        action: BlueprintAction,
    },
    /// Read or transform a learning target through the first-class learn
    /// owner.  An omitted target asks the host for the current learn status.
    Learn {
        target: Option<String>,
    },
    /// Persist one validated Learn JSON document in the session's domain
    /// store.  The source file is read with a bounded, symlink-free reader by
    /// the host; it is never sent to the provider.
    LearnPut {
        path: String,
    },
    /// Attach one existing repository-relative artifact as durable Learn
    /// evidence. The host validates the path before touching the domain
    /// store; submitting the same reference again is idempotent.
    LearnEvidence {
        id: String,
        reference: String,
    },
    /// Inspect a validated Learn checkpoint. This is deliberately a
    /// read-only recovery projection until an external owner supplies a
    /// worker; it never pretends to resume model execution locally.
    LearnResume {
        id: String,
    },
    /// Display recent session entries.
    History {
        limit: Option<usize>,
    },
    /// Collect a bounded workspace/resource snapshot without contacting the
    /// provider. The optional path is resolved by the host.
    Resources {
        path: Option<String>,
    },
    /// Inspect or mutate the user-owned BentoBox layout. Layout commands are
    /// local control-plane messages and never become provider prompts.
    Layout {
        action: LayoutAction,
    },
    /// Focus or change visibility of one BentoBox pane.
    Pane {
        action: PaneAction,
    },
    /// Navigate the durable session owner. Parsing never touches the file
    /// system; hosts must execute supported actions explicitly.
    Session {
        action: SessionAction,
    },
    /// Inspect or mutate the active session's durable mailbox. The mailbox
    /// owner remains the session host; parsing never opens a journal.
    Mailbox {
        action: MailboxAction,
    },
    /// Replay or recover a bounded event suffix.
    Resume {
        sequence: Option<u64>,
    },
    /// Compact the current context through the host's context manager.
    Compact,
    /// Inspect a bounded file diff. The path remains workspace-relative data.
    Diff {
        path: Option<String>,
    },
    /// Attach a workspace-relative file to the next provider turn.
    Attach {
        path: String,
    },
    /// Resolve a pending side-effect approval.
    Approve {
        id: String,
        decision: ApproveDecision,
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

/// Durable session operations exposed by `/session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    List,
    Agents,
    Inspect {
        path: String,
    },
    Open {
        path: String,
    },
    ResumeLast,
    Fork {
        source: String,
        destination: String,
    },
    Export {
        source: String,
        destination: String,
    },
    Import {
        source: String,
        destination: String,
    },
    Migrate {
        source: String,
        destination: String,
    },
    Archive {
        path: String,
    },
    Unarchive {
        path: String,
    },
    Delete {
        path: String,
        confirmed: bool,
    },
    Queue {
        source: String,
        recipient: String,
        request_id: String,
        payload: serde_json::Value,
        ttl_ms: u64,
    },
    Gc {
        policy: SessionGcPolicy,
    },
}

/// Slash projection of the typed protocol mailbox request. This keeps TUI
/// and headless hosts on the same command grammar while preserving the
/// protocol's bounded payload and explicit lifecycle transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxAction {
    Send {
        recipient_session_id: String,
        message_id: String,
        text: String,
        ttl_ms: u64,
    },
    List {
        after_sequence: u64,
        limit: u16,
    },
    Acknowledge {
        message_id: String,
    },
    Claim {
        message_id: String,
    },
    Complete {
        message_id: String,
        outcome: crate::protocol::MailboxOutcome,
        result: Option<String>,
    },
}

/// Explicit retention and confirmation policy for `/session gc`.
///
/// A slash command must carry both retention dimensions and an explicit
/// confirmation token.  Keeping this as typed data prevents a host from
/// accidentally treating a bare `/session gc` as permission to delete files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionGcPolicy {
    pub retain_newest: usize,
    pub older_than_seconds: u64,
    pub confirm: bool,
}

/// Operations supported by `/layout`.
///
/// `tab: None` means the host's active tab. A headless host, which has no
/// terminal focus, uses the Project tab as its deterministic default. The
/// `Preset` operation selects a tab and restores its built-in geometry;
/// `Reset` removes the persisted customization for one tab (or the whole
/// profile when omitted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutAction {
    Show { tab: Option<TabId> },
    Preset { tab: Option<TabId> },
    Reset { tab: Option<TabId> },
    Save,
}

/// Operations supported by `/pane`.
///
/// A bare pane name is a focus request. Explicit visibility actions make the
/// intent unambiguous for headless clients and future GUI hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneAction {
    Show,
    Focus { pane: PaneId },
    Collapse { pane: PaneId },
    Expand { pane: PaneId },
    Toggle { pane: PaneId },
}

/// Slash-level approval intent. `Always` maps to an allow decision with the
/// host's remember flag; `Once` maps to a one-shot allow; `Deny` refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApproveDecision {
    Once,
    Always,
    Deny,
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
    /// Persist one validated Blueprint JSON document in the session's domain
    /// store.  This is deliberately an explicit path-based operation rather
    /// than an arbitrary inline payload, keeping command framing bounded.
    Put { path: String },
    /// Ask the runtime host to execute one bounded blueprint target.
    Run { target: String },
    /// Queue one declarative task for an external b3ehive owner.
    Handoff { target: String },
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
            Self::GoalPut { .. } => "goal",
            Self::Plan { .. } => "plan",
            Self::Model { .. } => "model",
            Self::Models => "models",
            Self::Doctor => "doctor",
            Self::Blueprint { .. } => "blueprint",
            Self::Learn { .. } => "learn",
            Self::LearnPut { .. } => "learn",
            Self::LearnEvidence { .. } => "learn",
            Self::LearnResume { .. } => "learn",
            Self::History { .. } => "history",
            Self::Resources { .. } => "resources",
            Self::Layout { .. } => "layout",
            Self::Pane { .. } => "pane",
            Self::Session { .. } => "session",
            Self::Mailbox { .. } => "mailbox",
            Self::Resume { .. } => "resume",
            Self::Compact => "compact",
            Self::Diff { .. } => "diff",
            Self::Attach { .. } => "attach",
            Self::Approve { .. } => "approve",
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
            Self::Goal { .. }
                | Self::GoalPut { .. }
                | Self::Plan { .. }
                | Self::Model { .. }
                | Self::Models
                | Self::Doctor
                | Self::Blueprint { .. }
                | Self::Learn { .. }
                | Self::LearnPut { .. }
                | Self::LearnEvidence { .. }
                | Self::LearnResume { .. }
                | Self::Layout { .. }
                | Self::Pane { .. }
                | Self::Session { .. }
                | Self::Mailbox { .. }
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
        usage: "/goal <instruction> | /goal put <json-path>",
        summary: "inspect or persist a bounded b3ehive goal",
    },
    SlashCommandSpec {
        name: "plan",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/plan [instruction]",
        summary: "propose a plan for the first-class blueprint owner",
    },
    SlashCommandSpec {
        name: "model",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/model [name]",
        summary: "show or select the provider model",
    },
    SlashCommandSpec {
        name: "models",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/models",
        summary: "list configured provider models and profiles",
    },
    SlashCommandSpec {
        name: "doctor",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/doctor",
        summary: "run redacted configuration and runtime diagnostics",
    },
    SlashCommandSpec {
        name: "blueprint",
        aliases: BLUEPRINT_ALIASES,
        route: SlashRoute::Local,
        usage: "/blueprint [show|status|validate|put|run|handoff|open]",
        summary: "inspect and control the first-class blueprint",
    },
    SlashCommandSpec {
        name: "learn",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/learn [show] | /learn put <json-path> | /learn evidence <id> <ref> | /learn resume <id>",
        summary: "inspect, evidence, or validate a first-class learn target",
    },
    SlashCommandSpec {
        name: "history",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/history [count]",
        summary: "show recent session history",
    },
    SlashCommandSpec {
        name: "resources",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/resources [path]",
        summary: "collect bounded workspace and host resource signals",
    },
    SlashCommandSpec {
        name: "layout",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/layout [show|preset [tab]|reset [tab]|save]",
        summary: "inspect or persist the BentoBox workspace layout",
    },
    SlashCommandSpec {
        name: "pane",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/pane [name|focus|collapse|expand|toggle NAME]",
        summary: "focus or change visibility of a BentoBox pane",
    },
    SlashCommandSpec {
        name: "session",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/session [list|agents|inspect PATH|open PATH|resume-last|fork SOURCE DEST|export SOURCE DEST|import SOURCE DEST|migrate SOURCE DEST|archive PATH --yes|unarchive PATH|delete PATH --yes|queue SOURCE RECIPIENT ID TTL_MS JSON|gc ...]",
        summary: "navigate durable sessions",
    },
    SlashCommandSpec {
        name: "mailbox",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/mailbox [send RECIPIENT ID TTL_MS TEXT|list [AFTER] [LIMIT]|ack ID|claim ID|complete ID succeeded|failed|abandoned [RESULT]]",
        summary: "exchange durable session messages",
    },
    SlashCommandSpec {
        name: "resume",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/resume [sequence]",
        summary: "replay or recover a bounded event suffix",
    },
    SlashCommandSpec {
        name: "compact",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/compact",
        summary: "compact context with a durable marker",
    },
    SlashCommandSpec {
        name: "diff",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/diff [path]",
        summary: "inspect bounded pending file changes",
    },
    SlashCommandSpec {
        name: "attach",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/attach <path>",
        summary: "attach a bounded workspace file to the next turn",
    },
    SlashCommandSpec {
        name: "approve",
        aliases: NO_ALIASES,
        route: SlashRoute::Local,
        usage: "/approve <id> once|always|deny",
        summary: "answer a pending side-effect request",
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
        usage: "/compete [submit] <task...> | /compete status",
        summary: "persist a bounded request for an external competition owner",
    },
    SlashCommandSpec {
        name: "loop",
        aliases: NO_ALIASES,
        route: SlashRoute::Runtime,
        usage: "/loop [start] <task...> | /loop status",
        summary: "persist a bounded request for an external loop owner",
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
    #[error("user-shell input exceeds {MAX_USER_SHELL_INPUT_BYTES} bytes")]
    UserShellTooLong,
    #[error("user-shell input contains an unsupported control character")]
    UserShellControlCharacter,
    #[error("`!!` is not supported; use a single `!` for a local shell command")]
    UnsupportedUserShellExtension,
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
    #[error("/learn {action} requires a learn id and repository-relative reference")]
    MissingLearnArgument { action: &'static str },
    #[error("/learn {action} received an unexpected argument")]
    UnexpectedLearnArgument { action: &'static str },
    #[error("/history count must be a positive integer")]
    InvalidHistoryLimit,
    #[error("/resume sequence must be a non-negative integer")]
    InvalidResumeSequence,
    #[error("/session received an unknown action `/{action}`")]
    UnknownSessionAction { action: String },
    #[error("/session {action} requires a path")]
    MissingSessionPath { action: &'static str },
    #[error("/session {action} received an unexpected argument")]
    UnexpectedSessionArgument { action: &'static str },
    #[error("/session {action} requires explicit confirmation")]
    MissingSessionConfirmation { action: &'static str },
    #[error("/session queue payload must be valid JSON")]
    InvalidSessionPayload,
    #[error("/session queue TTL must be a positive bounded integer")]
    InvalidSessionTtl,
    #[error("/mailbox {action} received invalid arguments")]
    InvalidMailboxArgument { action: &'static str },
    #[error("/mailbox complete outcome must be succeeded, failed, or abandoned")]
    InvalidMailboxOutcome,
    #[error("/session gc {flag} must be a non-negative integer")]
    InvalidSessionGcValue { flag: &'static str },
    #[error("/session gc requires --retain-newest N --older-than-seconds N --yes")]
    MissingSessionGcPolicy,
    #[error("/session gc requires --yes confirmation")]
    MissingSessionGcConfirmation,
    #[error("/approve decision must be once, always, or deny")]
    InvalidApproveDecision,
    #[error("/blueprint has unknown action `/{action}`")]
    UnknownBlueprintAction { action: String },
    #[error("/layout has unknown action `/{action}`")]
    UnknownLayoutAction { action: String },
    #[error("/layout {action} requires a tab name")]
    MissingLayoutTab { action: &'static str },
    #[error("/layout {action} received an unexpected argument")]
    UnexpectedLayoutArgument { action: &'static str },
    #[error("unknown layout tab `{0}`; expected project, goal, learn, review, or session")]
    UnknownLayoutTab(String),
    #[error("/pane has unknown action `/{action}`")]
    UnknownPaneAction { action: String },
    #[error("/pane {action} requires a pane name")]
    MissingPaneName { action: &'static str },
    #[error("/pane {action} received an unexpected argument")]
    UnexpectedPaneArgument { action: &'static str },
    #[error("unknown pane `{0}`; use /help pane for pane names")]
    UnknownPane(String),
}

/// Classification result for an input buffer before it reaches the model.
///
/// Hosts should call [`route_input`] at their input boundary.  A `Slash`
/// value is control-plane data and a `UserShell` value is an explicit local
/// execution request; only a `Prompt` value is eligible for provider submission.
/// Keeping this small
/// enum next to the parser makes it difficult for one transport to silently
/// treat an unknown slash command as ordinary model text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputRoute {
    Prompt(String),
    Slash(SlashCommand),
    /// Trimmed raw input including the leading `!`. A bare `!` requests help.
    /// Shell syntax is preserved for the authorized shell owner, not parsed
    /// as slash-command arguments or executed by this routing layer.
    UserShell(String),
}

/// Classify one user input without executing it.
pub fn route_input(input: &str) -> Result<InputRoute, SlashError> {
    let trimmed = input.trim();
    if trimmed.starts_with('!') {
        if input.len() > MAX_USER_SHELL_INPUT_BYTES {
            return Err(SlashError::UserShellTooLong);
        }
        if input
            .chars()
            .any(|character| character != '\n' && character != '\t' && character.is_control())
        {
            return Err(SlashError::UserShellControlCharacter);
        }
        if trimmed.starts_with("!!") {
            return Err(SlashError::UnsupportedUserShellExtension);
        }
        return Ok(InputRoute::UserShell(trimmed.to_owned()));
    }
    match parse(input)? {
        Some(command) => Ok(InputRoute::Slash(command)),
        None => Ok(InputRoute::Prompt(input.to_owned())),
    }
}

/// Parse an input buffer.
///
/// `Ok(None)` means the input is not a slash command. Call [`route_input`] to
/// distinguish ordinary model text from an explicit user-shell request. A
/// leading slash after optional whitespace is always a control command;
/// malformed or unknown commands never silently reach the model.
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
        "goal" => parse_goal(args)?,
        "plan" => SlashCommand::Plan {
            instruction: optional_join(args),
        },
        "model" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "model" });
            }
            SlashCommand::Model {
                name: args.first().cloned(),
            }
        }
        "models" => unit_command(args, "models", SlashCommand::Models)?,
        "doctor" => unit_command(args, "doctor", SlashCommand::Doctor)?,
        "blueprint" | "bp" => SlashCommand::Blueprint {
            action: parse_blueprint(args)?,
        },
        "learn" => parse_learn(args)?,
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
        "resources" | "resource" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument {
                    command: "resources",
                });
            }
            SlashCommand::Resources {
                path: args.first().cloned(),
            }
        }
        "layout" => SlashCommand::Layout {
            action: parse_layout(args)?,
        },
        "pane" => SlashCommand::Pane {
            action: parse_pane(args)?,
        },
        "session" => SlashCommand::Session {
            action: parse_session(args)?,
        },
        "mailbox" => SlashCommand::Mailbox {
            action: parse_mailbox(args)?,
        },
        "resume" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "resume" });
            }
            let sequence = args.first().map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| SlashError::InvalidResumeSequence)
            });
            SlashCommand::Resume {
                sequence: sequence.transpose()?,
            }
        }
        "compact" => unit_command(args, "compact", SlashCommand::Compact)?,
        "diff" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedArgument { command: "diff" });
            }
            SlashCommand::Diff {
                path: args.first().cloned(),
            }
        }
        "attach" => {
            if args.len() != 1 {
                return if args.is_empty() {
                    Err(SlashError::MissingArgument { command: "attach" })
                } else {
                    Err(SlashError::UnexpectedArgument { command: "attach" })
                };
            }
            if args[0].trim().is_empty() {
                return Err(SlashError::MissingArgument { command: "attach" });
            }
            SlashCommand::Attach {
                path: args[0].clone(),
            }
        }
        "approve" => SlashCommand::Approve {
            id: {
                let id = args
                    .first()
                    .cloned()
                    .ok_or(SlashError::MissingArgument { command: "approve" })?;
                if id.trim().is_empty() {
                    return Err(SlashError::MissingArgument { command: "approve" });
                }
                id
            },
            decision: parse_approve(args.get(1), args.len())?,
        },
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

fn parse_layout(args: &[String]) -> Result<LayoutAction, SlashError> {
    let Some(action) = args.first() else {
        return Ok(LayoutAction::Show { tab: None });
    };
    let action_lower = action.to_ascii_lowercase();
    match action_lower.as_str() {
        "show" | "status" => {
            if args.len() > 2 {
                return Err(SlashError::UnexpectedLayoutArgument { action: "show" });
            }
            Ok(LayoutAction::Show {
                tab: args.get(1).map(|value| parse_tab(value)).transpose()?,
            })
        }
        "preset" | "use" | "select" => {
            if args.len() > 2 {
                return Err(SlashError::UnexpectedLayoutArgument { action: "preset" });
            }
            Ok(LayoutAction::Preset {
                tab: args.get(1).map(|value| parse_tab(value)).transpose()?,
            })
        }
        "reset" => {
            if args.len() > 2 {
                return Err(SlashError::UnexpectedLayoutArgument { action: "reset" });
            }
            let tab = match args.get(1).map(String::as_str) {
                None | Some("all") | Some("profile") => None,
                Some(value) => Some(parse_tab(value)?),
            };
            Ok(LayoutAction::Reset { tab })
        }
        "save" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedLayoutArgument { action: "save" });
            }
            Ok(LayoutAction::Save)
        }
        // A bare tab name is a convenient shorthand for `/layout preset TAB`.
        value => Ok(LayoutAction::Preset {
            tab: Some(parse_tab(value)?),
        }),
    }
}

fn parse_pane(args: &[String]) -> Result<PaneAction, SlashError> {
    let Some(action) = args.first() else {
        return Ok(PaneAction::Show);
    };
    let action_lower = action.to_ascii_lowercase();
    match action_lower.as_str() {
        "show" | "list" | "status" if args.len() == 1 => Ok(PaneAction::Show),
        "focus" | "select" | "use" => Ok(PaneAction::Focus {
            pane: required_pane_name(args, "focus")?,
        }),
        "collapse" | "hide" => Ok(PaneAction::Collapse {
            pane: required_pane_name(args, "collapse")?,
        }),
        "expand" | "show-pane" => Ok(PaneAction::Expand {
            pane: required_pane_name(args, "expand")?,
        }),
        "toggle" => Ok(PaneAction::Toggle {
            pane: required_pane_name(args, "toggle")?,
        }),
        _ => {
            if args.len() != 1 {
                return Err(SlashError::UnknownPaneAction {
                    action: action.clone(),
                });
            }
            Ok(PaneAction::Focus {
                pane: parse_pane_name(action)?,
            })
        }
    }
}

fn parse_tab(value: &str) -> Result<TabId, SlashError> {
    match value.to_ascii_lowercase().as_str() {
        "project" | "proj" => Ok(TabId::Project),
        "goal" => Ok(TabId::Goal),
        "learn" => Ok(TabId::Learn),
        "review" => Ok(TabId::Review),
        "session" | "sessions" => Ok(TabId::Session),
        _ => Err(SlashError::UnknownLayoutTab(value.to_owned())),
    }
}

fn required_pane_name(args: &[String], action: &'static str) -> Result<PaneId, SlashError> {
    if args.len() < 2 {
        return Err(SlashError::MissingPaneName { action });
    }
    if args.len() > 2 {
        return Err(SlashError::UnexpectedPaneArgument { action });
    }
    parse_pane_name(&args[1])
}

fn parse_pane_name(value: &str) -> Result<PaneId, SlashError> {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    let pane = match normalized.as_str() {
        "project_conversation" | "project" | "conversation" => PaneId::ProjectConversation,
        "resources" | "resource" => PaneId::Resources,
        "goal_conversation" | "goal" => PaneId::GoalConversation,
        "gantt" | "board" => PaneId::Gantt,
        "browser" | "web" => PaneId::Browser,
        "terminal" | "pty" => PaneId::Terminal,
        "learn_conversation" | "learn" => PaneId::LearnConversation,
        "learn_resources" | "source_resources" => PaneId::LearnResources,
        "learn_queue" | "queue" => PaneId::LearnQueue,
        "learn_mapping" | "mapping" => PaneId::LearnMapping,
        "evidence" => PaneId::Evidence,
        "review_conversation" | "review" => PaneId::ReviewConversation,
        "checks" | "check" => PaneId::Checks,
        "approval_queue" | "approvals" | "approval" => PaneId::ApprovalQueue,
        "diff" => PaneId::Diff,
        "session_list" | "sessions" | "session" => PaneId::SessionList,
        "session_conversation" => PaneId::SessionConversation,
        "replay_controls" | "replay" => PaneId::ReplayControls,
        "event_timeline" | "events" | "timeline" => PaneId::EventTimeline,
        _ => return Err(SlashError::UnknownPane(value.to_owned())),
    };
    Ok(pane)
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
        "handoff" => {
            if args.len() < 2 {
                return Err(SlashError::MissingArgument {
                    command: "blueprint handoff",
                });
            }
            if args.len() > 2 {
                return Err(SlashError::UnexpectedArgument {
                    command: "blueprint",
                });
            }
            Ok(BlueprintAction::Handoff {
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
        "put" => Ok(BlueprintAction::Put {
            path: required_domain_path(args, "blueprint put")?,
        }),
        _ => Err(SlashError::UnknownBlueprintAction {
            action: action.clone(),
        }),
    }
}

fn parse_goal(args: &[String]) -> Result<SlashCommand, SlashError> {
    let Some(action) = args.first() else {
        return Err(SlashError::MissingArgument { command: "goal" });
    };
    if action.eq_ignore_ascii_case("put") {
        return Ok(SlashCommand::GoalPut {
            path: required_domain_path(args, "goal put")?,
        });
    }
    Ok(SlashCommand::Goal {
        instruction: required_join(args, "goal")?,
    })
}

fn parse_learn(args: &[String]) -> Result<SlashCommand, SlashError> {
    let Some(action) = args.first() else {
        return Ok(SlashCommand::Learn { target: None });
    };
    if action.eq_ignore_ascii_case("put") {
        return Ok(SlashCommand::LearnPut {
            path: required_domain_path(args, "learn put")?,
        });
    }
    if action.eq_ignore_ascii_case("evidence") {
        if args.len() < 3 {
            return Err(SlashError::MissingLearnArgument { action: "evidence" });
        }
        if args.len() > 3 {
            return Err(SlashError::UnexpectedLearnArgument { action: "evidence" });
        }
        if args[1].trim().is_empty() || args[2].trim().is_empty() {
            return Err(SlashError::MissingLearnArgument { action: "evidence" });
        }
        return Ok(SlashCommand::LearnEvidence {
            id: args[1].clone(),
            reference: args[2].clone(),
        });
    }
    if action.eq_ignore_ascii_case("resume") {
        if args.len() < 2 {
            return Err(SlashError::MissingLearnArgument { action: "resume" });
        }
        if args.len() > 2 {
            return Err(SlashError::UnexpectedLearnArgument { action: "resume" });
        }
        if args[1].trim().is_empty() {
            return Err(SlashError::MissingLearnArgument { action: "resume" });
        }
        return Ok(SlashCommand::LearnResume {
            id: args[1].clone(),
        });
    }
    Ok(SlashCommand::Learn {
        target: optional_join(args),
    })
}

fn required_domain_path(args: &[String], command: &'static str) -> Result<String, SlashError> {
    if args.len() < 2 {
        return Err(SlashError::MissingArgument { command });
    }
    if args.len() > 2 {
        return Err(SlashError::UnexpectedArgument { command });
    }
    let path = &args[1];
    if path.trim().is_empty() {
        return Err(SlashError::MissingArgument { command });
    }
    Ok(path.clone())
}

fn parse_session(args: &[String]) -> Result<SessionAction, SlashError> {
    let Some(action) = args.first() else {
        return Ok(SessionAction::List);
    };
    let action_lower = action.to_ascii_lowercase();
    match action_lower.as_str() {
        "list" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedSessionArgument { action: "list" });
            }
            Ok(SessionAction::List)
        }
        "agents" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedSessionArgument { action: "agents" });
            }
            Ok(SessionAction::Agents)
        }
        "inspect" => Ok(SessionAction::Inspect {
            path: required_session_path(args, "inspect")?,
        }),
        "open" => Ok(SessionAction::Open {
            path: required_session_path(args, "open")?,
        }),
        "resume-last" | "resume_last" => {
            if args.len() > 1 {
                return Err(SlashError::UnexpectedSessionArgument {
                    action: "resume-last",
                });
            }
            Ok(SessionAction::ResumeLast)
        }
        "fork" => {
            let (source, destination) = required_session_pair(args, "fork")?;
            Ok(SessionAction::Fork {
                source,
                destination,
            })
        }
        "export" => {
            let (source, destination) = required_session_pair(args, "export")?;
            Ok(SessionAction::Export {
                source,
                destination,
            })
        }
        "import" => {
            let (source, destination) = required_session_pair(args, "import")?;
            Ok(SessionAction::Import {
                source,
                destination,
            })
        }
        "migrate" => {
            let (source, destination) = required_session_pair(args, "migrate")?;
            Ok(SessionAction::Migrate {
                source,
                destination,
            })
        }
        "archive" => {
            let path = required_session_path_with_yes(args, "archive")?;
            Ok(SessionAction::Archive { path })
        }
        "unarchive" => Ok(SessionAction::Unarchive {
            path: required_session_path(args, "unarchive")?,
        }),
        "delete" => {
            let path = required_session_path_with_yes(args, "delete")?;
            Ok(SessionAction::Delete {
                path,
                confirmed: true,
            })
        }
        "queue" => parse_session_queue(args),
        "gc" => parse_session_gc(args),
        _ => Err(SlashError::UnknownSessionAction {
            action: action.clone(),
        }),
    }
}

fn required_session_path_with_yes(
    args: &[String],
    action: &'static str,
) -> Result<String, SlashError> {
    if args.len() != 3 || args[2] != "--yes" {
        return if args.len() < 2 {
            Err(SlashError::MissingSessionPath { action })
        } else {
            Err(SlashError::MissingSessionConfirmation { action })
        };
    }
    if args[1].trim().is_empty() {
        return Err(SlashError::MissingSessionPath { action });
    }
    Ok(args[1].clone())
}

fn parse_session_queue(args: &[String]) -> Result<SessionAction, SlashError> {
    if args.len() < 6 {
        return Err(SlashError::MissingSessionPath { action: "queue" });
    }
    let ttl_ms = args[4]
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= crate::session::MAX_MAILBOX_TTL_MS)
        .ok_or(SlashError::InvalidSessionTtl)?;
    let payload_text = args[5..].join(" ");
    let payload =
        serde_json::from_str(&payload_text).map_err(|_| SlashError::InvalidSessionPayload)?;
    Ok(SessionAction::Queue {
        source: args[1].clone(),
        recipient: args[2].clone(),
        request_id: args[3].clone(),
        payload,
        ttl_ms,
    })
}

fn parse_mailbox(args: &[String]) -> Result<MailboxAction, SlashError> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    match action.to_ascii_lowercase().as_str() {
        "send" if args.len() >= 5 => {
            let ttl_ms = args[3]
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0 && *value <= crate::protocol::MAX_MAILBOX_TTL_MS)
                .ok_or(SlashError::InvalidSessionTtl)?;
            Ok(MailboxAction::Send {
                recipient_session_id: args[1].clone(),
                message_id: args[2].clone(),
                ttl_ms,
                text: args[4..].join(" "),
            })
        }
        "list" if args.len() <= 3 => {
            let after_sequence = args
                .get(1)
                .map(|value| value.parse::<u64>())
                .transpose()
                .map_err(|_| SlashError::InvalidMailboxArgument { action: "list" })?
                .unwrap_or(0);
            let limit = args
                .get(2)
                .map(|value| value.parse::<u16>())
                .transpose()
                .map_err(|_| SlashError::InvalidMailboxArgument { action: "list" })?
                .unwrap_or(64);
            if limit == 0 || limit > 64 {
                return Err(SlashError::InvalidMailboxArgument { action: "list" });
            }
            Ok(MailboxAction::List {
                after_sequence,
                limit,
            })
        }
        "ack" | "acknowledge" if args.len() == 2 => Ok(MailboxAction::Acknowledge {
            message_id: args[1].clone(),
        }),
        "claim" if args.len() == 2 => Ok(MailboxAction::Claim {
            message_id: args[1].clone(),
        }),
        "complete" if (4..=5).contains(&args.len()) => {
            let outcome = match args[2].to_ascii_lowercase().as_str() {
                "succeeded" | "success" => crate::protocol::MailboxOutcome::Succeeded,
                "failed" | "failure" => crate::protocol::MailboxOutcome::Failed,
                "abandoned" | "abandon" => crate::protocol::MailboxOutcome::Abandoned,
                _ => return Err(SlashError::InvalidMailboxOutcome),
            };
            Ok(MailboxAction::Complete {
                message_id: args[1].clone(),
                outcome,
                result: args.get(3).cloned(),
            })
        }
        _ => Err(SlashError::InvalidMailboxArgument { action: "mailbox" }),
    }
}

fn parse_session_gc(args: &[String]) -> Result<SessionAction, SlashError> {
    let mut retain_newest = None;
    let mut older_than_seconds = None;
    let mut confirm = false;
    let mut index = 1;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--yes" {
            if confirm {
                return Err(SlashError::UnexpectedSessionArgument { action: "gc" });
            }
            confirm = true;
            index += 1;
            continue;
        }
        if argument == "--retain-newest" || argument.starts_with("--retain-newest=") {
            if retain_newest.is_some() {
                return Err(SlashError::UnexpectedSessionArgument { action: "gc" });
            }
            let value = argument
                .strip_prefix("--retain-newest=")
                .map(str::to_owned)
                .or_else(|| args.get(index + 1).cloned())
                .ok_or(SlashError::InvalidSessionGcValue {
                    flag: "--retain-newest",
                })?;
            if argument == "--retain-newest" {
                index += 1;
            }
            let parsed = value
                .parse::<usize>()
                .ok()
                .filter(|value| *value <= MAX_SESSION_GC_RETAIN_NEWEST)
                .ok_or(SlashError::InvalidSessionGcValue {
                    flag: "--retain-newest",
                })?;
            retain_newest = Some(parsed);
            index += 1;
            continue;
        }
        if argument == "--older-than-seconds" || argument.starts_with("--older-than-seconds=") {
            if older_than_seconds.is_some() {
                return Err(SlashError::UnexpectedSessionArgument { action: "gc" });
            }
            let value = argument
                .strip_prefix("--older-than-seconds=")
                .map(str::to_owned)
                .or_else(|| args.get(index + 1).cloned())
                .ok_or(SlashError::InvalidSessionGcValue {
                    flag: "--older-than-seconds",
                })?;
            if argument == "--older-than-seconds" {
                index += 1;
            }
            let parsed = value
                .parse::<u64>()
                .ok()
                .filter(|value| *value <= MAX_SESSION_GC_AGE_SECONDS)
                .ok_or(SlashError::InvalidSessionGcValue {
                    flag: "--older-than-seconds",
                })?;
            older_than_seconds = Some(parsed);
            index += 1;
            continue;
        }
        return Err(SlashError::UnexpectedSessionArgument { action: "gc" });
    }
    let Some(retain_newest) = retain_newest else {
        return Err(SlashError::MissingSessionGcPolicy);
    };
    let Some(older_than_seconds) = older_than_seconds else {
        return Err(SlashError::MissingSessionGcPolicy);
    };
    if !confirm {
        return Err(SlashError::MissingSessionGcConfirmation);
    }
    if retain_newest == 0 && older_than_seconds == 0 {
        return Err(SlashError::MissingSessionGcPolicy);
    }
    Ok(SessionAction::Gc {
        policy: SessionGcPolicy {
            retain_newest,
            older_than_seconds,
            confirm,
        },
    })
}

fn required_session_path(args: &[String], action: &'static str) -> Result<String, SlashError> {
    if args.len() < 2 {
        return Err(SlashError::MissingSessionPath { action });
    }
    if args.len() > 2 {
        return Err(SlashError::UnexpectedSessionArgument { action });
    }
    if args[1].trim().is_empty() {
        return Err(SlashError::MissingSessionPath { action });
    }
    Ok(args[1].clone())
}

fn required_session_pair(
    args: &[String],
    action: &'static str,
) -> Result<(String, String), SlashError> {
    if args.len() < 3 {
        return Err(SlashError::MissingSessionPath { action });
    }
    if args.len() > 3 {
        return Err(SlashError::UnexpectedSessionArgument { action });
    }
    if args[1].trim().is_empty() || args[2].trim().is_empty() {
        return Err(SlashError::MissingSessionPath { action });
    }
    Ok((args[1].clone(), args[2].clone()))
}

fn parse_approve(
    value: Option<&String>,
    argument_count: usize,
) -> Result<ApproveDecision, SlashError> {
    if argument_count < 2 {
        return Err(SlashError::MissingArgument { command: "approve" });
    }
    if argument_count > 2 {
        return Err(SlashError::UnexpectedArgument { command: "approve" });
    }
    match value
        .map(String::as_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("once") => Ok(ApproveDecision::Once),
        Some("always") => Ok(ApproveDecision::Always),
        Some("deny") => Ok(ApproveDecision::Deny),
        _ => Err(SlashError::InvalidApproveDecision),
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

    #[test]
    fn layout_and_pane_commands_parse_to_bounded_typed_actions() {
        assert_eq!(
            parse("/layout preset learn").unwrap(),
            Some(SlashCommand::Layout {
                action: LayoutAction::Preset {
                    tab: Some(TabId::Learn),
                },
            })
        );
        assert_eq!(
            parse("/layout reset").unwrap(),
            Some(SlashCommand::Layout {
                action: LayoutAction::Reset { tab: None },
            })
        );
        assert_eq!(
            parse("/pane collapse gantt").unwrap(),
            Some(SlashCommand::Pane {
                action: PaneAction::Collapse {
                    pane: PaneId::Gantt
                },
            })
        );
        assert_eq!(
            parse("/pane terminal").unwrap(),
            Some(SlashCommand::Pane {
                action: PaneAction::Focus {
                    pane: PaneId::Terminal,
                },
            })
        );
    }

    #[test]
    fn layout_and_pane_commands_fail_closed_on_invalid_arguments() {
        assert!(matches!(
            parse("/layout preset unknown").unwrap_err(),
            SlashError::UnknownLayoutTab(_)
        ));
        assert!(matches!(
            parse("/layout save extra").unwrap_err(),
            SlashError::UnexpectedLayoutArgument { .. }
        ));
        assert!(matches!(
            parse("/pane collapse").unwrap_err(),
            SlashError::MissingPaneName { .. }
        ));
        assert!(matches!(
            parse("/pane nope").unwrap_err(),
            SlashError::UnknownPane(_)
        ));
    }

    #[test]
    fn session_gc_requires_explicit_bounded_policy_and_confirmation() {
        let command = parse("/session gc --older-than-seconds=3600 --retain-newest 3 --yes")
            .unwrap()
            .unwrap();
        assert_eq!(
            command,
            SlashCommand::Session {
                action: SessionAction::Gc {
                    policy: SessionGcPolicy {
                        retain_newest: 3,
                        older_than_seconds: 3600,
                        confirm: true,
                    },
                },
            }
        );
        assert!(matches!(
            parse("/session gc").unwrap_err(),
            SlashError::MissingSessionGcPolicy
        ));
        assert!(matches!(
            parse("/session gc --retain-newest 1 --older-than-seconds 1").unwrap_err(),
            SlashError::MissingSessionGcConfirmation
        ));
        assert!(matches!(
            parse("/session gc --retain-newest 999999 --older-than-seconds 1 --yes").unwrap_err(),
            SlashError::InvalidSessionGcValue {
                flag: "--retain-newest"
            }
        ));
        assert!(parse("/session gc --retain-newest 0 --older-than-seconds 0 --yes").is_err());
    }

    #[test]
    fn session_lifecycle_and_mailbox_commands_are_typed() {
        assert_eq!(
            parse("/session resume-last").unwrap(),
            Some(SlashCommand::Session {
                action: SessionAction::ResumeLast,
            })
        );
        assert_eq!(
            parse("/session archive child.jsonl --yes").unwrap(),
            Some(SlashCommand::Session {
                action: SessionAction::Archive {
                    path: "child.jsonl".into(),
                },
            })
        );
        assert!(matches!(
            parse("/session delete child.jsonl").unwrap_err(),
            SlashError::MissingSessionConfirmation { action: "delete" }
        ));
        assert_eq!(
            parse("/mailbox list 4 12").unwrap(),
            Some(SlashCommand::Mailbox {
                action: MailboxAction::List {
                    after_sequence: 4,
                    limit: 12,
                },
            })
        );
        assert_eq!(
            parse("/mailbox send recipient request-1 1000 hello world").unwrap(),
            Some(SlashCommand::Mailbox {
                action: MailboxAction::Send {
                    recipient_session_id: "recipient".into(),
                    message_id: "request-1".into(),
                    text: "hello world".into(),
                    ttl_ms: 1000,
                },
            })
        );
    }
}
