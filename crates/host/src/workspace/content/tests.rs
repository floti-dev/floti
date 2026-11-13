//! The content service over a store and real folders: scopes, reading,
//! rendering, searching, wikilinks, artifacts, writes and what a write is
//! refused. Every test makes its own folders and its own store, with a
//! space in every path; the agent's config folder is the test's own fake
//! home's (or the folder `CLAUDE_CONFIG_DIR` names inside it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use floti_protocol::channels::workspace::{
    ConfigScopeKind, ConfigWriteReason, ContentFileKind, Project, ProjectKind, WriteConfigRequest,
    WriteConfigResult,
};
use serde_json::{Value, json};

use super::ContentService;
use crate::agent::claude_code::ClaudeCodeSettings;
use crate::agent_config::{AgentConfigFolders, hash_content, normalize, snapshots};
use crate::platform::folders::UserEnv;
use crate::server::backends;
use crate::store::Store;
use crate::store::projects::{EMPTY_INVENTORY, cache_projects};
use crate::testkit::{TestDirs, link_file, link_folder};
use crate::workspace::content::testing::{fixture, put};
use crate::workspace::files::FilesService;
use crate::workspace::roots::KnownRoots;

// ---------------------------------------------------------------------------
// The world: a store, the folders it knows, and the service over them.

struct World {
    store: Arc<Mutex<Store>>,
    sessions: Arc<RwLock<Vec<String>>>,
    content: ContentService,
    files: FilesService,
    /// Last: fields drop in order, and the folder goes only once nothing in it is open.
    dirs: TestDirs,
}

impl World {
    /// The service over the test's folders, with Claude Code's config where
    /// the test's fake home puts it: `.claude` and `.claude.json` in it.
    fn new() -> Self {
        Self::with_config_dir(|_| None)
    }

    /// The same, with `CLAUDE_CONFIG_DIR` set to the folder `config_dir`
    /// names in the test's folders, or unset for `None`: resolved as the
    /// host resolves it for a launch.
    fn with_config_dir(config_dir: impl FnOnce(&TestDirs) -> Option<PathBuf>) -> Self {
        let dirs = TestDirs::new();
        let mut env = UserEnv {
            home: Some(dirs.home.clone()),
            vars: BTreeMap::new(),
        };
        if let Some(dir) = config_dir(&dirs) {
            env.vars
                .insert("CLAUDE_CONFIG_DIR".into(), dir.into_os_string());
        }
        let agent_config = Arc::new(AgentConfigFolders::of(&backends(
            &env,
            ClaudeCodeSettings::from_env(&env),
        )));
        let store = Arc::new(Mutex::new(Store::open(&dirs.data).unwrap()));
        let sessions = Arc::new(RwLock::new(Vec::new()));
        let seen = Arc::clone(&sessions);
        let roots = Arc::new(KnownRoots::new(Arc::clone(&store), move || {
            seen.read().unwrap().clone()
        }));
        let content = ContentService::new(
            Arc::clone(&store),
            Arc::clone(&roots),
            Arc::clone(&agent_config),
        );
        let files = FilesService::new(roots, agent_config, |_, _| {});
        Self {
            store,
            sessions,
            content,
            files,
            dirs,
        }
    }

    /// A session is running in `folder`, which makes it a known root.
    fn session_in(&self, folder: &Path) {
        self.sessions.write().unwrap().push(shown(folder));
    }

    /// A version of `path` in `scope`'s history, as an earlier save left it.
    fn version_of(&self, scope: &Path, path: &Path, content: &str) -> u64 {
        let store = self.store.lock().unwrap();
        snapshots::insert(
            store.connection(),
            &snapshots::NewSnapshot {
                scope_path: &shown(&normalize(scope)),
                file_path: &crate::agent_config::snapshot_key(scope, path),
                content,
                content_hash: &hash_content(content),
                reason: ConfigWriteReason::Edit,
            },
        )
        .unwrap()
    }

