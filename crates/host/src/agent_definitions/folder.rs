//! The agents folder: `agents` in the host's config folder, one `<name>.md`
//! per agent. Read on every call and cached nowhere, so a file the user adds,
//! edits or deletes is seen by the next start or listing with no restart.
//!
//! A file is an agent when its stem is an agent name (lowercase letters,
//! digits and hyphens) and its extension is `md`. Everything else in the
//! folder is ignored: other extensions, other names and folders.
//!
//! The folder was called `roles` before agents were called agents, and the
//! default agent was called `agent`. A host that finds `roles` and no
//! `agents` at start moves the folder whole to `agents`
//! ([`prepare_agents_folder`]), rewriting no file in it; an `agent.md` in it
//! is the default wherever there is no `default.md`.
//!
//! A host whose config folder is another host's to write
//! ([`read_agents_folder`]) moves, seeds and writes nothing there: it reads
//! `agents`, else `roles` where it is, and with neither it serves the shipped
//! agents from memory.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::parse::parse_definition;
use super::seed::SHIPPED_AGENTS;
use super::{
    AgentDefinition, BadDefinition, DEFAULT_AGENT_NAME, DefinitionListing, DefinitionLookup,
    DefinitionSource, RETIRED_DEFAULT_NAME, is_agent_name, is_default_name,
};
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
    /// roles folder exists, and the host may write its config folder.
    pub seed: bool,
    /// Whether the shipped agents are served from memory while `dir` does
    /// not exist, because the host may not write them there.
    pub shipped: bool,
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
    match (exists(&agents), exists(&roles)) {
        (true, true) => PreparedFolder {
            log: Some(format!(
                "floti host: agents are read from {}; the roles folder {} beside it is no longer read and was left as it is",
                agents.display(),
                roles.display()
            )),
            dir: agents,
            seed: false,
            shipped: false,
        },
        (true, false) => PreparedFolder {
            dir: agents,
            seed: false,
            shipped: false,
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
                shipped: false,
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
                shipped: false,
            },
        },
        (false, false) => PreparedFolder {
            dir: agents,
            seed: true,
            shipped: false,
            log: None,
        },
    }
}

