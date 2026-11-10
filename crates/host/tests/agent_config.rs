//! What composition reads of an agent's config against real folders, a real
//! floti.db and the fake claude: the scopes a template imports from, their
//! trees, and the effective view of a launch, read directly and as a client
//! asks for them. Nothing here writes into the agent's config folder, and a
//! host serves no channel that wrote one.
//!
//! Every test makes its own folders and its own store, and the agent's CLI
//! is `node test/fake-agents/claude-code.mjs`, never a real agent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use floti_host::agent::AgentBackend;
use floti_host::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use floti_host::agent_config::{ConfigReader, NO_AGENT_CONFIG, normalize};
use floti_host::platform::process_tree::Native as NativeTree;
use floti_host::platform::socket::Endpoint;
use floti_host::server::{HostConfig, backends, shell_backend};
use floti_host::store::Store;
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_protocol::channel::to_payload;
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::workspace::{
    ConfigFileKind, ConfigScopeKind, ConfigScopeRef, EffectiveViewRequest, ListConfigScopes,
    McpScope, ReadConfigTree, ReadEffectiveView, SettingsLayerKind,
};
use floti_protocol::message::ErrorCode;
use serde_json::json;

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("fake-agents")
        .join("claude-code.mjs")
}

fn claude_settings(claude: &Path) -> ClaudeCodeSettings {
    ClaudeCodeSettings {
        search_first: Vec::new(),
        program: floti_host::testkit::node().into(),
        leading_args: vec![fake_claude().into_os_string()],
        config_dir: claude.to_path_buf(),
        pass_config_dir: true,
        api_key_set: false,
    }
}

fn put(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn scope(path: &Path) -> String {
    normalize(path).display().to_string()
}

fn same(a: &str, b: &Path) -> bool {
    normalize(Path::new(a)) == normalize(b)
}

fn add_profile(db: &rusqlite::Connection, name: &str, root: &Path, overlays: &[&Path]) -> u64 {
    let overlays: Vec<String> = overlays
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    db.execute(
        "INSERT INTO profiles (name, root, overlays) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            name,
            root.display().to_string(),
            json!(overlays).to_string()
        ],
    )
    .unwrap();
    u64::try_from(db.last_insert_rowid()).unwrap()
}

/// Every file under `dir` with its bytes, to show nothing in it changed.
fn contents(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                found.push((path, bytes));
            }
        }
    }
    found.sort();
    found
}

/// A user, a project with a space in its path, a store, and the reader.
struct World {
    /// The agent's config folder, the user scope.
    claude: PathBuf,
    project: PathBuf,
    other: PathBuf,
    store: Arc<Mutex<Store>>,
    reader: ConfigReader,
    /// Last: fields drop in order, and the folder goes only once nothing in it is open.
    _dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let claude = dirs.home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let project = dirs.folder("my project");
        let other = dirs.folder("someone else");
        let backend = Arc::new(ClaudeCodeBackend::new(
            claude_settings(&claude),
            Arc::new(NativeTree),
        ));
        let agent = backend.agent_config();
        let store = Arc::new(Mutex::new(Store::open(&dirs.data).unwrap()));
        let reader = ConfigReader::new(backend, agent, Arc::clone(&store));
        Self {
            _dirs: dirs,
            claude,
            project,
            other,
            store,
            reader,
        }
    }

    fn add_profile(&self, name: &str, root: &Path, overlays: &[&Path]) -> u64 {
        add_profile(
            self.store.lock().unwrap().connection(),
            name,
            root,
            overlays,
        )
    }
}

#[test]
fn scopes_are_the_user_scope_then_each_profile_folder_once() {
    let world = World::new();
    world.add_profile("Work", &world.project, &[&world.other, &world.project]);
    world.add_profile("Again", &world.other, &[]);

    let scopes = world.reader.scopes().unwrap();
    let kinds: Vec<ConfigScopeKind> = scopes.iter().map(|scope| scope.kind).collect();
    assert_eq!(
        kinds,
        [
            ConfigScopeKind::User,
            ConfigScopeKind::Project,
            ConfigScopeKind::Project
        ]
    );
    assert_eq!(scopes[0].label, "User");
    assert!(same(&scopes[0].path, &world.claude));
    assert!(scopes[0].exists);
    assert!(same(&scopes[1].path, &world.project));
    assert_eq!(scopes[1].label, "my project");
    assert!(same(&scopes[1].claude_dir, &world.project.join(".claude")));
    assert!(same(&scopes[2].path, &world.other));
}