    /// `name` as a folder discovery found, `kind` of it.
    fn scanned(&self, name: &str, kind: ProjectKind) -> PathBuf {
        let path = self.dirs.folder(name);
        let project = Project {
            path: path.display().to_string(),
            name: name.to_owned(),
            kind,
            workspace_path: None,
            has_claude_dir: false,
            inventory: EMPTY_INVENTORY,
            git: None,
        };
        let mut store = self.store.lock().unwrap();
        cache_projects(
            store.connection_mut(),
            &[project],
            &[],
            "2026-10-28T00:00:00Z",
        )
        .unwrap();
        normalize(&path)
    }

    fn add_profile(&self, root: &Path) {
        let store = self.store.lock().unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO profiles (name, root, overlays) VALUES (?1, ?2, ?3)",
                rusqlite::params!["Outside", root.display().to_string(), json!([]).to_string()],
            )
            .unwrap();
    }

    /// From here on, the store refuses every snapshot.
    fn break_snapshots(&self) {
        self.store
            .lock()
            .unwrap()
            .connection()
            .execute_batch(
                "CREATE TRIGGER no_snapshots BEFORE INSERT ON config_snapshots \
                 BEGIN SELECT RAISE(ABORT, 'the disk is full'); END;",
            )
            .unwrap();
    }

    fn snapshot_count(&self) -> i64 {
        self.store
            .lock()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM config_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn snapshot_content(&self, id: u64) -> String {
        let store = self.store.lock().unwrap();
        snapshots::read(store.connection(), id)
            .unwrap()
            .unwrap()
            .content
    }

    fn write(
        &self,
        scope: &Path,
        path: &Path,
        content: &str,
        expected: Option<String>,
    ) -> Result<WriteConfigResult, String> {
        self.content.write(&WriteConfigRequest {
            scope_path: scope.display().to_string(),
            path: path.display().to_string(),
            content: content.to_owned(),
            expected_hash: expected,
            reason: ConfigWriteReason::Edit,
        })
    }
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

// ---------------------------------------------------------------------------
// The service: scopes, reading, rendering, searching, wikilinks, artifacts.

#[test]
fn scopes_are_workspaces_then_projects_then_profile_roots_read_per_call() {
    let world = World::new();
    let project = world.scanned("a project", ProjectKind::Folder);
    let workspace = world.scanned("the workspace", ProjectKind::Workspace);
    let outside = world.dirs.folder("elsewhere tooling");
    let session = world.dirs.folder("a session folder");
    world.sessions.write().unwrap().push(shown(&session));

    let lower = |world: &World| -> Vec<String> {
        world
            .content
            .scopes()
            .unwrap()
            .iter()
            .map(|scope| scope.path.to_lowercase())
            .collect()
    };
    assert_eq!(
        lower(&world),
        [
            shown(&workspace).to_lowercase(),
            shown(&project).to_lowercase()
        ]
    );

    // Asked again, which is what Refresh does: the list is read per call.
    world.add_profile(&outside);
    let scopes = world.content.scopes().unwrap();
    assert_eq!(scopes.len(), 3);
    assert_eq!(scopes[0].kind, ConfigScopeKind::Workspace);
    assert_eq!(scopes[0].label, "the workspace");
    assert_eq!(
        scopes[2].path.to_lowercase(),
        shown(&normalize(&outside)).to_lowercase()
    );
    assert_eq!(scopes[2].label, "elsewhere tooling");

    // A session's folder is reachable without being offered.
    put(&session.join("notes").join("today.md"), "# Today\n");
    let document = world
        .content
        .document(
            &shown(&session),
            &shown(&session.join("notes").join("today.md")),
        )
        .unwrap();
    assert_eq!(document.file.title, "Today");
}

#[test]
fn a_document_reads_renders_and_resolves_its_wikilinks_against_its_scope() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    fixture(&scope);
    let alpha = scope.join("notes").join("alpha.md");

    let document = world
        .content
        .document(&shown(&scope), &shown(&alpha))
        .unwrap();
    assert_eq!(document.file.rel_path, "notes/alpha.md");
    assert_eq!(document.file.note_type.as_deref(), Some("journal"));
    assert_eq!(document.content.hash, hash_content(&text(&alpha)));
    let rendered = document.rendered.unwrap();
    assert_eq!(rendered.counts.wikilinks, 2);
    assert_eq!(rendered.counts.broken_wikilinks, 1);
    let beta = rendered
        .links
        .iter()
        .find(|link| link.target == "beta")
        .unwrap();
    assert_eq!(
        beta.resolved.as_deref().map(str::to_lowercase),
        Some(shown(&scope.join("notes").join("beta.md")).to_lowercase())
    );
    assert_eq!(document.error, None);

    // A file the walk never listed opens with its real kind.
    let script = scope.join("repos").join("other").join("build.py");
    put(&script, "print(1)\n");
    let unlisted = world
        .content
        .document(&shown(&scope), &shown(&script))
        .unwrap();
    assert_eq!(unlisted.file.kind, ContentFileKind::Source);
    assert_eq!(unlisted.file.rel_path, "repos/other/build.py");
    assert!(unlisted.rendered.is_none());

    // A file that went away says so; the source pane shows nothing.
    let gone = world
        .content
        .document(&shown(&scope), &shown(&scope.join("notes").join("gone.md")))
        .unwrap();
    assert_eq!(
        gone.error.as_deref(),
        Some("That file is not there any more.")
    );
    assert!(!gone.content.exists);

    // A draft renders against the same scope.
    let draft = world
        .content
        .render(
            &shown(&scope),
            &shown(&alpha),
            "Now [[beta]] and [[gamma]].\n",
        )
        .unwrap();
    assert_eq!(draft.counts.broken_wikilinks, 1);

    // The frame's clicks resolve beside the file they came from.
    let link = world
        .content
        .wikilink(&shown(&scope), "beta", &shown(&alpha))
        .unwrap();
    assert!(link.is_some());
    assert_eq!(
        world
            .content
            .wikilink(&shown(&scope), "nowhere", &shown(&alpha)),
        Ok(None)
    );
}

