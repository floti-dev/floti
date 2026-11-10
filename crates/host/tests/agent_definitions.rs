//! Agents: the files an agent starts its children in, read from the host's
//! config folder. A host whose sessions run the fake `claude`
//! (`test/fake-agents/claude-code.mjs`) and the other fake agent
//! (`test/fake-agents/other-agent.mjs`), never a real agent. A session's agent
//! tools are called over HTTP with the URL and bearer token the session itself
//! was given, and `floti agent` runs its own code against the same route.
//!
//! No client is attached while a session works: a client starts the session
//! and goes, and comes back only to restore sessions after the host restarts.
//! A client lists, reads, edits and watches the agents over the protocol
//! (S19). The agents folder is under the test's own run root, so no test
//! reads or writes a real one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use floti_host::agent::AgentBackend;
use floti_host::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use floti_host::agent_cli::{
    self, Command as Subcommand, Start as StartCommand, Stop as StopCommand,
};
use floti_host::platform::process_tree::Native as NativeTree;
use floti_host::platform::socket::Endpoint;
use floti_host::server::HostConfig;
use floti_host::testkit::{
    GitRepo, OtherAgent, OtherAgentRecord, PATIENCE, RunningHost, TestClient, TestDirs,
};
use floti_protocol::BackendId;
use floti_protocol::channels::agents::{
    AddShippedAgent, AgentName, AgentProblem, AgentUnusedLine, AgentsChange, AgentsListing,
    DeleteAgent, DeleteAgentRequest, ListAgents, ReadAgent, RestoreAgent, WriteAgent,
    WriteAgentRequest,
};
use floti_protocol::channels::common::{PermissionMode, SessionRecord};
use floti_protocol::channels::host::{HostStop, HostStopReason, HostStopRequest};
use floti_protocol::channels::sessions::{
    HistoryQuery, HistoryRefresh, HistoryResume, HistorySession, HistorySessions, LaunchChoices,
    LaunchSessionRequest, RestoreSessionsRequest, ResumeSessionRequest, SessionLaunch, SessionList,
    SessionRestorable, SessionRestore, SessionStart, SessionToRestore, StartSessionRequest,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TERMINAL: &[&str] = &["terminal"];

/// The two backends the host has, by id: the fake `claude` and the other fake.
const CLAUDE: &str = "claude-code";
const OTHER: &str = "other-agent";

/// The agents family, as a session's tool file names it.
const FAMILY: &str = "floti-agents";

// ---------------------------------------------------------------------------
// A host with two backends and an agents folder of its own
// ---------------------------------------------------------------------------

struct World {
    endpoint: Endpoint,
    /// The fake `claude`'s config folder: `CLAUDE_CONFIG_DIR` for its runs.
    claude: PathBuf,
    other: OtherAgent,
    /// The workspace profile's folder: every project is in it.
    work: PathBuf,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let other = OtherAgent::new(&dirs);
        Self {
            endpoint: dirs.endpoint(),
            claude: resolved(&dirs.folder("claude config")),
            work: resolved(&dirs.folder("workspace")),
            other,
            dirs,
        }
    }

    fn config(&self) -> HostConfig {
        let claude = ClaudeCodeSettings {
            search_first: Vec::new(),
            program: floti_host::testkit::node().into(),
            leading_args: vec![fake_claude().into_os_string()],
            config_dir: self.claude.clone(),
            pass_config_dir: true,
            api_key_set: false,
        };
        let backends: Vec<Arc<dyn AgentBackend>> = vec![
            Arc::new(ClaudeCodeBackend::new(claude, Arc::new(NativeTree))),
            Arc::new(self.other.backend()),
        ];
        HostConfig {
            run: Default::default(),
            folders: self.dirs.folders(),
            endpoint: self.endpoint.clone(),
            backends,
            inherited_env: std::env::vars_os()
                .filter(|(key, _)| {
                    !["SHELL", "HOME", "USERPROFILE"].contains(&&*key.to_string_lossy())
                })
                .chain([
                    (OsString::from("SHELL"), OsString::from("/bin/sh")),
                    (
                        OsString::from("HOME"),
                        self.dirs.home.clone().into_os_string(),
                    ),
                    (
                        OsString::from("USERPROFILE"),
                        self.dirs.home.clone().into_os_string(),
                    ),
                ])
                .collect(),
        }
    }

    /// The agents folder the host reads: `agents` in its config folder.
    fn agents_dir(&self) -> PathBuf {
        self.dirs.config.join("agents")
    }

    /// Writes the agent file `<name>.md`. Making the folder is what keeps the
    /// host from seeding it.
    fn agent(&self, name: &str, content: &str) {
        std::fs::create_dir_all(self.agents_dir()).expect("make the agents folder");
        std::fs::write(self.agents_dir().join(format!("{name}.md")), content)
            .expect("write an agent file");
    }

    fn remove_agent(&self, name: &str) {
        std::fs::remove_file(self.agents_dir().join(format!("{name}.md")))
            .expect("delete an agent");
    }

    /// A host that serves once this returns, reading the agents folder as the
    /// installed host does, with the workspace tracked.
    async fn host(&self) -> RunningHost {
        let host = RunningHost::start(self.config());
        // A client is welcomed once the host serves.
        drop(self.client().await);
        let known: i64 = self
            .db()
            .query_row(
                "SELECT count(*) FROM profiles WHERE name = 'workspace'",
                [],
                |row| row.get(0),
            )
            .expect("count the profiles");
        if known == 0 {
            self.profile("workspace", &self.work, None, None, None);
        }
        host
    }

    async fn client(&self) -> TestClient {
        TestClient::connect(&self.endpoint, TERMINAL).await
    }

    /// A folder in the workspace, which Floti tracks.
    fn project(&self, name: &str) -> PathBuf {
        resolved(&self.dirs.folder(&format!("workspace/{name}")))
    }

    /// A git repository in the workspace with one commit, for the agents that
    /// work in a worktree.
    fn repository(&self, name: &str) -> PathBuf {
        let repo = GitRepo::init(self.project(name));
        repo.write("README.md", "hello\n");
        repo.commit_all("first");
        resolved(repo.path())
    }

    /// The host's database, as a test reads it and writes fixtures into it.
    fn db(&self) -> rusqlite::Connection {
        let db =
            rusqlite::Connection::open(self.dirs.data.join("floti.db")).expect("open floti.db");
        db.busy_timeout(PATIENCE)
            .expect("wait for the host's writes");
        db
    }

    /// A saved profile, as the profiles editor writes one.
    fn profile(
        &self,
        name: &str,
        root: &Path,
        model: Option<&str>,
        effort: Option<&str>,
        mode: Option<&str>,
    ) -> i64 {
        let db = self.db();
        db.execute(
            "INSERT INTO profiles (name, root, overlays, access, model, effort, permission_mode, \
             agent, opening_prompt, session_agents) \
             VALUES (?1, ?2, '[]', '[]', ?3, ?4, ?5, NULL, NULL, NULL)",
            rusqlite::params![name, text(root), model, effort, mode],
        )
        .expect("insert a profile");
        db.last_insert_rowid()
    }

    // -- sessions -----------------------------------------------------------

    /// A session on `backend` in `folder`, started by a client that is gone
    /// once the session is.
    async fn session(&self, backend: &str, folder: &Path, name: &str) -> Agent {
        self.session_as(request(folder, name, backend)).await
    }

    /// As [`Self::session`], in a permission mode.
    async fn session_in(
        &self,
        backend: &str,
        folder: &Path,
        name: &str,
        mode: PermissionMode,
    ) -> Agent {
        let mut asked = request(folder, name, backend);
        asked.permission_mode = Some(mode);
        self.session_as(asked).await
    }

    async fn session_as(&self, request: StartSessionRequest) -> Agent {
        let mut client = self.client().await;
        let record = client
            .request::<SessionStart>(&request)
            .await
            .expect("the session starts");
        drop(client);
        self.agent_of(record, &[]).await
    }

    /// The agent behind `record`, once its fake has logged a run other than
    /// the ones with a pid in `seen`.
    async fn agent_of(&self, record: SessionRecord, seen: &[u32]) -> Agent {
        let conversation = record
            .conversation_id
            .clone()
            .expect("the session has a conversation");
        let run = eventually("the agent to log its run", async || {
            self.runs_of(&conversation)
                .into_iter()
                .rev()
                .find(|run| !seen.contains(&run.pid))
        })
        .await;
        Agent { record, run }
    }

    /// Types `line` into `agent` and waits for the fake to take it. The fake
    /// `claude` keeps a conversation to resume only once it has been typed to.
    async fn type_line(&self, agent: &Agent, line: &str) {
        let conversation = agent
            .record
            .conversation_id
            .clone()
            .expect("the session has a conversation");
        let mut client = self.client().await;
        client.attach(1, agent.id()).await;
        client.type_into(1, format!("{line}\r").as_bytes()).await;
        eventually("the agent to take the line", async || {
            self.runs_of(&conversation)
                .into_iter()
                .find(|run| run.pid == agent.run.pid)
                .filter(|run| run.received.iter().any(|got| got == line))
                .map(|_| ())
        })
        .await;
        drop(client);
    }

    /// The child `parent` started under `name`.
    async fn child_of(&self, parent: &Agent, name: &str) -> Agent {
        let id = self
            .child_id(parent.id(), name)
            .unwrap_or_else(|| panic!("{} has no child named {name}", parent.name()));
        let record = self.record(id).await;
        self.agent_of(record, &[]).await
    }

    fn child_id(&self, parent: u64, name: &str) -> Option<u64> {
        self.db()
            .query_row(
                "SELECT id FROM sessions WHERE parent_session_id = ?1 AND child_name = ?2",
                rusqlite::params![i64::try_from(parent).unwrap(), name],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .and_then(|id| u64::try_from(id).ok())
    }

    async fn sessions(&self) -> Vec<SessionRecord> {
        let mut client = self.client().await;
        client
            .request::<SessionList>(&())
            .await
            .expect("the sessions are listed")
    }

    async fn record(&self, id: u64) -> SessionRecord {
        self.sessions()
            .await
            .into_iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("no session {id}"))
    }

    /// One column of session `id`'s row.
    fn column<T: rusqlite::types::FromSql>(&self, id: u64, column: &str) -> T {
        self.db()
            .query_row(
                &format!("SELECT {column} FROM sessions WHERE id = ?1"),
                [i64::try_from(id).unwrap()],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("read {column} of session {id}: {err}"))
    }

    /// How many rows were started by an agent tool.
    fn children_made(&self) -> i64 {
        self.db()
            .query_row(
                "SELECT count(*) FROM sessions WHERE child_name IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("count the children")
    }

    // -- runs ---------------------------------------------------------------

    /// Every run of a conversation either fake logged, by pid.
    fn runs_of(&self, conversation: &str) -> Vec<Run> {
        let mut runs: Vec<Run> = self
            .claude_logs()
            .iter()
            .filter(|log| log["sessionId"].as_str() == Some(conversation))
            .map(Run::of_claude)
            .collect();
        runs.extend(
            self.other
                .records()
                .iter()
                .filter(|record| record.conversation == conversation)
                .map(Run::of_other),
        );
        runs.sort_by_key(|run| run.pid);
        runs
    }

    fn claude_logs(&self) -> Vec<Value> {
        let Ok(entries) = std::fs::read_dir(self.claude.join("fake-agent")) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()? != "json" {
                    return None;
                }
                serde_json::from_slice(&std::fs::read(path).ok()?).ok()
            })
            .collect()
    }

    /// Every run of either fake: one more than before means a process started.
    fn run_count(&self) -> usize {
        self.claude_logs().len() + self.other.records().len()
    }
}

fn request(folder: &Path, name: &str, backend: &str) -> StartSessionRequest {
    StartSessionRequest {
        cwd: text(folder),
        cols: 80,
        rows: 24,
        name: Some(name.to_owned()),
        backend: Some(BackendId::new(backend)),
        ..StartSessionRequest::default()
    }
}

// ---------------------------------------------------------------------------
// A session, and what it was given
// ---------------------------------------------------------------------------

/// One run of either fake, as it logged itself.
#[derive(Debug, Clone)]
struct Run {
    pid: u32,
    argv: Vec<String>,
    opening_prompt: Option<String>,
    /// The instructions it loaded from the launch's own file, when it did.
    instructions: Option<String>,
    /// The tool servers it loaded, by name: URL and bearer token.
    servers: BTreeMap<String, (String, String)>,
    received: Vec<String>,
}

