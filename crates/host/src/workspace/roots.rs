//! The folders the host knows: every Files and content request names one,
//! and nothing under any other folder is listed, read, watched or written.
//!
//! A folder is known while one of these holds it, read afresh on every call
//! so it is known as soon as a scan, a profile save or a session start makes
//! it so and stops being known once that goes: a workspace or project the
//! discovery cache holds, a saved profile's root or overlay folder, or a
//! hosted session's working directory. Folders are compared as today's app
//! compares them, made absolute, `.` and `..` folded away, and ignoring
//! case; a folder's parent is not known because the folder is.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::channels::workspace::ProjectKind;
use rusqlite::Connection;

use crate::agent_config::normalize;
use crate::store::Store;
use crate::store::projects::read_cached_projects;
use crate::workspace::files::paths::is_inside_root;
use crate::{Error, Result};

/// Why a folder is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// A workspace discovery found.
    Workspace,
    /// A project discovery found that is not a workspace.
    Project,
    /// A saved profile's root or one of its overlay folders.
    Profile,
    /// A hosted session's working directory.
    Session,
}

/// One known folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRoot {
    /// The folder as its source spells it, made absolute and folded.
    pub path: PathBuf,
    pub kind: RootKind,
    /// The name discovery gave it, for a workspace or project.
    pub name: Option<String>,
}

type SessionFolders = Box<dyn Fn() -> Vec<String> + Send + Sync>;

/// The registry of known folders.
pub struct KnownRoots {
    store: Arc<Mutex<Store>>,
    sessions: SessionFolders,
}

impl KnownRoots {
    /// Folders known from `store` (the discovery cache and the profiles) and
    /// from `sessions`, which answers every hosted session's working
    /// directory now.
    pub fn new(
        store: Arc<Mutex<Store>>,
        sessions: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            store,
            sessions: Box::new(sessions),
        }
    }

    /// Every known folder, each once: workspaces first, then projects, then
    /// each profile's root and overlays, then the sessions' folders.
    pub fn all(&self) -> Result<Vec<KnownRoot>> {
        let mut out: Vec<KnownRoot> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |path: &str, kind: RootKind, name: Option<&str>| {
            if path.trim().is_empty() {
                return;
            }
            let path = normalize(Path::new(path));
            if seen.insert(key(&path)) {
                out.push(KnownRoot {
                    path,
                    kind,
                    name: name.map(str::to_owned),
                });
            }
        };
        let (projects, profiles) = {
            let store = lock(&self.store);
            (
                read_cached_projects(store.connection())?,
                profile_folders(store.connection())?,
            )
        };
        for project in projects.iter().filter(|p| p.kind == ProjectKind::Workspace) {
            add(&project.path, RootKind::Workspace, Some(&project.name));
        }
        for project in projects.iter().filter(|p| p.kind != ProjectKind::Workspace) {
            add(&project.path, RootKind::Project, Some(&project.name));
        }
        for folder in &profiles {
            add(folder, RootKind::Profile, None);
        }
        for folder in (self.sessions)() {
            add(&folder, RootKind::Session, None);
        }
        Ok(out)
    }

    /// The folders Floti tracks: every known folder but a hosted session's
    /// working directory. Running somewhere does not make a folder tracked,
    /// so a session cannot widen what an agent it starts may reach by being
    /// started elsewhere.
    pub fn tracked(&self) -> Result<Vec<KnownRoot>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|root| root.kind != RootKind::Session)
            .collect())
    }

    /// The known folder `root` names, as its source spells it, or a sentence
    /// naming the folder when the host does not know it.
    pub fn known(&self, root: &str) -> std::result::Result<KnownRoot, String> {
        let wanted = key(&normalize(Path::new(root)));
        let all = self.all().map_err(|err| err.to_string())?;
        all.into_iter()
            .find(|candidate| key(&candidate.path) == wanted)
            .ok_or_else(|| refusal(root))
    }

    /// The deepest known folder holding `path`, the folder itself included,
    /// judged as the files service judges a path inside a root: before and
    /// after links are resolved, so a link out of a known folder is outside
    /// it. Or the sentence naming the rule.
    pub fn holding(&self, path: &Path) -> std::result::Result<KnownRoot, String> {
        self.deepest(path, |_| true)?
            .ok_or_else(|| outside_refusal(path))
    }

    /// The deepest workspace or project discovery found that holds `path`,
    /// or the sentence saying there is none.
    pub fn project_holding(&self, path: &Path) -> std::result::Result<KnownRoot, String> {
        self.deepest(path, |kind| {
            matches!(kind, RootKind::Workspace | RootKind::Project)
        })?
        .ok_or_else(|| {
            format!(
                "{} is not in a project or workspace Floti found, so there is no project page to open for it.",
                path.display()
            )
        })
    }

    /// The known folder of a kind `wanted` takes holding `path` with the most
    /// folders to its name.
    fn deepest(
        &self,
        path: &Path,
        wanted: impl Fn(RootKind) -> bool,
    ) -> std::result::Result<Option<KnownRoot>, String> {
        let path = normalize(path);
        let all = self.all().map_err(|err| err.to_string())?;
        Ok(all
            .into_iter()
            .filter(|root| wanted(root.kind) && is_inside_root(&root.path, &path))
            .max_by_key(|root| root.path.components().count()))
    }
}

