//! floti-desktop: tools that open Floti's views for the user, served to the
//! host's sessions on the agent tools listener at `/mcp/desktop`, and carried
//! out in an attached window.
//!
//! The name, route, instructions and every tool's description and input
//! schema come from `desktop_tools.json`. `desktop_open` opens one view: a
//! file's tab at a line, the Files view on a folder, a project's page, the
//! History palette with a search typed in, or a Settings section.
//! `desktop_settings` lists Floti's settings and `desktop_set` changes them:
//! the settings, what each takes and what a session may do to it are the
//! window's (its settings table), so the host passes those calls on as they
//! are and checks only their shape.
//!
//! `desktop_status` and `desktop_log` diagnose Floti. The host answers its
//! own part with no window ([`crate::status`], and the tail of its own
//! `host.log`), then asks the window for the part only it can tell: the
//! desktop's version, the hosts it knows with how each is connected and its
//! last error, and main's log. With no window attached, or one that leaves
//! or fails, the answer is the host's part and names what only a window can
//! tell. Every string in either part has what is shaped like a secret hidden
//! ([`hide_secrets`]), so no answer holds a token, key or credential.
//!
//! **The host has no window.** Each call is relayed ([`DesktopRelay`]) to an
//! attached client that declared `desktop`, the one the relay chooses for the
//! caller, and that client's answer, an MCP tool result, is the tool's answer
//! as it is.
//!
//! **Paths are the host's to judge.** Before anything is relayed, a relative
//! path is resolved against the caller's folder, folded, and must sit under a
//! folder the host knows (`KnownRoots::holding`), as a file where a file is
//! asked for, as a folder where a folder is, and as either for a project. A
//! path that fails is refused
//! with the sentence saying why, and the window is never asked: nothing
//! opens. The window is told the known folder holding the path and the path,
//! as a [`DesktopOpen`].
//!
//! **No window, no family.** It is on while the `desktopMcp` setting is, and
//! a launch is given it only while a window that takes its calls is
//! attached: a session started with none has no `floti-desktop` entry and
//! its token is answered 404 on this route. A session that was given it
//! keeps the route; while no window is attached its calls fail, saying so.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, OnceLock, Weak};

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::agent_desktop::{AgentDesktopOutcome, DesktopOpen};
use floti_protocol::{PROTOCOL_VERSION, Payload};
use serde::Deserialize;
use serde_json::{Value, json};

use super::features::{DesktopCaller, DesktopRelay};
use super::hosted::{Hosted, session_id};
use crate::agent_config::normalize;
use crate::agent_tools::{
    CallToolResult, Caller, ContentBlock, JsonObject, Tool, ToolAnnotations, ToolFamily, ToolFuture,
};
use crate::host_log::{LOG_FILE, TAIL_MOST, hide_secrets, tail};
use crate::platform::folders::Folders;
use crate::plugins::WeakRegistry;
use crate::session::SessionId;
use crate::status::{self, HostStatus, StatusFolders, StatusParts, ToolFamiliesStatus};
use crate::workspace::roots::{KnownRoot, KnownRoots};
use crate::workspace::settings::SettingsService;
use crate::workspace::setup::setup_status;

#[derive(Debug, Deserialize)]
struct Table {
    name: String,
    path: String,
    instructions: String,
    tools: Vec<Spec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Spec {
    name: String,
    description: String,
    input_schema: JsonObject,
    /// It changes nothing: listed with `readOnlyHint`.
    #[serde(default)]
    read_only: bool,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("desktop_tools.json"))
        .expect("desktop_tools.json is the desktop tools' table")
});

/// The family's name, as `desktop_tools.json` gives it.
pub(crate) const DESKTOP_FAMILY: &str = "floti-desktop";

const DESKTOP_OPEN: &str = "desktop_open";
const DESKTOP_SETTINGS: &str = "desktop_settings";
const DESKTOP_SET: &str = "desktop_set";
const DESKTOP_STATUS: &str = "desktop_status";
const DESKTOP_LOG: &str = "desktop_log";

/// The command that runs `desktop_open` from a session's shell.
pub(crate) const OPEN_COMMAND: &str = "floti desktop open";
/// The command that runs `desktop_settings` from a session's shell.
pub(crate) const SETTINGS_COMMAND: &str = "floti desktop settings";
/// The command that runs `desktop_set` from a session's shell.
pub(crate) const SET_COMMAND: &str = "floti desktop set";
/// The command that runs `desktop_status` from a session's shell.
pub(crate) const STATUS_COMMAND: &str = "floti desktop status";
/// The command that runs `desktop_log` from a session's shell.
pub(crate) const LOG_COMMAND: &str = "floti desktop log";

/// How many lines `desktop_log` answers of each log when not told.
const LOG_LINES: usize = 100;

/// What only a window can tell, as `desktop_status` names it when no window
/// answers.
const WINDOW_PARTS: [&str; 3] = [
    "the desktop's version",
    "the hosts the window knows with how each is connected and its last error",
    "where main's log is",
];

/// The answer to a call from a token whose session has not started yet.
const NO_SESSION_YET: &str = "Floti cannot tell which session this call is from yet - it is still starting. Try again in a moment.";

/// The answer while the host is still assembling what the tools read.
const STILL_STARTING: &str =
    "Floti cannot answer that right now - it is still starting up. Try again in a moment.";

/// The answer for a token whose session the host has no record of.
const NO_RECORD: &str = "Floti has no record of this session, which should not happen while it is running, so it cannot tell what your folder is.";

/// How many sessions above the caller are looked at: a guard against a
/// record that names itself somewhere above, never a limit a real tree meets.
const LINEAGE_MOST: usize = 32;

/// What the host knows of one of its sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionFacts {
    /// Its working directory.
    pub cwd: String,
    /// The session that started it with an agent tool, if one did.
    pub parent: Option<u64>,
    /// The fingerprint of the device that started it, if a device did.
    pub starter: Option<String>,
}