impl Run {
    fn of_claude(log: &Value) -> Self {
        let servers = log["mcpServers"]
            .as_object()
            .map(|servers| {
                servers
                    .iter()
                    .filter_map(|(name, server)| {
                        let url = server["url"].as_str()?.to_owned();
                        let token = server["headers"]["Authorization"]
                            .as_str()?
                            .strip_prefix("Bearer ")?
                            .to_owned();
                        Some((name.clone(), (url, token)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            pid: u32::try_from(log["pid"].as_u64().expect("a pid")).unwrap(),
            argv: strings(&log["argv"]),
            opening_prompt: log["openingPrompt"].as_str().map(str::to_owned),
            instructions: log["composedInstructions"].as_str().map(str::to_owned),
            servers,
            received: strings(&log["received"]),
        }
    }

    fn of_other(record: &OtherAgentRecord) -> Self {
        Self {
            pid: record.pid,
            argv: record.argv.clone(),
            opening_prompt: record.opening_prompt.clone(),
            instructions: record.instructions.clone(),
            servers: record
                .mcp_servers
                .iter()
                .map(|(name, server)| (name.clone(), (server.url.clone(), server.bearer.clone())))
                .collect(),
            received: record.received.clone(),
        }
    }

    /// The tools and commands its argv lets it use without asking.
    fn allowed(&self) -> Vec<String> {
        let mut allowed = values_after(&self.argv, "--allowedTools");
        allowed.extend(every_value_of(&self.argv, "--allow-tool"));
        allowed.extend(every_value_of(&self.argv, "--allow-shell"));
        allowed
    }

    /// The tools its argv never lets it use.
    fn disallowed(&self) -> Vec<String> {
        values_after(&self.argv, "--disallowedTools")
    }

    fn model(&self) -> Option<&str> {
        value_of(&self.argv, "--model")
    }

    fn effort(&self) -> Option<&str> {
        value_of(&self.argv, "--effort")
    }
}

/// A session and the run it started, with the token it was given.
#[derive(Debug, Clone)]
struct Agent {
    record: SessionRecord,
    run: Run,
}

/// A tool's answer: whether it failed, and the blocks of text it gave.
#[derive(Debug)]
struct Called {
    failed: bool,
    blocks: Vec<String>,
}

impl Called {
    fn text(&self) -> &str {
        self.blocks.first().map_or("", String::as_str)
    }

    /// The JSON block of an answer that succeeded.
    fn done(&self) -> Value {
        assert!(!self.failed, "the call was refused: {self:?}");
        serde_json::from_str(self.blocks.get(1).expect("a JSON block")).expect("JSON")
    }

    /// The sentence of an answer that was refused.
    fn refused(&self) -> &str {
        assert!(self.failed, "the call was not refused: {self:?}");
        self.text()
    }
}

impl Agent {
    fn id(&self) -> u64 {
        self.record.id
    }

    fn name(&self) -> &str {
        &self.record.name
    }

    /// The agents family's URL and the session's token.
    fn route(&self) -> (String, String) {
        self.run
            .servers
            .get(FAMILY)
            .cloned()
            .expect("the session was given the agents family")
    }

    async fn call(&self, tool: &str, arguments: Value) -> Called {
        let (url, token) = self.route();
        called(&url, &token, tool, arguments).await
    }

    async fn start(&self, arguments: Value) -> Called {
        self.call("agents_start", arguments).await
    }

    async fn list(&self) -> Called {
        self.call("agents_list", json!({})).await
    }

    async fn agents(&self) -> Called {
        self.call("agents_available", json!({})).await
    }

    /// Each agent `agents_available` lists, by name: its JSON entry.
    async fn agent_entries(&self) -> BTreeMap<String, Value> {
        self.agents().await.done()["agents"]
            .as_array()
            .expect("a list of agents")
            .iter()
            .map(|agent| {
                (
                    agent["name"].as_str().expect("a name").to_owned(),
                    agent.clone(),
                )
            })
            .collect()
    }

    /// The names `agents_available` lists, in the order it lists them.
    async fn agent_names(&self) -> Vec<String> {
        self.agents().await.done()["agents"]
            .as_array()
            .expect("a list of agents")
            .iter()
            .map(|agent| agent["name"].as_str().expect("a name").to_owned())
            .collect()
    }
}

/// The backends of one `agents_available` entry.
fn backends_of(agent: &Value) -> Vec<String> {
    strings(&agent["backends"])
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The fake `claude`, run by Node.
fn fake_claude() -> PathBuf {
    resolved(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("test")
            .join("fake-agents")
            .join("claude-code.mjs"),
    )
}

/// `path` with links resolved, in the form a process reports its own folder
/// in (no `\\?\` on Windows).
fn resolved(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(plain) => PathBuf::from(plain),
        None => canonical,
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The arguments after `flag` up to the next flag.
fn values_after(argv: &[String], flag: &str) -> Vec<String> {
    argv.iter()
        .skip_while(|arg| *arg != flag)
        .skip(1)
        .take_while(|arg| !arg.starts_with("--"))
        .cloned()
        .collect()
}

/// The argument after each occurrence of `flag`.
fn every_value_of(argv: &[String], flag: &str) -> Vec<String> {
    argv.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
        .collect()
}

/// The argument after the first `flag`.
fn value_of<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

/// Waits until `check` passes, retrying a failure for the patience.
async fn eventually<T>(what: &str, mut check: impl AsyncFnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(found) = check().await {
            return found;
        }
        assert!(Instant::now() < deadline, "{what} within {PATIENCE:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// HTTP, as an MCP client speaks it
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn result(&self) -> Value {
        assert_eq!(self.status, 200, "{self:?}");
        let answer: Value = serde_json::from_slice(&self.body)
            .unwrap_or_else(|err| panic!("{err}: {}", String::from_utf8_lossy(&self.body)));
        assert!(answer.get("error").is_none(), "{answer}");
        answer["result"].clone()
    }
}

/// `http://<address><path>` split.
fn split_url(url: &str) -> (SocketAddr, String) {
    let rest = url.strip_prefix("http://").expect("an http URL");
    let (address, path) = rest.split_at(rest.find('/').expect("a path"));
    (address.parse().expect("an address"), path.to_owned())
}

/// One POST of `body` to `url`, carrying `token` when there is one.
async fn post(url: &str, token: Option<&str>, body: &Value) -> Reply {
    let (address, path) = split_url(url);
    let body = body.to_string();
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\
         Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("\r\n");
    tokio::time::timeout(PATIENCE, async {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream.write_all(head.as_bytes()).await.expect("send");
        stream.write_all(body.as_bytes()).await.expect("send");
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.expect("read");
        parse_reply(&answer)
    })
    .await
    .expect("an answer within the patience")
}

fn parse_reply(answer: &[u8]) -> Reply {
    let end = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a head");
    let head = String::from_utf8_lossy(&answer[..end]).into_owned();
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status");
    let chunked = head.lines().any(|line| {
        line.to_ascii_lowercase()
            .starts_with("transfer-encoding: chunked")
    });
    let mut body = answer[end + 4..].to_vec();
    if chunked {
        let mut plain = Vec::new();
        let mut rest = body.as_slice();
        while let Some(at) = rest.windows(2).position(|window| window == b"\r\n") {
            let size = usize::from_str_radix(std::str::from_utf8(&rest[..at]).unwrap().trim(), 16)
                .expect("a chunk size");
            if size == 0 {
                break;
            }
            plain.extend_from_slice(&rest[at + 2..at + 2 + size]);
            rest = &rest[at + 2 + size + 2..];
        }
        body = plain;
    }
    Reply { status, body }
}

async fn rpc(url: &str, token: Option<&str>, method: &str, params: Value) -> Reply {
    post(
        url,
        token,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
    )
    .await
}

async fn called(url: &str, token: &str, tool: &str, arguments: Value) -> Called {
    let result = rpc(
        url,
        Some(token),
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
    )
    .await
    .result();
    Called {
        failed: result["isError"].as_bool().unwrap_or(false),
        blocks: result["content"]
            .as_array()
            .expect("content")
            .iter()
            .filter_map(|block| block["text"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// `floti agent`'s own code, run against the agents route with a token, off
/// the test's runtime: it speaks blocking HTTP, as the program does.
async fn subcommand(command: Subcommand, url: String, token: String) -> agent_cli::Outcome {
    tokio::task::spawn_blocking(move || agent_cli::execute(&command, &url, &token))
        .await
        .expect("the subcommand ran")
}

// ---------------------------------------------------------------------------
// The agent files these tests write
// ---------------------------------------------------------------------------

/// An agent on both fakes with a body, a model for each and an effort for the
/// first.
const TUNED: &str = "---\n\
description: Tuned for each backend.\n\
backend: claude-code, other-agent\n\
model:\n  claude-code: opus\n  other-agent: small\n\
effort:\n  claude-code: high\n\
---\n\
Work carefully.\n";

// ---------------------------------------------------------------------------
// S1: agents_available lists what there is
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s1_agents_available_lists_each_agent_with_what_it_is_and_the_backends_it_can_run_on() {
    let world = World::new();
    world.agent(
        "scout",
        "---\ndescription: Looks around.\nbackend: claude-code\nworktree: yes\ncan_start: yes\n---\nLook.\n",
    );
    world.agent("plain-reader", "---\ndescription: Reads.\n---\n");
    world.agent("codex-only", "---\nbackend: codex\n---\n");
    let host = world.host().await;
    let folder = world.project("listing");

    for backend in [CLAUDE, OTHER] {
        let parent = world
            .session(backend, &folder, &format!("parent {backend}"))
            .await;
        let asked = parent.agents().await;
        let listed = asked.done();

        // The default agent comes first, then the agents by name.
        let names = parent.agent_names().await;
        assert_eq!(
            names,
            ["default", "codex-only", "plain-reader", "scout"],
            "{backend}"
        );
        assert!(listed["bad"].as_array().unwrap().is_empty(), "{listed}");

        let agents = parent.agent_entries().await;
        let scout = &agents["scout"];
        assert_eq!(scout["description"], "Looks around.");
        assert_eq!(scout["worktree"], true);
        assert_eq!(scout["canStart"], true);
        assert_eq!(backends_of(scout), [CLAUDE]);
        assert!(scout.get("class").is_none(), "{scout}");
        assert_eq!(scout["unusedLines"], json!([]));

        // No backend named: every installed agent backend, in the host's order.
        let reader = &agents["plain-reader"];
        assert_eq!(reader["worktree"], false);
        assert_eq!(reader["canStart"], false);
        assert_eq!(backends_of(reader), [CLAUDE, OTHER]);
        assert_eq!(backends_of(&agents["default"]), [CLAUDE, OTHER]);

        // A backend that is not installed here runs nothing.
        assert!(backends_of(&agents["codex-only"]).is_empty());

        // The text a model reads names each agent and what it is.
        let text = asked.text();
        for word in [
            "scout",
            "Looks around.",
            "worktree yes",
            "plain-reader",
            "codex-only",
        ] {
            assert!(text.contains(word), "{word} in {text}");
        }
        assert!(!text.contains("class"), "{text}");
    }
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S2: every call reads the folder afresh
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s2_an_agent_file_added_edited_or_deleted_is_seen_by_the_next_listing_and_start() {
    let world = World::new();
    world.agent("existing", "---\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("fresh"), "parent")
        .await;
    assert_eq!(parent.agent_names().await, ["default", "existing"]);

    // Added: listed and started at once, with no restart.
    world.agent("added", "---\ndescription: One.\n---\nFirst words.\n");
    assert_eq!(parent.agent_names().await, ["default", "added", "existing"]);
    parent
        .start(json!({ "task": "go", "agent": "added", "name": "first" }))
        .await
        .done();
    let first = world.child_of(&parent, "first").await;
    assert!(
        first
            .run
            .instructions
            .as_deref()
            .unwrap()
            .contains("First words.")
    );

    // Edited: the listing and the next start have the new text.
    world.agent("added", "---\ndescription: Two.\n---\nSecond words.\n");
    assert_eq!(parent.agent_entries().await["added"]["description"], "Two.");
    parent
        .start(json!({ "task": "go", "agent": "added", "name": "second" }))
        .await
        .done();
    let second = world.child_of(&parent, "second").await;
    let composed = second.run.instructions.as_deref().unwrap();
    assert!(
        composed.contains("Second words.") && !composed.contains("First words."),
        "{composed}"
    );

    // Deleted: gone from the listing, and a start naming it is refused.
    world.remove_agent("added");
    assert_eq!(parent.agent_names().await, ["default", "existing"]);
    let refused = parent
        .start(json!({ "task": "go", "agent": "added" }))
        .await;
    assert!(
        refused.refused().contains("no agent named added"),
        "{refused:?}"
    );
    assert!(
        refused.refused().contains("existing"),
        "the agents are listed: {refused:?}"
    );

    // default.md replaces the built-in default agent.
    world.agent(
        "default",
        "---\ndescription: Mine.\ncan_start: yes\n---\nBe brief.\n",
    );
    assert_eq!(
        parent.agent_entries().await["default"]["description"],
        "Mine."
    );
    parent
        .start(json!({ "task": "go", "name": "default" }))
        .await
        .done();
    let default = world.child_of(&parent, "default").await;
    assert!(
        default
            .run
            .instructions
            .as_deref()
            .unwrap()
            .contains("Be brief.")
    );
    assert!(world.column::<bool>(default.id(), "can_start"));
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S3: a file that is not an agent
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s3_a_bad_agent_file_is_skipped_and_named_and_a_start_naming_it_is_refused() {
    let world = World::new();
    world.agent("fine", "---\ndescription: Fine.\n---\n");
    world.agent("typo", "---\ndisalowed_tools: Edit\n---\nbody\n");
    world.agent("broken", "---\ndescription: [unclosed\n---\n");
    world.agent(
        "odd",
        "---\nworktree: maybe\neffort:\n  claude-code: loud\n---\n",
    );
    world.agent("naked", "no front matter at all\n");
    let host = world.host().await;
    let parent = world.session(CLAUDE, &world.project("bad"), "parent").await;

    let asked = parent.agents().await;
    let listed = asked.done();
    assert_eq!(parent.agent_names().await, ["default", "fine"]);
    let bad: BTreeMap<String, String> = listed["bad"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bad| {
            (
                bad["file"].as_str().unwrap().to_owned(),
                bad["problem"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        bad.keys().map(String::as_str).collect::<Vec<_>>(),
        ["broken.md", "naked.md", "odd.md", "typo.md"]
    );
    assert!(bad["typo.md"].contains("disalowed_tools"), "{bad:?}");
    assert!(bad["broken.md"].contains("does not parse"), "{bad:?}");
    assert!(bad["naked.md"].contains("no front matter"), "{bad:?}");
    assert!(
        bad["odd.md"].contains("worktree must be")
            && bad["odd.md"].contains("effort for claude-code"),
        "every problem is named: {bad:?}"
    );
    // The text names each bad file and its problem too.
    for file in ["typo.md", "broken.md", "naked.md", "odd.md"] {
        assert!(asked.text().contains(file), "{file} in {}", asked.text());
    }
    assert!(asked.text().contains("disalowed_tools"));

    // A start naming one is refused, naming the file and the problem.
    let runs = world.run_count();
    let refused = parent.start(json!({ "task": "go", "agent": "typo" })).await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("typo.md") && sentence.contains("disalowed_tools"),
        "{sentence}"
    );
    assert!(sentence.contains("no agent was started"), "{sentence}");
    assert!(
        parent
            .start(json!({ "task": "go", "agent": "Naked" }))
            .await
            .refused()
            .contains("naked.md")
    );
    assert_eq!(world.children_made(), 0);
    assert_eq!(world.run_count(), runs, "no process started for a refusal");

    // A file that is fine still starts, and so does the default agent.
    parent
        .start(json!({ "task": "go", "agent": "fine", "name": "ok" }))
        .await
        .done();
    parent
        .start(json!({ "task": "go", "name": "default" }))
        .await
        .done();
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_a_bad_default_md_refuses_a_start_with_no_agent_and_does_not_fall_back_to_the_built_in()
{
    let world = World::new();
    world.agent("default", "---\nworktree: maybe\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("bad default"), "parent")
        .await;

    let refused = parent.start(json!({ "task": "go" })).await;
    assert!(refused.refused().contains("default.md"), "{refused:?}");
    assert!(
        parent.agent_names().await.is_empty(),
        "no agent to list but the bad file"
    );
    assert!(
        parent.agents().await.done()["bad"][0]["file"] == "default.md",
        "the bad file is named"
    );
    assert_eq!(world.children_made(), 0);
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S4: the body is instructions, never typed
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s4_an_agents_body_reaches_the_child_as_instructions_on_either_backend_and_is_never_typed()
{
    let world = World::new();
    world.agent(
        "briefed",
        "---\ndescription: Has a brief.\n---\nCheck the notes before you answer.\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("brief"), "parent")
        .await;

    for backend in [CLAUDE, OTHER] {
        let name = format!("kid-{backend}");
        parent
            .start(json!({ "task": "do the thing", "agent": "briefed", "backend": backend, "name": name }))
            .await
            .done();
        let child = world.child_of(&parent, &name).await;
        // The project has no overlays: the instructions are the agent's alone.
        let loaded = child
            .run
            .instructions
            .as_deref()
            .expect("instructions loaded");
        assert!(loaded.contains("# Agent: briefed"), "{backend}: {loaded}");
        assert!(
            loaded.contains("Check the notes before you answer."),
            "{backend}: {loaded}"
        );
        // The task is the only thing typed, and the body is never sent.
        assert_eq!(child.run.opening_prompt.as_deref(), Some("do the thing"));
        assert!(
            child.run.received.is_empty(),
            "{backend}: {:?}",
            child.run.received
        );
        assert!(
            !child
                .run
                .argv
                .iter()
                .any(|arg| arg.contains("Check the notes")),
            "{backend}: the body is not an argument: {:?}",
            child.run.argv
        );
    }

    // An agent with no body adds no instructions.
    world.agent("bare", "---\ndescription: No brief.\n---\n");
    parent
        .start(json!({ "task": "x", "agent": "bare", "name": "bare-kid" }))
        .await
        .done();
    let bare = world.child_of(&parent, "bare-kid").await;
    // Only what Floti adds to every child: the sections it composes.
    let told = bare.run.instructions.as_deref().unwrap_or_default();
    let headings: Vec<&str> = told.lines().filter(|line| line.starts_with("# ")).collect();
    assert_eq!(
        headings,
        [
            "# Started by another session",
            "# This session's title",
            "# Starting and waiting on agents from your shell"
        ],
        "{told}"
    );
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S5, S6: model and effort for the backend a child runs on
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s5_an_agents_model_and_effort_for_the_chosen_backend_replace_the_profiles() {
    let world = World::new();
    world.agent("tuned", TUNED);
    let host = world.host().await;
    let folder = world.project("tuned");
    world.profile("tuned", &folder, Some("profile-model"), Some("low"), None);
    let parent = world
        .session(CLAUDE, &world.project("elsewhere"), "parent")
        .await;
    let request = |backend: &str, name: &str| json!({ "task": "go", "agent": "tuned", "backend": backend, "name": name, "folder": text(&folder) });

    parent.start(request(CLAUDE, "on-claude")).await.done();
    let on_claude = world.child_of(&parent, "on-claude").await;
    assert_eq!(on_claude.run.model(), Some("opus"));
    assert_eq!(on_claude.run.effort(), Some("high"));

    // The other backend gets its own model and no effort at all.
    parent.start(request(OTHER, "on-other")).await.done();
    let on_other = world.child_of(&parent, "on-other").await;
    assert_eq!(on_other.run.model(), Some("small"));
    assert_eq!(on_other.run.effort(), None);

    // An agent that gives neither leaves the profile's choices as they are.
    world.agent("quiet", "---\nbackend: claude-code\n---\n");
    parent
        .start(
            json!({ "task": "go", "agent": "quiet", "name": "quiet-kid", "folder": text(&folder) }),
        )
        .await
        .done();
    let quiet = world.child_of(&parent, "quiet-kid").await;
    assert_eq!(quiet.run.model(), Some("profile-model"));
    assert_eq!(quiet.run.effort(), Some("low"));
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s6_an_effort_for_a_backend_with_no_effort_setting_refuses_the_start_naming_both() {
    let world = World::new();
    world.agent(
        "efforty",
        "---\nbackend: other-agent\neffort:\n  other-agent: high\n---\n",
    );
    world.agent(
        "either",
        "---\nbackend: other-agent, claude-code\neffort:\n  other-agent: high\n---\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("efforts"), "parent")
        .await;
    let runs = world.run_count();

    let refused = parent
        .start(json!({ "task": "go", "agent": "efforty" }))
        .await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("effort") && sentence.contains(OTHER),
        "{sentence}"
    );
    assert!(sentence.contains("no agent was started"), "{sentence}");
    // The same when the backend is asked for by name.
    let refused = parent
        .start(json!({ "task": "go", "agent": "either", "backend": OTHER }))
        .await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("effort") && sentence.contains(OTHER),
        "{sentence}"
    );
    assert_eq!(world.children_made(), 0);
    assert_eq!(world.run_count(), runs, "no process started for a refusal");

    // With no backend asked for, the first one that can run the agent is used,
    // and the listing shows the same.
    assert!(backends_of(&parent.agent_entries().await["efforty"]).is_empty());
    assert_eq!(
        backends_of(&parent.agent_entries().await["either"]),
        [CLAUDE]
    );
    parent
        .start(json!({ "task": "go", "agent": "either", "name": "picked" }))
        .await
        .done();
    let picked = world.child_of(&parent, "picked").await;
    assert_eq!(picked.record.backend, BackendId::new(CLAUDE));
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S7: permission, held back only by a parent launched to be
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s7_an_agents_permission_replaces_the_profiles_and_only_a_parent_in_plan_dont_ask_or_bypass_holds_it_back()
 {
    let world = World::new();
    world.agent(
        "ceiling",
        "---\nbackend: claude-code\npermission: bypass-permissions\n---\n",
    );
    world.agent("calm", "---\nbackend: claude-code\npermission: plan\n---\n");
    world.agent("silent", "---\nbackend: claude-code\n---\n");
    let host = world.host().await;
    let plain = world.project("plain");
    let profiled = world.project("profiled");
    world.profile("profiled", &profiled, None, None, Some("acceptEdits"));

    let mode_of = |child: &Agent| world.column::<Option<String>>(child.id(), "permission_mode");

    // A parent in plan: the agent asks for bypass, and the child plans.
    let planner = world
        .session_in(CLAUDE, &plain, "planner", PermissionMode::Plan)
        .await;
    planner
        .start(json!({ "task": "go", "agent": "ceiling", "name": "capped" }))
        .await
        .done();
    let capped = world.child_of(&planner, "capped").await;
    assert_eq!(mode_of(&capped).as_deref(), Some("plan"));
    assert_eq!(
        value_of(&capped.run.argv, "--permission-mode"),
        Some("plan")
    );

    // A parent in don't ask: the child gets no more, and an agent that names
    // nothing gets don't ask too.
    let quiet = world
        .session_in(CLAUDE, &plain, "quiet parent", PermissionMode::DontAsk)
        .await;
    quiet
        .start(json!({ "task": "go", "agent": "silent", "name": "quiet-kid" }))
        .await
        .done();
    assert_eq!(
        mode_of(&world.child_of(&quiet, "quiet-kid").await).as_deref(),
        Some("dontAsk")
    );

    // A parent in manual does not hold its child to manual, and bypass is
    // not its to give: the child runs in auto.
    let manual = world
        .session_in(CLAUDE, &plain, "manual parent", PermissionMode::Manual)
        .await;
    manual
        .start(json!({ "task": "go", "agent": "ceiling", "name": "manual-kid" }))
        .await
        .done();
    let manual_kid = world.child_of(&manual, "manual-kid").await;
    assert_eq!(mode_of(&manual_kid).as_deref(), Some("auto"));
    assert_eq!(
        value_of(&manual_kid.run.argv, "--permission-mode"),
        Some("auto")
    );

    // A parent launched in no mode: an agent that names none runs in auto.
    let unset = world.session(CLAUDE, &plain, "unset parent").await;
    unset
        .start(json!({ "task": "go", "agent": "silent", "name": "unset-kid" }))
        .await
        .done();
    let unset_kid = world.child_of(&unset, "unset-kid").await;
    assert_eq!(mode_of(&unset_kid).as_deref(), Some("auto"));
    assert_eq!(
        value_of(&unset_kid.run.argv, "--permission-mode"),
        Some("auto")
    );

    // A parent in bypass keeps what the agent asks for: nothing is above it.
    let bypasser = world
        .session_in(
            CLAUDE,
            &plain,
            "bypasser",
            PermissionMode::BypassPermissions,
        )
        .await;
    bypasser
        .start(json!({ "task": "go", "agent": "ceiling", "name": "free" }))
        .await
        .done();
    assert_eq!(
        mode_of(&world.child_of(&bypasser, "free").await).as_deref(),
        Some("bypassPermissions")
    );

    // The agent's permission replaces the profile's, then the cap applies.
    bypasser
        .start(
            json!({ "task": "go", "agent": "calm", "name": "replaced", "folder": text(&profiled) }),
        )
        .await
        .done();
    assert_eq!(
        mode_of(&world.child_of(&bypasser, "replaced").await).as_deref(),
        Some("plan")
    );
    // An agent that says nothing leaves the profile's permission.
    bypasser
        .start(
            json!({ "task": "go", "agent": "silent", "name": "kept", "folder": text(&profiled) }),
        )
        .await
        .done();
    assert_eq!(
        mode_of(&world.child_of(&bypasser, "kept").await).as_deref(),
        Some("acceptEdits")
    );
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S8: may the child start children
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s8_can_start_comes_from_the_agent_unless_the_start_says_otherwise_and_depth_still_holds() {
    let world = World::new();
    world.agent("boss", "---\nbackend: claude-code\ncan_start: yes\n---\n");
    world.agent("pawn", "---\nbackend: claude-code\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("depth"), "parent")
        .await;

    parent
        .start(json!({ "task": "lead", "agent": "boss", "name": "boss" }))
        .await
        .done();
    parent
        .start(json!({ "task": "work", "agent": "pawn", "name": "pawn" }))
        .await
        .done();
    parent
        .start(json!({ "task": "lead", "agent": "boss", "name": "muzzled", "can_start": false }))
        .await
        .done();
    parent
        .start(json!({ "task": "work", "agent": "pawn", "name": "promoted", "can_start": true }))
        .await
        .done();
    let boss = world.child_of(&parent, "boss").await;
    let pawn = world.child_of(&parent, "pawn").await;
    let muzzled = world.child_of(&parent, "muzzled").await;
    let promoted = world.child_of(&parent, "promoted").await;
    assert!(world.column::<bool>(boss.id(), "can_start"));
    assert!(!world.column::<bool>(pawn.id(), "can_start"));
    assert!(!world.column::<bool>(muzzled.id(), "can_start"));
    assert!(world.column::<bool>(promoted.id(), "can_start"));

    // A child that may not start agents is told so by the same limit as ever.
    let refused = pawn.start(json!({ "task": "more" })).await;
    assert!(
        refused.refused().contains("without can_start"),
        "{refused:?}"
    );
    let refused = muzzled.start(json!({ "task": "more" })).await;
    assert!(
        refused.refused().contains("without can_start"),
        "{refused:?}"
    );
    // One that may, starts its own.
    boss.start(json!({ "task": "help", "agent": "pawn", "name": "grandchild" }))
        .await
        .done();
    promoted
        .start(json!({ "task": "help", "agent": "pawn", "name": "grandchild" }))
        .await
        .done();
    assert_eq!(
        world.child_of(&boss, "grandchild").await.record.parent_id,
        Some(boss.id())
    );

    // The depth limit holds for an agent that says it may start agents.
    let grandchild = world.child_of(&boss, "grandchild").await;
    let refused = grandchild.start(json!({ "task": "deeper" })).await;
    assert!(refused.refused().contains("depth limit"), "{refused:?}");
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S9: a class line, no longer used, and worktree
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s9_an_agent_file_with_a_class_line_runs_like_any_and_the_listing_names_the_line_as_no_longer_used()
 {
    let world = World::new();
    let bulk_file = "---\nbackend: claude-code\nclass: heavy\nworktree: yes\n---\n";
    let nimble_file = "---\nbackend: claude-code\nclass: light\n---\n";
    world.agent("bulk", bulk_file);
    world.agent("nimble", nimble_file);
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.repository("retired lines"), "parent")
        .await;

    // Listed as agents, each naming its class line as one no longer used.
    let asked = parent.agents().await;
    let agents = parent.agent_entries().await;
    for name in ["bulk", "nimble"] {
        let agent = &agents[name];
        assert!(agent.get("class").is_none(), "{agent}");
        assert_eq!(agent["unusedLines"][0]["line"], "class", "{agent}");
        let sentence = format!("The class line in {name}.md is no longer used");
        assert!(
            agent["unusedLines"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with(&sentence),
            "{agent}"
        );
        assert!(asked.text().contains(&sentence), "{}", asked.text());
    }
    assert!(asked.done()["bad"].as_array().unwrap().is_empty());

    parent
        .start(json!({ "task": "build", "agent": "bulk", "name": "bulk-kid" }))
        .await
        .done();
    parent
        .start(json!({ "task": "look", "agent": "nimble", "name": "nimble-kid" }))
        .await
        .done();
    let bulk = world.child_of(&parent, "bulk-kid").await;
    let nimble = world.child_of(&parent, "nimble-kid").await;

    // Kept on the row beside the agent, as any child's is.
    assert!(world.column::<bool>(bulk.id(), "child_worktree"));
    assert_eq!(world.column::<String>(bulk.id(), "agent"), "bulk");
    assert!(!world.column::<bool>(nimble.id(), "child_worktree"));

    // Shown by agents_list with its agent and worktree, and no class.
    let listed = parent.list().await;
    let json = listed.done();
    let children = json["children"].as_array().unwrap();
    let find = |name: &str| children.iter().find(|child| child["name"] == name).unwrap();
    assert_eq!(find("bulk-kid")["agent"], "bulk");
    assert_eq!(find("bulk-kid")["worktree"], true);
    assert_eq!(find("nimble-kid")["worktree"], false);
    for child in children {
        assert!(child.get("class").is_none(), "{child}");
    }
    assert!(!listed.text().contains("class"), "{}", listed.text());
    assert!(!listed.text().contains("heavy"), "{}", listed.text());
    // The agent's worktree is named after the child; an agent without one runs
    // in the folder it was given.
    let expected = Path::new(&parent.record.cwd).join(".floti/worktrees/bulk-kid");
    assert_eq!(Path::new(&bulk.record.cwd), expected.as_path());
    assert_eq!(bulk.record.branch.as_deref(), Some("agent/bulk-kid"));
    assert_eq!(nimble.record.cwd, parent.record.cwd);

    // Nothing in the folder was rewritten.
    for (file, written) in [("bulk.md", bulk_file), ("nimble.md", nimble_file)] {
        let now = std::fs::read_to_string(world.agents_dir().join(file)).unwrap();
        assert_eq!(now, written, "{file}");
    }
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s9_two_children_whose_agent_file_says_class_heavy_run_side_by_side_held_only_by_the_generic_limits()
 {
    let world = World::new();
    world.agent("suite", "---\nbackend: claude-code\nclass: heavy\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("side by side"), "parent")
        .await;
    // Two running children per parent: the generic limit this test reaches.
    world
        .db()
        .execute(
            "INSERT INTO app_settings (key, value) VALUES ('agentChildrenPerParent', '2') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .unwrap();

    for name in ["first", "second"] {
        parent
            .start(json!({ "task": "run the suite", "agent": "suite", "name": name }))
            .await
            .done();
    }
    world.child_of(&parent, "first").await;
    world.child_of(&parent, "second").await;
    let listed = parent.list().await.done();
    let running: Vec<&str> = listed["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    assert_eq!(running, ["first", "second"], "{listed}");

    // A third is held by the limit on children per parent, and by nothing
    // that has to do with its agent.
    let refused = parent
        .start(json!({ "task": "run the suite", "agent": "suite", "name": "third" }))
        .await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("The limit of 2 running agents per parent holds"),
        "{sentence}"
    );
    assert!(
        sentence.contains("first") && sentence.contains("second"),
        "{sentence}"
    );
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S10: tool rules, on both backends
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s10_allowed_tools_become_each_backends_own_allow_form_beside_the_families() {
    let world = World::new();
    world.agent(
        "gated",
        "---\nbackend: claude-code, other-agent\nallowed_tools: Read, Bash(git status:*)\n---\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("allow"), "parent")
        .await;

    parent
        .start(json!({ "task": "go", "agent": "gated", "backend": CLAUDE, "name": "on-claude" }))
        .await
        .done();
    let on_claude = world.child_of(&parent, "on-claude").await;
    let allowed = on_claude.run.allowed();
    for rule in [
        "Read",
        "Bash(git status:*)",
        "mcp__floti-agents__agents_start",
    ] {
        assert!(
            allowed.iter().any(|got| got == rule),
            "{rule} in {allowed:?}"
        );
    }

    parent
        .start(json!({ "task": "go", "agent": "gated", "backend": OTHER, "name": "on-other" }))
        .await
        .done();
    let on_other = world.child_of(&parent, "on-other").await;
    let allowed = on_other.run.allowed();
    for rule in ["read", "git status", "floti-agents.agents_start"] {
        assert!(
            allowed.iter().any(|got| got == rule),
            "{rule} in {allowed:?}"
        );
    }
    assert_eq!(
        on_other
            .run
            .argv
            .iter()
            .filter(|arg| *arg == "--allow-shell")
            .count(),
        // The agent's `git status`, floti agent start, send, wait, read and
        // stop, and floti session title.
        7
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s10_disallowed_tools_are_passed_to_claude_and_refuse_a_backend_that_cannot_enforce_them() {
    let world = World::new();
    world.agent(
        "locked-down",
        "---\nbackend: claude-code\ndisallowed_tools: Edit, Write\n---\n",
    );
    world.agent(
        "either-way",
        "---\nbackend: other-agent, claude-code\ndisallowed_tools: Bash(rm:*)\n---\n",
    );
    world.agent(
        "other-only",
        "---\nbackend: other-agent\ndisallowed_tools: Edit\n---\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("deny"), "parent")
        .await;
    let runs = world.run_count();

    parent
        .start(json!({ "task": "go", "agent": "locked-down", "name": "denied" }))
        .await
        .done();
    let denied = world.child_of(&parent, "denied").await;
    assert_eq!(denied.run.disallowed(), ["Edit", "Write"]);

    // The other backend has no flag that disallows a tool: refused, naming it
    // and the rule, with nothing started.
    let refused = parent
        .start(json!({ "task": "go", "agent": "other-only" }))
        .await;
    let sentence = refused.refused();
    assert!(
        sentence.contains(OTHER) && sentence.contains("disallowed_tools rule Edit"),
        "{sentence}"
    );
    let refused = parent
        .start(json!({ "task": "go", "agent": "either-way", "backend": OTHER }))
        .await;
    assert!(
        refused
            .refused()
            .contains("disallowed_tools rule Bash(rm:*)"),
        "{refused:?}"
    );
    assert_eq!(world.run_count(), runs + 1, "only the allowed one started");

    // With no backend asked for, the first that can enforce it runs it, and
    // the listing shows only that one.
    let agents = parent.agent_entries().await;
    assert_eq!(backends_of(&agents["either-way"]), [CLAUDE]);
    assert!(backends_of(&agents["other-only"]).is_empty());
    parent
        .start(json!({ "task": "go", "agent": "either-way", "name": "picked" }))
        .await
        .done();
    let picked = world.child_of(&parent, "picked").await;
    assert_eq!(picked.record.backend, BackendId::new(CLAUDE));
    assert_eq!(picked.run.disallowed(), ["Bash(rm:*)"]);
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S11, S12: the folder is seeded once
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s11_a_host_started_with_no_agents_folder_writes_the_shipped_agents_and_lists_them() {
    let world = World::new();
    assert!(!world.agents_dir().exists());
    let host = world.host().await;

    let mut files: Vec<String> = std::fs::read_dir(world.agents_dir())
        .expect("the agents folder was made")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        ["lead.md", "researcher.md", "reviewer.md", "worker.md"]
    );

    let parent = world
        .session(CLAUDE, &world.project("seeded"), "parent")
        .await;
    assert_eq!(
        parent.agent_names().await,
        ["default", "lead", "researcher", "reviewer", "worker"]
    );
    let agents = parent.agent_entries().await;
    assert_eq!(agents["lead"]["canStart"], true);
    assert_eq!(agents["worker"]["worktree"], true);
    assert_eq!(agents["reviewer"]["canStart"], false);
    assert_eq!(agents["researcher"]["canStart"], false);
    for name in ["lead", "researcher", "reviewer", "worker"] {
        assert_eq!(backends_of(&agents[name]), [CLAUDE], "{name}");
    }
    assert!(
        parent.agents().await.done()["bad"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s12_a_later_start_never_rewrites_the_folder_an_edit_a_delete_and_an_empty_folder_stay() {
    let world = World::new();
    let host = world.host().await;
    host.stop(HostStopReason::Stop).await;

    // Edited stays edited, deleted stays deleted, and nothing new appears.
    let edited = "---\ndescription: Mine.\n---\nMy own words.\n";
    std::fs::write(world.agents_dir().join("worker.md"), edited).unwrap();
    world.remove_agent("lead");
    let host = world.host().await;
    assert_eq!(
        std::fs::read_to_string(world.agents_dir().join("worker.md")).unwrap(),
        edited
    );
    assert!(!world.agents_dir().join("lead.md").exists());
    assert!(world.agents_dir().join("reviewer.md").exists());
    host.stop(HostStopReason::Stop).await;

    // An emptied folder stays empty.
    for name in ["worker", "reviewer", "researcher"] {
        world.remove_agent(name);
    }
    let host = world.host().await;
    assert_eq!(std::fs::read_dir(world.agents_dir()).unwrap().count(), 0);
    let parent = world
        .session(CLAUDE, &world.project("emptied"), "parent")
        .await;
    assert_eq!(parent.agent_names().await, ["default"]);
    host.stop(HostStopReason::Stop).await;

    // Deleting the folder brings the originals back at the next start.
    std::fs::remove_dir_all(world.agents_dir()).unwrap();
    let host = world.host().await;
    assert!(world.agents_dir().join("lead.md").exists());
    assert_eq!(
        std::fs::read_dir(world.agents_dir()).unwrap().count(),
        4,
        "the four shipped agents"
    );
    host.stop(HostStopReason::Stop).await;
}

/// Every file under `dir`, by its path from `dir`, with its bytes.
fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).expect("read a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path
                    .strip_prefix(dir)
                    .expect("under the folder")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((name, std::fs::read(&path).expect("read a file")));
            }
        }
    }
    found.sort();
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn s12_a_roles_folder_with_no_agents_folder_is_moved_at_start_and_lists_the_same_agents() {
    let world = World::new();
    let roles = world.dirs.config.join("roles");
    let edited = "---\ndescription: My lead.\ncan_start: yes\n---\nMy own words.\r\n";
    for (file, content) in [
        ("lead.md", edited),
        ("scout.md", "---\ndescription: Looks around.\n---\nLook.\n"),
        ("typo.md", "---\ndisalowed_tools: Edit\n---\n"),
        ("notes.txt", "not an agent"),
    ] {
        std::fs::create_dir_all(&roles).unwrap();
        std::fs::write(roles.join(file), content).unwrap();
    }
    let before = files_in(&roles);
    assert!(!world.agents_dir().exists());

    let host = world.host().await;
    assert!(!roles.exists(), "the roles folder was moved");
    assert_eq!(
        files_in(&world.agents_dir()),
        before,
        "every file as it was, and nothing seeded beside them"
    );
    let parent = world
        .session(CLAUDE, &world.project("moved"), "parent")
        .await;
    assert_eq!(parent.agent_names().await, ["default", "lead", "scout"]);
    let listed = parent.agents().await.done();
    assert_eq!(listed["bad"][0]["file"], "typo.md", "{listed}");
    assert_eq!(
        parent.agent_entries().await["lead"]["description"],
        "My lead."
    );
    host.stop(HostStopReason::Stop).await;

    // The next start finds the agents folder and moves nothing.
    let host = world.host().await;
    assert!(!roles.exists());
    assert_eq!(files_in(&world.agents_dir()), before);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s12_beside_an_agents_folder_a_roles_folder_is_left_alone_and_not_read() {
    let world = World::new();
    let roles = world.dirs.config.join("roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(roles.join("old.md"), "---\n---\nOld.\n").unwrap();
    world.agent("new", "---\n---\nNew.\n");
    let (roles_before, agents_before) = (files_in(&roles), files_in(&world.agents_dir()));

    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("both"), "parent")
        .await;
    assert_eq!(parent.agent_names().await, ["default", "new"]);
    assert_eq!(files_in(&roles), roles_before);
    assert_eq!(files_in(&world.agents_dir()), agents_before);
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S13, S14: the shipped lead, worker, reviewer and researcher
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s13_the_shipped_lead_starts_agents_cannot_edit_and_carries_its_voice_and_tracker_rules() {
    let world = World::new();
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.repository("lead"), "parent")
        .await;

    parent
        .start(json!({ "task": "run the board", "agent": "lead", "name": "lead" }))
        .await
        .done();
    let lead = world.child_of(&parent, "lead").await;
    assert!(world.column::<bool>(lead.id(), "can_start"));
    assert!(!world.column::<bool>(lead.id(), "child_worktree"));
    assert_eq!(lead.run.disallowed(), ["Edit", "Write", "NotebookEdit"]);
    // It names no mode, so it runs in a child's own, auto: nobody is at its
    // terminal to answer a prompt.
    assert_eq!(value_of(&lead.run.argv, "--permission-mode"), Some("auto"));
    let instructions = lead
        .run
        .instructions
        .as_deref()
        .expect("the lead's instructions");
    for part in [
        "# Agent: lead",
        "Start a worker",
        "## Keeping the tracker",
        "## Voice",
        "ABC-12",
    ] {
        assert!(instructions.contains(part), "{part} in {instructions}");
    }
    assert_eq!(lead.run.opening_prompt.as_deref(), Some("run the board"));
    assert!(lead.run.received.is_empty());

    // It starts a worker of its own, which is what it is for.
    lead.start(json!({ "task": "do ABC-12", "agent": "worker", "name": "helper" }))
        .await
        .done();
    assert_eq!(
        world.child_of(&lead, "helper").await.record.parent_id,
        Some(lead.id())
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s14_the_shipped_worker_edits_in_a_worktree_and_cannot_start_agents() {
    let world = World::new();
    let host = world.host().await;
    // A parent in accept edits holds its worker back to nothing.
    let parent = world
        .session_in(
            CLAUDE,
            &world.repository("worker"),
            "parent",
            PermissionMode::AcceptEdits,
        )
        .await;

    parent
        .start(json!({ "task": "do ABC-12", "agent": "worker", "name": "worker" }))
        .await
        .done();
    let worker = world.child_of(&parent, "worker").await;
    assert!(world.column::<bool>(worker.id(), "child_worktree"));
    assert_eq!(
        Path::new(&worker.record.cwd),
        Path::new(&parent.record.cwd)
            .join(".floti/worktrees/worker")
            .as_path()
    );
    assert_eq!(worker.record.branch.as_deref(), Some("agent/worker"));
    assert!(!world.column::<bool>(worker.id(), "can_start"));
    assert_eq!(
        value_of(&worker.run.argv, "--permission-mode"),
        Some("auto")
    );
    assert!(worker.run.disallowed().is_empty(), "{:?}", worker.run.argv);
    let instructions = worker
        .run
        .instructions
        .as_deref()
        .expect("the worker's instructions");
    for part in [
        "# Agent: worker",
        "Touch only that item",
        "Also fixed:",
        "Found:",
    ] {
        assert!(instructions.contains(part), "{part} in {instructions}");
    }
    assert_eq!(worker.run.opening_prompt.as_deref(), Some("do ABC-12"));

    // It may not start agents of its own.
    let refused = worker.start(json!({ "task": "more" })).await;
    assert!(
        refused.refused().contains("without can_start"),
        "{refused:?}"
    );

    // The parent's view of it.
    let listed = parent.list().await.done();
    assert_eq!(listed["children"][0]["worktree"], true);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s14_the_shipped_reviewer_and_researcher_launch_unable_to_edit_in_a_childs_own_mode() {
    let world = World::new();
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.repository("look"), "parent")
        .await;

    for (agent, disallowed, words) in [
        (
            "researcher",
            &["Edit", "NotebookEdit"][..],
            "change no file of the project",
        ),
        (
            "reviewer",
            &["Edit", "Write", "NotebookEdit"][..],
            "change nothing",
        ),
    ] {
        parent
            .start(json!({ "task": "find out about ABC-12", "agent": agent, "name": agent }))
            .await
            .done();
        let child = world.child_of(&parent, agent).await;
        assert_eq!(child.run.disallowed(), disallowed, "{agent}");
        // It names no mode, so it runs in a child's own, which asks nothing.
        assert_eq!(
            value_of(&child.run.argv, "--permission-mode"),
            Some("auto"),
            "{agent}"
        );
        assert!(!world.column::<bool>(child.id(), "can_start"), "{agent}");
        assert!(
            !world.column::<bool>(child.id(), "child_worktree"),
            "{agent}"
        );
        let instructions = child.run.instructions.as_deref().unwrap_or_default();
        for part in [format!("# Agent: {agent}"), words.to_owned()] {
            assert!(instructions.contains(&part), "{part} in {instructions}");
        }
    }
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S16: allowed up front, and a deny rule still wins
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s16_agents_available_is_allowed_up_front_with_the_other_tools_and_a_deny_rule_still_wins()
{
    let world = World::new();
    world.agent(
        "no-agents",
        "---\nbackend: claude-code\ndisallowed_tools: mcp__floti-agents__agents_available\n---\n",
    );
    let host = world.host().await;
    let folder = world.project("up front");

    for (backend, rule) in [
        (CLAUDE, "mcp__floti-agents__agents_available"),
        (OTHER, "floti-agents.agents_available"),
    ] {
        let parent = world
            .session(backend, &folder, &format!("parent {backend}"))
            .await;
        let allowed = parent.run.allowed();
        assert!(
            allowed.iter().any(|got| got == rule),
            "{backend}: {rule} in {allowed:?}"
        );
        // It is a tool and not a command: no `floti agent available`.
        assert!(
            !allowed.iter().any(|got| got.contains("agent available")),
            "{backend}: {allowed:?}"
        );
        // Nothing is allowed by a wildcard over the family.
        assert!(
            !allowed.iter().any(|got| got.ends_with("floti-agents__*")),
            "{allowed:?}"
        );
        assert_eq!(parent.run.disallowed(), Vec::<String>::new());
        let (url, token) = parent.route();
        let tools = rpc(&url, Some(&token), "tools/list", json!({}))
            .await
            .result();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"agents_available"), "{names:?}");
    }

    // A deny rule of an agent stays beside the allow rule.
    let parent = world.session(CLAUDE, &folder, "parent").await;
    parent
        .start(json!({ "task": "go", "agent": "no-agents", "name": "denied" }))
        .await
        .done();
    let denied = world.child_of(&parent, "denied").await;
    assert_eq!(
        denied.run.disallowed(),
        ["mcp__floti-agents__agents_available"]
    );
    assert!(
        denied
            .run
            .allowed()
            .iter()
            .any(|got| got == "mcp__floti-agents__agents_available")
    );

    // The subcommand set has no command for the listing.
    for word in ["available", "agents", "roles"] {
        assert!(agent_cli::parse(&[word.to_owned()]).is_err(), "{word}");
    }
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S17: only the backends that are installed and can run it
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s17_an_agent_runs_on_the_installed_backends_it_lists_and_a_start_says_when_there_are_none()
{
    let world = World::new();
    world.agent("codex-first", "---\nbackend: codex, claude-code\n---\n");
    world.agent("gone", "---\nbackend: codex\n---\n");
    let host = world.host().await;
    let parent = world
        .session(OTHER, &world.project("installed"), "parent")
        .await;

    let agents = parent.agent_entries().await;
    assert_eq!(backends_of(&agents["codex-first"]), [CLAUDE]);
    assert!(backends_of(&agents["gone"]).is_empty());

    // The first one the agent lists that is installed.
    parent
        .start(json!({ "task": "go", "agent": "codex-first", "name": "first" }))
        .await
        .done();
    assert_eq!(
        world.child_of(&parent, "first").await.record.backend,
        BackendId::new(CLAUDE)
    );

    // One that is listed and not installed.
    let refused = parent
        .start(json!({ "task": "go", "agent": "codex-first", "backend": "codex" }))
        .await;
    assert!(
        refused.refused().contains("codex") && refused.refused().contains("allows: claude-code."),
        "{refused:?}"
    );
    // One that is installed and not listed.
    let refused = parent
        .start(json!({ "task": "go", "agent": "codex-first", "backend": OTHER }))
        .await;
    assert!(refused.refused().contains(OTHER), "{refused:?}");
    // An agent none of whose backends is here.
    let refused = parent.start(json!({ "task": "go", "agent": "gone" })).await;
    assert!(
        refused
            .refused()
            .contains("None of the backends the agent gone lists"),
        "{refused:?}"
    );
    assert_eq!(world.children_made(), 1);

    // An agent that lists no backend is for every installed agent backend.
    world.agent("anywhere", "---\n---\n");
    assert_eq!(
        backends_of(&parent.agent_entries().await["anywhere"]),
        [CLAUDE, OTHER]
    );
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// S18: the agent's profile
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn s18_an_agent_names_the_profile_its_children_run_under_and_a_missing_one_refuses_the_start()
{
    let world = World::new();
    world.agent(
        "profiled",
        "---\nbackend: claude-code\nprofile: Roleprof\n---\n",
    );
    world.agent(
        "lost",
        "---\nbackend: claude-code\nprofile: Nonesuch\n---\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("profiles"), "parent")
        .await;
    let elsewhere = world.project("profile root");
    let profile = world.profile(
        "Roleprof",
        &elsewhere,
        Some("profile-model"),
        None,
        Some("plan"),
    );
    world.profile(
        "Asked",
        &world.project("asked root"),
        Some("asked-model"),
        None,
        None,
    );

    // The agent's profile, even though the folder is another profile's.
    parent
        .start(json!({ "task": "go", "agent": "profiled", "name": "by-agent" }))
        .await
        .done();
    let by_agent = world.child_of(&parent, "by-agent").await;
    assert_eq!(world.column::<i64>(by_agent.id(), "profile_id"), profile);
    assert_eq!(by_agent.run.model(), Some("profile-model"));
    assert_eq!(
        value_of(&by_agent.run.argv, "--permission-mode"),
        Some("plan")
    );

    // A profile named in the start wins over the agent's.
    parent
        .start(json!({ "task": "go", "agent": "profiled", "name": "by-ask", "profile": "Asked" }))
        .await
        .done();
    let by_ask = world.child_of(&parent, "by-ask").await;
    assert_eq!(by_ask.run.model(), Some("asked-model"));

    // A profile that is not there refuses the start, naming file and profile.
    let runs = world.run_count();
    let refused = parent.start(json!({ "task": "go", "agent": "lost" })).await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("lost.md") && sentence.contains("Nonesuch"),
        "{sentence}"
    );
    assert!(sentence.contains("No agent was started"), "{sentence}");
    assert_eq!(world.run_count(), runs);
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// Both backends, floti agent, and a child reopened
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn floti_agent_start_and_stop_take_an_agent_and_agents_available_is_called_from_either_backend()
 {
    let world = World::new();
    world.agent(
        "briefed",
        "---\ndescription: Has a brief.\nbackend: claude-code, other-agent\n---\nBe thorough.\n",
    );
    let host = world.host().await;
    let folder = world.project("cli");

    for (backend, other) in [(CLAUDE, OTHER), (OTHER, CLAUDE)] {
        let parent = world
            .session(backend, &folder, &format!("parent {backend}"))
            .await;
        let (url, token) = parent.route();
        let started = subcommand(
            Subcommand::Start(StartCommand {
                task: "by the command".to_owned(),
                agent: Some("briefed".to_owned()),
                backend: Some(other.to_owned()),
                name: Some("by-cli".to_owned()),
                ..StartCommand::default()
            }),
            url.clone(),
            token.clone(),
        )
        .await;
        assert_eq!(started.code, 0, "{started:?}");
        let child = world.child_of(&parent, "by-cli").await;
        assert_eq!(child.record.backend, BackendId::new(other));
        assert!(
            child
                .run
                .instructions
                .as_deref()
                .unwrap()
                .contains("Be thorough.")
        );
        assert_eq!(child.run.opening_prompt.as_deref(), Some("by the command"));

        // An agent that does not exist is a refusal the command prints.
        let refused = subcommand(
            Subcommand::Start(StartCommand {
                task: "x".to_owned(),
                agent: Some("wizard".to_owned()),
                ..StartCommand::default()
            }),
            url.clone(),
            token.clone(),
        )
        .await;
        assert_eq!(refused.code, 1, "{refused:?}");
        assert!(refused.err.unwrap().contains("no agent named wizard"));

        // The agents, over the same route.
        assert!(parent.agent_names().await.contains(&"briefed".to_owned()));

        let stopped = subcommand(
            Subcommand::Stop(StopCommand {
                child: "by-cli".to_owned(),
                keep: false,
                json: false,
            }),
            url,
            token,
        )
        .await;
        assert_eq!(stopped.code, 0, "{stopped:?}");
        let id = child.id();
        eventually("the child to stop", async || {
            (world.column::<String>(id, "status") != "running").then_some(())
        })
        .await;
    }
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s2_a_lost_child_is_reopened_in_its_agent_as_the_file_reads_when_it_comes_back() {
    let world = World::new();
    world.agent("edited", "---\nbackend: claude-code\n---\nFirst version.\n");
    world.agent(
        "broken",
        "---\nbackend: claude-code\n---\nWill be broken.\n",
    );
    world.agent("gone", "---\nbackend: claude-code\n---\nWill be deleted.\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("reopen"), "parent")
        .await;
    for agent in ["edited", "broken", "gone"] {
        parent
            .start(json!({ "task": "work", "agent": agent, "name": agent }))
            .await
            .done();
    }
    let mut kids = BTreeMap::new();
    for agent in ["edited", "broken", "gone"] {
        kids.insert(agent, world.child_of(&parent, agent).await);
    }
    assert!(
        kids["edited"]
            .run
            .instructions
            .as_deref()
            .unwrap()
            .contains("First version.")
    );
    // Each has been worked with, so there is a conversation to resume.
    for kid in kids.values() {
        world.type_line(kid, "carry on").await;
    }

    // The host stops for an update: every row is left for the next start.
    let mut client = world.client().await;
    client
        .request::<HostStop>(&HostStopRequest {
            reason: HostStopReason::Update,
        })
        .await
        .expect("the stop is taken");
    drop(client);
    assert_eq!(host.stopped().await, HostStopReason::Update);

    // The files change while it is down.
    world.agent(
        "edited",
        "---\nbackend: claude-code\n---\nSecond version.\n",
    );
    world.agent("broken", "---\nworktree: maybe\n---\nStill here.\n");
    world.remove_agent("gone");

    let host = world.host().await;
    let mut client = world.client().await;
    let result = client
        .request::<SessionRestore>(&RestoreSessionsRequest {
            sessions: ["edited", "broken", "gone"]
                .iter()
                .map(|agent| SessionToRestore {
                    id: kids[agent].id(),
                    cols: 80,
                    rows: 24,
                    without_agent: false,
                })
                .collect(),
        })
        .await
        .expect("the restore is taken");
    drop(client);

    // The edited agent composes the reopened child again, as it reads now.
    assert_eq!(result.restored.len(), 2, "{result:?}");
    let edited = result
        .restored
        .iter()
        .find(|restored| restored.from == kids["edited"].id())
        .expect("the edited child came back");
    let again = world
        .agent_of(edited.launched.session.clone(), &[kids["edited"].run.pid])
        .await;
    let composed = again
        .run
        .instructions
        .as_deref()
        .expect("the agent's instructions");
    assert!(composed.contains("Second version."), "{composed}");
    assert!(!composed.contains("First version."), "{composed}");
    assert!(
        again.run.argv.iter().any(|arg| arg == "--resume"),
        "{:?}",
        again.run.argv
    );

    // An agent whose file cannot be used refuses the reopen, naming the file.
    assert_eq!(result.failed.len(), 1, "{result:?}");
    assert_eq!(result.failed[0].id, kids["broken"].id());
    assert!(
        result.failed[0].reason.contains("broken.md"),
        "{:?}",
        result.failed[0]
    );
    assert!(result.failed[0].reason.contains("worktree must be"));

    // An agent that is gone reopens without it, and says so.
    let gone = result
        .restored
        .iter()
        .find(|restored| restored.from == kids["gone"].id())
        .expect("the child of the deleted agent came back");
    assert!(
        gone.launched
            .warnings
            .iter()
            .any(|warning| warning.contains("no longer exists")),
        "{:?}",
        gone.launched.warnings
    );
    let reopened = world
        .agent_of(gone.launched.session.clone(), &[kids["gone"].run.pid])
        .await;
    // Floti's own guide is all it is given: nothing of the deleted agent's body.
    let told = reopened.run.instructions.as_deref().unwrap_or_default();
    assert!(!told.contains("Will be deleted."), "{told}");
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// Sessions and rows from before agents were called agents
// ---------------------------------------------------------------------------

/// The newest row `parent` has under `name`: a resume adds one.
fn newest_child_id(world: &World, parent: u64, name: &str) -> Option<u64> {
    world
        .db()
        .query_row(
            "SELECT id FROM sessions WHERE parent_session_id = ?1 AND child_name = ?2 \
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![i64::try_from(parent).unwrap(), name],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .and_then(|id| u64::try_from(id).ok())
}

/// Writes `agent` into child `id`'s row, as an earlier host wrote it.
fn set_row_agent(world: &World, id: u64, agent: &str) {
    let changed = world
        .db()
        .execute(
            "UPDATE sessions SET agent = ?2 WHERE id = ?1",
            rusqlite::params![i64::try_from(id).unwrap(), agent],
        )
        .expect("write the row's agent");
    assert_eq!(changed, 1);
}

/// Stops `parent`'s child `name` and waits for its row to say it ended.
async fn stop_and_wait(world: &World, parent: &Agent, name: &str, id: u64) {
    parent
        .call("agents_stop", json!({ "child": name }))
        .await
        .done();
    eventually("the child to end", async || {
        (world.column::<String>(id, "status") != "running").then_some(())
    })
    .await;
}

const CAREFUL: &str =
    "---\ndescription: Careful.\nbackend: claude-code\npermission: plan\n---\nCareful words.\n";

#[tokio::test(flavor = "multi_thread")]
async fn a_session_launched_before_the_rename_starts_by_role_calls_agents_roles_and_runs_floti_agent_start_role()
 {
    let world = World::new();
    world.agent("careful", CAREFUL);
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("old names"), "parent")
        .await;

    // role is read as agent: the child runs as that agent, its rules and
    // body included, never as the default.
    parent
        .start(json!({ "task": "go", "role": "careful", "name": "by-role" }))
        .await
        .done();
    let by_role = world.child_of(&parent, "by-role").await;
    assert_eq!(world.column::<String>(by_role.id(), "agent"), "careful");
    assert_eq!(
        value_of(&by_role.run.argv, "--permission-mode"),
        Some("plan"),
        "{:?}",
        by_role.run.argv
    );
    let told = by_role.run.instructions.as_deref().unwrap_or_default();
    assert!(told.contains("Careful words."), "{told}");

    // The same agent under both names is one agent; two are refused naming
    // both, and nothing starts.
    parent
        .start(json!({ "task": "go", "agent": "careful", "role": "careful", "name": "both" }))
        .await
        .done();
    let made = world.children_made();
    let refused = parent
        .start(json!({ "task": "go", "agent": "careful", "role": "default" }))
        .await;
    let sentence = refused.refused();
    assert!(
        sentence.contains("agent names careful") && sentence.contains("role names default"),
        "{sentence}"
    );
    assert_eq!(world.children_made(), made, "nothing was started");

    // A resume refuses role as it refuses agent.
    world.type_line(&by_role, "remember this").await;
    stop_and_wait(&world, &parent, "by-role", by_role.id()).await;
    let refused = parent
        .start(json!({ "task": "again", "resume": "by-role", "role": "careful" }))
        .await;
    assert!(
        refused.refused().contains("you also gave role"),
        "{refused:?}"
    );

    // agents_roles answers as agents_available, and no listing shows it.
    let (url, token) = parent.route();
    let old = parent.call("agents_roles", json!({})).await;
    assert_eq!(old.done(), parent.agents().await.done());
    let tools = rpc(&url, Some(&token), "tools/list", json!({}))
        .await
        .result();
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"agents_available"), "{names:?}");
    assert!(!names.contains(&"agents_roles"), "{names:?}");
    for tool in tools["tools"].as_array().unwrap() {
        assert!(
            !tool.to_string().contains("role"),
            "no tool says role: {tool}"
        );
    }

    // floti agent start --role reads as --agent.
    let command = agent_cli::parse(&[
        "start".to_owned(),
        "--task".to_owned(),
        "go".to_owned(),
        "--role".to_owned(),
        "careful".to_owned(),
        "--name".to_owned(),
        "by-flag".to_owned(),
    ])
    .expect("--role parses");
    let outcome = subcommand(command, url, token).await;
    assert_eq!(outcome.code, 0, "{outcome:?}");
    let by_flag = world.child_of(&parent, "by-flag").await;
    assert_eq!(world.column::<String>(by_flag.id(), "agent"), "careful");
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_child_whose_row_says_agent_comes_back_as_the_default_with_no_warning() {
    let world = World::new();
    world.agent("existing", "---\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("old default"), "parent")
        .await;
    parent
        .start(json!({ "task": "work", "name": "kid" }))
        .await
        .done();
    let kid = world.child_of(&parent, "kid").await;
    assert_eq!(world.column::<String>(kid.id(), "agent"), "default");
    // A child started as the default is told nothing about an agent.
    let told = kid.run.instructions.as_deref().unwrap_or_default();
    assert!(
        told.contains("You were started by another session, parent. Your first message"),
        "{told}"
    );
    world.type_line(&kid, "carry on").await;

    let mut client = world.client().await;
    client
        .request::<HostStop>(&HostStopRequest {
            reason: HostStopReason::Update,
        })
        .await
        .expect("the stop is taken");
    drop(client);
    assert_eq!(host.stopped().await, HostStopReason::Update);
    // What an earlier host wrote for a child started with no role.
    set_row_agent(&world, kid.id(), "agent");

    let host = world.host().await;
    let mut client = world.client().await;
    let result = client
        .request::<SessionRestore>(&RestoreSessionsRequest {
            sessions: vec![SessionToRestore {
                id: kid.id(),
                cols: 80,
                rows: 24,
                without_agent: false,
            }],
        })
        .await
        .expect("the restore is taken");
    drop(client);
    assert_eq!(result.restored.len(), 1, "{result:?}");
    assert!(
        result.restored[0].launched.warnings.is_empty(),
        "{:?}",
        result.restored[0].launched.warnings
    );
    let again = world
        .agent_of(result.restored[0].launched.session.clone(), &[kid.run.pid])
        .await;
    let told = again.run.instructions.as_deref().unwrap_or_default();
    assert!(!told.contains("no longer exists"), "{told}");
    assert!(!told.contains("session, parent, as"), "{told}");
    assert!(
        again.run.argv.iter().any(|arg| arg == "--resume"),
        "{:?}",
        again.run.argv
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_md_with_no_default_md_is_the_default_for_a_new_start_and_a_resumed_child() {
    let world = World::new();
    // What a roles folder moved to agents may hold: the old default's file.
    world.agent(
        "agent",
        "---\ndescription: My old default.\nbackend: claude-code\n---\nOld default words.\n",
    );
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("agent md"), "parent")
        .await;

    // Listed as the default, under the default's name, and not twice.
    assert_eq!(parent.agent_names().await, ["default"]);
    let entries = parent.agent_entries().await;
    assert_eq!(entries["default"]["description"], "My old default.");
    assert_eq!(backends_of(&entries["default"]), [CLAUDE]);

    // A start that names no agent gets it.
    parent
        .start(json!({ "task": "work", "name": "kid" }))
        .await
        .done();
    let kid = world.child_of(&parent, "kid").await;
    let told = kid.run.instructions.as_deref().unwrap_or_default();
    assert!(told.contains("Old default words."), "{told}");
    assert!(!told.contains("session, parent, as"), "{told}");

    // A child whose row still says agent resumes as it too, saying nothing
    // of an agent that no longer exists.
    world.type_line(&kid, "remember this").await;
    stop_and_wait(&world, &parent, "kid", kid.id()).await;
    set_row_agent(&world, kid.id(), "agent");
    let resumed = parent
        .start(json!({ "task": "again", "resume": "kid" }))
        .await;
    assert!(!resumed.text().contains("no longer exists"), "{resumed:?}");
    resumed.done();
    let id = newest_child_id(&world, parent.id(), "kid").expect("the resumed row");
    assert_ne!(id, kid.id());
    let record = world.record(id).await;
    let again = world.agent_of(record, &[kid.run.pid]).await;
    let told = again.run.instructions.as_deref().unwrap_or_default();
    assert!(told.contains("Old default words."), "{told}");
    assert_eq!(world.column::<String>(id, "agent"), "default");

    // A default.md beside it wins, and agent.md is not listed.
    world.agent("default", "---\ndescription: The new one.\n---\n");
    assert_eq!(parent.agent_names().await, ["default"]);
    assert_eq!(
        parent.agent_entries().await["default"]["description"],
        "The new one."
    );
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// A host whose config folder is another host's to write
// ---------------------------------------------------------------------------

impl World {
    /// A host told its config folder is not its to write, as Helm starts
    /// one in every run but the installed app's.
    async fn host_read_only(&self) -> RunningHost {
        let mut config = self.config();
        config.run.config_read_only = true;
        let host = RunningHost::start(config);
        drop(self.client().await);
        let known: i64 = self
            .db()
            .query_row(
                "SELECT count(*) FROM profiles WHERE name = 'workspace'",
                [],
                |row| row.get(0),
            )
            .expect("count the profiles");
        if known == 0 {
            self.profile("workspace", &self.work, None, None, None);
        }
        host
    }

    /// Every entry under the config folder, by its path from it, with a
    /// file's bytes.
    fn config_files(&self) -> Vec<(String, Option<Vec<u8>>)> {
        let mut found = Vec::new();
        let mut stack = vec![self.dirs.config.clone()];
        while let Some(at) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&at) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("an entry").path();
                let name = path
                    .strip_prefix(&self.dirs.config)
                    .expect("under the config folder")
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.is_dir() {
                    found.push((name, None));
                    stack.push(path);
                } else {
                    found.push((name, Some(std::fs::read(&path).expect("read a file"))));
                }
            }
        }
        found.sort();
        found
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_host_reads_a_roles_folder_where_it_is_and_moves_and_writes_nothing() {
    let world = World::new();
    let roles = world.dirs.config.join("roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(roles.join("careful.md"), CAREFUL).unwrap();
    std::fs::write(
        roles.join("lead.md"),
        "---\ndescription: My lead.\n---\nMine.\n",
    )
    .unwrap();
    let before = world.config_files();

    let host = world.host_read_only().await;
    let parent = world
        .session(CLAUDE, &world.project("shared config"), "parent")
        .await;
    assert_eq!(parent.agent_names().await, ["default", "careful", "lead"]);
    parent
        .start(json!({ "task": "go", "agent": "careful", "name": "kid" }))
        .await
        .done();
    let kid = world.child_of(&parent, "kid").await;
    assert!(
        kid.run
            .instructions
            .as_deref()
            .unwrap_or_default()
            .contains("Careful words.")
    );
    host.stop(HostStopReason::Stop).await;
    assert_eq!(
        world.config_files(),
        before,
        "the config folder is as it was"
    );
    assert!(!world.agents_dir().exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_host_with_neither_folder_serves_the_shipped_agents_and_writes_none() {
    let world = World::new();
    let before = world.config_files();
    let host = world.host_read_only().await;
    let parent = world
        .session(CLAUDE, &world.project("no folder"), "parent")
        .await;
    assert_eq!(
        parent.agent_names().await,
        ["default", "lead", "researcher", "reviewer", "worker"]
    );
    parent
        .start(json!({ "task": "look", "agent": "reviewer", "name": "kid" }))
        .await
        .done();
    let kid = world.child_of(&parent, "kid").await;
    assert!(
        kid.run
            .instructions
            .as_deref()
            .unwrap_or_default()
            .contains("Read the code before you give a finding"),
        "{:?}",
        kid.run.instructions
    );
    host.stop(HostStopReason::Stop).await;
    assert_eq!(world.config_files(), before, "nothing was written");
    assert!(!world.agents_dir().exists());
    assert!(!world.dirs.config.join("roles").exists());
}

// ---------------------------------------------------------------------------
// S19: a client lists, reads, edits and watches the host's agents
// ---------------------------------------------------------------------------

/// What a client is answered on `agents:list`.
async fn listed(client: &mut TestClient) -> AgentsListing {
    client
        .request::<ListAgents>(&())
        .await
        .expect("the agents are listed")
}

fn listed_names(listing: &AgentsListing) -> Vec<&str> {
    listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect()
}

/// Subscribes `client` to `agents:changed`, and answers once the host has
/// taken the subscription: a request after it is answered after it.
async fn listen_for_changes(client: &mut TestClient) {
    client.subscribe_quietly(1, "agents:changed").await;
    listed(client).await;
}

/// The names the next `agents:changed` event `client` is sent tells,
/// whatever they are: a test that writes again and expects the next event to
/// name only that write knows the one before was told once.
async fn next_change(client: &mut TestClient) -> Vec<String> {
    client
        .event_on::<AgentsChange>(1, "the next agents:changed", |_| true)
        .await
        .names
}

/// The hash the host lists for the agent file `name`.
async fn listed_hash(client: &mut TestClient, name: &str) -> Option<String> {
    listed(client)
        .await
        .agents
        .into_iter()
        .find(|agent| agent.name == name)
        .and_then(|agent| agent.hash)
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_a_client_lists_the_hosts_agents_its_bad_files_retired_lines_and_missing_shipped_ones_and_each_host_answers_its_own()
 {
    let world = World::new();
    world.agent(
        "reviewer",
        "---\ndescription: Reads a change.\nclass: light\n---\nRead it.\n",
    );
    world.agent("typo", "---\ndescription: Typo.\nworktree: maybe\n---\n");
    let other = World::new();
    other.agent("scout", "---\ndescription: Looks around.\n---\n");
    let host = world.host().await;
    let other_host = other.host().await;

    let mut client = world.client().await;
    let listing = listed(&mut client).await;
    assert_eq!(listing.folder, text(&world.agents_dir()));
    assert_eq!(listing.read_only, None);
    assert_eq!(listed_names(&listing), ["default", "reviewer"]);
    let (default, reviewer) = (&listing.agents[0], &listing.agents[1]);
    assert_eq!(
        (default.file.as_deref(), default.hash.as_deref()),
        (None, None)
    );
    assert!(
        default.backends.iter().any(|backend| backend == CLAUDE),
        "{default:?}"
    );
    assert_eq!(reviewer.description, "Reads a change.");
    assert_eq!(reviewer.file.as_deref(), Some("reviewer.md"));
    assert_eq!(
        reviewer.unused_lines,
        [AgentUnusedLine {
            field: "class".into(),
            line: 3,
        }]
    );
    let read = client
        .request::<ReadAgent>(&AgentName {
            name: "reviewer".into(),
        })
        .await
        .expect("the reviewer is read");
    assert_eq!(reviewer.hash.as_deref(), Some(read.hash.as_str()));
    assert_eq!(read.body, "Read it.\n");

    let [typo] = listing.bad.as_slice() else {
        panic!("one bad file: {listing:?}");
    };
    assert_eq!(
        (typo.name.as_str(), typo.file.as_str()),
        ("typo", "typo.md")
    );
    assert_eq!(
        typo.problems,
        [AgentProblem {
            field: Some("worktree".into()),
            line: Some(3),
            message: "worktree must be yes, no, true or false, not maybe".into(),
        }]
    );
    let shipped: Vec<&str> = listing
        .shipped
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(
        shipped,
        ["lead", "researcher", "worker"],
        "the reviewer's file is there"
    );

    // The other host answers with its own folder.
    let mut elsewhere = TestClient::connect(&other.endpoint, TERMINAL).await;
    let theirs = listed(&mut elsewhere).await;
    assert_eq!(theirs.folder, text(&other.agents_dir()));
    assert_eq!(listed_names(&theirs), ["default", "scout"]);
    assert!(theirs.bad.is_empty());
    assert_eq!(theirs.shipped.len(), 4);

    drop((client, elsewhere));
    other_host.stop(HostStopReason::Stop).await;
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_an_agent_a_client_writes_is_what_the_next_agents_start_runs_with_no_restart() {
    let world = World::new();
    world.agent("existing", "---\n---\n");
    let host = world.host().await;
    let parent = world
        .session(CLAUDE, &world.project("written"), "parent")
        .await;
    let mut client = world.client().await;

    let created = client
        .request::<WriteAgent>(&WriteAgentRequest {
            name: "scout".into(),
            text: "---\ndescription: Scouts.\n---\nLook around first.\n".into(),
            expected_hash: None,
            rename_from: None,
        })
        .await
        .expect("the write is answered");
    assert!(created.changed, "{created:?}");
    parent
        .start(json!({ "task": "go", "agent": "scout", "name": "first" }))
        .await
        .done();
    let first = world.child_of(&parent, "first").await;
    assert!(
        first
            .run
            .instructions
            .as_deref()
            .unwrap_or_default()
            .contains("Look around first.")
    );

    let replaced = client
        .request::<WriteAgent>(&WriteAgentRequest {
            name: "scout".into(),
            text: "---\ndescription: Scouts.\n---\nLook around twice.\n".into(),
            expected_hash: created.file.map(|file| file.hash),
            rename_from: None,
        })
        .await
        .expect("the write is answered");
    assert!(replaced.changed, "{replaced:?}");
    parent
        .start(json!({ "task": "go", "agent": "scout", "name": "second" }))
        .await
        .done();
    let second = world.child_of(&parent, "second").await;
    let composed = second.run.instructions.as_deref().unwrap_or_default();
    assert!(
        composed.contains("Look around twice.") && !composed.contains("Look around first."),
        "{composed}"
    );

    // A text the parser refuses changes nothing a start reads.
    let refused = client
        .request::<WriteAgent>(&WriteAgentRequest {
            name: "scout".into(),
            text: "---\nworktree: maybe\n---\n".into(),
            expected_hash: replaced.file.map(|file| file.hash),
            rename_from: None,
        })
        .await
        .expect("the write is answered");
    assert!(
        !refused.changed && !refused.problems.is_empty(),
        "{refused:?}"
    );
    assert!(
        std::fs::read_to_string(world.agents_dir().join("scout.md"))
            .unwrap()
            .contains("Look around twice.")
    );
    drop(client);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_an_edit_outside_floti_and_the_hosts_own_reach_every_listening_client_as_agents_changed()
 {
    let world = World::new();
    world.agent("reviewer", "---\ndescription: Reads.\n---\n");
    let host = world.host().await;
    let mut first = world.client().await;
    let mut second = world.client().await;
    listen_for_changes(&mut first).await;
    listen_for_changes(&mut second).await;

    // An editor writes a new agent file: each listener is told once.
    world.agent("scout", "---\ndescription: Looks around.\n---\n");
    assert_eq!(next_change(&mut first).await, ["scout"]);
    assert_eq!(next_change(&mut second).await, ["scout"]);

    // The host's own delete, made for one client, is told to both, once,
    // though both the host and the OS say it happened.
    let hash = listed_hash(&mut first, "reviewer").await;
    let deleted = first
        .request::<DeleteAgent>(&DeleteAgentRequest {
            name: "reviewer".into(),
            expected_hash: hash,
        })
        .await
        .expect("the delete is answered");
    assert!(deleted.changed, "{deleted:?}");
    assert_eq!(next_change(&mut first).await, ["reviewer"]);
    assert_eq!(next_change(&mut second).await, ["reviewer"]);

    // The next event is the next change, so nothing was told twice.
    world.agent("marker", "---\n---\n");
    assert_eq!(next_change(&mut first).await, ["marker"]);
    assert_eq!(next_change(&mut second).await, ["marker"]);

    drop((first, second));
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_a_host_that_writes_no_config_lists_reads_and_watches_its_agents_and_refuses_every_change()
 {
    let world = World::new();
    let reviewer = "---\ndescription: Reads.\n---\n";
    world.agent("reviewer", reviewer);
    let host = world.host_read_only().await;
    let mut client = world.client().await;
    listen_for_changes(&mut client).await;

    let listing = listed(&mut client).await;
    let sentence = listing
        .read_only
        .clone()
        .expect("the host says it is read-only");
    assert!(
        sentence.contains("does not change its config folder"),
        "{sentence}"
    );
    assert_eq!(listed_names(&listing), ["default", "reviewer"]);
    let read = client
        .request::<ReadAgent>(&AgentName {
            name: "reviewer".into(),
        })
        .await
        .expect("the reviewer is read");

    let name = || AgentName {
        name: "reviewer".into(),
    };
    let changes = [
        client
            .request::<WriteAgent>(&WriteAgentRequest {
                name: "reviewer".into(),
                text: "---\ndescription: Mine.\n---\n".into(),
                expected_hash: Some(read.hash.clone()),
                rename_from: None,
            })
            .await,
        client
            .request::<DeleteAgent>(&DeleteAgentRequest {
                name: "reviewer".into(),
                expected_hash: None,
            })
            .await,
        client.request::<RestoreAgent>(&name()).await,
        client
            .request::<AddShippedAgent>(&AgentName {
                name: "worker".into(),
            })
            .await,
    ];
    for change in changes {
        let change = change.expect("the change is answered");
        assert!(!change.changed, "{change:?}");
        assert_eq!(change.refusal.as_deref(), Some(sentence.as_str()));
    }
    assert_eq!(
        std::fs::read_to_string(world.agents_dir().join("reviewer.md")).unwrap(),
        reviewer
    );
    assert!(!world.agents_dir().join("worker.md").exists());

    // It still watches: an outside edit is told, once.
    world.agent("reviewer", "---\ndescription: Edited by hand.\n---\n");
    assert_eq!(next_change(&mut client).await, ["reviewer"]);
    world.agent("marker", "---\n---\n");
    assert_eq!(next_change(&mut client).await, ["marker"]);
    drop(client);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_a_host_that_writes_no_config_and_has_no_agents_folder_lists_each_shipped_agent_once() {
    let world = World::new();
    let host = world.host_read_only().await;
    let mut client = world.client().await;

    let listing = listed(&mut client).await;
    assert_eq!(
        listed_names(&listing),
        ["default", "lead", "researcher", "reviewer", "worker"]
    );
    assert!(
        listing.agents.iter().all(|agent| agent.file.is_none()),
        "no agent is read from a file: {listing:?}"
    );
    assert!(
        listing.shipped.is_empty(),
        "an agent it lists is not offered to add: {listing:?}"
    );
    let lead = client
        .request::<ReadAgent>(&AgentName {
            name: "lead".into(),
        })
        .await
        .expect("the shipped lead is read");
    assert!(!lead.in_folder);
    assert!(!world.agents_dir().exists(), "nothing was written");
    drop(client);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s19_an_agents_folder_deleted_and_made_again_while_a_client_listens_is_watched_anew() {
    let world = World::new();
    world.agent("reviewer", "---\ndescription: Reads.\n---\n");
    let host = world.host().await;
    let mut client = world.client().await;
    listen_for_changes(&mut client).await;

    std::fs::remove_dir_all(world.agents_dir()).expect("delete the agents folder");
    assert_eq!(next_change(&mut client).await, ["reviewer"]);

    // Made again, with a file in it.
    world.agent("scout", "---\ndescription: Looks around.\n---\n");
    assert_eq!(next_change(&mut client).await, ["scout"]);
    // A file written in the folder made again is told: it is watched anew.
    world.agent("worker", "---\ndescription: Builds.\n---\n");
    assert_eq!(next_change(&mut client).await, ["worker"]);
    drop(client);
    host.stop(HostStopReason::Stop).await;
}

// ---------------------------------------------------------------------------
// A session a person starts as an agent
// ---------------------------------------------------------------------------

/// An agent a person plans a project with: its own model, effort, permission
/// and rules, and children of its own.
const RESEARCH_LEAD: &str = "---\n\
    description: Researches a new project with you and writes the plan.\n\
    backend: claude-code\n\
    model:\n  claude-code: opus\n\
    effort:\n  claude-code: high\n\
    permission: plan\n\
    can_start: yes\n\
    allowed_tools: Read, Grep\n\
    disallowed_tools: Edit, Write\n\
    ---\n\
    Research the project with the user and write the plan.\n";

/// What a launcher sends to start a session in `folder` as `agent`.
fn launch_request(folder: &Path, agent: Option<&str>) -> LaunchSessionRequest {
    LaunchSessionRequest {
        cwd: text(folder),
        project_path: Some(text(folder)),
        name: None,
        profile_id: None,
        permission_mode: None,
        resume: None,
        backend: None,
        agent: agent.map(str::to_owned),
        chosen: None,
        cols: 80,
        rows: 24,
    }
}

impl World {
    /// A session a person starts from a launcher, the client gone once it
    /// started; the host's sentence when it refuses.
    async fn launch(&self, request: LaunchSessionRequest) -> Result<Agent, String> {
        let mut client = self.client().await;
        let launched = client.request::<SessionLaunch>(&request).await;
        drop(client);
        match launched {
            Ok(launched) => Ok(self.agent_of(launched.session, &[]).await),
            Err(err) => Err(err.message),
        }
    }

    /// The conversation `agent` is in, once a line was typed into it so that
    /// history can reopen it, and its session has ended.
    async fn worked_and_ended(&self, agent: &Agent) -> String {
        self.type_line(agent, "remember this conversation").await;
        self.end_from_its_terminal(agent).await;
        agent
            .record
            .conversation_id
            .clone()
            .expect("the session has a conversation")
    }

    /// Ends `agent` from its own terminal and waits for its row to say so.
    async fn end_from_its_terminal(&self, agent: &Agent) {
        self.type_line(agent, "/exit").await;
        eventually("the session to end", async || {
            (self.column::<String>(agent.id(), "status") != "running").then_some(())
        })
        .await;
    }

    /// The history index's entry for `conversation`, read afresh.
    async fn history_entry(&self, conversation: &str) -> HistorySession {
        let mut client = self.client().await;
        client
            .request::<HistoryRefresh>(&())
            .await
            .expect("the history is read");
        let page = client
            .request::<HistorySessions>(&HistoryQuery::default())
            .await
            .expect("the history is listed");
        page.sessions
            .into_iter()
            .find(|session| session.session_id.eq_ignore_ascii_case(conversation))
            .unwrap_or_else(|| panic!("{conversation} is not in the history"))
    }

    /// Reopens `conversation` from History, as `agent` or as none: the run
    /// that is none of the earlier runs `seen`.
    async fn resume_from_history(
        &self,
        conversation: &str,
        agent: Option<&str>,
        seen: &[u32],
    ) -> Result<Agent, String> {
        let mut client = self.client().await;
        client
            .request::<HistoryRefresh>(&())
            .await
            .expect("the history is read");
        let resumed = client
            .request::<HistoryResume>(&ResumeSessionRequest {
                session_id: conversation.to_owned(),
                agent: agent.map(str::to_owned),
                cols: 80,
                rows: 24,
            })
            .await;
        drop(client);
        match resumed {
            Ok(resumed) => Ok(self.agent_of(resumed.session, seen).await),
            Err(err) => Err(err.message),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_starts_a_session_as_an_agent_composed_as_a_childs_with_no_parent_and_nothing_typed()
 {
    let world = World::new();
    world.agent("research-lead", RESEARCH_LEAD);
    let host = world.host().await;
    let folder = world.project("orbit-api");

    let lead = world
        .launch(launch_request(&folder, Some("research-lead")))
        .await
        .expect("the session starts as the agent");
    assert_eq!(lead.record.agent.as_deref(), Some("research-lead"));
    assert_eq!(lead.record.parent_id, None);
    assert_eq!(lead.record.name, "orbit-api", "named after its folder");
    // Its model, effort, permission and rules, as a child of it would have.
    assert_eq!(lead.run.model(), Some("opus"));
    assert_eq!(lead.run.effort(), Some("high"));
    assert_eq!(value_of(&lead.run.argv, "--permission-mode"), Some("plan"));
    for tool in ["Read", "Grep"] {
        assert!(
            lead.run.allowed().iter().any(|allowed| allowed == tool),
            "{:?}",
            lead.run.allowed()
        );
    }
    assert_eq!(lead.run.disallowed(), ["Edit", "Write"]);
    // Its body is composed in; it is told of no parent; nothing is typed.
    let instructions = lead.run.instructions.as_deref().expect("instructions");
    assert!(
        instructions.contains("# Agent: research-lead"),
        "{instructions}"
    );
    assert!(instructions.contains("Research the project with the user"));
    assert!(
        !instructions.contains("# Started by another session"),
        "{instructions}"
    );
    assert_eq!(lead.run.opening_prompt, None);
    assert!(lead.run.received.is_empty(), "{:?}", lead.run.received);
    // A person is at it: it is not launched as an unattended child is.
    assert!(!lead.run.argv.iter().any(|arg| arg == "--strict-mcp-config"));
    // It counts toward no agent limit: it is no child.
    assert_eq!(world.children_made(), 0);

    // What the launcher chose replaces the agent's, and none passes no flag.
    let chosen = world
        .launch(LaunchSessionRequest {
            name: Some("tuned".into()),
            permission_mode: Some(PermissionMode::AcceptEdits),
            chosen: Some(LaunchChoices {
                model: Some("sonnet".into()),
                effort: None,
            }),
            ..launch_request(&folder, Some("research-lead"))
        })
        .await
        .expect("the session starts as the agent with the launcher's choices");
    assert_eq!(chosen.run.model(), Some("sonnet"));
    assert_eq!(chosen.run.effort(), None);
    assert_eq!(
        value_of(&chosen.run.argv, "--permission-mode"),
        Some("acceptEdits")
    );
    assert_eq!(chosen.run.disallowed(), ["Edit", "Write"], "its rules stay");
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_can_start_agents_gets_floti_agents_and_starts_its_own_and_one_that_cannot_has_none()
 {
    let world = World::new();
    world.agent("research-lead", RESEARCH_LEAD);
    world.agent(
        "planner",
        "---\nbackend: claude-code\ncan_start: no\n---\nPlan it.\n",
    );
    let host = world.host().await;
    let folder = world.project("plans");

    let lead = world
        .launch(launch_request(&folder, Some("research-lead")))
        .await
        .expect("the lead starts");
    assert!(
        lead.run.servers.contains_key(FAMILY),
        "{:?}",
        lead.run.servers
    );
    lead.start(json!({ "task": "read the code", "name": "scout" }))
        .await
        .done();
    assert_eq!(
        world.child_of(&lead, "scout").await.record.parent_id,
        Some(lead.id())
    );

    let planner = world
        .launch(launch_request(&folder, Some("planner")))
        .await
        .expect("the planner starts");
    assert!(
        !planner.run.servers.contains_key(FAMILY),
        "{:?}",
        planner.run.servers
    );
    assert!(
        !planner
            .run
            .allowed()
            .iter()
            .any(|allowed| allowed.contains("floti-agents") || allowed.contains("floti agent")),
        "{:?}",
        planner.run.allowed()
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_works_in_a_worktree_starts_in_a_new_one_named_after_the_session_and_outside_git_nothing_starts()
 {
    let world = World::new();
    world.agent(
        "builder",
        "---\nbackend: claude-code\nworktree: yes\n---\nBuild it.\n",
    );
    let host = world.host().await;
    let repo = world.repository("orbit-api");

    let builder = world
        .launch(launch_request(&repo, Some("builder")))
        .await
        .expect("the builder starts");
    assert_eq!(
        Path::new(&builder.record.cwd),
        repo.join(".floti/worktrees/orbit-api").as_path()
    );
    assert_eq!(builder.record.branch.as_deref(), Some("agent/orbit-api"));
    assert_eq!(
        builder.record.project_path.as_deref(),
        Some(text(&repo).as_str()),
        "it stays under its project"
    );
    // A second one is made unique, as a child's is.
    let again = world
        .launch(launch_request(&repo, Some("builder")))
        .await
        .expect("a second builder starts");
    assert_eq!(again.record.branch.as_deref(), Some("agent/orbit-api-2"));

    // Outside a git repository the start is refused and nothing runs.
    let runs = world.run_count();
    let notes = world.project("notes");
    let refused = world
        .launch(launch_request(&notes, Some("builder")))
        .await
        .expect_err("not in a git repository");
    assert!(
        refused.contains("builder works in a worktree of its own")
            && refused.contains("is not in a git repository"),
        "{refused}"
    );
    assert_eq!(world.run_count(), runs);
    assert_eq!(world.sessions().await.len(), 2);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_is_gone_or_cannot_be_loaded_is_named_and_nothing_starts() {
    let world = World::new();
    world.agent("typo", "---\nworktree: maybe\n---\n");
    let host = world.host().await;
    let folder = world.project("anywhere");

    let gone = world
        .launch(launch_request(&folder, Some("research-lead")))
        .await
        .expect_err("there is no such agent");
    assert!(
        gone.contains("research-lead is no longer among this host's agents"),
        "{gone}"
    );
    let bad = world
        .launch(launch_request(&folder, Some("typo")))
        .await
        .expect_err("its file cannot be loaded");
    assert!(
        bad.contains("typo.md cannot be loaded") && bad.contains("worktree must be"),
        "{bad}"
    );
    assert_eq!(world.run_count(), 0, "nothing ran");
    assert!(world.sessions().await.is_empty());
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn history_names_a_conversations_agent_and_resuming_composes_it_from_its_file_as_it_is_now() {
    let world = World::new();
    world.agent(
        "research-lead",
        "---\nbackend: claude-code\n---\nFirst version.\n",
    );
    let host = world.host().await;
    let folder = world.project("ledger");
    let lead = world
        .launch(launch_request(&folder, Some("research-lead")))
        .await
        .expect("the lead starts");
    let conversation = world.worked_and_ended(&lead).await;
    assert_eq!(
        world.history_entry(&conversation).await.agent.as_deref(),
        Some("research-lead")
    );

    // The file changed since: the resume is composed as it reads now.
    world.agent(
        "research-lead",
        "---\nbackend: claude-code\n---\nSecond version.\n",
    );
    let resumed = world
        .resume_from_history(&conversation, Some("research-lead"), &[lead.run.pid])
        .await
        .expect("it resumes as its agent");
    let composed = resumed.run.instructions.as_deref().expect("instructions");
    assert!(composed.contains("Second version."), "{composed}");
    assert!(!composed.contains("First version."), "{composed}");
    assert_eq!(resumed.record.agent.as_deref(), Some("research-lead"));
    assert!(resumed.run.argv.iter().any(|arg| arg == "--resume"));
    world.end_from_its_terminal(&resumed).await;

    // Gone: named, and nothing starts until it is resumed without it.
    world.remove_agent("research-lead");
    let runs = world.run_count();
    let refused = world
        .resume_from_history(&conversation, Some("research-lead"), &[])
        .await
        .expect_err("the agent is gone");
    assert!(
        refused.contains("research-lead is no longer among this host's agents"),
        "{refused}"
    );
    assert_eq!(world.run_count(), runs);
    let without = world
        .resume_from_history(&conversation, None, &[lead.run.pid, resumed.run.pid])
        .await
        .expect("it resumes without the agent");
    assert_eq!(without.record.agent, None);
    let told = without.run.instructions.as_deref().unwrap_or_default();
    assert!(!told.contains("# Agent:"), "{told}");
    // Its newest session is no agent's now, and History says so.
    world.end_from_its_terminal(&without).await;
    assert_eq!(world.history_entry(&conversation).await.agent, None);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_restore_offer_names_an_agents_problem_and_restores_it_without_the_agent_when_asked() {
    let world = World::new();
    world.agent(
        "planner",
        "---\nbackend: claude-code\ncan_start: no\n---\nFirst plan.\n",
    );
    world.agent(
        "scribe",
        "---\nbackend: claude-code\n---\nWill be deleted.\n",
    );
    let host = world.host().await;
    let planner = world
        .launch(launch_request(&world.project("plans"), Some("planner")))
        .await
        .expect("the planner starts");
    let scribe = world
        .launch(launch_request(&world.project("notes"), Some("scribe")))
        .await
        .expect("the scribe starts");
    for agent in [&planner, &scribe] {
        world.type_line(agent, "carry on").await;
    }

    let mut client = world.client().await;
    client
        .request::<HostStop>(&HostStopRequest {
            reason: HostStopReason::Update,
        })
        .await
        .expect("the stop is taken");
    drop(client);
    assert_eq!(host.stopped().await, HostStopReason::Update);
    world.agent(
        "planner",
        "---\nbackend: claude-code\ncan_start: no\n---\nSecond plan.\n",
    );
    world.remove_agent("scribe");

    let host = world.host().await;
    let mut client = world.client().await;
    let offer = client
        .request::<SessionRestorable>(&())
        .await
        .expect("the offer is read")
        .expect("there is an offer");
    let offered = |id: u64| {
        offer
            .sessions
            .iter()
            .find(|session| session.id == id)
            .expect("offered")
            .clone()
    };
    assert_eq!(offered(planner.id()).agent.as_deref(), Some("planner"));
    assert_eq!(offered(planner.id()).agent_problem, None);
    let gone = offered(scribe.id());
    assert_eq!(gone.agent.as_deref(), Some("scribe"));
    assert!(
        gone.agent_problem
            .as_deref()
            .is_some_and(|problem| problem.contains("scribe is no longer among")),
        "{gone:?}"
    );

    let result = client
        .request::<SessionRestore>(&RestoreSessionsRequest {
            sessions: vec![
                SessionToRestore {
                    id: planner.id(),
                    cols: 80,
                    rows: 24,
                    without_agent: false,
                },
                SessionToRestore {
                    id: scribe.id(),
                    cols: 80,
                    rows: 24,
                    without_agent: true,
                },
            ],
        })
        .await
        .expect("the restore is taken");
    drop(client);
    assert_eq!(result.restored.len(), 2, "{result:?}");
    let restored = |from: u64| {
        result
            .restored
            .iter()
            .find(|restored| restored.from == from)
            .expect("restored")
            .launched
            .clone()
    };
    // The planner comes back as its file reads now, still without the
    // agents family it may not have.
    let planner_again = world
        .agent_of(restored(planner.id()).session, &[planner.run.pid])
        .await;
    let composed = planner_again
        .run
        .instructions
        .as_deref()
        .expect("instructions");
    assert!(composed.contains("Second plan."), "{composed}");
    assert_eq!(planner_again.record.agent.as_deref(), Some("planner"));
    assert!(!planner_again.run.servers.contains_key(FAMILY));
    assert!(
        !planner_again
            .run
            .argv
            .iter()
            .any(|arg| arg == "--strict-mcp-config")
    );
    // The scribe comes back without its agent, as the person chose.
    let scribe_again = world
        .agent_of(restored(scribe.id()).session, &[scribe.run.pid])
        .await;
    assert_eq!(scribe_again.record.agent, None);
    let told = scribe_again.run.instructions.as_deref().unwrap_or_default();
    assert!(!told.contains("Will be deleted."), "{told}");
    host.stop(HostStopReason::Stop).await;
}
