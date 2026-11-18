//! floti-desktop: tools that open Floti's views for the user, served to the
//! host's sessions on the agent tools listener at `/mcp/desktop`, and carried
//! out in an attached window.
//!
//! The name, route, instructions and every tool's description and input
//! schema come from `desktop_tools.json`. `desktop_open` opens one view: a
//! file's tab at a line, the Files view on a folder, a project's page, the
//! History palette with a search typed in, or a Settings section.
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

use floti_protocol::Payload;
use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::agent_desktop::{AgentDesktopOutcome, DesktopOpen};
use serde::Deserialize;
use serde_json::Value;

use super::features::{DesktopCaller, DesktopRelay};
use super::hosted::{Hosted, session_id};
use crate::agent_config::normalize;
use crate::agent_tools::{
    CallToolResult, Caller, ContentBlock, JsonObject, Tool, ToolFamily, ToolFuture,
};
use crate::session::SessionId;
use crate::workspace::roots::{KnownRoot, KnownRoots};

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
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("desktop_tools.json"))
        .expect("desktop_tools.json is the desktop tools' table")
});

/// The family's name, as `desktop_tools.json` gives it.
pub(crate) const DESKTOP_FAMILY: &str = "floti-desktop";

const DESKTOP_OPEN: &str = "desktop_open";

/// The command that runs `desktop_open` from a session's shell.
pub(crate) const OPEN_COMMAND: &str = "floti desktop open";

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
                Tool::new(
                    spec.name.clone(),
                    spec.description.clone(),
                    spec.input_schema.clone(),
                )
            })
            .collect()
    }

    /// Opening a view changes nothing but what the user's window shows,
    /// beside the caller's own tab, so it is allowed at launch.
    fn allowed_tools(&self) -> Vec<String> {
        vec![DESKTOP_OPEN.to_owned()]
    }

    fn allowed_commands(&self) -> Vec<String> {
        vec![OPEN_COMMAND.to_owned()]
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

/// The world the host answers from: its sessions and the folders it knows.
/// It holds both weakly, so it keeps nothing of the host's alive: the host's
/// sessions hold the agent tools, and the folders it knows read the sessions.
pub(crate) struct HostDesktopWorld {
    pub hosted: Weak<Hosted>,
    pub roots: Weak<KnownRoots>,
}

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
    }

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
    fn the_table_is_floti_desktop_at_mcp_desktop_with_desktop_open() {
        assert_eq!(TABLE.name, DESKTOP_FAMILY);
        assert_eq!(TABLE.path, "/mcp/desktop");
        let names: Vec<&str> = TABLE.tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, [DESKTOP_OPEN]);
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
    }

    #[test]
    fn desktop_open_and_its_command_are_allowed_at_launch_each_named_on_its_own() {
        let events = Arc::new(super::super::events::Events::new());
        let focus = Arc::new(std::sync::Mutex::new(Default::default()));
        let tools = DesktopTools::new(Arc::new(DesktopRelay::new(&events, focus, |_| None)));
        assert_eq!(tools.allowed_tools(), [DESKTOP_OPEN]);
        assert_eq!(tools.allowed_commands(), ["floti desktop open"]);
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