/// What the tools read, from what the host already knows.
pub(crate) trait DesktopWorld: Send + Sync + 'static {
    /// Session `id`'s folder, parent and starting device, none for a
    /// session the host has no record of, or the sentence saying why it
    /// cannot look (the host is stopping). Blocking: it reads the store.
    fn session(&self, id: u64) -> Result<Option<SessionFacts>, String>;

    /// The deepest known folder holding `path`, or the sentence naming the
    /// rule. Blocking: it reads the store.
    fn holding(&self, path: &Path) -> Result<KnownRoot, String>;

    /// The deepest workspace or project holding `path`, or the sentence
    /// saying there is none. Blocking: it reads the store.
    fn project_holding(&self, path: &Path) -> Result<KnownRoot, String>;

    /// The host's own part of `desktop_status`, or the sentence saying why
    /// it cannot be had. Blocking: it asks each agent CLI its version.
    fn status(&self) -> Result<HostStatus, String>;

    /// The host's own log file.
    fn host_log(&self) -> PathBuf;
}

/// The family: its switch, the `desktopMcp` setting as last written, the
/// relay its calls go through, and the world, once the host has built it.
pub(crate) struct DesktopTools {
    on: AtomicBool,
    relay: Arc<DesktopRelay>,
    world: OnceLock<Arc<dyn DesktopWorld>>,
}

impl DesktopTools {
    /// Off, with no world: the host switches it as its settings say once it
    /// has read them.
    pub fn new(relay: Arc<DesktopRelay>) -> Self {
        Self {
            on: AtomicBool::new(false),
            relay,
            world: OnceLock::new(),
        }
    }

    /// Switches it on or off; a request reads it, so it takes effect at once.
    pub fn switch(&self, on: bool) {
        self.on.store(on, Ordering::SeqCst);
    }

    /// What the tools read from now on. Only the first call counts.
    pub fn answer_from(&self, world: Arc<dyn DesktopWorld>) {
        let _ = self.world.set(world);
    }

    async fn open(&self, session: u64, arguments: JsonObject) -> CallToolResult {
        let Some(world) = self.world.get().cloned() else {
            return problem(STILL_STARTING);
        };
        let resolved =
            tokio::task::spawn_blocking(move || resolve_call(world.as_ref(), session, &arguments))
                .await;
        let (caller, open) = match resolved {
            Ok(Ok(resolved)) => resolved,
            Ok(Err(refused)) => return problem(&refused),
            Err(_) => return problem(STILL_STARTING),
        };
        let arguments = match to_payload(&open) {
            Ok(arguments) => arguments,
            Err(err) => return problem(&format!("Floti could not pass that call on: {err}.")),
        };
        tool_result(self.relay.call(&caller, DESKTOP_OPEN, arguments).await)
    }

    /// `desktop_settings` and `desktop_set`: checked for their shape and
    /// passed on to the window as they are, as the caller asked them.
    async fn pass_on(
        &self,
        session: u64,
        tool: &'static str,
        arguments: JsonObject,
    ) -> CallToolResult {
        if let Err(refused) = settings_shape(tool, &arguments) {
            return problem(&refused);
        }
        let Some(world) = self.world.get().cloned() else {
            return problem(STILL_STARTING);
        };
        let caller = tokio::task::spawn_blocking(move || {
            let mine = world.session(session)?.ok_or(NO_RECORD)?;
            Ok::<_, String>(caller_of(world.as_ref(), session, mine))
        })
        .await;
        let caller = match caller {
            Ok(Ok(caller)) => caller,
            Ok(Err(refused)) => return problem(&refused),
            Err(_) => return problem(STILL_STARTING),
        };
        let arguments = match to_payload(&arguments) {
            Ok(arguments) => arguments,
            Err(err) => return problem(&format!("Floti could not pass that call on: {err}.")),
        };
        tool_result(self.relay.call(&caller, tool, arguments).await)
    }

    /// `desktop_status`: the host's part, then the window's when one answers.
    async fn status(&self, session: u64, arguments: JsonObject) -> CallToolResult {
        if let Some(stray) = arguments.keys().next() {
            return problem(&format!("{DESKTOP_STATUS} takes nothing, not `{stray}`."));
        }
        let Some(world) = self.world.get().cloned() else {
            return problem(STILL_STARTING);
        };
        let looked = tokio::task::spawn_blocking(move || {
            (world.status(), caller_for(world.as_ref(), session))
        })
        .await;
        let (host, caller) = match looked {
            Ok((Ok(host), caller)) => (host, caller),
            Ok((Err(refused), _)) => return problem(&refused),
            Err(_) => return problem(STILL_STARTING),
        };
        let window = self.window_part(caller, DESKTOP_STATUS, &json!({})).await;
        status_answer(&host, window)
    }

    /// `desktop_log`: the latest lines of the host's log, then of main's
    /// when a window answers.
    async fn log(&self, session: u64, arguments: JsonObject) -> CallToolResult {
        let (lines, word) = match log_arguments(&arguments) {
            Ok(asked) => asked,
            Err(refused) => return problem(&refused),
        };
        let Some(world) = self.world.get().cloned() else {
            return problem(STILL_STARTING);
        };
        let read_word = word.clone();
        let looked = tokio::task::spawn_blocking(move || {
            let path = world.host_log();
            let read = tail(&path, lines, read_word.as_deref())
                .map_err(|err| format!("Floti could not read {}: {err}.", path.display()));
            (path, read, caller_for(world.as_ref(), session))
        })
        .await;
        let Ok((path, host, caller)) = looked else {
            return problem(STILL_STARTING);
        };
        let mut asked = json!({ "lines": lines });
        if let Some(word) = &word {
            asked["word"] = json!(word);
        }
        let window = self.window_part(caller, DESKTOP_LOG, &asked).await;
        log_answer(&path, host, word.as_deref(), window)
    }

    /// What the window answers `tool` with for `caller`: its sentence and
    /// its structured part, or why there is none.
    async fn window_part(
        &self,
        caller: Result<DesktopCaller, String>,
        tool: &str,
        arguments: &Value,
    ) -> Result<WindowPart, String> {
        let caller = caller?;
        if !self.relay.has_window() {
            return Err("no Floti window is attached".to_owned());
        }
        let arguments = to_payload(arguments).map_err(|err| err.to_string())?;
        match self.relay.call(&caller, tool, arguments).await {
            AgentDesktopOutcome::Done { value } => window_part(&value),
            AgentDesktopOutcome::Failed { message } => Err(message),
        }
    }
}

/// What a window answered a diagnosis call with: its sentence and its
/// structured part, every string with its secrets hidden.
#[derive(Debug, Clone, PartialEq)]
struct WindowPart {
    text: String,
    value: Value,
}