#[test]
fn a_search_is_cold_once_then_warm_and_a_save_makes_it_cold_again() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    fixture(&scope);
    let first = world.content.search(&shown(&scope), "geofenc").unwrap();
    assert!(first.cold);
    assert_eq!(first.total_matches, 1);
    assert!(
        !world
            .content
            .search(&shown(&scope), "geofenc")
            .unwrap()
            .cold
    );

    let beta = scope.join("notes").join("beta.md");
    let before = hash_content(&text(&beta));
    world
        .write(
            &scope,
            &beta,
            "# Beta\n\ngeofencing and geofences.\n",
            Some(before),
        )
        .unwrap();
    let after = world.content.search(&shown(&scope), "geofenc").unwrap();
    assert!(after.cold);
    assert_eq!(after.total_matches, 2);
}

/// main's fixture: a note, its neighbour, an artifact linking to one of them
/// and to nothing, a script beside it, and a secret one folder up.
const SECRET: &str = "TOP SECRET BYTES";

fn artifact_world() -> (World, PathBuf, PathBuf) {
    let world = World::new();
    let scope = world.scanned("alpha project", ProjectKind::Folder);
    let notes = scope.join("notes");
    put(&notes.join("first.md"), "# First\n\nSee [[second]].\n");
    put(&notes.join("second.md"), "# Second\n");
    put(
        &notes.join("lesson.html"),
        "<!doctype html><html><head><title>Lesson</title></head><body><p>Read [[second]] and [[unwritten]].</p></body></html>\n",
    );
    put(&notes.join("chart.js"), "window.chart = 1\n");
    put(&scope.join("secret.txt"), SECRET);
    (world, scope, notes)
}

