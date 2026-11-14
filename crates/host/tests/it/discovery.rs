//! Project discovery, scan roots and git state as a client of the host sees
//! them: a scan and what its subscribers are told, the cache it leaves, a git
//! refresh, and the desktop's "scan roots and pins" settings tests, run
//! against a host through its socket. What a scan finds and how a project's
//! `.claude` is counted are unit tests of `workspace::discovery`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

use floti_host::platform::socket::Endpoint;
use floti_host::server::{HostConfig, HostRun, backends};
use floti_host::testkit::{GitRepo, PATIENCE, RunningHost, TestClient, TestDirs};
use floti_host::workspace::discovery::suggest::suggest_roots;
use floti_protocol::channel::{EventChannel, from_payload};
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::workspace::{
    AcceptRoot, AppMode, CachedProject, DiscoveryResult, DiscoveryUpdated, GitState, GitUpdated,
    HostSettings, HostSettingsPatch, ProjectKind, ReadCachedDiscovery, ReadSettings, RefreshGit,
    RemoveRoot, RootPath, ScanForProjects, ScanRequest, ScanStatus, ScanStatusChanged,
    SettingsChanged, SuggestRoots, WriteSettings,
};
use floti_protocol::{Message, Payload};

const SCAN_STATUS: u64 = 1;
const DISCOVERY: u64 = 2;
const GIT: u64 = 3;
const SETTINGS: u64 = 4;

/// A host over a test's folders, with a client connected to it.
struct World {
    host: Option<RunningHost>,
    client: TestClient,
    endpoint: Endpoint,
    /// Where each test's fixtures go; a scan root or the folder holding them.
    root: PathBuf,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

fn config(dirs: &TestDirs, endpoint: &Endpoint) -> HostConfig {
    HostConfig {
        folders: dirs.folders(),
        endpoint: endpoint.clone(),
        backends: backends(&dirs.user_env(), None),
        inherited_env: std::env::vars_os()
            .filter(|(key, _)| key != "SHELL")
            .chain([(OsString::from("SHELL"), OsString::from("/bin/sh"))])
            .collect(),
        run: HostRun {
            mode: AppMode::Dev,
            home: Some(dirs.home.clone()),
            node: None,
            desktop: false,
            keeper: None,
            wsl: None,
            config_read_only: false,
            updates: None,
        },
    }
}

impl World {
    async fn start() -> Self {
        let dirs = TestDirs::new();
        let root = dirs.folder("scan root");
        let endpoint = dirs.endpoint();
        let host = RunningHost::start(config(&dirs, &endpoint));
        let mut client = TestClient::connect(&endpoint, &[]).await;
        settled(&mut client).await;
        Self {
            host: Some(host),
            client,
            endpoint,
            root,
            dirs,
        }
    }

    /// Stops the host and starts another over the same folders.
    async fn restart(&mut self) {
        self.host
            .take()
            .expect("a running host")
            .stop(HostStopReason::Stop)
            .await;
        self.host = Some(RunningHost::start(config(&self.dirs, &self.endpoint)));
        self.client = TestClient::connect(&self.endpoint, &[]).await;
        settled(&mut self.client).await;
    }