/// The window's tool result `value` as its part, or why it is not one.
fn window_part(value: &Payload) -> Result<WindowPart, String> {
    let result: Value = from_payload(value).map_err(|err| err.to_string())?;
    let text = |at: usize| result["content"][at]["text"].as_str();
    // The sentence is hidden as text; the JSON only once parsed, string by
    // string, since a hidden value may run into the quote that ends it.
    let sentence = |at: usize| text(at).map(|text| hide_secrets(text).into_owned());
    if result["isError"].as_bool() == Some(true) {
        return Err(sentence(0).unwrap_or_else(|| "Floti's window could not answer.".to_owned()));
    }
    let structured = text(1)
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .ok_or_else(|| "Floti's window answered without its part.".to_owned())?;
    Ok(WindowPart {
        text: sentence(0).unwrap_or_default(),
        value: hidden(structured),
    })
}

/// `value` with every string in it, at any depth, its secrets hidden.
fn hidden(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(hide_secrets(&text).into_owned()),
        Value::Array(items) => Value::Array(items.into_iter().map(hidden).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, hidden(value)))
                .collect(),
        ),
        other => other,
    }
}

/// Who `session` is to the relay, or why the host cannot tell.
fn caller_for(world: &dyn DesktopWorld, session: u64) -> Result<DesktopCaller, String> {
    let mine = world.session(session)?.ok_or(NO_RECORD)?;
    Ok(caller_of(world, session, mine))
}

