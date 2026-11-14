//! Discovery's rules, one module at a time: the parse of `git status`
//! (core's git.test.ts), which cached rows a scan or a removed root proves
//! wrong (scan.test.ts, "reconciling the discovery cache"), what a scan of
//! folders the test makes finds and how a project's `.claude` is counted
//! (core's scan and claude-inventory tests), and scans that overlap
//! (desktop's services.scan.test.ts), with the same inputs and expectations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use floti_protocol::channels::workspace::{
    ClaudeInventory, DiscoveryError, DiscoveryResult, GitState, HostSettingsPatch, Millis, Project,
    ProjectKind,
};

use super::git::parse_git_status;
use super::inventory::read_claude_inventory;
use super::scan::{disproved_project_paths, orphaned_project_paths, scan};
use super::suggest::{find_enclosing_workspace, suggest_roots};
use super::*;
use crate::store::Store;
use crate::store::projects::EMPTY_INVENTORY;
use crate::testkit::{GitRepo, TestDirs};

fn state(branch: Option<&str>, detached: bool, dirty: u32, ahead: u32, behind: u32) -> GitState {
    GitState {
        branch: branch.map(str::to_owned),
        detached,
        dirty,
        ahead,
        behind,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// parseGitStatus: real `git status --porcelain=v2 --branch` outputs.
// ---------------------------------------------------------------------------

#[test]
fn reads_a_clean_branch_with_an_upstream() {
    let out = [
        "# branch.oid 38a0c50f0e9a4f1b0b2f3d4c5e6a7b8c9d0e1f22",
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +0 -0",
        "",
    ]
    .join("\n");
    assert_eq!(parse_git_status(&out), state(Some("main"), false, 0, 0, 0));
}

#[test]
fn counts_changed_renamed_unmerged_and_untracked_paths_as_dirty() {
    let out = [
        "# branch.head feature/discovery",
        "# branch.upstream origin/feature/discovery",
        "# branch.ab +2 -3",
        "1 .M N... 100644 100644 100644 aaa bbb packages/core/src/index.ts",
        "1 M. N... 100644 100644 100644 ccc ddd packages/core/src/types.ts",
        "2 R. N... 100644 100644 100644 eee fff R100 new/path.ts\told/path.ts",
        "u UU N... 100644 100644 100644 100644 111 222 333 conflicted.ts",
        "? untracked.ts",
        "",
    ]
    .join("\n");
    assert_eq!(
        parse_git_status(&out),
        state(Some("feature/discovery"), false, 5, 2, 3)
    );
}

#[test]
fn recognises_a_detached_head() {
    let out = ["# branch.oid 38a0c50", "# branch.head (detached)", ""].join("\n");
    let parsed = parse_git_status(&out);
    assert!(parsed.detached);
    assert_eq!(parsed.branch, None);
}

#[test]
fn leaves_ahead_and_behind_at_zero_when_there_is_no_upstream() {
    let out = [
        "# branch.oid 38a0c50",
        "# branch.head local-only",
        "? notes.md",
        "",
    ]
    .join("\n");
    assert_eq!(
        parse_git_status(&out),
        state(Some("local-only"), false, 1, 0, 0)
    );
}

#[test]
fn handles_names_containing_spaces_in_the_path_column() {
    let out = [
        "# branch.head main",
        "# branch.ab +0 -0",
        "1 .M N... 100644 100644 100644 aaa bbb Acme Project/README.md",
        "",
    ]
    .join("\n");
    assert_eq!(parse_git_status(&out).dirty, 1);
}

// ---------------------------------------------------------------------------
// Reconciling the discovery cache.
// ---------------------------------------------------------------------------

fn join(base: &Path, parts: &[&str]) -> String {
    let mut path = base.to_path_buf();
    for part in parts {
        path.push(part);
    }
    path.to_string_lossy().into_owned()
}

fn scanned(roots: &[String], projects: &[String], errors: &[(String, &str)]) -> DiscoveryResult {
    DiscoveryResult {
        roots: roots.to_vec(),
        workspaces: Vec::new(),
        projects: projects
            .iter()
            .map(|path| Project {
                path: path.clone(),
                name: path.clone(),
                kind: ProjectKind::Folder,
                workspace_path: None,
                has_claude_dir: false,
                inventory: EMPTY_INVENTORY,
                git: None,
            })
            .collect(),
        errors: errors
            .iter()
            .map(|(path, message)| DiscoveryError {
                path: path.clone(),
                message: (*message).to_owned(),
            })
            .collect(),
        scanned_at: String::new(),
        duration_ms: Millis(0.0),
    }
}

#[test]
fn disproves_a_cached_row_a_completed_scan_of_its_own_root_did_not_return() {
    let dirs = TestDirs::new();
    let tool = join(dirs.root(), &["tool"]);
    let cached = [
        join(dirs.root(), &["tool", "src"]),
        join(dirs.root(), &["tool", "tests"]),
        tool.clone(),
    ];
    assert_eq!(
        disproved_project_paths(
            &cached,
            &scanned(
                std::slice::from_ref(&tool),
                std::slice::from_ref(&tool),
                &[]
            )
        ),
        [cached[0].clone(), cached[1].clone()]
    );
}

#[test]
fn keeps_everything_under_a_root_the_scan_could_not_read() {
    let dirs = TestDirs::new();
    let gone = join(dirs.root(), &["gone"]);
    let cached = [join(dirs.root(), &["gone", "alpha"])];
    assert!(
        disproved_project_paths(
            &cached,
            &scanned(
                std::slice::from_ref(&gone),
                &[],
                &[(gone.clone(), "not a directory")]
            )
        )
        .is_empty()
    );
    // An error below the root is the same answer: the walk was incomplete.
    assert!(
        disproved_project_paths(
            &cached,
            &scanned(
                std::slice::from_ref(&gone),
                &[],
                &[(join(dirs.root(), &["gone", "beta"]), "EPERM")]
            )
        )
        .is_empty()
    );
}

#[test]
fn keeps_a_cached_row_that_belongs_to_no_current_root_at_all() {
    let dirs = TestDirs::new();
    let cached = [join(dirs.root(), &["elsewhere", "alpha"])];
    assert!(
        disproved_project_paths(&cached, &scanned(&[join(dirs.root(), &["here"])], &[], &[]))
            .is_empty()
    );
}

/// On every OS: the host compares folders ignoring case everywhere, as the
/// discovery cache does.
#[test]
fn matches_roots_and_rows_across_two_spellings_of_one_directory() {
    let dirs = TestDirs::new();
    let cached = [join(dirs.root(), &["Tool", "SRC"])];
    let tool = join(dirs.root(), &["tool"]);
    assert_eq!(
        disproved_project_paths(
            &cached,
            &scanned(
                std::slice::from_ref(&tool),
                std::slice::from_ref(&tool),
                &[]
            )
        ),
        cached
    );
    assert!(
        disproved_project_paths(
            &cached,
            &scanned(
                std::slice::from_ref(&tool),
                &[join(dirs.root(), &["tool", "src"])],
                &[]
            )
        )
        .is_empty()
    );
}

#[test]
fn orphans_the_rows_a_removed_root_put_there_and_only_those() {
    let dirs = TestDirs::new();
    let removed = join(dirs.root(), &["tool"]);
    let kept = join(dirs.root(), &["work"]);
    let cached = [
        join(dirs.root(), &["tool", "src"]),
        removed.clone(),
        join(dirs.root(), &["work", "alpha"]),
    ];
    assert_eq!(
        orphaned_project_paths(&cached, &removed, &[kept]),
        [cached[0].clone(), cached[1].clone()]
    );
}

#[test]
fn keeps_an_orphan_that_another_root_still_covers() {
    let dirs = TestDirs::new();
    let outer = dirs.root().to_string_lossy().into_owned();
    let inner = join(dirs.root(), &["tool"]);
    let cached = [join(dirs.root(), &["tool", "src"])];
    assert!(orphaned_project_paths(&cached, &inner, &[outer]).is_empty());
}

// ---------------------------------------------------------------------------
// Suggested roots (roots.ts).
// ---------------------------------------------------------------------------

#[test]
fn suggests_the_folder_around_an_enclosing_workspace_first_then_the_home_workspace_folder() {
    let dirs = TestDirs::new();
    let workspaces = dirs.folder("workspaces");
    let dev = workspaces.join("dev");
    std::fs::create_dir_all(dev.join("repos").join("alpha")).unwrap();
    std::fs::write(dev.join("workspace.yaml"), "name: dev\n").unwrap();
    std::fs::create_dir_all(dirs.home.join(".harness")).unwrap();
    let inside = dev.join("repos").join("alpha");

    assert_eq!(find_enclosing_workspace(&inside), Some(dev.clone()));
    assert_eq!(
        suggest_roots(&inside, Some(&dirs.home)),
        [
            workspaces.to_string_lossy().into_owned(),
            dirs.home.join(".harness").to_string_lossy().into_owned()
        ]
    );
    // Nothing that is not there, and never the home folder itself.
    let elsewhere = dirs.folder("elsewhere");
    std::fs::remove_dir(dirs.home.join(".harness")).unwrap();
    assert!(suggest_roots(&elsewhere, Some(&dirs.home)).is_empty());
}

/// A workspace folder `dev` under `root` with `alpha` in `repos/` and `beta`
/// in `projects/`, so which manifest was read shows in its repos.
fn workspace_folder(root: &Path) -> PathBuf {
    let dev = root.join("dev");
    std::fs::create_dir_all(dev.join("repos").join("alpha")).unwrap();
    std::fs::create_dir_all(dev.join("projects").join("beta")).unwrap();
    dev
}

/// What the scan made of `dev`: its name, template and the names of its repos.
fn read_back(dev: &Path) -> (String, Option<String>, Vec<String>) {
    let result = scan(&[dev.to_string_lossy().into_owned()], false);
    assert_eq!(result.workspaces.len(), 1, "{result:?}");
    let workspace = &result.workspaces[0];
    let dev_row = result
        .projects
        .iter()
        .find(|project| project.path == workspace.path)
        .expect("the workspace is a project too");
    assert_eq!(dev_row.kind, ProjectKind::Workspace);
    let repos = workspace
        .repo_paths
        .iter()
        .map(|path| {
            Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    (workspace.name.clone(), workspace.template.clone(), repos)
}

#[test]
fn a_folder_with_workspace_yaml_is_a_workspace_read_from_it() {
    let dirs = TestDirs::new();
    let dev = workspace_folder(&dirs.folder("all"));
    std::fs::write(
        dev.join("workspace.yaml"),
        "name: \"new\"\ntemplate: \"minimal\"\nrepos: \"projects\"\n",
    )
    .unwrap();

    assert_eq!(
        read_back(&dev),
        (
            "new".to_owned(),
            Some("minimal".to_owned()),
            vec!["beta".to_owned()]
        )
    );
}

#[test]
fn a_folder_with_harness_yaml_is_a_workspace_read_as_before_and_left_as_it_is() {
    let dirs = TestDirs::new();
    let dev = workspace_folder(&dirs.folder("all"));
    let manifest = "name: \"old\"\ntemplate: \"dev\"\nrepos: \"projects\"\n";
    std::fs::write(dev.join("harness.yaml"), manifest).unwrap();

    assert_eq!(
        read_back(&dev),
        (
            "old".to_owned(),
            Some("dev".to_owned()),
            vec!["beta".to_owned()]
        )
    );
    assert!(super::scan::is_workspace(&dev));
    assert_eq!(
        std::fs::read_to_string(dev.join("harness.yaml")).unwrap(),
        manifest
    );
    assert!(
        !dev.join("workspace.yaml").exists(),
        "a scan writes no manifest"
    );
}

#[test]
fn a_folder_with_both_manifests_is_read_from_workspace_yaml() {
    let dirs = TestDirs::new();
    let dev = workspace_folder(&dirs.folder("all"));
    std::fs::write(dev.join("workspace.yaml"), "name: \"new\"\n").unwrap();
    std::fs::write(
        dev.join("harness.yaml"),
        "name: \"old\"\ntemplate: \"dev\"\nrepos: \"projects\"\n",
    )
    .unwrap();

    assert_eq!(
        read_back(&dev),
        ("new".to_owned(), None, vec!["alpha".to_owned()])
    );
}

#[test]
fn a_harness_yaml_encloses_a_repo_for_suggestions_as_a_workspace_yaml_does() {
    let dirs = TestDirs::new();
    let dev = workspace_folder(&dirs.folder("all"));
    std::fs::write(dev.join("harness.yaml"), "").unwrap();
    let inside = dev.join("repos").join("alpha");
    assert_eq!(find_enclosing_workspace(&inside), Some(dev));
}

#[test]
fn a_suggestion_named_twice_is_offered_once() {
    let dirs = TestDirs::new();
    // The home folder's .harness is itself the folder around the workspace.
    let dev = dirs.home.join(".harness").join("dev");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(dev.join("workspace.yaml"), "").unwrap();
    assert_eq!(
        suggest_roots(&dev, Some(&dirs.home)),
        [dirs.home.join(".harness").to_string_lossy().into_owned()]
    );
}

// ---------------------------------------------------------------------------
// Overlapping scans (services.scan.test.ts): the startup scan is still
// walking when a root accepted in setup starts another one.
// ---------------------------------------------------------------------------

struct World {
    service: DiscoveryService,
    settings: Arc<SettingsService>,
    first: String,
    second: String,
    added: String,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let store = Arc::new(Mutex::new(Store::open(&dirs.data).unwrap()));
        let settings = Arc::new(SettingsService::new(Arc::clone(&store)));
        let service = DiscoveryService::new(store, Arc::clone(&settings), None);
        let first = dirs.folder("projects");
        for name in ["alpha", "beta"] {
            std::fs::create_dir_all(first.join(name).join(".git")).unwrap();
        }
        let second = dirs.folder("more");
        let added = second.join("gamma");
        std::fs::create_dir_all(added.join(".claude")).unwrap();
        Self {
            service,
            settings,
            first: first.to_string_lossy().into_owned(),
            second: second.to_string_lossy().into_owned(),
            added: added.to_string_lossy().into_owned(),
            dirs,
        }
    }

    /// Starts a scan over `roots` the way `roots:accept` and then a rescan
    /// do, and walks it; it lands when the test says.
    fn start(&self, roots: &[&String]) -> (ScanTicket, DiscoveryResult) {
        self.settings
            .write(&HostSettingsPatch {
                scan_roots: Some(roots.iter().map(|root| (*root).clone()).collect()),
                ..HostSettingsPatch::default()
            })
            .unwrap();
        let ticket = self.service.begin_scan().unwrap();
        let result = scan(&ticket.roots, false);
        (ticket, result)
    }

    fn cached_paths(&self) -> Vec<String> {
        self.service
            .cached()
            .unwrap()
            .into_iter()
            .map(|row| row.path)
            .collect()
    }
}

fn paths(result: &DiscoveryResult) -> Vec<String> {
    result.projects.iter().map(|p| p.path.clone()).collect()
}

#[test]
fn keeps_the_newer_scan_when_the_older_one_finishes_after_it() {
    let world = World::new();
    let startup = world.start(&[&world.first]);
    let accepted = world.start(&[&world.first, &world.second]);
    assert_eq!(startup.0.roots, std::slice::from_ref(&world.first));
    assert_eq!(
        accepted.0.roots,
        [world.first.clone(), world.second.clone()]
    );

    world.service.finish_scan(&accepted.0, accepted.1).unwrap();
    let told = world.service.finish_scan(&startup.0, startup.1).unwrap();

    // What the host holds, and what the older scan's caller was handed,
    // both describe the roots as they are now.
    let last = world.service.last().unwrap();
    assert_eq!(last.roots, [world.first.clone(), world.second.clone()]);
    assert!(paths(&last).contains(&world.added));
    assert_eq!(told.roots, [world.first.clone(), world.second.clone()]);
    assert!(world.cached_paths().contains(&world.added));
}

#[test]
fn takes_the_newer_scan_when_the_two_finish_in_the_order_they_started() {
    let world = World::new();
    let older = world.start(&[&world.second]);
    let newer = world.start(&[&world.first, &world.second]);

    world.service.finish_scan(&older.0, older.1).unwrap();
    world.service.finish_scan(&newer.0, newer.1).unwrap();

    let last = world.service.last().unwrap();
    assert_eq!(last.roots, [world.first.clone(), world.second.clone()]);
    let found = paths(&last);
    for project in [
        Path::new(&world.first).join("alpha"),
        Path::new(&world.first).join("beta"),
    ] {
        assert!(
            found.contains(&project.to_string_lossy().into_owned()),
            "{found:?}"
        );
    }
    assert!(found.contains(&world.added));
    assert!(Path::new(&world.added).starts_with(world.dirs.root()));
}

#[test]
fn the_branch_at_spawn_is_the_checked_out_branch_and_none_without_one() {
    let dirs = TestDirs::new();
    let repo = crate::testkit::GitRepo::init(dirs.folder("repo"));
    repo.write("readme.md", "one");
    repo.commit_all("first");
    assert_eq!(
        super::git::read_git_branch(repo.path()).as_deref(),
        Some("main")
    );
    repo.git(&["checkout", "--quiet", "-b", "feature/two"]);
    assert_eq!(
        super::git::read_git_branch(repo.path()).as_deref(),
        Some("feature/two")
    );
    // A detached head has no branch, and neither has a folder outside any
    // repository: neither is guessed.
    repo.git(&["checkout", "--quiet", "--detach"]);
    assert_eq!(super::git::read_git_branch(repo.path()), None);
    assert_eq!(super::git::read_git_branch(&dirs.folder("plain")), None);
}

// ---------------------------------------------------------------------------
// The scan (scan.test.ts): folders the test makes, scanned without git.
// ---------------------------------------------------------------------------

/// A folder each test's fixtures go in, as scan roots or the folder holding
/// them.
struct Fixtures {
    root: PathBuf,
    /// Last: fields drop in order.
    _dirs: TestDirs,
}

impl Fixtures {
    fn new() -> Self {
        let dirs = TestDirs::new();
        Self {
            root: dirs.folder("scan root"),
            _dirs: dirs,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    fn text(&self, relative: &str) -> String {
        self.path(relative).to_string_lossy().into_owned()
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

    /// The fixtures under `relative` (each a folder in this test's root)
    /// scanned as the roots, without git.
    fn scan_of(&self, relative: &[&str]) -> DiscoveryResult {
        let roots: Vec<String> = relative.iter().map(|path| self.text(path)).collect();
        scan(&roots, false)
    }
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

#[test]
fn reports_a_root_that_does_not_exist_as_an_error_rather_than_failing() {
    let world = Fixtures::new();
    let result = world.scan_of(&["nope"]);
    assert!(result.projects.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].path, world.text("nope"));
    assert_eq!(result.errors[0].message, "not a directory");
}

#[test]
fn finds_a_workspace_pointed_at_directly_and_its_repos() {
    let world = Fixtures::new();
    world.file(
        "dev/workspace.yaml",
        "name: \"dev\"\ntemplate: \"dev\"\nversion: \"0.1.0\"\n",
    );
    world.dir("dev/repos/alpha");
    world.dir("dev/repos/beta");
    world.file("dev/.claude/skills/notes/SKILL.md", "");
    world.file("dev/repos/alpha/.claude/skills/one/SKILL.md", "");
    world.file("dev/repos/alpha/.claude/agents/reviewer.md", "");

    let result = world.scan_of(&["dev"]);

    assert_eq!(result.harnesses.len(), 1);
    let workspace = &result.harnesses[0];
    assert_eq!(
        (
            workspace.name.as_str(),
            workspace.template.as_deref(),
            workspace.version.as_deref()
        ),
        ("dev", Some("dev"), Some("0.1.0"))
    );
    assert_eq!(names(&result), sorted(&["alpha", "beta", "dev"]));
    let by_name: BTreeMap<&str, _> = result
        .projects
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    assert_eq!(by_name["dev"].kind, ProjectKind::Workspace);
    assert_eq!(by_name["alpha"].kind, ProjectKind::Repo);
    assert_eq!(
        by_name["alpha"].harness_path.as_deref(),
        Some(world.text("dev").as_str())
    );
    assert_eq!(by_name["dev"].inventory.skills, 1);
    assert_eq!(by_name["alpha"].inventory.skills, 1);
    assert_eq!(by_name["alpha"].inventory.agents, 1);
    assert_eq!(by_name["beta"].inventory.skills, 0);
}

#[test]
fn finds_workspaces_one_level_below_the_root() {
    let world = Fixtures::new();
    world.file("all/dev/workspace.yaml", "name: \"dev\"\n");
    world.dir("all/dev/repos/alpha");
    world.file("all/work/workspace.yaml", "name: \"work\"\n");
    world.dir("all/work/repos/gamma");
    // A sibling that is not a workspace is not a project here: the root
    // turned out to be a folder of workspaces.
    world.dir("all/downloads");

    let result = world.scan_of(&["all"]);

    let mut workspaces: Vec<&str> = result.harnesses.iter().map(|h| h.name.as_str()).collect();
    workspaces.sort_unstable();
    assert_eq!(workspaces, ["dev", "work"]);
    assert_eq!(names(&result), sorted(&["alpha", "dev", "gamma", "work"]));
}

#[test]
fn degrades_to_plain_folders_when_there_is_no_workspace_anywhere() {
    let world = Fixtures::new();
    world.dir("all/one");
    world.dir("all/two");
    world.file("all/one/CLAUDE.md", "");

    let result = world.scan_of(&["all"]);

    assert!(result.harnesses.is_empty());
    assert_eq!(names(&result), sorted(&["one", "two"]));
    assert!(
        result
            .projects
            .iter()
            .all(|p| p.kind == ProjectKind::Folder)
    );
    let one = result.projects.iter().find(|p| p.name == "one").unwrap();
    assert!(one.inventory.claude_md);
}

#[test]
fn lists_the_folder_itself_when_none_of_its_children_looks_like_a_project() {
    let world = Fixtures::new();
    for child in ["data", "scripts", "src", "tests"] {
        world.dir(&format!("tool/{child}"));
    }
    world.file("tool/pyproject.toml", "");

    let result = world.scan_of(&["tool"]);

    assert_eq!(names(&result), ["tool"]);
    assert_eq!(result.projects[0].path, world.text("tool"));
    assert_eq!(result.projects[0].kind, ProjectKind::Folder);
}

#[test]
fn still_lists_the_children_when_one_of_them_looks_like_a_project() {
    let world = Fixtures::new();
    world.dir("repos/alpha");
    world.dir("repos/beta");
    world.dir("repos/gamma");

    let bare = world.scan_of(&["repos"]);
    assert_eq!(names(&bare), ["repos"]);

    for marker in [
        "alpha/.git",
        "alpha/.claude/skills/one/SKILL.md",
        "alpha/CLAUDE.md",
    ] {
        std::fs::remove_dir_all(world.path("repos/alpha")).unwrap();
        world.file(&format!("repos/{marker}"), "");
        let result = world.scan_of(&["repos"]);
        assert_eq!(
            names(&result),
            sorted(&["alpha", "beta", "gamma"]),
            "{marker}"
        );
    }
}

#[test]
fn sees_a_git_worktree_whose_git_is_a_file_rather_than_a_folder() {
    let world = Fixtures::new();
    world.dir("checkouts/branch-b");
    world.file(
        "checkouts/branch-a/.git",
        "gitdir: ../../real/.git/worktrees/branch-a\n",
    );

    let result = world.scan_of(&["checkouts"]);

    assert_eq!(names(&result), sorted(&["branch-a", "branch-b"]));
}

#[test]
fn lists_an_empty_root_as_itself_rather_than_as_nothing_at_all() {
    let world = Fixtures::new();
    world.dir("empty");
    let result = world.scan_of(&["empty"]);
    assert_eq!(names(&result), ["empty"]);
    assert!(result.errors.is_empty());
}

#[test]
fn handles_a_workspace_whose_manifest_is_malformed() {
    let world = Fixtures::new();
    world.file("dev/workspace.yaml", ":\n  - this is not: valid: yaml\n");
    world.dir("dev/repos/alpha");

    let result = world.scan_of(&["dev"]);

    // The file's presence is the signal; its contents are decoration.
    assert_eq!(result.harnesses.len(), 1);
    assert_eq!(result.harnesses[0].name, "dev");
    assert_eq!(names(&result), sorted(&["alpha", "dev"]));
}

#[test]
fn handles_a_workspace_with_no_repos_directory() {
    let world = Fixtures::new();
    world.file("dev/workspace.yaml", "name: \"dev\"\n");
    let result = world.scan_of(&["dev"]);
    assert!(result.harnesses[0].repo_paths.is_empty());
    assert_eq!(names(&result), ["dev"]);
}

#[test]
fn honours_a_repos_key_naming_a_different_directory() {
    let world = Fixtures::new();
    world.file("dev/workspace.yaml", "name: \"dev\"\nrepos: \"projects\"\n");
    world.dir("dev/projects/alpha");
    world.dir("dev/projects/beta");
    // The default folder exists too and is ignored: `repos:` names the
    // folder, it does not add one.
    world.dir("dev/repos/ignored");

    let result = world.scan_of(&["dev"]);

    assert_eq!(names(&result), sorted(&["alpha", "beta", "dev"]));
    let mut repos = result.harnesses[0].repo_paths.clone();
    repos.sort();
    assert_eq!(
        repos,
        [
            world.text("dev/projects/alpha"),
            world.text("dev/projects/beta")
        ]
    );
}

#[test]
fn falls_back_to_repos_when_the_key_is_absent_empty_or_not_a_string() {
    let world = Fixtures::new();
    world.file("a/workspace.yaml", "name: \"a\"\n");
    world.dir("a/repos/one");
    world.file("b/workspace.yaml", "name: \"b\"\nrepos: \"\"\n");
    world.dir("b/repos/two");
    world.file("c/workspace.yaml", "name: \"c\"\nrepos: 3\n");
    world.dir("c/repos/three");

    for (name, repo) in [("a", "one"), ("b", "two"), ("c", "three")] {
        let result = world.scan_of(&[name]);
        assert_eq!(names(&result), sorted(&[name, repo]), "{name}");
    }
}

#[test]
fn does_not_hide_the_repos_of_a_folder_converted_in_place_with_repos_dot() {
    let world = Fixtures::new();
    world.dir("work/alpha");
    world.dir("work/beta");
    world.file("work/alpha/.claude/skills/one/SKILL.md", "");

    let before = world.scan_of(&["work"]);
    assert_eq!(names(&before), sorted(&["alpha", "beta"]));

    world.file("work/workspace.yaml", "name: \"work\"\nrepos: \".\"\n");
    let after = world.scan_of(&["work"]);
    assert_eq!(
        after
            .harnesses
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>(),
        ["work"]
    );
    assert_eq!(names(&after), sorted(&["alpha", "beta", "work"]));
    let alpha = after.projects.iter().find(|p| p.name == "alpha").unwrap();
    assert_eq!(
        alpha.harness_path.as_deref(),
        Some(world.text("work").as_str())
    );
    assert_eq!(alpha.inventory.skills, 1);
}

#[test]
fn refuses_a_repos_key_that_leaves_the_workspace_rather_than_scanning_it() {
    let world = Fixtures::new();
    world.dir("outside/secret");
    world.file(
        "dev/workspace.yaml",
        "name: \"dev\"\nrepos: \"../outside\"\n",
    );
    world.dir("dev/repos/alpha");

    let escaped = world.scan_of(&["dev"]);
    assert_eq!(names(&escaped), sorted(&["alpha", "dev"]));

    let absolute = serde_json::to_string(&world.text("outside")).unwrap();
    world.file(
        "dev/workspace.yaml",
        &format!("name: \"dev\"\nrepos: {absolute}\n"),
    );
    let absolute = world.scan_of(&["dev"]);
    assert_eq!(names(&absolute), sorted(&["alpha", "dev"]));
}

#[test]
fn handles_repo_directories_whose_names_contain_spaces() {
    let world = Fixtures::new();
    world.file("dev/workspace.yaml", "name: \"dev\"\n");
    world.file("dev/repos/Acme Project/.claude/skills/one/SKILL.md", "");

    let result = world.scan_of(&["dev"]);

    let project = result
        .projects
        .iter()
        .find(|p| p.name == "Acme Project")
        .expect("the project with a space in its name");
    assert_eq!(project.inventory.skills, 1);
}

#[test]
fn skips_node_modules_and_dotted_directories() {
    let world = Fixtures::new();
    world.dir("all/project/node_modules/left-pad");
    world.dir("all/project/.git");
    world.dir("all/project/src");

    let result = world.scan_of(&["all"]);

    assert_eq!(names(&result), ["project"]);
}

#[test]
fn does_not_report_the_same_project_twice_when_two_roots_overlap() {
    let world = Fixtures::new();
    world.file("all/dev/workspace.yaml", "name: \"dev\"\n");
    world.dir("all/dev/repos/alpha");

    let result = world.scan_of(&["all", "all/dev"]);

    assert_eq!(names(&result), sorted(&["alpha", "dev"]));
}

#[test]
fn leaves_git_null_unless_asked_for_it() {
    let world = Fixtures::new();
    world.file("dev/workspace.yaml", "name: \"dev\"\n");
    GitRepo::init(world.path("dev/repos/alpha"));
    let result = world.scan_of(&["dev"]);
    assert!(result.projects.iter().all(|p| p.git.is_none()));
}

// ---------------------------------------------------------------------------
// A project's `.claude` counted (claude-inventory.test.ts).
// ---------------------------------------------------------------------------

/// The inventory of a project folder holding `files`.
fn inventory_of(files: &[(&str, &str)]) -> ClaudeInventory {
    let world = Fixtures::new();
    world.dir("project");
    for (path, content) in files {
        world.file(&format!("project/{path}"), content);
    }
    read_claude_inventory(&world.path("project"))
}

#[test]
fn reports_nothing_for_a_directory_with_no_claude_folder_and_no_claude_md() {
    assert_eq!(
        inventory_of(&[]),
        ClaudeInventory {
            skills: 0,
            commands: 0,
            agents: 0,
            hooks: false,
            settings: false,
            claude_md: false,
            mcp: false
        }
    );
}

#[test]
fn sees_a_claude_md_even_without_a_claude_folder() {
    let inventory = inventory_of(&[("CLAUDE.md", "# rules")]);
    assert!(inventory.claude_md);
    assert_eq!(inventory.skills, 0);
}

#[test]
fn counts_a_skill_as_a_directory_holding_skill_md_not_as_a_directory() {
    let inventory = inventory_of(&[
        (".claude/skills/notes/SKILL.md", ""),
        (".claude/skills/notes/reference.md", ""),
        (".claude/skills/notes/scripts/helper.md", ""),
        // A folder under skills/ with no SKILL.md is not a skill.
        (".claude/skills/leftovers/README.md", ""),
    ]);
    assert_eq!(inventory.skills, 1);
}

#[test]
fn counts_namespaced_commands_at_every_depth() {
    let inventory = inventory_of(&[
        (".claude/commands/spec/plan.md", ""),
        (".claude/commands/spec/board.md", ""),
        (".claude/commands/status.md", ""),
    ]);
    assert_eq!(inventory.commands, 3);
}

#[test]
fn counts_agents_as_markdown_files() {
    let inventory = inventory_of(&[
        (".claude/agents/reviewer.md", ""),
        (".claude/agents/planner.md", ""),
        (".claude/agents/notes.txt", ""),
    ]);
    assert_eq!(inventory.agents, 2);
}

#[test]
fn flags_hooks_settings_claude_md_and_mcp_json() {
    let inventory = inventory_of(&[
        (".claude/hooks/post-bash.ps1", ""),
        (".claude/settings.json", "{}"),
        ("CLAUDE.md", ""),
        (".mcp.json", "{}"),
    ]);
    assert!(inventory.hooks && inventory.settings && inventory.claude_md && inventory.mcp);
}

#[test]
fn treats_settings_local_json_as_settings() {
    let inventory = inventory_of(&[(".claude/settings.local.json", "{}")]);
    assert!(inventory.settings);
}