/// The sentence for a folder the host does not know.
pub fn refusal(root: &str) -> String {
    format!("{root} is not a folder Floti knows, so it will not list or read it.")
}

/// The sentence for a path under no folder the host knows.
pub fn outside_refusal(path: &Path) -> String {
    format!(
        "{} is not inside a folder Floti knows (a workspace or project it found, a profile's folder, or a running session's folder), so it will not open it.",
        path.display()
    )
}

/// How two spellings of one folder compare equal: folded, then lower-cased.
pub fn key(path: &Path) -> String {
    let text = path.display().to_string();
    let trimmed = text.trim_end_matches(['/', '\\']);
    // The root itself keeps its separator.
    let text = if trimmed.is_empty() || trimmed.ends_with(':') {
        text.as_str()
    } else {
        trimmed
    };
    text.to_lowercase()
}

/// Every folder a saved profile names: its root, then its overlays.
fn profile_folders(conn: &Connection) -> Result<Vec<String>> {
    let mut select = conn
        .prepare("SELECT root, overlays FROM profiles ORDER BY id")
        .map_err(sql)?;
    let rows = select
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(sql)?;
    let mut out = Vec::new();
    for row in rows {
        let (root, overlays) = row.map_err(sql)?;
        out.push(root);
        out.extend(
            overlays
                .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
                .unwrap_or_default(),
        );
    }
    Ok(out)
}