/// `desktop_status`'s answer: the host's lines and the window's, or the
/// parts only a window can tell and why none did; as JSON beside it.
fn status_answer(host: &HostStatus, window: Result<WindowPart, String>) -> CallToolResult {
    let host_lines = status::lines(host)
        .iter()
        .map(|line| hide_secrets(line).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let mut structured =
        json!({ "host": hidden(serde_json::to_value(host).unwrap_or(Value::Null)) });
    let window_text = match window {
        Ok(part) => {
            structured["window"] = part.value;
            part.text
        }
        Err(why) => {
            structured["window"] = Value::Null;
            structured["needsWindow"] = json!(WINDOW_PARTS);
            structured["windowProblem"] = json!(why);
            let [version, hosts, log] = WINDOW_PARTS;
            format!(
                "Only a Floti window can tell {version}, {hosts}, and {log}: {}.",
                why.trim_end_matches('.')
            )
        }
    };
    told(
        &format!("This host:\n{host_lines}\n\nFloti's window:\n{window_text}"),
        &structured,
    )
}

/// `desktop_log`'s answer: the host's lines from `path`, then main's, or
/// why there are none; as JSON beside it.
fn log_answer(
    path: &Path,
    host: Result<Option<Vec<String>>, String>,
    word: Option<&str>,
    window: Result<WindowPart, String>,
) -> CallToolResult {
    let holding = word
        .map(|word| format!(" holding \"{word}\""))
        .unwrap_or_default();
    let mut structured = json!({ "host": { "path": path.display().to_string() } });
    let host_text = match host {
        Ok(Some(lines)) => {
            let text = if lines.is_empty() {
                format!("{LOG_FILE} ({}) has no lines{holding}.", path.display())
            } else {
                format!(
                    "{LOG_FILE} ({}), its last {} lines{holding}:\n{}",
                    path.display(),
                    lines.len(),
                    lines.join("\n")
                )
            };
            structured["host"]["lines"] = json!(lines);
            text
        }
        Ok(None) => {
            structured["host"]["lines"] = json!([]);
            format!("{LOG_FILE} ({}) is not there yet.", path.display())
        }
        Err(why) => {
            structured["host"]["problem"] = json!(why);
            why
        }
    };
    let main_text = match window {
        Ok(part) => {
            structured["main"] = part.value;
            part.text
        }
        Err(why) => {
            structured["main"] = Value::Null;
            structured["windowProblem"] = json!(why);
            format!(
                "Main's log is read through a Floti window: {}.",
                why.trim_end_matches('.')
            )
        }
    };
    told(&format!("{host_text}\n\n{main_text}"), &structured)
}

/// How many lines `desktop_log` was asked for and the word they hold, or
/// the sentence saying why not.
fn log_arguments(arguments: &JsonObject) -> Result<(usize, Option<String>), String> {
    if let Some(stray) = arguments
        .keys()
        .find(|key| !["lines", "word"].contains(&key.as_str()))
    {
        return Err(format!(
            "`{stray}` is not something {DESKTOP_LOG} takes: it takes lines and word."
        ));
    }
    let lines = match arguments.get("lines") {
        None => LOG_LINES,
        Some(lines) => lines
            .as_u64()
            .and_then(|lines| usize::try_from(lines).ok())
            .filter(|lines| (1..=TAIL_MOST).contains(lines))
            .ok_or_else(|| {
                format!("lines {lines} is not a number of lines from 1 to {TAIL_MOST}.")
            })?,
    };
    let word = match arguments.get("word") {
        None => None,
        Some(Value::String(word)) => Some(word.trim().to_owned()).filter(|word| !word.is_empty()),
        Some(_) => return Err("word is the text each line must hold.".to_owned()),
    };
    Ok((lines, word))
}

/// A tool result: its sentence, and its JSON as the second block.
fn told(sentence: &str, structured: &Value) -> CallToolResult {
    CallToolResult::success(vec![
        ContentBlock::text(sentence.to_owned()),
        ContentBlock::text(structured.to_string()),
    ])
}

impl ToolFamily for DesktopTools {
    fn name(&self) -> &str {
        &TABLE.name
    }

    fn route(&self) -> &str {
        &TABLE.path
    }

    fn instructions(&self) -> &str {
        &TABLE.instructions
    }

    fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    fn grantable(&self) -> bool {
        self.relay.has_window()
    }

    fn tools(&self) -> Vec<Tool> {
        TABLE
            .tools
            .iter()
            .map(|spec| {
                let tool = Tool::new(
                    spec.name.clone(),
                    spec.description.clone(),
                    spec.input_schema.clone(),
                );
                if spec.read_only {
                    tool.with_annotations(ToolAnnotations::new().read_only(true))
                } else {
                    tool
                }
            })
            .collect()
    }

    /// Every tool is allowed at launch, each named on its own: opening a view
    /// changes nothing but what the user's window shows, beside the caller's
    /// own tab; listing the settings, the status and the logs changes
    /// nothing; and a set can only narrow what agents may reach or do, shows
    /// the user a toast with Undo, and is refused for anything else the
    /// window does not allow.
    fn allowed_tools(&self) -> Vec<String> {
        vec![
            DESKTOP_OPEN.to_owned(),
            DESKTOP_SETTINGS.to_owned(),
            DESKTOP_SET.to_owned(),
            DESKTOP_STATUS.to_owned(),
            DESKTOP_LOG.to_owned(),
        ]
    }

    fn allowed_commands(&self) -> Vec<String> {
        vec![
            OPEN_COMMAND.to_owned(),
            SETTINGS_COMMAND.to_owned(),
            SET_COMMAND.to_owned(),
            STATUS_COMMAND.to_owned(),
            LOG_COMMAND.to_owned(),
        ]
    }

    fn call<'a>(&'a self, caller: Caller, name: &'a str, arguments: JsonObject) -> ToolFuture<'a> {
        Box::pin(async move {
            let Some(session) = caller
                .session
                .and_then(|session| u64::try_from(session.0).ok())
            else {
                return problem(NO_SESSION_YET);
            };
            match name {
                DESKTOP_OPEN => self.open(session, arguments).await,
                DESKTOP_SETTINGS => self.pass_on(session, DESKTOP_SETTINGS, arguments).await,
                DESKTOP_SET => self.pass_on(session, DESKTOP_SET, arguments).await,
                DESKTOP_STATUS => self.status(session, arguments).await,
                DESKTOP_LOG => self.log(session, arguments).await,
                other => problem(&format!("{} has no tool named {other}.", TABLE.name)),
            }
        })
    }
}

/// Who made a `desktop_open` call from `session` with `arguments`, and what
/// it asks the window to open, or the sentence saying why nothing opens.
fn resolve_call(
    world: &dyn DesktopWorld,
    session: u64,
    arguments: &JsonObject,
) -> Result<(DesktopCaller, DesktopOpen), String> {
    let mine = world.session(session)?.ok_or(NO_RECORD)?;
    let open = resolve_open(world, Path::new(&mine.cwd), arguments)?;
    Ok((caller_of(world, session, mine), open))
}

/// Who `session`, whose facts are `mine`, is to the relay: it and the
/// sessions above it, nearest first, and the device that started it or the
/// nearest above it that a device started.
fn caller_of(world: &dyn DesktopWorld, session: u64, mine: SessionFacts) -> DesktopCaller {
    let mut lineage = vec![session];
    let mut starter = mine.starter;
    let mut above = mine.parent;
    while let Some(parent) = above {
        if lineage.contains(&parent) || lineage.len() >= LINEAGE_MOST {
            break;
        }
        lineage.push(parent);
        let Ok(Some(facts)) = world.session(parent) else {
            break;
        };
        starter = starter.or(facts.starter);
        above = facts.parent;
    }
    DesktopCaller { lineage, starter }
}

/// The view `arguments` ask `desktop_open` for, every path resolved against
/// `cwd` and checked, or the sentence saying why nothing opens.
fn resolve_open(
    world: &dyn DesktopWorld,
    cwd: &Path,
    arguments: &JsonObject,
) -> Result<DesktopOpen, String> {
    let view = arguments
        .get("view")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "Say which view to open: file, folder, project, history or settings.".to_owned()
        })?;
    let takes: &[&str] = match view {
        "file" => &["view", "path", "line"],
        "folder" | "project" => &["view", "path"],
        "history" => &["view", "search"],
        "settings" => &["view", "section"],
        other => {
            return Err(format!(
                "`{other}` is not a view desktop_open opens: file, folder, project, history or settings."
            ));
        }
    };
    if let Some(stray) = arguments.keys().find(|key| !takes.contains(&key.as_str())) {
        return Err(format!(
            "`{stray}` does not go with view {view}: it takes {}.",
            takes[1..].join(" and ")
        ));
    }
    match view {
        "file" => {
            let path = path_in(arguments, cwd, "file")?;
            let line = match arguments.get("line") {
                None => None,
                Some(line) => Some(
                    line.as_u64()
                        .filter(|line| *line >= 1)
                        .and_then(|line| u32::try_from(line).ok())
                        .ok_or_else(|| {
                            format!("line {line} is not a line number: lines count from 1.")
                        })?,
                ),
            };
            let root = world.holding(&path)?;
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => {
                    return Err(format!(
                        "{} is a folder: open it with view folder.",
                        path.display()
                    ));
                }
                Err(_) => return Err(format!("There is no file at {}.", path.display())),
            }
            Ok(DesktopOpen::File {
                root: root.path.display().to_string(),
                path: path.display().to_string(),
                line,
            })
        }
        "folder" => {
            let path = path_in(arguments, cwd, "folder")?;
            let root = world.holding(&path)?;
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(format!(
                        "{} is a file: open it with view file.",
                        path.display()
                    ));
                }
                Err(_) => return Err(format!("There is no folder at {}.", path.display())),
            }
            Ok(DesktopOpen::Folder {
                root: root.path.display().to_string(),
                path: path.display().to_string(),
            })
        }
        "project" => {
            let path = path_in(arguments, cwd, "project")?;
            let project = world.project_holding(&path)?;
            if std::fs::metadata(&path).is_err() {
                return Err(format!("There is no file or folder at {}.", path.display()));
            }
            Ok(DesktopOpen::Project {
                path: project.path.display().to_string(),
            })
        }
        "history" => {
            let search = match arguments.get("search") {
                None => String::new(),
                Some(Value::String(search)) => search.trim().to_owned(),
                Some(_) => return Err("search is the text to search History for.".to_owned()),
            };
            Ok(DesktopOpen::History { search })
        }
        _ => {
            let section = match arguments.get("section") {
                None => None,
                Some(Value::String(section)) if settings_sections().contains(&section.as_str()) => {
                    Some(section.clone())
                }
                Some(other) => {
                    return Err(format!(
                        "{other} is not a Settings section: one of {}.",
                        settings_sections().join(", ")
                    ));
                }
            };
            Ok(DesktopOpen::Settings { section })
        }
    }
}

/// Whether `arguments` have the shape `tool`'s schema gives them, or the
/// sentence saying why not. What a setting is called and what it takes are
/// the window's to judge.
fn settings_shape(tool: &str, arguments: &JsonObject) -> Result<(), String> {
    let takes: &[&str] = if tool == DESKTOP_SET {
        &["changes", "host"]
    } else {
        &[]
    };
    if let Some(stray) = arguments.keys().find(|key| !takes.contains(&key.as_str())) {
        return Err(if takes.is_empty() {
            format!("{tool} takes nothing, not `{stray}`.")
        } else {
            format!("`{stray}` is not something {tool} takes: it takes changes and host.")
        });
    }
    if tool != DESKTOP_SET {
        return Ok(());
    }
    match arguments.get("changes") {
        Some(Value::Object(changes)) if !changes.is_empty() => {}
        _ => {
            return Err("Say what to change in changes: each setting's name and its new value, such as {\"sessionAgents\": false}.".to_owned());
        }
    }
    match arguments.get("host") {
        None | Some(Value::String(_)) => Ok(()),
        Some(_) => Err("host is a host's id, as desktop_settings lists it.".to_owned()),
    }
}