#[test]
fn a_scope_tree_lists_its_config_files_and_a_credential_file_by_name_only() {
    let world = World::new();
    put(&world.claude.join("CLAUDE.md"), "# me\n");
    put(&world.claude.join("settings.json"), "{}\n");
    put(
        &world.claude.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: Reviews.\n---\n",
    );
    put(&world.claude.join(".credentials.json"), "{\"secret\":true}");
    put(&world.claude.join("sessions/123.json"), "{}");
    put(&world.claude.join("sessions/123.abcdef.key"), "secret");
    put(&world.project.join("CLAUDE.md"), "# project\n");
    put(&world.project.join(".mcp.json"), "{\"mcpServers\":{}}\n");
    put(&world.project.join(".claude/settings.local.json"), "{}\n");
    put(
        &world.project.join(".claude/agents/helper.md"),
        "---\nname: helper\n---\n",
    );

    let user = world.reader.tree(&scope(&world.claude)).unwrap();
    assert!(user.errors.is_empty(), "{:?}", user.errors);
    let rel: Vec<&str> = user
        .files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect();
    assert!(rel.contains(&"CLAUDE.md"), "{rel:?}");
    assert!(rel.contains(&"settings.json"), "{rel:?}");
    assert!(rel.contains(&"skills/review/SKILL.md"), "{rel:?}");
    // Listed from its directory entry, so a listing can say what it is.
    let credential = user
        .files
        .iter()
        .find(|file| file.rel_path == ".credentials.json")
        .unwrap_or_else(|| panic!("{rel:?}"));
    assert_eq!(credential.kind, ConfigFileKind::Other);
    assert_eq!(credential.description, None);
    for file in &user.files {
        assert!(!file.rel_path.ends_with(".key"), "{rel:?}");
    }
    let skill = user
        .files
        .iter()
        .find(|file| file.kind == ConfigFileKind::Skill)
        .unwrap();
    assert_eq!(skill.description.as_deref(), Some("Reviews."));

    let project = world.reader.tree(&scope(&world.project)).unwrap();
    let kinds: Vec<ConfigFileKind> = project.files.iter().map(|file| file.kind).collect();
    for kind in [
        ConfigFileKind::ClaudeMd,
        ConfigFileKind::Mcp,
        ConfigFileKind::SettingsLocal,
        ConfigFileKind::Agent,
    ] {
        assert!(kinds.contains(&kind), "{kinds:?}");
    }
    assert_eq!(project.scope.kind, ConfigScopeKind::Project);
}

