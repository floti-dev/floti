//! Where each agent on the host keeps its config, which Floti never writes.
//!
//! Taken from the backends ([`ConfigSource::agent_config`]), so another
//! backend's folder is covered as Claude Code's is: its config folder,
//! resolved as its launches resolve it (`CLAUDE_CONFIG_DIR` for Claude
//! Code), and its global config file. A path at or under either is the
//! agent's own, and the agent's own commands edit it.
//!
//! [`ConfigSource::agent_config`]: crate::agent::ConfigSource::agent_config

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::files::normalize;
use crate::agent::AgentBackend;
use crate::workspace::content::guard::segments_below;

/// One agent's config: the paths that hold it and who edits them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentFolder {
    paths: Vec<PathBuf>,
    agent: String,
    commands: Vec<String>,
}

impl AgentFolder {
    fn holds(&self, path: &Path) -> bool {
        self.paths
            .iter()
            .any(|held| segments_below(held, path).is_some())
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
    /// config - or `None` when no agent's config holds it.
    pub fn holding(&self, path: &Path) -> Option<String> {
        let path = normalize(path);
        self.agents
            .iter()
            .find(|agent| agent.holds(&path))
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