#[test]
fn an_artifact_refuses_a_path_out_of_its_folder_encoded_or_not_and_leaks_no_bytes() {
    let (world, scope, notes) = artifact_world();
    let link = world
        .content
        .artifact(&shown(&scope), &shown(&notes.join("lesson.html")))
        .unwrap();
    assert!(
        link.url
            .starts_with(&format!("floti-artifact://{}/", link.token))
    );
    assert!(link.url.ends_with("/lesson.html"));
    let base = &link.url[..link.url.rfind('/').unwrap()];

    for escape in [
        "..%2fsecret.txt",
        "../secret.txt",
        "..%5csecret.txt",
        "%2e%2e/secret.txt",
    ] {
        let answer = world.content.serve_artifact(&format!("{base}/{escape}"));
        assert_ne!(answer.status, 200, "{escape}");
        assert!(
            !String::from_utf8_lossy(&answer.body).contains(SECRET),
            "{escape} leaked"
        );
    }
    assert_eq!(
        world
            .content
            .serve_artifact(&format!("{base}/..%2fsecret.txt"))
            .status,
        403
    );
    // A token the host never minted addresses nothing.
    assert_eq!(
        world
            .content
            .serve_artifact("floti-artifact://0123456789abcdef/lesson.html")
            .status,
        404
    );
    assert_eq!(
        world
            .content
            .serve_artifact("https://x.test/lesson.html")
            .status,
        400
    );

    // Only a folder the host knows can be pinned.
    let stranger = world.dirs.folder("stranger");
    put(&stranger.join("page.html"), "<p>x</p>");
    let refused = world
        .content
        .artifact(&shown(&stranger), &shown(&stranger.join("page.html")))
        .unwrap_err();
    assert!(refused.contains("is not a folder Floti knows"), "{refused}");
}

