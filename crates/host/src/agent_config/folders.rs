//! Where each agent on the host keeps its config, which Floti never writes.
//!
//! Taken from the backends ([`ConfigSource::agent_config`]), so another
//! backend's folder is covered as Claude Code's is: its config folder,
//! resolved as its launches resolve it (`CLAUDE_CONFIG_DIR` for Claude
//! Code), and its global config file. A path at or under either is the
//! agent's own, and the agent's own commands edit it.
//!
//! A path is judged both as it is spelled and where it leads. A folder Floti
//! knows can hold a junction or symlink into an agent's config (a link to
//! `~/.claude/skills` is a common setup), and a save through it lands in the
//! config however it is spelled, so the path and each config path are
//! resolved as the disk resolves them ([`real_path`], the resolution the
//! Files view's known-root check uses) and compared again. A link whose
//! target is not there yet does not resolve, yet a save through it creates
//! the target, so such a link is followed to where it points ([`leads_to`]).
//! The spelled comparison stays, so a path that cannot be resolved is still
//! the agent's when it names its config.
//!
//! [`ConfigSource::agent_config`]: crate::agent::ConfigSource::agent_config

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::files::normalize;
use crate::agent::AgentBackend;
use crate::platform::links::is_link;
use crate::workspace::content::guard::segments_below;
use crate::workspace::files::paths::real_path;

/// How many links that do not resolve a path is followed through before it
/// is judged where it has got to, as an OS gives up on a loop.
const MAX_LINK_HOPS: usize = 40;

/// Where a write to `path` lands: the path as the disk resolves it
/// ([`real_path`]), with each link on the way whose target is not there yet
/// (a dangling file or folder link) replaced by that target, so a save that
/// would create the target through the link is judged where it creates it.
/// A relative target is taken from the link's own folder, resolved. A loop
/// stops after [`MAX_LINK_HOPS`] links, at wherever it has got to.
fn leads_to(path: &Path) -> PathBuf {
    let mut path = normalize(path);
    if let Ok(real) = std::fs::canonicalize(&path) {
        // It resolves, so no link on its way dangles.
        return real;
    }
    for _ in 0..MAX_LINK_HOPS {
        match past_dangling_link(&path) {
            Some(followed) => path = followed,
            None => break,
        }
    }
    real_path(&path)
}

/// `path` with its deepest link that does not resolve replaced by the link's
/// target, or `None` when no link on its way dangles.
fn past_dangling_link(path: &Path) -> Option<PathBuf> {
    let link = path
        .ancestors()
        .find(|at| is_link(at) && std::fs::canonicalize(at).is_err())?;
    let target = std::fs::read_link(link).ok()?;
    let rest = path.strip_prefix(link).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        real_path(link.parent()?).join(target)
    };
    Some(normalize(&target.join(rest)))
}

/// One agent's config: the paths that hold it and who edits them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentFolder {
    paths: Vec<PathBuf>,
    agent: String,
    commands: Vec<String>,
}

impl AgentFolder {
    /// Whether `path` as spelled, or `real` (where it leads), is at or under
    /// one of this agent's config paths, spelled or resolved likewise. Each
    /// config path is resolved per call: the folder can be made, moved or
    /// relinked while the host runs.
    fn holds(&self, path: &Path, real: &Path) -> bool {
        self.paths.iter().any(|held| {
            segments_below(held, path).is_some() || segments_below(&leads_to(held), real).is_some()
        })
    }

    /// The sentence saying who edits it: the agent itself, with its own
    /// commands where it has some.
    fn edited_by(&self) -> String {
        match self.commands.as_slice() {
            [] => format!(
                "It is {agent}'s own config, which {agent} edits itself.",
                agent = self.agent
            ),
            commands => format!(
                "It is {agent}'s own config, which {agent} edits itself: {}, or ask the session.",
                spoken_list(commands),
                agent = self.agent
            ),
        }
    }
}

/// `a`, `a and b`, `a, b and c`.
fn spoken_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Every agent's config on the host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentConfigFolders {
    agents: Vec<AgentFolder>,
}

impl AgentConfigFolders {
    /// The config of every backend that says where it keeps one.
    pub fn of(backends: &[Arc<dyn AgentBackend>]) -> Self {
        let agents = backends
            .iter()
            .filter_map(|backend| {
                let config = backend.config()?.agent_config()?;
                Some(AgentFolder {
                    paths: vec![
                        normalize(&config.config_dir),
                        normalize(&config.global_config),
                    ],
                    agent: backend.name().to_owned(),
                    commands: config.own_commands,
                })
            })
            .collect();
        Self { agents }
    }