/// The `path` argument, resolved against `cwd` when relative and folded.
fn path_in(arguments: &JsonObject, cwd: &Path, what: &str) -> Result<PathBuf, String> {
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| format!("Name the {what} to open in path."))?;
    let given = Path::new(path);
    Ok(normalize(&if given.is_absolute() {
        given.to_path_buf()
    } else {
        cwd.join(given)
    }))
}

/// The Settings sections `desktop_open` opens, as its schema lists them.
fn settings_sections() -> Vec<&'static str> {
    TABLE
        .tools
        .iter()
        .find(|tool| tool.name == DESKTOP_OPEN)
        .and_then(|tool| tool.input_schema.get("properties"))
        .and_then(|properties| properties.get("section"))
        .and_then(|section| section.get("enum"))
        .and_then(Value::as_array)
        .map(|sections| sections.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// What the agent reads for what the window answered: its tool result as it
/// is, or the sentence it failed with.
fn tool_result(outcome: AgentDesktopOutcome) -> CallToolResult {
    match outcome {
        AgentDesktopOutcome::Done { value } => answered(&value),
        AgentDesktopOutcome::Failed { message } => problem(&message),
    }
}

fn answered(value: &Payload) -> CallToolResult {
    from_payload::<CallToolResult>(value).unwrap_or_else(|err| {
        problem(&format!(
            "Floti's window answered with something that is not a tool result: {err}."
        ))
    })
}

fn problem(body: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(body.to_owned())])
}

/// The world the host answers from: its sessions, the folders it knows, its
/// settings, its plugins, its folders and its agent tools listener. It holds
/// what is the host's weakly, so it keeps nothing of the host's alive: the
/// host's sessions hold the agent tools, and the folders it knows read the
/// sessions.
pub(crate) struct HostDesktopWorld {
    pub hosted: Weak<Hosted>,
    pub roots: Weak<KnownRoots>,
    pub settings: Weak<SettingsService>,
    pub plugins: WeakRegistry,
    pub folders: Folders,
    /// The families on and the listener's address, while the host runs.
    pub tool_families: Box<dyn Fn() -> Option<ToolFamiliesStatus> + Send + Sync>,
}

/// The answer to `desktop_status` once the host has let go of what it reads.
const STOPPING_STATUS: &str = "Floti's host is stopping, so it cannot say how it stands now.";

/// The answer once the host has let go of what the tools read: it is
/// stopping.
const STOPPING: &str = "Floti's host is stopping, so nothing can be opened now.";

impl DesktopWorld for HostDesktopWorld {
    fn session(&self, id: u64) -> Result<Option<SessionFacts>, String> {
        let hosted = self.hosted.upgrade().ok_or_else(|| STOPPING.to_owned())?;
        let Some(id): Option<SessionId> = session_id(id) else {
            return Ok(None);
        };
        Ok(hosted.record(id).ok().map(|record| SessionFacts {
            cwd: record.cwd,
            parent: record.parent_id,
            starter: hosted.starter_of(id),
        }))
    }

    fn holding(&self, path: &Path) -> Result<KnownRoot, String> {
        self.roots
            .upgrade()
            .ok_or_else(|| STOPPING.to_owned())?
            .holding(path)
    }

    fn project_holding(&self, path: &Path) -> Result<KnownRoot, String> {
        self.roots
            .upgrade()
            .ok_or_else(|| STOPPING.to_owned())?
            .project_holding(path)
    }

    fn status(&self) -> Result<HostStatus, String> {
        let stopping = || STOPPING_STATUS.to_owned();
        let hosted = self.hosted.upgrade().ok_or_else(stopping)?;
        let settings = self
            .settings
            .upgrade()
            .ok_or_else(stopping)?
            .read()
            .map_err(|err| format!("Floti could not read its settings: {err}."))?;
        let plugins = self.plugins.upgrade().ok_or_else(stopping)?.list();
        let setup = setup_status(hosted.backends(), &settings.backend_paths);
        let backend_name = |id: &str| {
            hosted
                .backends()
                .iter()
                .find(|backend| backend.id().as_str() == id)
                .map(|backend| backend.name().to_owned())
        };
        Ok(status::host_status(&StatusParts {
            version: crate::version::FLOTI_VERSION,
            protocol: PROTOCOL_VERSION.to_string(),
            folders: StatusFolders {
                data: self.folders.data.display().to_string(),
                config: Some(self.folders.config.display().to_string()),
                logs: self.folders.logs.display().to_string(),
                host_log: self.host_log().display().to_string(),
            },
            setup: &setup,
            backend_name: &backend_name,
            plugins: &plugins,
            settings: &settings,
            tool_families: (self.tool_families)(),
        }))
    }