/// The agents folder in `config` for a host that may not write there: a
/// development or check run of Helm, whose config folder can be the
/// installed host's. It reads `agents`, else `roles` where it is, and with
/// neither serves the shipped agents from memory. It moves, seeds and
/// writes nothing.
pub fn read_agents_folder(config: &Path) -> PreparedFolder {
    let agents = config.join(AGENTS);
    let roles = config.join(RETIRED_ROLES);
    if exists(&agents) {
        return PreparedFolder {
            dir: agents,
            seed: false,
            shipped: false,
            log: None,
        };
    }
    if exists(&roles) {
        return PreparedFolder {
            log: Some(format!(
                "floti host: agents are read from the roles folder {}, which this host leaves where it is",
                roles.display()
            )),
            dir: roles,
            seed: false,
            shipped: false,
        };
    }
    PreparedFolder {
        log: Some(format!(
            "floti host: there is no agents folder at {}; the shipped agents are served and nothing is written there",
            agents.display()
        )),
        dir: agents,
        seed: false,
        shipped: true,
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// The agents in a folder, read fresh on every call.
#[derive(Debug)]
pub struct FolderDefinitions {
    dir: PathBuf,
    /// Whether the shipped agents are served while the folder is absent.
    shipped: bool,
    /// Whether the log has said what an `agent.md` from before agents were
    /// called agents is taken as. Said once.
    said_retired_default: AtomicBool,
}

impl FolderDefinitions {
    /// The agents in `dir`. The folder need not exist.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            shipped: false,
            said_retired_default: AtomicBool::new(false),
        }
    }

    /// The agents in `dir`, and while it does not exist the shipped ones.
    pub fn with_shipped(dir: impl Into<PathBuf>) -> Self {
        Self {
            shipped: true,
            ..Self::new(dir)
        }
    }

    /// The folder for `prepared`, with the shipped agents where it says.
    pub fn prepared(prepared: PreparedFolder) -> Self {
        if prepared.shipped {
            Self::with_shipped(prepared.dir)
        } else {
            Self::new(prepared.dir)
        }
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

    /// The default agent's file: `default.md`, else an `agent.md` from
    /// before agents were called agents, read as the default. Says once in
    /// the log what an `agent.md` was taken as.
    fn read_default(&self) -> DefinitionLookup {
        match self.read(DEFAULT_AGENT_NAME) {
            DefinitionLookup::Missing => {}
            found => {
                if self
                    .dir
                    .join(format!("{RETIRED_DEFAULT_NAME}.md"))
                    .is_file()
                {
                    self.say_once(format!(
                        "floti host: {} is not read: {} is the default agent",
                        self.dir
                            .join(format!("{RETIRED_DEFAULT_NAME}.md"))
                            .display(),
                        self.dir.join(format!("{DEFAULT_AGENT_NAME}.md")).display()
                    ));
                }
                return found;
            }
        }
        match self.read(RETIRED_DEFAULT_NAME) {
            DefinitionLookup::Found(mut agent) => {
                self.say_once(format!(
                    "floti host: {} is read as the default agent, as it was before agents were called agents; renaming it {DEFAULT_AGENT_NAME}.md keeps it so",
                    self.dir.join(format!("{RETIRED_DEFAULT_NAME}.md")).display()
                ));
                agent.name = DEFAULT_AGENT_NAME.to_owned();
                DefinitionLookup::Found(agent)
            }
            other => other,
        }
    }

    fn say_once(&self, line: String) {
        if !self.said_retired_default.swap(true, Ordering::Relaxed) {
            log_line!("{line}");
        }
    }

    /// Whether the folder is absent and the shipped agents stand in for it.
    fn serves_shipped(&self) -> bool {
        self.shipped && !exists(&self.dir)
    }
}

/// The shipped agents, as a folder seeded with them would list them.
pub fn shipped_listing() -> DefinitionListing {
    let mut listing = DefinitionListing::default();
    for (file, source) in SHIPPED_AGENTS {
        let name = file.strip_suffix(".md").unwrap_or(file);
        match parse_definition(name, source) {
            Ok(agent) => listing.agents.push(agent),
            Err(problem) => listing.bad.push(BadDefinition {
                file: (*file).to_owned(),
                problem,
            }),
        }
    }
    listing.agents.sort_by(|a, b| a.name.cmp(&b.name));
    listing
}

fn unreadable(file: String, err: &io::Error) -> BadDefinition {
    BadDefinition {
        file,
        problem: format!("it could not be read: {err}"),
    }
}

impl DefinitionSource for FolderDefinitions {
    fn listing(&self) -> DefinitionListing {
        if self.serves_shipped() {
            return shipped_listing();
        }
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
        let has_default = names.iter().any(|name| is_default_name(name));
        names.retain(|name| !is_default_name(name));
        if has_default {
            push(&mut listing, self.read_default());
        }
        for name in names {
            push(&mut listing, self.read(&name));
        }
        listing
    }

    fn lookup(&self, name: &str) -> DefinitionLookup {
        if self.serves_shipped() {
            return shipped_lookup(name);
        }
        let name = name.trim().to_ascii_lowercase();
        if is_default_name(&name) {
            self.read_default()
        } else if is_agent_name(&name) {
            self.read(&name)
        } else {
            DefinitionLookup::Missing
        }
    }
}

fn push(listing: &mut DefinitionListing, lookup: DefinitionLookup) {
    match lookup {
        DefinitionLookup::Found(agent) => listing.agents.push(*agent),
        DefinitionLookup::Bad(bad) => listing.bad.push(bad),
        DefinitionLookup::Missing => {}
    }
}

/// The shipped agent `name`, as a lookup in a folder seeded with them finds
/// it.
fn shipped_lookup(name: &str) -> DefinitionLookup {
    let listing = shipped_listing();
    let name = name.trim();
    listing
        .agents
        .into_iter()
        .find(|agent: &AgentDefinition| agent.name.eq_ignore_ascii_case(name))
        .map_or(DefinitionLookup::Missing, |agent| {
            DefinitionLookup::Found(Box::new(agent))
        })
}