    async fn stop(mut self) {
        if let Some(host) = self.host.take() {
            host.stop(HostStopReason::Stop).await;
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    fn text(&self, relative: &str) -> String {
        text(&self.path(relative))
    }

    /// A file at `relative` holding `content`, with its folders.
    fn file(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn dir(&self, relative: &str) {
        std::fs::create_dir_all(self.path(relative)).unwrap();
    }

    async fn write(&mut self, patch: HostSettingsPatch) -> Result<HostSettings, String> {
        self.client
            .request::<WriteSettings>(&patch)
            .await
            .map_err(|error| error.message)
    }

    async fn set_roots(&mut self, roots: &[String]) {
        self.write(HostSettingsPatch {
            scan_roots: Some(roots.to_vec()),
            ..HostSettingsPatch::default()
        })
        .await
        .expect("the roots are kept");
    }

    async fn scan(&mut self, include_git: bool) -> DiscoveryResult {
        self.client
            .request::<ScanForProjects>(&ScanRequest {
                include_git: Some(include_git),
            })
            .await
            .expect("a scan answers")
    }

    /// The fixtures under `relative` (each a folder in this test's root)
    /// made the scan roots, and scanned without git, as core's tests scan.
    async fn scan_of(&mut self, relative: &[&str]) -> DiscoveryResult {
        let roots: Vec<String> = relative.iter().map(|path| self.text(path)).collect();
        self.set_roots(&roots).await;
        self.scan(false).await
    }

    async fn cached(&mut self) -> Vec<CachedProject> {
        self.client
            .request::<ReadCachedDiscovery>(&())
            .await
            .expect("the cache answers")
    }

    async fn settings(&mut self) -> HostSettings {
        self.client.request::<ReadSettings>(&()).await.unwrap()
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn names(result: &DiscoveryResult) -> Vec<String> {
    let mut names: Vec<String> = result.projects.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    names.sort();
    names
}

/// Waits until the host's own first scan has landed, so what a test asks
/// for afterwards is not mixed up with it.
async fn settled(client: &mut TestClient) {
    let start = client
        .subscribe_on(DISCOVERY + 100, DiscoveryUpdated::NAME)
        .await;
    if let Message::Snapshot(snapshot) = &start {
        let state: Option<DiscoveryResult> = from_payload(&snapshot.state).unwrap();
        if state.is_some() {
            return;
        }
    }
    next_payload(client, DISCOVERY + 100).await;
}

/// The next event's payload on `subscription`.
async fn next_payload(client: &mut TestClient, subscription: u64) -> Payload {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match client.next(left).await {
            Some(Message::Event(event)) if event.subscription == subscription => {
                return event.payload;
            }
            Some(_) => continue,
            None => panic!("no event on subscription {subscription} within {PATIENCE:?}"),
        }
    }
}

fn state_of(client_start: &Message) -> Payload {
    match client_start {
        Message::Snapshot(snapshot) => snapshot.state.clone(),
        other => panic!("expected a snapshot, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// A scan as its subscribers see it, and the cache it leaves.
// ---------------------------------------------------------------------------

/// A workspace with two repositories, and beside it a folder of projects: a
/// repository, a plain folder and a folder with only a CLAUDE.md.
fn workspace_fixture(world: &World) -> (GitRepo, GitRepo) {
    world.file(
        "workspaces/dev/workspace.yaml",
        "name: \"dev\"\ntemplate: \"starter\"\nversion: \"0.1.0\"\n",
    );
    world.file("workspaces/dev/.claude/skills/notes/SKILL.md", "");
    let alpha = GitRepo::init(world.path("workspaces/dev/repos/alpha"));
    alpha.write("README.md", "# alpha\n");
    alpha.write(".claude/agents/reviewer.md", "");
    alpha.commit_all("first");
    alpha.write("README.md", "# alpha, changed\n");
    world.dir("workspaces/dev/repos/beta");
    let gamma = GitRepo::init(world.path("projects/gamma"));
    gamma.write("README.md", "# gamma\n");
    gamma.commit_all("first");
    world.dir("projects/plain/src");
    world.file("projects/rules/CLAUDE.md", "# rules\n");
    (alpha, gamma)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_is_pushed_between_running_and_stopped_and_its_projects_stay_cached_across_a_restart()
 {
    let mut world = World::start().await;
    let _repos = workspace_fixture(&world);
    let roots = [world.text("workspaces"), world.text("projects")];
    world.set_roots(&roots).await;
    let before = world
        .client
        .subscribe_on(SCAN_STATUS, ScanStatusChanged::NAME)
        .await;
    let idle: ScanStatus = from_payload(&state_of(&before)).unwrap();
    assert!(!idle.running, "{idle:?}");
    world
        .client
        .subscribe_on(DISCOVERY, DiscoveryUpdated::NAME)
        .await;

    let answer = world.scan(true).await;

    // What the subscriber saw, in order: running, the result, stopped.
    let mut seen: Vec<String> = Vec::new();
    let deadline = Instant::now() + PATIENCE;
    let mut pushed: Option<DiscoveryResult> = None;
    while seen.last().map(String::as_str) != Some("stopped") {
        let left = deadline.saturating_duration_since(Instant::now());
        match world.client.next(left).await {
            Some(Message::Event(event)) if event.subscription == SCAN_STATUS => {
                let status: ScanStatus = from_payload(&event.payload).unwrap();
                assert_eq!(status.error, None);
                seen.push(if status.running { "running" } else { "stopped" }.into());
            }
            Some(Message::Event(event)) if event.subscription == DISCOVERY => {
                pushed = Some(from_payload(&event.payload).unwrap());
                seen.push("result".into());
            }
            Some(_) => continue,
            None => panic!("the scan did not stop; saw {seen:?}"),
        }
    }
    assert_eq!(seen, ["running", "result", "stopped"]);
    assert_eq!(pushed.as_ref(), Some(&answer));

    // The answer: the workspace and its repositories, then the folder of
    // projects, which lists its children because one is a repository.
    assert_eq!(answer.roots, roots);
    assert!(answer.errors.is_empty(), "{:?}", answer.errors);
    assert_eq!(answer.workspaces.len(), 1);
    let workspace = &answer.workspaces[0];
    assert_eq!(workspace.name, "dev");
    assert_eq!(workspace.template.as_deref(), Some("starter"));
    assert_eq!(workspace.version.as_deref(), Some("0.1.0"));
    assert_eq!(
        workspace.repo_paths,
        [
            world.text("workspaces/dev/repos/alpha"),
            world.text("workspaces/dev/repos/beta")
        ]
    );
    let by_name: BTreeMap<&str, _> = answer
        .projects
        .iter()
        .map(|project| (project.name.as_str(), project))
        .collect();
    assert_eq!(
        by_name.keys().copied().collect::<Vec<_>>(),
        ["alpha", "beta", "dev", "gamma", "plain", "rules"]
    );
    let dev = world.text("workspaces/dev");
    assert_eq!(by_name["dev"].kind, ProjectKind::Workspace);
    assert_eq!(by_name["dev"].workspace_path.as_deref(), Some(dev.as_str()));
    assert_eq!(by_name["dev"].inventory.skills, 1);
    assert_eq!(by_name["alpha"].kind, ProjectKind::Repo);
    assert_eq!(
        by_name["alpha"].workspace_path.as_deref(),
        Some(dev.as_str())
    );
    assert!(by_name["alpha"].has_claude_dir);
    assert_eq!(by_name["alpha"].inventory.agents, 1);
    for folder in ["gamma", "plain", "rules"] {
        assert_eq!(by_name[folder].kind, ProjectKind::Folder, "{folder}");
        assert_eq!(by_name[folder].workspace_path, None, "{folder}");
    }
    assert!(by_name["rules"].inventory.claude_md);
    let alpha_git = by_name["alpha"]
        .git
        .as_ref()
        .expect("alpha is a repository");
    assert_eq!(alpha_git.branch.as_deref(), Some("main"));
    assert_eq!(alpha_git.dirty, 1);
    let gamma_git = by_name["gamma"]
        .git
        .as_ref()
        .expect("gamma is a repository");
    assert_eq!(
        (gamma_git.branch.as_deref(), gamma_git.dirty),
        (Some("main"), 0)
    );
    for not_a_repository in ["dev", "beta", "plain", "rules"] {
        assert_eq!(by_name[not_a_repository].git, None, "{not_a_repository}");
    }
    let paths: Vec<&str> = answer.projects.iter().map(|p| p.path.as_str()).collect();
    let mut in_order = paths.clone();
    in_order.sort_by_key(|path| path.to_lowercase());
    assert_eq!(paths, in_order, "projects come in path order");

    // The cache holds the same projects, and still does after a restart.
    let cached_of = |rows: &[CachedProject]| -> Vec<(String, ProjectKind, Option<String>)> {
        rows.iter()
            .map(|row| (row.path.clone(), row.kind, row.template.clone()))
            .collect()
    };
    let expected: Vec<(String, ProjectKind, Option<String>)> = answer
        .projects
        .iter()
        .map(|project| {
            let template = (project.kind == ProjectKind::Workspace).then(|| "starter".to_owned());
            (project.path.clone(), project.kind, template)
        })
        .collect();
    let rows = world.cached().await;
    assert_eq!(cached_of(&rows), expected);
    let alpha_row = rows.iter().find(|row| row.name == "alpha").unwrap();
    assert_eq!(alpha_row.git.as_ref(), Some(alpha_git));
    assert_eq!(alpha_row.inventory, by_name["alpha"].inventory);

    world.restart().await;
    assert_eq!(cached_of(&world.cached().await), expected);
    world.stop().await;
}

// ---------------------------------------------------------------------------
// Git state, refreshed.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_git_refresh_tells_subscribers_each_projects_branch_and_counts_and_null_where_there_is_no_repository()
 {
    let mut world = World::start().await;
    // On main, one commit ahead of the branch it tracks, one file changed.
    let alpha = GitRepo::init(world.path("projects/alpha"));
    alpha.write("README.md", "# alpha\n");
    alpha.commit_all("first");
    alpha.git(&["branch", "base"]);
    alpha.write("NOTES.md", "notes\n");
    alpha.commit_all("second");
    alpha.git(&["branch", "--set-upstream-to=base", "main"]);
    alpha.write("README.md", "# alpha, changed\n");
    // On a branch of its own, with an untracked file.
    let beta = GitRepo::init(world.path("projects/beta"));
    beta.write("README.md", "# beta\n");
    beta.commit_all("first");
    beta.git(&["checkout", "-q", "-b", "feature/discovery"]);
    beta.write("untracked.txt", "new\n");
    // Not a repository, and a worktree whose repository is gone.
    world.dir("projects/plain");
    world.file(
        "projects/broken/.git",
        "gitdir: ../nowhere/.git/worktrees/broken\n",
    );
    let found = world.scan_of(&["projects"]).await;
    assert_eq!(names(&found), sorted(&["alpha", "beta", "broken", "plain"]));
    assert!(found.projects.iter().all(|project| project.git.is_none()));
    // A project whose folder goes after the scan.
    world.dir("projects/gone");
    let found = world.scan(false).await;
    assert_eq!(found.projects.len(), 5);
    std::fs::remove_dir(world.path("projects/gone")).unwrap();

    world.client.subscribe_on(GIT, GitUpdated::NAME).await;
    let answer = world
        .client
        .request::<RefreshGit>(&())
        .await
        .expect("a refresh answers");
    let told: BTreeMap<String, Option<GitState>> =
        from_payload(&next_payload(&mut world.client, GIT).await).unwrap();
    assert_eq!(told, answer);

    let alpha_state = answer[&world.text("projects/alpha")].as_ref().unwrap();
    assert_eq!(
        (
            alpha_state.branch.as_deref(),
            alpha_state.detached,
            alpha_state.dirty,
            alpha_state.ahead,
            alpha_state.behind
        ),
        (Some("main"), false, 1, 1, 0)
    );
    let beta_state = answer[&world.text("projects/beta")].as_ref().unwrap();
    assert_eq!(
        (
            beta_state.branch.as_deref(),
            beta_state.dirty,
            beta_state.error.as_deref()
        ),
        (Some("feature/discovery"), 1, None)
    );
    assert_eq!(answer[&world.text("projects/plain")], None);
    assert_eq!(answer[&world.text("projects/gone")], None);
    // A repository git cannot answer for says why, and claims nothing else.
    let broken = answer[&world.text("projects/broken")].as_ref().unwrap();
    assert!(
        broken.error.as_deref().is_some_and(|why| !why.is_empty()),
        "{broken:?}"
    );
    assert_eq!(
        (
            broken.branch.as_deref(),
            broken.dirty,
            broken.ahead,
            broken.behind
        ),
        (None, 0, 0, 0)
    );

    // The cache carries what the refresh read.
    let rows = world.cached().await;
    let alpha_row = rows.iter().find(|row| row.name == "alpha").unwrap();
    assert_eq!(alpha_row.git.as_ref(), Some(alpha_state));

    // A later subscriber starts from the same state.
    let mut late = TestClient::connect(&world.endpoint, &[]).await;
    let start = late.subscribe_on(GIT, GitUpdated::NAME).await;
    let snapshot: BTreeMap<String, Option<GitState>> = from_payload(&state_of(&start)).unwrap();
    assert_eq!(snapshot, answer);
    drop(late);
    world.stop().await;
}

// ---------------------------------------------------------------------------
// Roots, as the desktop's settings-ipc.test.ts "scan roots and pins" has
// them, and suggested roots.
// ---------------------------------------------------------------------------

/// The desktop test world's projects folder: alpha and beta, each a
/// repository whose README says `# main`.
fn projects_fixture(world: &World) -> (String, String, String) {
    for name in ["alpha", "beta"] {
        let repo = GitRepo::init(world.path(&format!("projects/{name}")));
        repo.write("README.md", "# main\n");
        repo.commit_all("first");
    }
    (
        world.text("projects"),
        world.text("projects/alpha"),
        world.text("projects/beta"),
    )
}

fn lower_paths(result: &DiscoveryResult) -> Vec<String> {
    let mut paths: Vec<String> = result
        .projects
        .iter()
        .map(|p| p.path.to_lowercase())
        .collect();
    paths.sort();
    paths
}

/// The next `settings:changed` event's settings.
async fn next_settings(client: &mut TestClient) -> HostSettings {
    from_payload(&next_payload(client, SETTINGS).await).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_folder_becomes_a_root_and_the_next_scan_lists_what_is_in_it() {
    let mut world = World::start().await;
    let (projects, alpha, beta) = projects_fixture(&world);
    world.set_roots(std::slice::from_ref(&projects)).await;
    let more = world.text("more projects");
    let gamma = GitRepo::init(world.path("more projects/gamma"));
    gamma.write("README.md", "# gamma\n");
    let mut watcher = TestClient::connect(&world.endpoint, &[]).await;
    watcher.subscribe_on(SETTINGS, SettingsChanged::NAME).await;

    let roots = world
        .client
        .request::<AcceptRoot>(&RootPath { path: more.clone() })
        .await
        .unwrap();
    assert_eq!(roots, [projects.clone(), more.clone()]);
    assert_eq!(world.settings().await.scan_roots, roots);
    assert_eq!(next_settings(&mut watcher).await.scan_roots, roots);

    // Accepting it again, in another spelling, changes nothing.
    let again = world
        .client
        .request::<AcceptRoot>(&RootPath {
            path: more.to_uppercase(),
        })
        .await
        .unwrap();
    assert_eq!(again, roots);

    let scanned = world.scan(false).await;
    let mut expected: Vec<String> = [alpha, beta, text(gamma.path())]
        .iter()
        .map(|path| path.to_lowercase())
        .collect();
    expected.sort();
    assert_eq!(lower_paths(&scanned), expected);

    // A folder that is not a path a scan could read the same way twice is
    // refused with the settings' own sentence, and nothing is kept.
    let refused = world
        .client
        .request::<AcceptRoot>(&RootPath {
            path: "relative/folder".into(),
        })
        .await
        .unwrap_err();
    assert!(
        refused.message.contains("expected an absolute path"),
        "{}",
        refused.message
    );
    assert_eq!(world.settings().await.scan_roots, roots);
    drop(watcher);
    world.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_roots_projects_and_cached_rows_go_the_others_stay_and_nothing_on_disk_moves() {
    let mut world = World::start().await;
    let (projects, alpha, _) = projects_fixture(&world);
    let more = world.text("more projects");
    let gamma = GitRepo::init(world.path("more projects/gamma"));
    gamma.write("README.md", "# gamma\n");
    world.set_roots(&[projects.clone(), more.clone()]).await;
    world.scan(false).await;
    let mut watcher = TestClient::connect(&world.endpoint, &[]).await;
    watcher.subscribe_on(SETTINGS, SettingsChanged::NAME).await;

    let roots = world
        .client
        .request::<RemoveRoot>(&RootPath {
            path: projects.clone(),
        })
        .await
        .unwrap();
    assert_eq!(roots, std::slice::from_ref(&more));
    assert_eq!(
        world.settings().await.scan_roots,
        std::slice::from_ref(&more)
    );
    assert_eq!(
        next_settings(&mut watcher).await.scan_roots,
        std::slice::from_ref(&more)
    );
    let cached: Vec<String> = world
        .cached()
        .await
        .into_iter()
        .map(|row| row.path.to_lowercase())
        .collect();
    assert_eq!(cached, [text(gamma.path()).to_lowercase()]);

    let scanned = world.scan(false).await;
    assert_eq!(lower_paths(&scanned), [text(gamma.path()).to_lowercase()]);
    assert!(Path::new(&alpha).exists());
    assert_eq!(
        std::fs::read_to_string(Path::new(&alpha).join("README.md")).unwrap(),
        "# main\n"
    );
    drop(watcher);
    world.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_pin_stays_across_a_rescan_one_whose_folder_is_gone_included() {
    let mut world = World::start().await;
    let (projects, alpha, _) = projects_fixture(&world);
    world.set_roots(std::slice::from_ref(&projects)).await;
    let gone = world.text("unplugged/tools");
    let mut pins = vec![alpha, gone];
    pins.sort_by_key(|pin| pin.to_lowercase());
    world
        .write(HostSettingsPatch {
            pinned_projects: Some(pins.clone()),
            ..HostSettingsPatch::default()
        })
        .await
        .unwrap();

    world.scan(false).await;
    assert_eq!(world.settings().await.pinned_projects, pins);
    world.restart().await;
    world.scan(true).await;
    assert_eq!(world.settings().await.pinned_projects, pins);
    world.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn suggested_roots_put_an_enclosing_workspace_first_then_the_home_workspace_folder() {
    let mut world = World::start().await;
    std::fs::create_dir_all(world.dirs.home.join(".harness")).unwrap();
    let suggested = world.client.request::<SuggestRoots>(&()).await.unwrap();
    // The host looks around the folder it runs in, as today's app looks
    // around its own: that may or may not be inside a workspace.
    let start = std::env::current_dir().unwrap();
    assert_eq!(suggested, suggest_roots(&start, Some(&world.dirs.home)));
    assert_eq!(
        suggested.last(),
        Some(&text(&world.dirs.home.join(".harness")))
    );
    world.stop().await;
}