#[test]
fn an_artifacts_neighbour_is_served_under_a_policy_that_names_no_network() {
    let (world, scope, notes) = artifact_world();
    let link = world
        .content
        .artifact(&shown(&scope), &shown(&notes.join("lesson.html")))
        .unwrap();
    let base = &link.url[..link.url.rfind('/').unwrap()];
    let sibling = world.content.serve_artifact(&format!("{base}/chart.js"));
    assert_eq!(sibling.status, 200);
    assert_eq!(sibling.body, std::fs::read(notes.join("chart.js")).unwrap());
    assert_eq!(
        sibling.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(sibling.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(sibling.header("cache-control"), Some("no-store"));
    let csp = sibling.header("content-security-policy").unwrap();
    let directives: Vec<&str> = csp.split(';').map(str::trim).collect();
    assert!(directives.contains(&"default-src 'none'"));
    assert!(directives.contains(&"connect-src 'none'"));
    assert!(!csp.contains("http:") && !csp.contains("https:"), "{csp}");
}

#[test]
fn an_artifact_is_told_which_wikilinks_resolve_and_nothing_about_where() {
    let (world, scope, notes) = artifact_world();
    let link = world
        .content
        .artifact(&shown(&scope), &shown(&notes.join("lesson.html")))
        .unwrap();
    let entry = world.content.serve_artifact(&link.url);
    assert_eq!(entry.status, 200);
    assert_eq!(
        entry.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    let html = String::from_utf8(entry.body).unwrap();

    let open = "<script type=\"application/json\" data-helm-wikilinks>";
    let start = html.find(open).unwrap() + open.len();
    let end = start + html[start..].find("</script>").unwrap();
    let island: Value = serde_json::from_str(&html[start..end]).unwrap();
    assert_eq!(island, json!({ "second": true, "unwritten": false }));
    assert!(html.find(open).unwrap() < html.rfind("</body>").unwrap());

    let lower = html.to_lowercase();
    for path in [
        world.dirs.root().to_path_buf(),
        scope.clone(),
        notes.clone(),
    ] {
        let path = shown(&path);
        let escaped = serde_json::to_string(&path).unwrap();
        for spelling in [
            path.clone(),
            path.replace('\\', "/"),
            escaped[1..escaped.len() - 1].to_owned(),
        ] {
            assert!(
                !lower.contains(&spelling.to_lowercase()),
                "{spelling} leaked"
            );
        }
    }

    // A neighbour is served verbatim, with no bootstrap.
    let base = &link.url[..link.url.rfind('/').unwrap()];
    let script = world.content.serve_artifact(&format!("{base}/chart.js"));
    assert!(!String::from_utf8_lossy(&script.body).contains("data-helm-wikilinks"));
}

// ---------------------------------------------------------------------------
// Writing a note: snapshot first, conflicts, aborts, history and restore.

#[test]
fn a_save_takes_a_snapshot_of_the_old_bytes_before_writing_the_new() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let note = scope.join("notes").join("today.md");
    put(&note, "# Before\n");

    let saved = world
        .write(&scope, &note, "# After\n", Some(hash_content("# Before\n")))
        .unwrap();
    assert!(saved.ok);
    assert_eq!(text(&note), "# After\n");
    assert_eq!(saved.file.hash, hash_content("# After\n"));
    let id = saved.snapshot_id.unwrap();
    assert_eq!(world.snapshot_content(id), "# Before\n");

    // Saving the same bytes again writes nothing and takes no snapshot.
    let again = world
        .write(&scope, &note, "# After\n", Some(hash_content("# After\n")))
        .unwrap();
    assert!(again.unchanged);
    assert_eq!(world.snapshot_count(), 1);
}

#[test]
fn a_save_over_a_file_that_changed_outside_is_a_conflict_and_writes_nothing() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let note = scope.join("notes").join("today.md");
    put(&note, "# Opened\n");
    let opened = hash_content("# Opened\n");
    put(&note, "# Changed by an agent\n");

    let answer = world
        .write(&scope, &note, "# Mine\n", Some(opened))
        .unwrap();
    assert!(!answer.ok);
    let conflict = answer.conflict.unwrap();
    assert_eq!(conflict.on_disk_content, "# Changed by an agent\n");
    assert_eq!(
        conflict.on_disk_hash,
        hash_content("# Changed by an agent\n")
    );
    assert_eq!(text(&note), "# Changed by an agent\n");
    assert_eq!(world.snapshot_count(), 0);

    // No expected hash means "this file is new", so an existing one conflicts.
    let fresh = world.write(&scope, &note, "# Mine\n", None).unwrap();
    assert!(fresh.conflict.is_some());
}

#[test]
fn a_save_whose_snapshot_cannot_be_taken_is_aborted_and_the_file_left_alone() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let note = scope.join("notes").join("today.md");
    put(&note, "# Before\n");
    world.break_snapshots();

    let refused = world
        .write(&scope, &note, "# After\n", Some(hash_content("# Before\n")))
        .unwrap_err();
    assert!(refused.contains("could not take a snapshot"), "{refused}");
    assert!(refused.contains("the disk is full"), "{refused}");
    assert_eq!(text(&note), "# Before\n");

    let created = scope.join("notes").join("new.md");
    assert!(world.write(&scope, &created, "# New\n", None).is_err());
    assert!(!created.exists());
}

#[test]
fn a_notes_snapshots_list_newest_first_and_a_restore_snapshots_the_current_bytes_first() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let note = scope.join("notes").join("today.md");
    put(&note, "one\n");
    world
        .write(&scope, &note, "two\n", Some(hash_content("one\n")))
        .unwrap();
    world
        .write(&scope, &note, "three\n", Some(hash_content("two\n")))
        .unwrap();

    let versions = world
        .content
        .snapshots(&shown(&scope), &shown(&note))
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].file_path, "notes/today.md");
    assert_eq!(
        versions[0].scope_path.to_lowercase(),
        shown(&scope).to_lowercase()
    );
    assert_eq!(world.snapshot_content(versions[0].id), "two\n");
    assert_eq!(world.snapshot_content(versions[1].id), "one\n");

    let oldest = versions[1].id;
    let restored = world.content.restore(oldest, &shown(&note)).unwrap();
    assert!(restored.ok);
    assert_eq!(text(&note), "one\n");
    let taken = restored.snapshot_id.unwrap();
    assert_eq!(world.snapshot_content(taken), "three\n");

    let after = world
        .content
        .snapshots(&shown(&scope), &shown(&note))
        .unwrap();
    assert_eq!(after.len(), 3);
    assert_eq!(after[0].id, taken);
    assert_eq!(after[0].reason, ConfigWriteReason::Restore);

    // The next read sees the restored bytes, not a cached tree's.
    let document = world
        .content
        .document(&shown(&scope), &shown(&note))
        .unwrap();
    assert_eq!(document.content.content, "one\n");
}