fn sql(err: rusqlite::Error) -> Error {
    Error::io("read the folders Floti knows", std::io::Error::other(err))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::RwLock;

    use floti_protocol::channels::workspace::{Project, Workspace};

    use super::*;
    use crate::store::projects::{EMPTY_INVENTORY, cache_projects, forget_projects};
    use crate::testkit::TestDirs;

    struct World {
        store: Arc<Mutex<Store>>,
        sessions: Arc<RwLock<Vec<String>>>,
        roots: KnownRoots,
        dirs: TestDirs,
    }

    impl World {
        fn new() -> Self {
            let dirs = TestDirs::new();
            let store = Arc::new(Mutex::new(Store::open(&dirs.data).unwrap()));
            let sessions = Arc::new(RwLock::new(Vec::new()));
            let seen = Arc::clone(&sessions);
            let roots = KnownRoots::new(Arc::clone(&store), move || seen.read().unwrap().clone());
            Self {
                store,
                sessions,
                roots,
                dirs,
            }
        }

        fn scan(&self, path: &Path, kind: ProjectKind) {
            let project = Project {
                path: path.display().to_string(),
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                kind,
                workspace_path: None,
                has_claude_dir: false,
                inventory: EMPTY_INVENTORY,
                git: None,
            };
            let workspaces: Vec<Workspace> = Vec::new();
            cache_projects(
                lock(&self.store).connection_mut(),
                &[project],
                &workspaces,
                "2026-10-28T10:00:00.000Z",
            )
            .unwrap();
        }

        fn save_profile(&self, root: &Path, overlays: &[&Path]) {
            let overlays: Vec<String> = overlays.iter().map(|p| p.display().to_string()).collect();
            lock(&self.store)
                .connection()
                .execute(
                    "INSERT INTO profiles (name, root, overlays) VALUES (?1, ?2, ?3)",
                    [
                        "a profile".to_owned(),
                        root.display().to_string(),
                        serde_json::to_string(&overlays).unwrap(),
                    ],
                )
                .unwrap();
        }

        fn known(&self, path: &Path) -> std::result::Result<KnownRoot, String> {
            self.roots.known(&path.display().to_string())
        }
    }

    #[test]
    fn a_folder_is_known_once_a_scan_a_profile_or_a_session_makes_it_so() {
        let world = World::new();
        let project = world.dirs.folder("projects/alpha");
        let workspace = world.dirs.folder("work workspace");
        let profile_root = world.dirs.folder("profile root");
        let overlay = world.dirs.folder("an overlay");
        let session = world.dirs.folder("session folder");
        for folder in [&project, &workspace, &profile_root, &overlay, &session] {
            assert_eq!(
                world.known(folder),
                Err(refusal(&folder.display().to_string()))
            );
        }

        world.scan(&project, ProjectKind::Folder);
        world.scan(&workspace, ProjectKind::Workspace);
        world.save_profile(&profile_root, &[&overlay]);
        world
            .sessions
            .write()
            .unwrap()
            .push(session.display().to_string());

        assert_eq!(world.known(&project).unwrap().kind, RootKind::Project);
        assert_eq!(world.known(&workspace).unwrap().kind, RootKind::Workspace);
        assert_eq!(world.known(&profile_root).unwrap().kind, RootKind::Profile);
        assert_eq!(world.known(&overlay).unwrap().kind, RootKind::Profile);
        assert_eq!(world.known(&session).unwrap().kind, RootKind::Session);
        let kinds: Vec<RootKind> = world.roots.all().unwrap().iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RootKind::Workspace,
                RootKind::Project,
                RootKind::Profile,
                RootKind::Profile,
                RootKind::Session
            ]
        );
    }

    #[test]
    fn a_sessions_own_folder_is_known_but_not_tracked() {
        let world = World::new();
        let project = world.dirs.folder("projects/alpha");
        let session = world.dirs.folder("session folder");
        world.scan(&project, ProjectKind::Folder);
        world
            .sessions
            .write()
            .unwrap()
            .push(session.display().to_string());
        assert!(world.known(&session).is_ok());
        let tracked: Vec<PathBuf> = world
            .roots
            .tracked()
            .unwrap()
            .into_iter()
            .map(|root| root.path)
            .collect();
        assert_eq!(tracked, [project]);
    }

    #[test]
    fn a_folder_is_matched_ignoring_case_and_spelling_and_its_parent_is_not_known() {
        let world = World::new();
        let project = world.dirs.folder("projects/Alpha");
        world.scan(&project, ProjectKind::Folder);

        let spelled = format!(
            "{}{}..{}Alpha{}",
            project
                .parent()
                .unwrap()
                .join("x")
                .display()
                .to_string()
                .to_uppercase(),
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        // Case is ignored as today's app ignores it, on every OS.
        let found = world.roots.known(&spelled).unwrap();
        assert_eq!(key(&found.path), key(&normalize(&project)));
        assert_eq!(found.name.as_deref(), Some("Alpha"));

        let parent = project.parent().unwrap();
        assert_eq!(
            world.known(parent),
            Err(refusal(&parent.display().to_string()))
        );
        let child = project.join("src");
        assert!(
            world.known(&child).is_err(),
            "a folder inside a known one is not a root"
        );
    }

    #[test]
    fn a_folder_stops_being_known_once_what_made_it_known_goes() {
        let world = World::new();
        let project = world.dirs.folder("projects/beta");
        let session = world.dirs.folder("session folder");
        world.scan(&project, ProjectKind::Repo);
        world
            .sessions
            .write()
            .unwrap()
            .push(session.display().to_string());
        assert!(world.known(&project).is_ok());
        assert!(world.known(&session).is_ok());

        forget_projects(
            lock(&world.store).connection_mut(),
            &[project.display().to_string()],
        )
        .unwrap();
        world.sessions.write().unwrap().clear();
        assert!(world.known(&project).is_err());
        assert!(world.known(&session).is_err());
    }

    #[test]
    fn a_folder_two_sources_name_is_listed_once_under_the_first() {
        let world = World::new();
        let project = world.dirs.folder("shared");
        world.scan(&project, ProjectKind::Folder);
        world.save_profile(&project, &[]);
        world
            .sessions
            .write()
            .unwrap()
            .push(project.display().to_string().to_uppercase());
        let all = world.roots.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, RootKind::Project);
    }

    #[test]
    fn a_path_is_held_by_the_deepest_known_folder_it_is_inside_and_refused_outside_every_one() {
        let world = World::new();
        let workspace = world.dirs.folder("my work");
        let project = world.dirs.folder("my work/api server");
        let session = world.dirs.folder("my work/api server/sub dir");
        let elsewhere = world.dirs.folder("not known");
        world.scan(&workspace, ProjectKind::Workspace);
        world.scan(&project, ProjectKind::Repo);
        world
            .sessions
            .write()
            .unwrap()
            .push(session.display().to_string());

        let file = project.join("src").join("main.rs");
        let held = world.roots.holding(&file).unwrap();
        assert_eq!(key(&held.path), key(&normalize(&project)));
        assert_eq!(held.kind, RootKind::Project);
        // A folder is held by itself.
        assert_eq!(
            world.roots.holding(&project).unwrap().kind,
            RootKind::Project
        );
        // The deepest wins: a session's folder inside the project.
        let deep = world.roots.holding(&session.join("notes.md")).unwrap();
        assert_eq!(deep.kind, RootKind::Session);
        // But only a workspace or project has a project page.
        let page = world
            .roots
            .project_holding(&session.join("notes.md"))
            .unwrap();
        assert_eq!(key(&page.path), key(&normalize(&project)));
        // `..` folds away before the check, so it cannot climb out unseen.
        let climbing = project
            .join("..")
            .join("..")
            .join("not known")
            .join("x.txt");
        let refused = world.roots.holding(&climbing).unwrap_err();
        assert!(
            refused.contains("not inside a folder Floti knows"),
            "{refused}"
        );
        assert_eq!(
            world.roots.holding(&elsewhere.join("x.txt")),
            Err(outside_refusal(&elsewhere.join("x.txt")))
        );
        assert!(
            world
                .roots
                .project_holding(&elsewhere)
                .unwrap_err()
                .contains("no project page")
        );
    }
}