#[test]
fn the_effective_view_takes_each_setting_from_the_nearest_layer_and_writes_nothing() {
    let world = World::new();
    put(
        &world.claude.join("settings.json"),
        "{\"model\":\"user\",\"theme\":\"dark\"}\n",
    );
    put(
        &world.project.join(".claude/settings.json"),
        "{\"model\":\"project\"}\n",
    );
    put(
        &world.project.join(".claude/settings.local.json"),
        "{\"model\":\"local\",\"enabledMcpjsonServers\":[\"docs\"]}\n",
    );
    put(
        &world.project.join(".mcp.json"),
        "{\"mcpServers\":{\"docs\":{\"command\":\"node\",\"args\":[\"docs.js\"]}}}\n",
    );
    put(&world.project.join("CLAUDE.md"), "# project\n");
    put(&world.claude.join("CLAUDE.md"), "# me\n");
    let before = (contents(&world.claude), contents(&world.project));

    let view = world
        .reader
        .effective(&EffectiveViewRequest {
            profile_id: None,
            cwd: Some(world.project.display().to_string()),
            overlays: Vec::new(),
        })
        .unwrap();
    let model = view
        .settings
        .iter()
        .find(|setting| setting.key == "model")
        .unwrap();
    assert_eq!(model.winner, SettingsLayerKind::Local);
    assert_eq!(model.value, "\"local\"");
    assert!(model.overridden);
    assert_eq!(model.candidates.len(), 3);
    let theme = view
        .settings
        .iter()
        .find(|setting| setting.key == "theme")
        .unwrap();
    assert_eq!(theme.winner, SettingsLayerKind::User);
    assert!(!theme.overridden);

    let docs = view
        .mcp_servers
        .iter()
        .find(|server| server.name == "docs")
        .unwrap();
    assert_eq!(docs.scope, McpScope::Project);
    assert_eq!(docs.approved, Some(true));
    assert_eq!(view.instructions.len(), 2);

    let id = world.add_profile("Work", &world.project, &[]);
    let by_profile = world
        .reader
        .effective(&EffectiveViewRequest {
            profile_id: Some(id),
            cwd: None,
            overlays: Vec::new(),
        })
        .unwrap();
    assert_eq!(by_profile.profile_id, Some(id));
    assert_eq!(by_profile.profile_name.as_deref(), Some("Work"));
    assert!(same(&by_profile.cwd, &world.project));

    let gone = world.reader.effective(&EffectiveViewRequest {
        profile_id: Some(id + 1),
        cwd: None,
        overlays: Vec::new(),
    });
    assert_eq!(gone.unwrap_err(), "That profile no longer exists.");
    let neither = world.reader.effective(&EffectiveViewRequest {
        profile_id: None,
        cwd: Some("  ".to_owned()),
        overlays: Vec::new(),
    });
    assert!(
        neither
            .unwrap_err()
            .contains("profile or a working directory")
    );

    assert_eq!(
        (contents(&world.claude), contents(&world.project)),
        before,
        "reading wrote nothing"
    );
}

// --- Over the protocol ------------------------------------------------------

/// A host on the given backends, a user whose agent config folder is
/// `claude`, and a project folder with a space in its path.
struct Served {
    claude: PathBuf,
    project: PathBuf,
    other: PathBuf,
    endpoint: Endpoint,
    host: RunningHost,
    /// Last: fields drop in order, and the folder goes only once nothing in it is open.
    dirs: TestDirs,
}

impl Served {
    /// A host on today's backends: the shell, then Claude Code over the fake
    /// `claude`.
    fn start() -> Self {
        Self::start_on(|dirs, claude| backends(&dirs.user_env(), Some(claude_settings(claude))))
    }

    fn start_on(backends: impl FnOnce(&TestDirs, &Path) -> Vec<Arc<dyn AgentBackend>>) -> Self {
        let dirs = TestDirs::new();
        let claude = dirs.home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let project = dirs.folder("my project");
        let other = dirs.folder("someone else");
        let endpoint = dirs.endpoint();
        let host = RunningHost::start(HostConfig {
            run: Default::default(),
            folders: dirs.folders(),
            endpoint: endpoint.clone(),
            backends: backends(&dirs, &claude),
            inherited_env: std::env::vars_os().collect(),
        });
        Self {
            claude,
            project,
            other,
            endpoint,
            host,
            dirs,
        }
    }

    async fn stop(self) {
        self.host.stop(HostStopReason::Stop).await;
    }

    async fn client(&self) -> TestClient {
        TestClient::connect(&self.endpoint, &[]).await
    }