#[test]
fn a_restore_whose_scope_is_no_longer_known_is_refused() {
    let world = World::new();
    let session = world.dirs.folder("a session folder");
    world.sessions.write().unwrap().push(shown(&session));
    let note = session.join("notes").join("today.md");
    put(&note, "one\n");
    let saved = world
        .write(&session, &note, "two\n", Some(hash_content("one\n")))
        .unwrap();
    world.sessions.write().unwrap().clear();

    let refused = world
        .content
        .restore(saved.snapshot_id.unwrap(), &shown(&note))
        .unwrap_err();
    assert!(refused.contains("is not a folder Floti knows"), "{refused}");
    assert_eq!(text(&note), "two\n");
}

// ---------------------------------------------------------------------------
// Refusals: what a save may never touch.

/// Claude Code's config in `config_dir`: a skill and an instructions file.
fn agent_config_in(config_dir: &Path) -> [(PathBuf, &'static str); 2] {
    let files = [
        (
            config_dir.join("skills").join("x").join("SKILL.md"),
            "---\nname: x\n---\n# The skill\n",
        ),
        (config_dir.join("CLAUDE.md"), "# Mine\n"),
    ];
    for (path, content) in &files {
        put(path, content);
    }
    files
}

/// Every write and restore of `files` in `scope` is refused, saying Claude
/// Code edits its own config with its own commands, and each file is as it
/// was, with no version taken of it.
fn refused_and_unchanged(world: &World, scope: &Path, files: &[(PathBuf, &str)]) {
    for (path, content) in files {
        let version = world.version_of(scope, path, "# An older text\n");
        let before = world.snapshot_count();
        let refused = world
            .write(scope, path, "# Rewritten\n", Some(hash_content(content)))
            .unwrap_err();
        assert!(
            refused.contains("Claude Code's own config, which Claude Code edits itself")
                && refused.contains("/config, /mcp, /hooks and /agents"),
            "{refused}"
        );
        let refused = world.content.restore(version, &shown(path)).unwrap_err();
        assert!(refused.contains("Claude Code edits itself"), "{refused}");
        assert_eq!(text(path), *content, "{} is unchanged", path.display());
        assert_eq!(world.snapshot_count(), before, "no version was taken");

        // The Files view shows it read-only, saying why.
        let view = world.files.read(&shown(scope), &shown(path)).unwrap();
        assert_eq!(view.content, *content);
        let why = view.read_only.expect("an agent's config is read-only");
        assert!(why.contains("Claude Code edits itself"), "{why}");
    }
}

#[test]
fn a_save_or_restore_into_the_agents_config_folder_under_a_sessions_folder_is_refused() {
    let world = World::new();
    // A session in the fake home makes the home a known root, `.claude` in it.
    let home = normalize(&world.dirs.home);
    world.session_in(&home);
    let files = agent_config_in(&home.join(".claude"));
    refused_and_unchanged(&world, &home, &files);

    // Its global config file too, and a session started inside `.claude`.
    let global = home.join(".claude.json");
    put(&global, "{}\n");
    refused_and_unchanged(&world, &home, &[(global, "{}\n")]);
    let inside = home.join(".claude");
    world.session_in(&inside);
    refused_and_unchanged(&world, &inside, &files);

    // A note beside them is the Files view's to save, and not read-only.
    let note = home.join("notes").join("today.md");
    put(&note, "# Today\n");
    let saved = world
        .write(
            &home,
            &note,
            "# Today, again\n",
            Some(hash_content("# Today\n")),
        )
        .unwrap();
    assert!(saved.ok);
    let view = world.files.read(&shown(&home), &shown(&note)).unwrap();
    assert_eq!(view.read_only, None);
}

#[test]
fn a_save_or_restore_into_a_claude_config_dir_inside_a_known_root_is_refused() {
    let world = World::with_config_dir(|dirs| Some(dirs.folder("work").join("claude config")));
    let work = world.scanned("work", ProjectKind::Folder);
    let files = agent_config_in(&work.join("claude config"));
    refused_and_unchanged(&world, &work, &files);
    // With `CLAUDE_CONFIG_DIR` set, the global config is in that folder.
    let global = work.join("claude config").join(".claude.json");
    put(&global, "{}\n");
    refused_and_unchanged(&world, &work, &[(global, "{}\n")]);
}

#[test]
fn a_save_or_restore_through_a_link_in_a_known_root_into_the_agents_config_is_refused() {
    use crate::platform::links::{FolderLinks, Native};

    let world = World::new();
    let work = world.scanned("work", ProjectKind::Folder);
    let home = normalize(&world.dirs.home);
    let [(skill, skill_text), _] = agent_config_in(&home.join(".claude"));
    // A junction on Windows, a symlink elsewhere: the known root holds a
    // link to the fake home's skills, spelled nowhere near `.claude`.
    let link = work.join("skills-link");
    link_folder(&home.join(".claude").join("skills"), &link);
    let through = link.join("x").join("SKILL.md");
    assert_eq!(text(&through), skill_text, "the link leads to the skill");

    let version = world.version_of(&work, &through, "# An older text\n");
    let before = world.snapshot_count();
    let refused = world
        .write(
            &work,
            &through,
            "# Rewritten\n",
            Some(hash_content(skill_text)),
        )
        .unwrap_err();
    assert!(
        refused.contains("Claude Code's own config, which Claude Code edits itself"),
        "{refused}"
    );
    let refused = world
        .content
        .restore(version, &shown(&through))
        .unwrap_err();
    assert!(refused.contains("Claude Code edits itself"), "{refused}");
    // A new file through the link would land in the config too.
    let new = link.join("new").join("SKILL.md");
    let refused = world.write(&work, &new, "# New\n", None).unwrap_err();
    assert!(refused.contains("Claude Code edits itself"), "{refused}");
    assert_eq!(text(&skill), skill_text, "the skill is unchanged");
    assert!(!home.join(".claude").join("skills").join("new").exists());
    assert_eq!(world.snapshot_count(), before, "no version was taken");

    // The Files view says it is read-only, wherever it reads it from.
    let view = world.files.read(&shown(&work), &shown(&through)).unwrap();
    let why = view
        .read_only
        .expect("a file through the link is read-only");
    assert!(why.contains("Claude Code edits itself"), "{why}");

    // A note elsewhere in the root is still the Files view's to save.
    let note = work.join("notes").join("today.md");
    put(&note, "# Today\n");
    let saved = world
        .write(
            &work,
            &note,
            "# Today, again\n",
            Some(hash_content("# Today\n")),
        )
        .unwrap();
    assert!(saved.ok);
    assert_eq!(text(&note), "# Today, again\n");
    let view = world.files.read(&shown(&work), &shown(&note)).unwrap();
    assert_eq!(view.read_only, None);

    Native.unlink_folder(&link).unwrap();
}

#[test]
fn a_new_note_saved_through_a_dangling_link_into_the_agents_config_is_refused_and_nothing_made() {
    let world = World::new();
    let work = world.scanned("work", ProjectKind::Folder);
    let home = normalize(&world.dirs.home);
    let config = home.join(".claude");
    std::fs::create_dir_all(&config).unwrap();
    // A file symlink (developer mode or elevation on Windows; this fails
    // rather than skips without them) to a config file not made yet.
    let target = config.join("CLAUDE.md");
    std::fs::create_dir_all(work.join("notes")).unwrap();
    let note = work.join("notes").join("x.md");
    link_file(&target, &note);

    let before = world.snapshot_count();
    let refused = world.write(&work, &note, "# Mine\n", None).unwrap_err();
    assert!(
        refused.contains("Claude Code's own config, which Claude Code edits itself"),
        "{refused}"
    );
    assert!(!target.exists(), "nothing was made in the agent's config");
    assert_eq!(world.snapshot_count(), before, "no version was taken");
    let view = world.files.read(&shown(&work), &shown(&note)).unwrap();
    let why = view
        .read_only
        .expect("a note through the link is read-only");
    assert!(why.contains("Claude Code edits itself"), "{why}");

    std::fs::remove_file(&note).unwrap();
}

#[test]
fn a_projects_own_claude_folder_is_not_an_agents_config_and_its_skill_saves() {
    let world = World::new();
    let project = world.scanned("project", ProjectKind::Folder);
    let skill = project
        .join(".claude")
        .join("skills")
        .join("x")
        .join("SKILL.md");
    put(&skill, "# The skill\n");
    let saved = world
        .write(
            &project,
            &skill,
            "# The skill, edited\n",
            Some(hash_content("# The skill\n")),
        )
        .unwrap();
    assert!(saved.ok, "{saved:?}");
    assert_eq!(text(&skill), "# The skill, edited\n");
    let view = world.files.read(&shown(&project), &shown(&skill)).unwrap();
    assert_eq!(view.read_only, None);
}

#[test]
fn a_save_is_refused_for_a_relative_path() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let refused = world
        .write(&scope, Path::new("notes/today.md"), "x", None)
        .unwrap_err();
    assert!(
        refused.starts_with("Refusing to write a relative path"),
        "{refused}"
    );
}

