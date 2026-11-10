//! An agent's config as composition reads it - its scopes, their trees and
//! the effective view of a launch - and the file, hash, time and snapshot
//! helpers the host's other surfaces share.
//!
//! Reads come through the backend's
//! [`ConfigSource`](crate::agent::ConfigSource), which lists a credential
//! file without opening it. Nothing here writes into an agent's config
//! folder: it is read-only to Floti. The snapshot-first writer
//! ([`SnapshotWriter`]) is the one a note's save and a template's authoring
//! go through, each with its own [`PathGuard`].

mod effective;
mod files;
mod folders;
mod json;
mod sha256;
pub mod snapshots;
pub(crate) mod time;
mod tree;
mod writer;

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::channels::workspace::{
    ConfigScope, ConfigScopeKind, ConfigTree, EffectiveView, EffectiveViewRequest,
};
use rusqlite::params;

use crate::agent::AgentBackend;
use crate::store::Store;

pub use effective::{EffectiveInput, effective_view};
pub use files::{PathGuard, hash_content, mtime_ms, normalize, read_file, snapshot_key};
pub use folders::AgentConfigFolders;

use json::Json;
pub use sha256::hex_digest as sha256_hex;
pub use tree::{config_unit, project_scope, read_tree, user_scope};
pub use writer::SnapshotWriter;

/// The answer to whatever reads an agent's config on a host none of whose
/// agents has config files Floti can read.
pub const NO_AGENT_CONFIG: &str = "No agent on this host has config files Floti can read.";

/// Where an agent keeps its config: its config folder (the user scope) and
/// its global config file (where its `local` and `user` MCP servers live),
/// and the agent's own commands that edit them, which Floti names when it
/// refuses to write there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    pub config_dir: PathBuf,
    pub global_config: PathBuf,
    pub own_commands: Vec<String>,
}

/// Names ordered as a person reads them: case folded first, then exact.
pub(crate) fn compare_names(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// One saved profile, as the reader needs it.
#[derive(Debug, Clone)]
struct Profile {
    id: u64,
    name: String,
    root: PathBuf,
    overlays: Vec<PathBuf>,
}

impl Profile {
    fn of(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let overlays: String = row.get(3)?;
        Ok(Self {
            id: row.get::<_, i64>(0)?.max(0) as u64,
            name: row.get(1)?,
            root: PathBuf::from(row.get::<_, String>(2)?),
            overlays: Json::parse(&overlays)
                .map(|value| value.strings().into_iter().map(PathBuf::from).collect())
                .unwrap_or_default(),
        })
    }
}

/// An agent's config, read for composition: the scopes a template imports
/// from, their trees, and what a launch would be given.
pub struct ConfigReader {
    backend: Arc<dyn AgentBackend>,
    agent: AgentConfig,
    store: Arc<Mutex<Store>>,
}

impl ConfigReader {
    /// The reader for `backend`, reading the saved profiles from `store`.
    pub fn new(
        backend: Arc<dyn AgentBackend>,
        agent: AgentConfig,
        store: Arc<Mutex<Store>>,
    ) -> Self {
        Self {
            backend,
            agent,
            store,
        }
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn profiles(&self) -> Result<Vec<Profile>, String> {
        let store = self.store();
        let mut statement = store
            .connection()
            .prepare("SELECT id, name, root, overlays FROM profiles ORDER BY id")
            .map_err(|err| format!("Floti could not read the saved profiles: {err}"))?;
        let rows = statement
            .query_map([], Profile::of)
            .and_then(Iterator::collect)
            .map_err(|err| format!("Floti could not read the saved profiles: {err}"))?;
        Ok(rows)
    }

    fn profile(&self, id: u64) -> Result<Option<Profile>, String> {
        let store = self.store();
        let Ok(id) = i64::try_from(id) else {
            return Ok(None);
        };
        let found = store.connection().query_row(
            "SELECT id, name, root, overlays FROM profiles WHERE id = ?1",
            params![id],
            Profile::of,
        );
        match found {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(format!("Floti could not read the saved profile: {err}")),
        }
    }

    /// The scopes a client can choose between: the user's, then every folder
    /// a saved profile runs in or composes, each once. A profile's folders
    /// decide what its sessions see whether or not they are anywhere else.
    pub fn scopes(&self) -> Result<Vec<ConfigScope>, String> {
        let user = user_scope(&self.agent.config_dir);
        let mut seen = vec![user.path.to_lowercase()];
        let mut out = vec![user];
        for profile in self.profiles()? {
            for path in std::iter::once(&profile.root).chain(&profile.overlays) {
                let scope = project_scope(path, ConfigScopeKind::Project, None);
                let key = scope.path.to_lowercase();
                if !seen.contains(&key) {
                    seen.push(key);
                    out.push(scope);
                }
            }
        }
        Ok(out)
    }

    /// The scope at `scope_path`: a listed one, or a project scope for a
    /// folder no profile names.
    fn scope_for(&self, scope_path: &str) -> Result<ConfigScope, String> {
        let wanted = normalize(Path::new(scope_path))
            .display()
            .to_string()
            .to_lowercase();
        Ok(self
            .scopes()?
            .into_iter()
            .find(|scope| scope.path.to_lowercase() == wanted)
            .unwrap_or_else(|| {
                project_scope(Path::new(scope_path), ConfigScopeKind::Project, None)
            }))
    }

    pub fn tree(&self, scope_path: &str) -> Result<ConfigTree, String> {
        Ok(read_tree(
            self.backend.as_ref(),
            &self.scope_for(scope_path)?,
        ))
    }

    /// The effective view for a saved profile, read from the store so it is
    /// that composition's and no other, or for a folder and overlays.
    pub fn effective(&self, request: &EffectiveViewRequest) -> Result<EffectiveView, String> {
        let input = match request.profile_id {
            Some(id) => {
                let profile = self
                    .profile(id)?
                    .ok_or_else(|| "That profile no longer exists.".to_owned())?;
                EffectiveInput {
                    cwd: profile.root,
                    overlays: profile.overlays,
                    profile_id: Some(profile.id),
                    profile_name: Some(profile.name),
                }
            }
            None => {
                let cwd = request.cwd.as_deref().map(str::trim).unwrap_or_default();
                if cwd.is_empty() {
                    return Err(
                        "An effective view needs either a profile or a working directory."
                            .to_owned(),
                    );
                }
                EffectiveInput {
                    cwd: PathBuf::from(cwd),
                    overlays: request.overlays.iter().map(PathBuf::from).collect(),
                    profile_id: None,
                    profile_name: None,
                }
            }
        };
        Ok(effective_view(self.backend.as_ref(), &self.agent, &input))
    }
}