    /// Why `path` is not Floti's to write - it is at or under an agent's
    /// config, as spelled or where its links lead - or `None` when no agent's
    /// config holds it.
    pub fn holding(&self, path: &Path) -> Option<String> {
        if self.agents.is_empty() {
            return None;
        }
        let path = normalize(path);
        let real = leads_to(&path);
        self.agents
            .iter()
            .find(|agent| agent.holds(&path, &real))
            .map(AgentFolder::edited_by)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(root: &Path) -> AgentConfigFolders {
        AgentConfigFolders {
            agents: vec![AgentFolder {
                paths: vec![root.join(".claude"), root.join(".claude.json")],
                agent: "Claude Code".to_owned(),
                commands: ["/config", "/mcp", "/hooks", "/agents"]
                    .map(str::to_owned)
                    .to_vec(),
            }],
        }
    }

    #[test]
    fn a_path_at_or_under_an_agents_config_is_its_own_and_says_who_edits_it() {
        let home = std::env::temp_dir().join("a fake home");
        let folders = folders(&home);
        for held in [
            home.join(".claude"),
            home.join(".claude")
                .join("skills")
                .join("x")
                .join("SKILL.md"),
            home.join(".CLAUDE").join("CLAUDE.md"),
            home.join(".claude.json"),
        ] {
            let said = folders
                .holding(&held)
                .unwrap_or_else(|| panic!("{} is the agent's", held.display()));
            assert_eq!(
                said,
                "It is Claude Code's own config, which Claude Code edits itself: \
                 /config, /mcp, /hooks and /agents, or ask the session."
            );
        }
        for not in [
            home.join("notes").join("a.md"),
            home.join(".claude-notes").join("a.md"),
            home.join(".claude.json.bak"),
            home.clone(),
        ] {
            assert_eq!(folders.holding(&not), None, "{}", not.display());
        }
    }

    #[test]
    fn a_path_through_a_link_into_an_agents_config_is_its_own_wherever_it_is_spelled() {
        use crate::platform::links::{FolderLinks, Native};
        use crate::testkit::{TestDirs, link_folder};

        let dirs = TestDirs::new();
        let held = folders(&dirs.home);
        let skills = dirs.home.join(".claude").join("skills");
        std::fs::create_dir_all(skills.join("x")).unwrap();
        std::fs::write(skills.join("x").join("SKILL.md"), "# x\n").unwrap();
        let project = dirs.folder("a project");
        let link = project.join("skills link");
        link_folder(&skills, &link);

        // A file there and one not made yet both land in the config.
        for through in [
            link.join("x").join("SKILL.md"),
            link.join("new").join("SKILL.md"),
            link.clone(),
        ] {
            assert!(
                held.holding(&through).is_some(),
                "{} leads into the agent's config",
                through.display()
            );
        }
        assert_eq!(
            held.holding(&project.join("notes").join("a.md")),
            None,
            "a note beside the link is not the agent's"
        );

        // A config folder that is itself a link holds where it leads too.
        let real = dirs.folder("real config");
        let other_home = dirs.folder("other home");
        link_folder(&real, &other_home.join(".claude"));
        let linked = folders(&other_home);
        assert!(linked.holding(&real.join("CLAUDE.md")).is_some());
        assert_eq!(linked.holding(&dirs.folder("elsewhere").join("a.md")), None);

        Native.unlink_folder(&link).unwrap();
        Native.unlink_folder(&other_home.join(".claude")).unwrap();
    }

    #[test]
    fn a_link_whose_target_is_not_there_yet_is_judged_where_a_save_would_create_it() {
        use crate::platform::links::{Native, remove_tree};
        use crate::testkit::{TestDirs, link_file, link_folder};

        let dirs = TestDirs::new();
        let held = folders(&dirs.home);
        let config = dirs.home.join(".claude");
        std::fs::create_dir_all(&config).unwrap();
        let project = dirs.folder("a project");
        let notes = dirs.folder("a project/notes");

        // A file link to a config file not made yet, absolute and relative.
        link_file(&config.join("CLAUDE.md"), &notes.join("x.md"));
        let relative = Path::new("..")
            .join("..")
            .join("home dir")
            .join(".claude")
            .join("new.md");
        link_file(&relative, &notes.join("relative.md"));

        // A folder link whose target folder is gone.
        let gone = config.join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        link_folder(&gone, &project.join("gone link"));
        std::fs::remove_dir(&gone).unwrap();

        // A file link through that dangling folder link: two hops.
        link_file(&project.join("gone link").join("y.md"), &notes.join("y.md"));

        for dangling in [
            notes.join("x.md"),
            notes.join("relative.md"),
            project.join("gone link").join("x.md"),
            notes.join("y.md"),
        ] {
            assert!(
                std::fs::canonicalize(&dangling).is_err(),
                "{} does not resolve",
                dangling.display()
            );
            assert!(
                held.holding(&dangling).is_some(),
                "{} leads into the agent's config",
                dangling.display()
            );
        }

        // A dangling link elsewhere is not the agent's.
        link_file(&dirs.root().join("elsewhere.md"), &notes.join("z.md"));
        assert_eq!(held.holding(&notes.join("z.md")), None);

        // A link to itself stops after the hop cap, outside the config.
        link_file(&notes.join("loop.md"), &notes.join("loop.md"));
        assert_eq!(held.holding(&notes.join("loop.md")), None);

        remove_tree(&Native, &project).unwrap();
    }

    #[test]
    fn an_agent_without_commands_is_still_said_to_edit_its_own_config() {
        let folders = AgentConfigFolders {
            agents: vec![AgentFolder {
                paths: vec![PathBuf::from("/x/.agent")],
                agent: "Other".to_owned(),
                commands: Vec::new(),
            }],
        };
        assert_eq!(
            folders.holding(Path::new("/x/.agent/a.md")).as_deref(),
            Some("It is Other's own config, which Other edits itself.")
        );
        assert_eq!(AgentConfigFolders::default().holding(Path::new("/x")), None);
    }
}