#[test]
fn a_save_is_refused_outside_the_scope() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let other = world.scanned("another project", ProjectKind::Folder);
    for path in [
        other.join("notes").join("a.md"),
        scope.join("..").join("another project").join("a.md"),
        scope.clone(),
    ] {
        let refused = world.write(&scope, &path, "x", None).unwrap_err();
        assert!(refused.contains("is not inside"), "{refused}");
        assert!(!path.join("a.md").exists());
    }
    assert!(!other.join("notes").join("a.md").exists());
}

#[test]
fn a_save_is_refused_under_repos_node_modules_git_or_build() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    for folder in ["repos", "node_modules", ".git", "build", "Repos"] {
        let path = scope.join(folder).join("inner").join("a.md");
        let refused = world.write(&scope, &path, "x", None).unwrap_err();
        assert!(
            refused.ends_with(&format!("{folder} is not content.")),
            "{folder}: {refused}"
        );
        assert!(!path.exists());
    }
    assert_eq!(world.snapshot_count(), 0);
}

#[test]
fn a_save_is_refused_for_a_binary_file() {
    let world = World::new();
    let scope = world.scanned("my workspace", ProjectKind::Workspace);
    let image = scope.join("notes").join("chart.png");
    put(&image, "bytes");
    let refused = world
        .write(&scope, &image, "x", Some(hash_content("bytes")))
        .unwrap_err();
    assert!(
        refused.ends_with("the content viewer edits text, not binary files."),
        "{refused}"
    );
    assert_eq!(text(&image), "bytes");
}

#[test]
fn a_save_to_a_scope_that_is_neither_a_content_scope_nor_a_known_root_is_refused() {
    let world = World::new();
    world.scanned("my workspace", ProjectKind::Workspace);
    let stranger = world.dirs.folder("stranger");
    let note = stranger.join("note.md");
    put(&note, "mine\n");
    let refused = world
        .write(
            &stranger,
            &note,
            "overwritten\n",
            Some(hash_content("mine\n")),
        )
        .unwrap_err();
    assert_eq!(
        refused,
        format!(
            "{} is not a folder Floti knows, so it will not list or read it.",
            shown(&stranger)
        )
    );
    assert_eq!(text(&note), "mine\n");
    assert!(
        world
            .content
            .snapshots(&shown(&stranger), &shown(&note))
            .is_err()
    );
    assert!(world.content.search(&shown(&stranger), "mine").is_err());
}
