//! The agents folder: `agents` in the host's config folder, one `<name>.md`
//! per agent. Read on every call and cached nowhere, so a file the user adds,
//! edits or deletes is seen by the next start or listing with no restart.
//!
//! A file is an agent when its stem is an agent name (lowercase letters,
//! digits and hyphens) and its extension is `md`. Everything else in the
//! folder is ignored: other extensions, other names and folders.
//!
//! The folder was called `roles` before agents were called agents. A host
//! that finds `roles` and no `agents` at start moves the folder whole to
//! `agents` ([`prepare_agents_folder`]), rewriting no file in it.

use std::io;
use std::path::{Path, PathBuf};

use super::parse::parse_definition;
use super::{BadDefinition, DefinitionListing, DefinitionLookup, DefinitionSource, is_agent_name};
use crate::platform::folders::Folders;

/// The folder's name in the host's config folder.
const AGENTS: &str = "agents";

/// What the folder was called before agents were called agents.
const RETIRED_ROLES: &str = "roles";

/// Where the user's agents live: `agents` in the host's config folder.
pub fn agents_folder(folders: &Folders) -> PathBuf {
    folders.config.join(AGENTS)
}

/// The agents folder a host reads, once it has been prepared at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedFolder {
    /// The folder to read agents from.
    pub dir: PathBuf,
    /// Whether it may be seeded: neither the agents folder nor the retired
    /// roles folder exists.
    pub seed: bool,
    /// What the host's log says about it, or `None` when there is nothing to
    /// say.
    pub log: Option<String>,
}

/// Prepares `agents` in `config` at host start:
///
/// - `agents` there: it is read, and a `roles` beside it is left as it is
///   and named in the log.
/// - only `roles` there: it is renamed to `agents`, moved whole with every
///   file in it as it was. A rename that fails leaves it where it is, read
///   from there for this run, and the log says why.
/// - neither: `agents` is to be seeded.
pub fn prepare_agents_folder(config: &Path) -> PreparedFolder {
    prepare_agents_folder_with(config, |from, to| std::fs::rename(from, to))
}

/// [`prepare_agents_folder`], moving the folder with `rename`.
pub(crate) fn prepare_agents_folder_with(
    config: &Path,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> PreparedFolder {
    let agents = config.join(AGENTS);
    let roles = config.join(RETIRED_ROLES);
    let exists = |path: &Path| std::fs::symlink_metadata(path).is_ok();
    match (exists(&agents), exists(&roles)) {
        (true, true) => PreparedFolder {
            log: Some(format!(
                "floti host: agents are read from {}; the roles folder {} beside it is no longer read and was left as it is",
                agents.display(),
                roles.display()
            )),
            dir: agents,
            seed: false,
        },
        (true, false) => PreparedFolder {
            dir: agents,
            seed: false,
            log: None,
        },
        (false, true) => match rename(&roles, &agents) {
            Ok(()) => PreparedFolder {
                log: Some(format!(
                    "floti host: moved the roles folder {} to {}",
                    roles.display(),
                    agents.display()
                )),
                dir: agents,
                seed: false,
            },
            Err(err) => PreparedFolder {
                log: Some(format!(
                    "floti host: the roles folder {} could not be moved to {}: {err}; agents are read from {} until it can be",
                    roles.display(),
                    agents.display(),
                    roles.display()
                )),
                dir: roles,
                seed: false,
            },
        },
        (false, false) => PreparedFolder {
            dir: agents,
            seed: true,
            log: None,
        },
    }
}

/// The agents in a folder, read fresh on every call.
#[derive(Debug, Clone)]
pub struct FolderDefinitions {
    dir: PathBuf,
}

impl FolderDefinitions {
    /// The agents in `dir`. The folder need not exist.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The agent file `name.md`, read and parsed.
    fn read(&self, name: &str) -> DefinitionLookup {
        let file = format!("{name}.md");
        let path = self.dir.join(&file);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => return DefinitionLookup::Missing,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return DefinitionLookup::Missing,
            Err(err) => return DefinitionLookup::Bad(unreadable(file, &err)),
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => return DefinitionLookup::Bad(unreadable(file, &err)),
        };
        let Ok(source) = String::from_utf8(bytes) else {
            return DefinitionLookup::Bad(BadDefinition {
                file,
                problem: "it is not UTF-8 text".to_owned(),
            });
        };
        match parse_definition(name, &source) {
            Ok(agent) => DefinitionLookup::Found(Box::new(agent)),
            Err(problem) => DefinitionLookup::Bad(BadDefinition { file, problem }),
        }
    }
}

fn unreadable(file: String, err: &io::Error) -> BadDefinition {
    BadDefinition {
        file,
        problem: format!("it could not be read: {err}"),
    }
}

impl DefinitionSource for FolderDefinitions {
    fn listing(&self) -> DefinitionListing {
        let mut names: Vec<String> = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    if path.extension()? != "md" {
                        return None;
                    }
                    let stem = path.file_stem()?.to_str()?;
                    (is_agent_name(stem) && path.is_file()).then(|| stem.to_owned())
                })
                .collect(),
            // No folder is no agents; one that cannot be read is said in the
            // host's log and is no agents either.
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => {
                log_line!(
                    "floti host: the agents folder {} could not be read: {err}",
                    self.dir.display()
                );
                Vec::new()
            }
        };
        names.sort();
        let mut listing = DefinitionListing::default();
        for name in names {
            match self.read(&name) {
                DefinitionLookup::Found(agent) => listing.agents.push(*agent),
                DefinitionLookup::Bad(bad) => listing.bad.push(bad),
                DefinitionLookup::Missing => {}
            }
        }
        listing
    }

    fn lookup(&self, name: &str) -> DefinitionLookup {
        let name = name.trim().to_ascii_lowercase();
        if is_agent_name(&name) {
            self.read(&name)
        } else {
            DefinitionLookup::Missing
        }
    }
}