    fn host_log(&self) -> PathBuf {
        self.folders.logs.join(LOG_FILE)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::testkit::TestDirs;
    use crate::workspace::roots::{RootKind, outside_refusal};

    /// Sessions by id, and the folders known, each with its kind.
    struct World {
        sessions: HashMap<u64, SessionFacts>,
        known: Vec<(PathBuf, RootKind)>,
        /// The host is stopping: it has let go of its sessions.
        stopping: bool,
    }

    impl World {
        fn root(&self, path: &Path, wanted: impl Fn(RootKind) -> bool) -> Option<KnownRoot> {
            let path = normalize(path);
            self.known
                .iter()
                .filter(|(root, kind)| wanted(*kind) && path.starts_with(root))
                .max_by_key(|(root, _)| root.components().count())
                .map(|(root, kind)| KnownRoot {
                    path: root.clone(),
                    kind: *kind,
                    name: None,
                })
        }
    }

    impl DesktopWorld for World {
        fn session(&self, id: u64) -> Result<Option<SessionFacts>, String> {
            if self.stopping {
                return Err(STOPPING.to_owned());
            }
            Ok(self.sessions.get(&id).cloned())
        }

        fn holding(&self, path: &Path) -> Result<KnownRoot, String> {
            self.root(path, |_| true)
                .ok_or_else(|| outside_refusal(path))
        }

        fn project_holding(&self, path: &Path) -> Result<KnownRoot, String> {
            self.root(path, |kind| kind == RootKind::Project)
                .ok_or_else(|| "no project page".to_owned())
        }

        fn status(&self) -> Result<HostStatus, String> {
            if self.stopping {
                return Err(STOPPING_STATUS.to_owned());
            }
            Ok(host_status())
        }

        fn host_log(&self) -> PathBuf {
            PathBuf::from("logs dir").join(LOG_FILE)
        }
    }

    /// A host's own part, with a token in a plugin's error as a line that
    /// should never have held one would.
    fn host_status() -> HostStatus {
        HostStatus {
            version: "1.2.3".to_owned(),
            protocol: "4.3".to_owned(),
            folders: StatusFolders {
                data: "data dir".to_owned(),
                config: None,
                logs: "logs dir".to_owned(),
                host_log: "logs dir/host.log".to_owned(),
            },
            agents: Vec::new(),
            plugins: vec![status::PluginStatus {
                id: Some("p".to_owned()),
                name: "P".to_owned(),
                path: "plugins/p".to_owned(),
                on: true,
                loaded: false,
                error: Some(format!("sent Bearer {TOKEN}")),
                tools_on: None,
                background: None,
                service: None,
            }],
            tool_families: Some(ToolFamiliesStatus {
                on: vec![DESKTOP_FAMILY.to_owned()],
                address: Some("127.0.0.1:4100".to_owned()),
            }),
            limits: status::AgentLimits {
                sessions_can_start_agents: true,
                children_per_parent: 4,
                sessions_per_host: 16,
                depth: 3,
                starts_per_parent: 50,
                commands_per_parent: 4,
            },
        }
    }

    /// A token, as the listener mints them.
    const TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn facts(cwd: &Path, parent: Option<u64>, starter: Option<&str>) -> SessionFacts {
        SessionFacts {
            cwd: cwd.display().to_string(),
            parent,
            starter: starter.map(str::to_owned),
        }
    }

    fn object(value: Value) -> JsonObject {
        value.as_object().cloned().expect("an object")
    }

    /// A project with a source file and a folder in it, a session working
    /// there, and a folder nobody knows.
    struct Fixture {
        world: World,
        project: PathBuf,
        outside: PathBuf,
        _dirs: TestDirs,
    }

    fn fixture() -> Fixture {
        let dirs = TestDirs::new();
        let project = normalize(&dirs.folder("my project"));
        let outside = normalize(&dirs.folder("not known"));
        std::fs::create_dir_all(project.join("src dir")).unwrap();
        std::fs::write(project.join("src dir").join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(outside.join("secret.txt"), "no\n").unwrap();
        let world = World {
            sessions: HashMap::from([(4, facts(&project, None, Some("laptop")))]),
            known: vec![(project.clone(), RootKind::Project)],
            stopping: false,
        };
        Fixture {
            world,
            project,
            outside,
            _dirs: dirs,
        }
    }

    fn open(fixture: &Fixture, arguments: Value) -> Result<DesktopOpen, String> {
        resolve_open(&fixture.world, &fixture.project, &object(arguments))
    }

    #[test]
    fn the_table_is_floti_desktop_at_mcp_desktop_with_its_five_tools() {
        assert_eq!(TABLE.name, DESKTOP_FAMILY);
        assert_eq!(TABLE.path, "/mcp/desktop");
        let names: Vec<&str> = TABLE.tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(
            names,
            [
                DESKTOP_OPEN,
                DESKTOP_SETTINGS,
                DESKTOP_SET,
                DESKTOP_STATUS,
                DESKTOP_LOG
            ]
        );
        // The instructions name the diagnosis tools and their commands.
        for said in [DESKTOP_STATUS, DESKTOP_LOG, STATUS_COMMAND, LOG_COMMAND] {
            assert!(TABLE.instructions.contains(said), "{said}");
        }
        // The instructions say what each view is for.
        for view in [
            "file's tab",
            "Files view",
            "project's page",
            "History",
            "Settings",
        ] {
            assert!(TABLE.instructions.contains(view), "{view}");
        }
        assert!(TABLE.instructions.contains(OPEN_COMMAND));
        assert!(settings_sections().contains(&"agents"));
        // The instructions say a session narrows and never widens.
        assert!(TABLE.instructions.contains("narrow but never widen"));
    }

    #[test]
    fn the_settings_list_the_status_and_the_log_are_marked_read_only() {
        let events = Arc::new(super::super::events::Events::new());
        let focus = Arc::new(std::sync::Mutex::new(Default::default()));
        let tools = DesktopTools::new(Arc::new(DesktopRelay::new(&events, focus, |_| None)));
        assert_eq!(
            crate::agent_tools::read_only_tools(&tools.tools()),
            [DESKTOP_SETTINGS, DESKTOP_STATUS, DESKTOP_LOG]
        );
    }

    fn text_of(result: &CallToolResult) -> (String, Value) {
        let value = serde_json::to_value(result).unwrap();
        let text = |at: usize| {
            value["content"][at]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        let structured = serde_json::from_str(&text(1)).unwrap_or(Value::Null);
        (text(0), structured)
    }

    fn window(text: &str, value: Value) -> Payload {
        to_payload(&json!({
            "content": [
                { "type": "text", "text": text },
                { "type": "text", "text": value.to_string() },
            ],
        }))
        .unwrap()
    }

    #[test]
    fn a_status_with_no_window_answers_the_hosts_part_and_names_what_only_a_window_can_tell() {
        let (text, structured) = text_of(&status_answer(
            &host_status(),
            Err("no Floti window is attached".to_owned()),
        ));
        assert!(
            text.starts_with("This host:\nFloti host 1.2.3, protocol 4.3."),
            "{text}"
        );
        assert!(text.contains("listens on 127.0.0.1:4100"), "{text}");
        assert!(
            text.contains("Only a Floti window can tell the desktop's version, the hosts the window knows with how each is connected and its last error, and where main's log is: no Floti window is attached."),
            "{text}"
        );
        assert_eq!(structured["window"], Value::Null);
        assert_eq!(structured["needsWindow"], json!(WINDOW_PARTS));
        assert_eq!(structured["host"]["version"], "1.2.3");
        // The token in a plugin's error is hidden in both blocks.
        assert!(!text.contains(TOKEN) && !structured.to_string().contains(TOKEN));
        assert!(text.contains("Bearer [hidden]"), "{text}");
    }

    #[test]
    fn a_status_with_a_window_merges_its_part_with_its_secrets_hidden() {
        let part = window_part(&window(
            "Floti desktop 2.0.0.",
            json!({ "desktop": { "version": "2.0.0" }, "hosts": [{ "id": "box", "lastError": format!("key {TOKEN}") }, { "id": "vm", "lastError": "sent Bearer abc" }] }),
        ))
        .unwrap();
        let (text, structured) = text_of(&status_answer(&host_status(), Ok(part)));
        assert!(
            text.ends_with("Floti's window:\nFloti desktop 2.0.0."),
            "{text}"
        );
        assert_eq!(structured["window"]["desktop"]["version"], "2.0.0");
        assert_eq!(
            structured["window"]["hosts"][0]["lastError"],
            "key [hidden]"
        );
        assert_eq!(
            structured["window"]["hosts"][1]["lastError"],
            "sent Bearer [hidden]"
        );
        assert!(structured.get("needsWindow").is_none());
        // A window that answered with an error, or without its part, is no part.
        let refused = to_payload(
            &json!({ "content": [{ "type": "text", "text": "Could not." }], "isError": true }),
        )
        .unwrap();
        assert_eq!(window_part(&refused), Err("Could not.".to_owned()));
        let bare = to_payload(&json!({ "content": [{ "type": "text", "text": "Hi." }] })).unwrap();
        assert!(window_part(&bare).unwrap_err().contains("without its part"));
    }

    #[test]
    fn a_log_answers_the_hosts_lines_and_mains_or_why_not() {
        let path = PathBuf::from("logs dir").join(LOG_FILE);
        let main = window_part(&window(
            "main.log (x), its last 1 lines:\nmain started",
            json!({ "path": "x", "lines": ["main started"] }),
        ))
        .unwrap();
        let (text, structured) = text_of(&log_answer(
            &path,
            Ok(Some(vec!["host started".to_owned()])),
            Some("started"),
            Ok(main),
        ));
        assert!(
            text.contains("its last 1 lines holding \"started\":\nhost started"),
            "{text}"
        );
        assert!(text.ends_with("main started"), "{text}");
        assert_eq!(structured["host"]["lines"], json!(["host started"]));
        assert_eq!(structured["main"]["lines"], json!(["main started"]));

        let (text, structured) = text_of(&log_answer(
            &path,
            Ok(None),
            None,
            Err("no Floti window is attached".to_owned()),
        ));
        assert!(text.contains("is not there yet."), "{text}");
        assert!(
            text.contains(
                "Main's log is read through a Floti window: no Floti window is attached."
            ),
            "{text}"
        );
        assert_eq!(structured["main"], Value::Null);
    }

    #[test]
    fn a_log_call_takes_lines_from_1_to_500_and_a_word() {
        assert_eq!(log_arguments(&object(json!({}))), Ok((LOG_LINES, None)));
        assert_eq!(
            log_arguments(&object(json!({ "lines": 500, "word": " plugin " }))),
            Ok((500, Some("plugin".to_owned())))
        );
        assert_eq!(
            log_arguments(&object(json!({ "word": "" }))),
            Ok((LOG_LINES, None))
        );
        for (arguments, says) in [
            (json!({ "lines": 0 }), "from 1 to 500"),
            (json!({ "lines": 501 }), "from 1 to 500"),
            (json!({ "lines": "9" }), "from 1 to 500"),
            (json!({ "word": 3 }), "text each line"),
            (json!({ "since": "now" }), "takes lines and word"),
        ] {
            let refused = log_arguments(&object(arguments.clone())).unwrap_err();
            assert!(refused.contains(says), "{arguments}: {refused}");
        }
    }

    #[test]
    fn a_settings_call_is_refused_for_its_shape_before_it_is_passed_on() {
        for (tool, arguments, says) in [
            (
                DESKTOP_SETTINGS,
                json!({ "host": "local" }),
                "takes nothing",
            ),
            (DESKTOP_SET, json!({}), "Say what to change"),
            (DESKTOP_SET, json!({ "changes": {} }), "Say what to change"),
            (
                DESKTOP_SET,
                json!({ "changes": [false] }),
                "Say what to change",
            ),
            (
                DESKTOP_SET,
                json!({ "changes": { "sessionAgents": false }, "host": 3 }),
                "host's id",
            ),
            (
                DESKTOP_SET,
                json!({ "changes": { "sessionAgents": false }, "name": "x" }),
                "not something desktop_set takes",
            ),
        ] {
            let refused = settings_shape(tool, &object(arguments.clone())).unwrap_err();
            assert!(refused.contains(says), "{tool} {arguments}: {refused}");
        }
        assert_eq!(settings_shape(DESKTOP_SETTINGS, &object(json!({}))), Ok(()));
        assert_eq!(
            settings_shape(
                DESKTOP_SET,
                &object(json!({ "changes": { "sessionAgents": false }, "host": "build-box" }))
            ),
            Ok(())
        );
    }

    #[test]
    fn every_tool_and_its_command_are_allowed_at_launch_each_named_on_its_own() {
        let events = Arc::new(super::super::events::Events::new());
        let focus = Arc::new(std::sync::Mutex::new(Default::default()));
        let tools = DesktopTools::new(Arc::new(DesktopRelay::new(&events, focus, |_| None)));
        assert_eq!(
            tools.allowed_tools(),
            [
                DESKTOP_OPEN,
                DESKTOP_SETTINGS,
                DESKTOP_SET,
                DESKTOP_STATUS,
                DESKTOP_LOG
            ]
        );
        assert_eq!(
            tools.allowed_commands(),
            [
                "floti desktop open",
                "floti desktop settings",
                "floti desktop set",
                "floti desktop status",
                "floti desktop log"
            ]
        );
        // No window attached: a launch now is not given it.
        assert!(!tools.grantable());
    }

    #[test]
    fn a_relative_file_resolves_against_the_callers_folder_and_keeps_its_line() {
        let fixture = fixture();
        let opened = open(
            &fixture,
            json!({ "view": "file", "path": "src dir/main.rs", "line": 12 }),
        )
        .unwrap();
        assert_eq!(
            opened,
            DesktopOpen::File {
                root: fixture.project.display().to_string(),
                path: fixture
                    .project
                    .join("src dir")
                    .join("main.rs")
                    .display()
                    .to_string(),
                line: Some(12),
            }
        );
        let folder = open(&fixture, json!({ "view": "folder", "path": "./src dir" })).unwrap();
        assert_eq!(
            folder,
            DesktopOpen::Folder {
                root: fixture.project.display().to_string(),
                path: fixture.project.join("src dir").display().to_string(),
            }
        );
        let page = open(&fixture, json!({ "view": "project", "path": "src dir" })).unwrap();
        assert_eq!(
            page,
            DesktopOpen::Project {
                path: fixture.project.display().to_string()
            }
        );
    }

    #[test]
    fn a_path_outside_every_known_folder_is_refused_naming_the_rule_even_by_climbing() {
        let fixture = fixture();
        let secret = fixture.outside.join("secret.txt");
        for path in [
            secret.display().to_string(),
            format!(
                "..{}not known{}secret.txt",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            ),
        ] {
            let refused = open(&fixture, json!({ "view": "file", "path": path })).unwrap_err();
            assert_eq!(refused, outside_refusal(&secret), "{path}");
        }
        let refused = open(
            &fixture,
            json!({ "view": "folder", "path": fixture.outside }),
        )
        .unwrap_err();
        assert!(
            refused.contains("not inside a folder Floti knows"),
            "{refused}"
        );
    }

    #[test]
    fn a_project_path_that_is_not_there_is_refused_and_nothing_opens() {
        let fixture = fixture();
        let refused = open(&fixture, json!({ "view": "project", "path": "gone dir" })).unwrap_err();
        assert_eq!(
            refused,
            format!(
                "There is no file or folder at {}.",
                fixture.project.join("gone dir").display()
            )
        );
    }

    #[test]
    fn a_call_names_its_session_and_says_why_when_the_host_has_none_or_is_stopping() {
        let mut fixture = fixture();
        let arguments = object(json!({ "view": "history", "search": "x" }));
        let (caller, open) = resolve_call(&fixture.world, 4, &arguments).unwrap();
        assert_eq!(caller.lineage, [4]);
        assert_eq!(caller.starter.as_deref(), Some("laptop"));
        assert_eq!(
            open,
            DesktopOpen::History {
                search: "x".to_owned()
            }
        );
        assert_eq!(
            resolve_call(&fixture.world, 77, &arguments).unwrap_err(),
            NO_RECORD
        );
        fixture.world.stopping = true;
        assert_eq!(
            resolve_call(&fixture.world, 4, &arguments).unwrap_err(),
            STOPPING
        );
    }

    #[test]
    fn what_cannot_be_opened_is_refused_saying_why() {
        let fixture = fixture();
        for (arguments, says) in [
            (json!({}), "Say which view"),
            (json!({ "view": "window" }), "not a view"),
            (json!({ "view": "file" }), "Name the file"),
            (json!({ "view": "file", "path": "  " }), "Name the file"),
            (
                json!({ "view": "file", "path": "missing.rs" }),
                "There is no file",
            ),
            (json!({ "view": "file", "path": "src dir" }), "is a folder"),
            (
                json!({ "view": "file", "path": "src dir/main.rs", "line": 0 }),
                "lines count from 1",
            ),
            (
                json!({ "view": "file", "path": "src dir/main.rs", "line": "3" }),
                "lines count from 1",
            ),
            (
                json!({ "view": "folder", "path": "src dir/main.rs" }),
                "is a file",
            ),
            (
                json!({ "view": "folder", "path": "gone" }),
                "There is no folder",
            ),
            (
                json!({ "view": "folder", "path": "src dir", "line": 3 }),
                "does not go with view folder",
            ),
            (
                json!({ "view": "history", "path": "x" }),
                "does not go with view history",
            ),
            (json!({ "view": "history", "search": 3 }), "text to search"),
            (
                json!({ "view": "settings", "section": "secret stuff" }),
                "not a Settings section",
            ),
        ] {
            let refused = open(&fixture, arguments.clone()).unwrap_err();
            assert!(refused.contains(says), "{arguments}: {refused}");
        }
    }

    #[test]
    fn history_takes_its_search_and_settings_its_section() {
        let fixture = fixture();
        assert_eq!(
            open(
                &fixture,
                json!({ "view": "history", "search": " login bug " })
            ),
            Ok(DesktopOpen::History {
                search: "login bug".to_owned()
            })
        );
        assert_eq!(
            open(&fixture, json!({ "view": "history" })),
            Ok(DesktopOpen::History {
                search: String::new()
            })
        );
        assert_eq!(
            open(&fixture, json!({ "view": "settings", "section": "agents" })),
            Ok(DesktopOpen::Settings {
                section: Some("agents".to_owned())
            })
        );
        assert_eq!(
            open(&fixture, json!({ "view": "settings" })),
            Ok(DesktopOpen::Settings { section: None })
        );
    }

    #[test]
    fn a_child_is_called_by_its_lineage_and_the_device_that_started_the_nearest_above_it() {
        let folder = Path::new("/work");
        let world = World {
            sessions: HashMap::from([
                (1, facts(folder, None, Some("laptop"))),
                (2, facts(folder, Some(1), None)),
                (3, facts(folder, Some(2), None)),
                // A record naming itself above itself ends the walk.
                (8, facts(folder, Some(9), None)),
                (9, facts(folder, Some(8), None)),
            ]),
            known: Vec::new(),
            stopping: false,
        };
        let called = |id: u64| caller_of(&world, id, world.session(id).unwrap().unwrap());
        assert_eq!(
            called(3),
            DesktopCaller {
                lineage: vec![3, 2, 1],
                starter: Some("laptop".to_owned()),
            }
        );
        assert_eq!(
            called(8),
            DesktopCaller {
                lineage: vec![8, 9],
                starter: None,
            }
        );
    }

    #[test]
    fn a_done_answer_is_the_tool_result_and_a_failed_one_its_sentence() {
        let result = json!({
            "content": [{ "type": "text", "text": "Opened main.rs beside your tab." }],
            "isError": false,
        });
        let answered = tool_result(AgentDesktopOutcome::Done {
            value: to_payload(&result).unwrap(),
        });
        assert_eq!(serde_json::to_value(&answered).unwrap(), result);
        let failed = tool_result(AgentDesktopOutcome::no_window());
        let value = serde_json::to_value(&failed).unwrap();
        assert_eq!(value["isError"], true);
        assert!(
            value["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("No window is attached")
        );
    }
}