    /// The host's database, as a test writes fixtures into it.
    fn db(&self) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.dirs.data.join("floti.db")).unwrap();
        db.busy_timeout(PATIENCE).unwrap();
        db
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_reads_scopes_trees_and_the_effective_view() {
    let served = Served::start();
    add_profile(&served.db(), "Work", &served.project, &[&served.other]);
    put(&served.claude.join("CLAUDE.md"), "# me\n");
    put(
        &served.claude.join("settings.json"),
        "{\"model\":\"user\"}\n",
    );
    put(
        &served.claude.join(".credentials.json"),
        "{\"secret\":true}",
    );
    put(&served.claude.join("sessions/123.abcdef.key"), "secret");
    put(
        &served.project.join(".claude/settings.json"),
        "{\"model\":\"project\"}\n",
    );
    put(
        &served.project.join(".claude/agents/helper.md"),
        "---\nname: helper\n---\n",
    );
    let mut client = served.client().await;

    let scopes = client.request::<ListConfigScopes>(&()).await.unwrap();
    let kinds: Vec<ConfigScopeKind> = scopes.iter().map(|scope| scope.kind).collect();
    assert_eq!(
        kinds,
        [
            ConfigScopeKind::User,
            ConfigScopeKind::Project,
            ConfigScopeKind::Project
        ]
    );
    assert!(same(&scopes[0].path, &served.claude));
    assert!(same(&scopes[1].path, &served.project));
    assert!(same(&scopes[2].path, &served.other));

    let user = client
        .request::<ReadConfigTree>(&ConfigScopeRef {
            scope_path: scope(&served.claude),
        })
        .await
        .unwrap();
    let rel: Vec<&str> = user.files.iter().map(|f| f.rel_path.as_str()).collect();
    assert!(rel.contains(&"CLAUDE.md"), "{rel:?}");
    assert!(rel.contains(&"settings.json"), "{rel:?}");
    assert!(rel.contains(&".credentials.json"), "{rel:?}");
    for listed in &user.files {
        assert!(!listed.rel_path.ends_with(".key"), "{rel:?}");
    }
    let project = client
        .request::<ReadConfigTree>(&ConfigScopeRef {
            scope_path: scope(&served.project),
        })
        .await
        .unwrap();
    assert!(
        project
            .files
            .iter()
            .any(|f| f.kind == ConfigFileKind::Agent)
    );

    let view = client
        .request::<ReadEffectiveView>(&EffectiveViewRequest {
            profile_id: None,
            cwd: Some(served.project.display().to_string()),
            overlays: Vec::new(),
        })
        .await
        .unwrap();
    let model = view.settings.iter().find(|s| s.key == "model").unwrap();
    assert_eq!(model.winner, SettingsLayerKind::Project);
    assert_eq!(model.value, "\"project\"");
    assert!(model.overridden);
    served.stop().await;
}

/// Every channel the config page used that composition does not, as an
/// earlier client still names it.
const RETIRED_CHANNELS: &[&str] = &[
    "config:read",
    "config:render",
    "config:write",
    "config:snapshots",
    "config:snapshot",
    "config:restore",
    "config:watch",
    "config:create",
    "config:rename",
    "config:delete",
    "config:mcpPreview",
    "config:mcpAdd",
    "config:mcpRemove",
    "config:mcpApprove",
    "config:mcpList",
    "config:doctor",
    "carry:versions",
];

#[tokio::test(flavor = "multi_thread")]
async fn no_channel_that_wrote_an_agents_config_or_only_its_page_read_is_served() {
    let served = Served::start();
    let notes = served.claude.join("CLAUDE.md");
    put(&notes, "# me\n");
    let before = contents(&served.claude);
    let mut client = served.client().await;
    let payload = to_payload(&json!({
        "path": notes.display().to_string(),
        "scopePath": scope(&served.claude),
        "content": "# rewritten\n",
        "expectedHash": null,
        "reason": "edit",
    }))
    .unwrap();
    for channel in RETIRED_CHANNELS {
        let refused = client
            .request_on(channel, payload.clone())
            .await
            .expect_err(channel);
        assert_eq!(refused.code, ErrorCode::UnknownChannel, "{channel}");
        assert!(refused.message.contains(channel), "{channel}: {refused:?}");
    }
    assert_eq!(contents(&served.claude), before, "nothing was written");
    served.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_whose_agents_have_no_config_files_says_so_on_every_config_channel() {
    let served = Served::start_on(|dirs, _| {
        vec![Arc::new(shell_backend(&dirs.user_env())) as Arc<dyn AgentBackend>]
    });
    let mut client = served.client().await;
    let scopes = client.request::<ListConfigScopes>(&()).await.unwrap_err();
    let tree = client
        .request::<ReadConfigTree>(&ConfigScopeRef {
            scope_path: scope(&served.project),
        })
        .await
        .unwrap_err();
    let view = client
        .request::<ReadEffectiveView>(&EffectiveViewRequest {
            profile_id: None,
            cwd: Some(served.project.display().to_string()),
            overlays: Vec::new(),
        })
        .await
        .unwrap_err();
    for refused in [scopes, tree, view] {
        assert_eq!(refused.message, NO_AGENT_CONFIG, "{refused:?}");
    }
    served.stop().await;
}
