//! Host-owned executable hook generations. No callback receives a mutable host.
use crate::{
    extensions::{
        EXTENSION_HOOK_API_VERSION, ExtensionCatalog, LoadedExtension, MAX_EXTENSION_FRAME_BYTES,
    },
    tools::{ToolCall, ToolError, ToolRegistry},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::{
    io::{Read, Write},
    process::Child,
};

pub const MAX_HOOK_BYTES: usize = 64 * 1024;
const CHAIN_TIMEOUT: Duration = Duration::from_secs(2);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Input,
    Context,
    BeforeTool,
    AfterTool,
    SessionStart,
    AgentStart,
    AgentEnd,
    SessionClose,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LeaseIdentity {
    pub session_id: String,
    pub generation: u64,
    pub capability: String,
}

/// Clones can revoke/check an existing lease but cannot manufacture a live one
/// from the serialized identity supplied by a plugin.
#[derive(Debug, Clone)]
pub struct ExtensionLease {
    identity: LeaseIdentity,
    active: Arc<AtomicBool>,
}
impl ExtensionLease {
    fn new(session_id: &str) -> Self {
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let nonce = || RandomState::new().build_hasher().finish();
        Self {
            identity: LeaseIdentity {
                session_id: session_id.into(),
                generation,
                capability: format!("ext_{:016x}{:016x}", nonce(), nonce()),
            },
            active: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn identity(&self) -> &LeaseIdentity {
        &self.identity
    }
    pub fn revoke(&self) {
        self.active.store(false, Ordering::Release);
    }
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn check(&self) -> Result<(), ToolError> {
        if self.is_active() {
            Ok(())
        } else {
            Err(ToolError::Unsupported("stale extension generation".into()))
        }
    }
}

#[derive(Debug)]
pub struct ExtensionRuntime {
    root: PathBuf,
    workspace: PathBuf,
    catalog: ExtensionCatalog,
    lease: ExtensionLease,
    closed: bool,
    started: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum HookResult {
    Continue {},
    Transform { text: String },
    Context { instructions: String },
    Rewrite { arguments: Value },
    Deny { reason: String },
    Output { output: Value },
}

impl ExtensionRuntime {
    /// Every fallible candidate step happens while the old runtime stays live.
    pub fn prepare(
        root: &Path,
        workspace: &Path,
        session_id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, ToolError> {
        let catalog = ExtensionCatalog::load(root)
            .map_err(|e| ToolError::InvalidDefinition(e.to_string()))?;
        let runtime = Self {
            root: root.into(),
            workspace: workspace.into(),
            catalog,
            lease: ExtensionLease::new(session_id),
            closed: false,
            started: false,
        };
        let start = Instant::now();
        for extension in runtime.catalog.loaded.values() {
            if extension.manifest.api_version != EXTENSION_HOOK_API_VERSION {
                continue;
            }
            let tools: Vec<_> = extension
                .manifest
                .tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect();
            let reply = process_request(
                extension,
                workspace,
                Some(&runtime.lease),
                "initialize",
                json!({"api_version":2,"hooks":extension.manifest.hooks,"tools":extension.manifest.tools}),
                Duration::from_millis(extension.manifest.hook_timeout_ms),
                &|| cancelled() || start.elapsed() >= CHAIN_TIMEOUT,
            )?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Negotiation {
                api_version: u32,
                hooks: Vec<HookKind>,
                tools: Vec<String>,
            }
            let reply: Negotiation = serde_json::from_value(reply).map_err(ToolError::Json)?;
            if reply.api_version != 2
                || reply.hooks != extension.manifest.hooks
                || reply.tools != tools
            {
                return Err(ToolError::InvalidDefinition(
                    "extension negotiation disagrees with manifest".into(),
                ));
            }
        }
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(runtime)
    }
    pub fn lease(&self) -> ExtensionLease {
        self.lease.clone()
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn has_hook(&self, kind: HookKind) -> bool {
        self.catalog
            .loaded
            .values()
            .any(|e| e.manifest.hooks.contains(&kind))
    }
    pub fn has_hooks(&self) -> bool {
        self.catalog
            .loaded
            .values()
            .any(|e| !e.manifest.hooks.is_empty())
    }
    pub(crate) fn tool_names(&self) -> Vec<String> {
        self.catalog
            .loaded
            .values()
            .flat_map(|e| e.manifest.tools.iter().map(|t| t.name.clone()))
            .collect()
    }
    pub(crate) fn register_tools(&self, registry: &mut ToolRegistry) -> Result<(), ToolError> {
        self.catalog
            .register_with_lease(registry, Some(self.lease.clone()))
            .map_err(|e| ToolError::InvalidDefinition(e.to_string()))
    }
    fn chain(
        &self,
        kind: HookKind,
        event: &mut Value,
        cancelled: &dyn Fn() -> bool,
        mut apply: impl FnMut(HookResult, &mut Value) -> Result<bool, ToolError>,
    ) -> Result<(), ToolError> {
        self.lease.check()?;
        let start = Instant::now();
        for extension in self
            .catalog
            .loaded
            .values()
            .filter(|e| e.manifest.hooks.contains(&kind))
        {
            let reply = process_request(
                extension,
                &self.workspace,
                Some(&self.lease),
                "hooks/call",
                json!({"hook":kind,"event":event}),
                Duration::from_millis(extension.manifest.hook_timeout_ms),
                &|| cancelled() || start.elapsed() >= CHAIN_TIMEOUT,
            )?;
            let reply: HookResult = serde_json::from_value(reply).map_err(ToolError::Json)?;
            if !apply(reply, event)? {
                break;
            }
            if serde_json::to_vec(event).map_err(ToolError::Json)?.len() > MAX_HOOK_BYTES {
                return Err(ToolError::LimitExceeded("hook chain result".into()));
            }
        }
        self.lease.check()?;
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(())
    }
    pub(crate) fn input(
        &self,
        turn_id: &str,
        text: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<String, ToolError> {
        let mut event = json!({"turn_id":turn_id,"text":text});
        self.chain(HookKind::Input, &mut event, cancelled, |reply, event| {
            match reply {
                HookResult::Continue {} => {}
                HookResult::Transform { text } => {
                    check_text(&text)?;
                    event["text"] = text.into();
                }
                _ => return Err(bad_result()),
            }
            Ok(true)
        })?;
        Ok(event["text"].as_str().expect("typed text").into())
    }
    /// Request-local instructions only: native/tool/canonical history is never
    /// handed back as mutable authority or accepted from the plugin.
    pub(crate) fn context(
        &self,
        turn_id: &str,
        text: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<String, ToolError> {
        let mut event = json!({"turn_id":turn_id,"instructions":text});
        self.chain(HookKind::Context, &mut event, cancelled, |reply, event| {
            match reply {
                HookResult::Continue {} => {}
                HookResult::Context { instructions } => {
                    check_text(&instructions)?;
                    event["instructions"] = instructions.into();
                }
                _ => return Err(bad_result()),
            }
            Ok(true)
        })?;
        Ok(event["instructions"]
            .as_str()
            .expect("typed instructions")
            .into())
    }
    pub(crate) fn before_tool(
        &self,
        call: &ToolCall,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ToolCall, ToolError> {
        let mut event = json!({"call_id":call.id,"tool":call.name,"arguments":call.arguments});
        self.chain(
            HookKind::BeforeTool,
            &mut event,
            cancelled,
            |reply, event| {
                match reply {
                    HookResult::Continue {} => {}
                    HookResult::Rewrite { arguments } => {
                        if !arguments.is_object() {
                            return Err(bad_result());
                        }
                        event["arguments"] = arguments;
                    }
                    HookResult::Deny { reason } => {
                        check_text(&reason)?;
                        return Err(ToolError::InvalidArguments(format!(
                            "extension denied tool: {reason}"
                        )));
                    }
                    _ => return Err(bad_result()),
                }
                Ok(true)
            },
        )?;
        Ok(ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: event["arguments"].clone(),
        })
    }
    pub(crate) fn after_tool(
        &self,
        call: &ToolCall,
        output: &Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        let mut event =
            json!({"call_id":call.id,"tool":call.name,"arguments":call.arguments,"output":output});
        self.chain(
            HookKind::AfterTool,
            &mut event,
            cancelled,
            |reply, event| {
                match reply {
                    HookResult::Continue {} => {}
                    HookResult::Output { output } => event["output"] = output,
                    _ => return Err(bad_result()),
                }
                Ok(true)
            },
        )?;
        Ok(crate::security::redact_json(&event["output"], &[]))
    }
    pub(crate) fn notify(
        &self,
        kind: HookKind,
        turn_id: Option<&str>,
        reason: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Vec<String> {
        self.notify_with_lease(&self.lease, kind, turn_id, reason, cancelled)
    }
    fn notify_with_lease(
        &self,
        lease: &ExtensionLease,
        kind: HookKind,
        turn_id: Option<&str>,
        reason: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Vec<String> {
        // Lifecycle failure is isolated per extension; one bad handler cannot
        // suppress cleanup notifications to subsequent extensions.
        let mut errors = Vec::new();
        let start = Instant::now();
        for extension in self
            .catalog
            .loaded
            .values()
            .filter(|e| e.manifest.hooks.contains(&kind))
        {
            let result = process_request(
                extension,
                &self.workspace,
                Some(lease),
                "hooks/call",
                json!({"hook":kind,"event":{"turn_id":turn_id,"reason":reason}}),
                Duration::from_millis(extension.manifest.hook_timeout_ms),
                &|| cancelled() || start.elapsed() >= CHAIN_TIMEOUT,
            )
            .and_then(|result| {
                let result: HookResult = serde_json::from_value(result).map_err(ToolError::Json)?;
                if matches!(result, HookResult::Continue {}) {
                    Ok(())
                } else {
                    Err(bad_result())
                }
            });
            if let Err(error) = result {
                errors.push(format!("{}: {error}", extension.manifest.name));
            }
        }
        errors
    }
    pub(crate) fn start(&mut self, reason: &str) -> Vec<String> {
        if self.started {
            return Vec::new();
        }
        self.started = true;
        self.notify(HookKind::SessionStart, None, reason, &|| false)
    }
    pub(crate) fn close(&mut self, reason: &str) -> Vec<String> {
        if self.closed {
            return Vec::new();
        }
        self.closed = true;
        self.lease.revoke();
        // A terminal notification has a private, one-call-only lease. Old tool
        // and callback clones are already revoked while close handlers run.
        let closing = ExtensionLease {
            identity: self.lease.identity.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let errors =
            self.notify_with_lease(&closing, HookKind::SessionClose, None, reason, &|| false);
        closing.revoke();
        errors
    }
}
impl Drop for ExtensionRuntime {
    fn drop(&mut self) {
        if self.started && !self.closed {
            let _ = self.close("dispose");
        }
        self.lease.revoke();
    }
}
fn bad_result() -> ToolError {
    ToolError::InvalidArguments("hook returned an invalid result for this event".into())
}
fn check_text(text: &str) -> Result<(), ToolError> {
    if text.len() > MAX_HOOK_BYTES || text.contains('\0') {
        Err(bad_result())
    } else {
        Ok(())
    }
}

/// One process per call. Both pipes and child exit are inside the same deadline.
/// Pipe I/O is nonblocking and owned by this call; no worker can outlive it.
pub(crate) fn process_request(
    extension: &LoadedExtension,
    workspace: &Path,
    lease: Option<&ExtensionLease>,
    method: &str,
    params: Value,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value, ToolError> {
    if cancelled() {
        return Err(ToolError::Cancelled);
    }
    if let Some(lease) = lease {
        lease.check()?;
    }
    let api2 = extension.manifest.api_version == EXTENSION_HOOK_API_VERSION;
    if api2 && lease.is_none() {
        return Err(ToolError::Unsupported("API 2 requires a host lease".into()));
    }
    let request_id = if api2 {
        NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
    } else {
        1
    };
    let mut request = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":params});
    if api2 {
        request["capability"] =
            serde_json::to_value(lease.expect("checked").identity()).map_err(ToolError::Json)?;
    }
    let mut encoded = serde_json::to_vec(&request).map_err(ToolError::Json)?;
    let limit = if method == "tools/call" {
        MAX_EXTENSION_FRAME_BYTES
    } else {
        MAX_HOOK_BYTES
    };
    if encoded.len() > limit {
        return Err(ToolError::LimitExceeded("extension request frame".into()));
    }
    encoded.push(b'\n');
    let mut command = Command::new(&extension.executable);
    command
        .args(&extension.manifest.args)
        .current_dir(&extension.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear();
    for (key, value) in crate::security::child_environment() {
        command.env(key, value);
    }
    command
        .env("ZENPI_EXTENSION_NAME", &extension.manifest.name)
        .env("ZENPI_WORKSPACE", workspace);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let frame = exchange_frame(&mut command, &encoded, limit, timeout, lease, cancelled)?;
    if cancelled() {
        return Err(ToolError::Cancelled);
    }
    if let Some(lease) = lease {
        lease.check()?;
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        jsonrpc: String,
        id: u64,
        #[serde(default)]
        capability: Option<LeaseIdentity>,
        result: Value,
        #[serde(default)]
        error: Option<Value>,
    }
    let strict: StrictValue = serde_json::from_slice(&frame).map_err(ToolError::Json)?;
    let reply: Reply = serde_json::from_value(strict.0).map_err(ToolError::Json)?;
    if reply.jsonrpc != "2.0"
        || reply.id != request_id
        || (api2 && reply.capability.as_ref() != lease.map(ExtensionLease::identity))
        || reply.error.is_some()
    {
        return Err(ToolError::CommandFailed(
            "invalid extension reply identity or error".into(),
        ));
    }
    Ok(reply.result)
}
// A pipe retained by a setsid descendant does not become EOF/broken-pipe
// when we kill the original process group. Own both nonblocking endpoints in
// the supervisor itself so deadline, cancellation, revocation and unwind do
// not depend on any descendant closing its copy or on joining blocked I/O.
#[cfg(unix)]
fn exchange_frame(
    command: &mut Command,
    encoded: &[u8],
    limit: usize,
    timeout: Duration,
    lease: Option<&ExtensionLease>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, ToolError> {
    let started = Instant::now();
    let mut owned = OwnedChild(command.spawn()?, false);
    let stdin = owned
        .0
        .stdin
        .take()
        .ok_or_else(|| ToolError::CommandFailed("extension stdin".into()))?;
    let mut stdout = owned
        .0
        .stdout
        .take()
        .ok_or_else(|| ToolError::CommandFailed("extension stdout".into()))?;
    crate::tools::make_pipe_nonblocking(&stdin)?;
    crate::tools::make_pipe_nonblocking(&stdout)?;
    let mut stdin = Some(stdin);
    let mut written = 0;
    let mut frame = Vec::new();
    let mut eof = false;
    let mut status = None;
    loop {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        if let Some(lease) = lease {
            lease.check()?;
        }
        if started.elapsed() >= timeout {
            return Err(ToolError::CommandTimeout(timeout.as_millis() as u64));
        }
        let mut progress = false;
        if let Some(pipe) = &mut stdin {
            let end = encoded.len().min(written + 8192);
            match pipe.write(&encoded[written..end]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "extension stdin closed",
                    )
                    .into());
                }
                Ok(count) => {
                    written += count;
                    progress = true;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
            if written == encoded.len() {
                stdin = None;
            }
        }
        if !eof {
            let mut buffer = [0_u8; 8192];
            let remaining = buffer.len().min(limit + 1 - frame.len());
            match stdout.read(&mut buffer[..remaining]) {
                Ok(0) => {
                    eof = true;
                    progress = true;
                }
                Ok(count) => {
                    frame.extend_from_slice(&buffer[..count]);
                    if frame.len() > limit {
                        return Err(ToolError::LimitExceeded("extension response frame".into()));
                    }
                    progress = true;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
        if status.is_none() {
            status = owned.0.try_wait()?;
        }
        if let Some(status) = status
            && stdin.is_none()
            && eof
        {
            return if status.success() {
                Ok(frame)
            } else {
                Err(ToolError::CommandFailed(format!(
                    "extension exited with {status}"
                )))
            };
        }
        if !progress {
            std::thread::sleep(
                Duration::from_millis(5).min(timeout.saturating_sub(started.elapsed())),
            );
        }
    }
}

// Match the command owner's fail-closed platform boundary. Blocking anonymous
// pipe workers cannot provide this contract; a native cancellable pipe adapter
// is required before enabling executable extensions on another platform.
#[cfg(not(unix))]
fn exchange_frame(
    _command: &mut Command,
    _encoded: &[u8],
    _limit: usize,
    _timeout: Duration,
    _lease: Option<&ExtensionLease>,
    _cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, ToolError> {
    Err(ToolError::Unsupported(
        "cancellable extension pipe I/O is unavailable on this platform".into(),
    ))
}

#[cfg(unix)]
struct OwnedChild(Child, bool);
#[cfg(unix)]
impl OwnedChild {
    fn terminate(&mut self) {
        if self.1 {
            return;
        }
        self.1 = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(unix)]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

// Reject duplicate members before converting any hook reply to Value. In
// particular, a second action key must not erase an earlier deny.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate extension reply key"));
                    }
                    let value = map.next_value::<StrictValue>()?;
                    values.insert(key, value.0);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(all(test, unix))]
mod pipe_deadline_tests {
    use super::*;
    use std::fs;

    // Write data for an installed interpreter, not a new executable image.
    // Executing a newly linked image (or a new shebang script) can consume the
    // 800 ms host deadline before fixture entry on macOS. The helper launches
    // Python directly below; there is no warmup, retry, or deadline adjustment.
    fn fixture_executable(directory: &Path) {
        use sha2::Digest;
        const SOURCE: &str = r#"import os
import sys
import time


def phase(name):
    now = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
    with open("phases.log", "a") as out:
        out.write(f"{now // 1_000_000_000}.{now % 1_000_000_000:09d} pid={os.getpid()} {name}\n")


def pid_file(name):
    with open(name, "w") as out:
        out.write(str(os.getpid()))


phase(f"python-parent-entry interpreter={sys.executable}")
with open("pipe-mode") as source:
    mode = source.read()
if mode not in ("stdin", "stdout"):
    raise ValueError("invalid fixture pipe mode")
pid_file("parent.pid")
phase("fork-start")
child = os.fork()
if child == 0:
    os.setsid()
    os.close(1 if mode == "stdin" else 0)
    phase("python-holder-entry")
    pid_file("escaped.pid")
    phase("escaped-ready")
    time.sleep(10)
    os._exit(0)
phase("fork-return")
while not os.path.exists("escaped.pid"):
    time.sleep(0.001)
os._exit(0)
"#;
        let executable = directory.join("plugin");
        fs::write(&executable, SOURCE).unwrap();
        println!(
            "interpreted_fixture_setup interpreter=python3 source_bytes={} source_sha256={:x}",
            SOURCE.len(),
            sha2::Sha256::digest(SOURCE.as_bytes())
        );
    }

    fn fixture_phase(path: &Path, name: &str) {
        use std::io::Write;
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime initializes the supplied valid timespec.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) },
            0
        );
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}.{:09} pid={} {}",
            now.tv_sec,
            now.tv_nsec,
            std::process::id(),
            name
        )
        .unwrap();
    }

    // Drop before the TempDir on return or unwind. The child may never get to
    // its own println after the outer watchdog, so the parent preserves evidence.
    struct FixturePhaseLog(PathBuf);
    impl Drop for FixturePhaseLog {
        fn drop(&mut self) {
            use std::io::Read;
            use std::os::unix::fs::OpenOptionsExt;
            const MAX_PHASE_BYTES: usize = 8 * 1024;
            let snapshot = (|| -> std::io::Result<_> {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(self.0.join("phases.log"))?;
                if !file.metadata()?.is_file() {
                    return Err(std::io::Error::other("phase log is not a regular file"));
                }
                let mut bytes = Vec::new();
                file.take((MAX_PHASE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                let truncated = bytes.len() > MAX_PHASE_BYTES;
                bytes.truncate(MAX_PHASE_BYTES);
                Ok((bytes, truncated))
            })();
            match snapshot {
                Ok((bytes, truncated)) => println!(
                    "fixture_phase_snapshot dir={:?} bytes={} truncated={truncated} phases={}",
                    self.0,
                    bytes.len(),
                    String::from_utf8_lossy(&bytes)
                ),
                Err(error) => println!(
                    "fixture_phase_snapshot dir={:?} unavailable={error}",
                    self.0
                ),
            }
        }
    }

    struct FixturePids(PathBuf);
    impl Drop for FixturePids {
        fn drop(&mut self) {
            // Only IDs written by this test's own children in its private directory.
            for name in ["escaped.pid"] {
                if let Ok(text) = fs::read_to_string(self.0.join(name))
                    && let Ok(pid) = text.parse::<i32>()
                    && pid > 1
                {
                    // SAFETY: these are the fixture's recorded process IDs; no broad process scan.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
        }
    }

    fn isolated_case(api: u32, pipe: &str, stop: &str) -> bool {
        let dir = tempfile::tempdir().unwrap();
        let ext = dir.path().join("fixture");
        fs::create_dir(&ext).unwrap();
        let _phase_log = FixturePhaseLog(ext.clone());
        fs::write(ext.join("pipe-mode"), pipe).unwrap();
        fixture_executable(&ext);
        fs::write(ext.join("extension.toml"), format!(
            "name='fixture'\nversion='1.0.0'\napi_version={api}\nexecutable='plugin'\n[permissions]\nworkspace_read=true\n[[tools]]\nname='fixture_echo'\ndescription='pipe fixture'\nside_effect='read_only'\ninput_schema={{type='object'}}\n"
        )).unwrap();
        let cleanup = FixturePids(ext.clone());
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "extension_runtime::pipe_deadline_tests::pipe_probe_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ZENPI_PIPE_CASE_DIR", dir.path())
            .env("ZENPI_PIPE_CASE_API", api.to_string())
            .env("ZENPI_PIPE_CASE_STOP", stop)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let timed_out = loop {
            if helper.try_wait().unwrap().is_some() {
                break false;
            }
            if start.elapsed() >= Duration::from_secs(2) {
                break true;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let pids = ["parent.pid", "escaped.pid"]
            .map(|p| fs::read_to_string(ext.join(p)).unwrap_or_default());
        // First release only fixture descendants. A broken host's blocked worker
        // can then unwind; kill only this helper if it still cannot finish.
        drop(cleanup);
        if timed_out {
            let grace = Instant::now();
            while helper.try_wait().unwrap().is_none()
                && grace.elapsed() < Duration::from_millis(500)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            if helper.try_wait().unwrap().is_none() {
                helper.kill().unwrap();
            }
        }
        let output = helper.wait_with_output().unwrap();
        println!(
            "api={api} pipe={pipe} stop={stop} outer_timeout={timed_out} elapsed_ms={} fixture_pids={pids:?} status={} stdout={} stderr={}",
            start.elapsed().as_millis(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        !timed_out && output.status.success()
    }

    #[test]
    fn escaped_pipe_timeout_is_bounded() {
        let mut failures = Vec::new();
        for api in [1, 2] {
            for pipe in ["stdout", "stdin"] {
                if !isolated_case(api, pipe, "timeout") {
                    failures.push((api, pipe));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "escaped pipe timeout failures: {failures:?}"
        );
    }

    #[test]
    fn escaped_pipe_cancel_is_bounded() {
        let mut failures = Vec::new();
        for api in [1, 2] {
            for pipe in ["stdout", "stdin"] {
                if !isolated_case(api, pipe, "cancel") {
                    failures.push((api, pipe));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "escaped pipe cancel failures: {failures:?}"
        );
    }

    #[test]
    fn escaped_pipe_revoke_is_bounded() {
        for pipe in ["stdout", "stdin"] {
            assert!(isolated_case(2, pipe, "revoke"), "{pipe}");
        }
    }

    #[test]
    fn escaped_pipe_unwind_closes_owned_endpoints() {
        for pipe in ["stdout", "stdin"] {
            assert!(isolated_case(2, pipe, "panic"), "{pipe}");
        }
    }

    #[test]
    #[ignore = "subprocess helper with parent-owned deadline and fixture cleanup"]
    fn pipe_probe_child() {
        let dir = PathBuf::from(std::env::var_os("ZENPI_PIPE_CASE_DIR").unwrap());
        fixture_phase(&dir.join("fixture/phases.log"), "helper-entry");
        let api: u32 = std::env::var("ZENPI_PIPE_CASE_API")
            .unwrap()
            .parse()
            .unwrap();
        let stop = std::env::var("ZENPI_PIPE_CASE_STOP").unwrap();
        let catalog = ExtensionCatalog::load(&dir).unwrap();
        let mut ext = catalog.loaded.get("fixture").unwrap().clone();
        // This unit fixture exercises process_request's pipe ownership, not
        // catalog executable-path confinement. Keep the catalog load above,
        // then invoke the installed interpreter with the private script as
        // data, so a new fixture inode is never the executable passed to spawn.
        // Missing Python is a failed prerequisite, never a skipped/passed case.
        ext.executable = PathBuf::from("/usr/bin/python3");
        ext.manifest.args = vec!["plugin".into()];
        let pipe = fs::read_to_string(ext.directory.join("pipe-mode")).unwrap();
        let lease = ExtensionLease::new("isolated-pipe-fixture");
        let count_fds = || fs::read_dir("/dev/fd").unwrap().count();
        let before_fds = count_fds();
        let start = Instant::now();
        fixture_phase(&ext.directory.join("phases.log"), "host-start");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_request(
                &ext,
                &dir,
                (api == 2).then_some(&lease),
                "tools/call",
                json!({"text": "x".repeat(if pipe == "stdin" { 900_000 } else { 10 })}),
                Duration::from_millis(if stop == "timeout" { 800 } else { 1000 }),
                &|| {
                    let ready = fs::read_to_string(ext.directory.join("escaped.pid"))
                        .is_ok_and(|p| p.parse::<i32>().is_ok());
                    if ready && start.elapsed() >= Duration::from_millis(200) {
                        if stop == "revoke" {
                            lease.revoke();
                        }
                        assert_ne!(stop, "panic", "fixture cancellation predicate unwind");
                        return stop == "cancel";
                    }
                    false
                },
            )
        }));
        fixture_phase(&ext.directory.join("phases.log"), "host-return");
        println!(
            "actual_host_outcome={outcome:?} phases={}",
            fs::read_to_string(ext.directory.join("phases.log")).unwrap()
        );
        assert!(
            ext.directory.join("escaped.pid").is_file(),
            "fixture never escaped"
        );
        let parent: i32 = fs::read_to_string(ext.directory.join("parent.pid"))
            .unwrap()
            .parse()
            .unwrap();
        let escaped: i32 = fs::read_to_string(ext.directory.join("escaped.pid"))
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: signal 0 only queries the private fixture's recorded PIDs.
        assert_eq!(
            unsafe { libc::kill(parent, 0) },
            -1,
            "direct child not reaped"
        );
        assert_eq!(
            unsafe { libc::kill(escaped, 0) },
            0,
            "escaped endpoint holder must still be live"
        );
        // SAFETY: query only this fixture's recorded, still-live descendant.
        assert_eq!(
            unsafe { libc::getsid(escaped) },
            escaped,
            "holder did not escape the process group"
        );
        let after_fds = count_fds();
        assert_eq!(before_fds, after_fds, "host pipe descriptor leak");
        println!(
            "inner_elapsed_ms={} fd_count={before_fds}->{after_fds} escaped_still_alive=true outcome={outcome:?}",
            start.elapsed().as_millis()
        );
        assert!(start.elapsed() < Duration::from_millis(1500));
        if stop == "panic" {
            assert!(outcome.is_err());
            return;
        }
        let result = outcome.unwrap();
        match stop.as_str() {
            "timeout" => assert!(matches!(result, Err(ToolError::CommandTimeout(800)))),
            "cancel" => assert!(matches!(result, Err(ToolError::Cancelled))),
            "revoke" => {
                assert!(matches!(result, Err(ToolError::Unsupported(ref s)) if s.contains("stale")))
            }
            _ => panic!("unknown fixture mode"),
        }
    }
}
