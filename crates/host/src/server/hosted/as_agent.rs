//! A session a person starts as an agent, from a launcher: a lead to work
//! through an epic with, a research lead to plan a project with.
//!
//! The agent is composed as a child's is (`children::compose`): its body as
//! its own section after the project's instructions, its model and effort for
//! the backend, its tool rules, and the profile it names unless the person
//! picked one. Nothing of it is typed into the terminal. It has no parent, so
//! it is told nothing of one, a person is at it (it is not launched
//! unattended), and it counts toward no agent limit. `can_start: no` leaves
//! floti-agents out of its launch, and `worktree: yes` starts it in a new agent
//! worktree named after the session, made as a child's is.
//!
//! A resume and a restore compose the agent again from its file as it is
//! then, and keep the permission mode the session ran with: a resume never
//! runs in a mode the person did not choose. A session the host takes back
//! from the keeper never stopped, and is composed by nothing.

use std::path::Path;

use floti_protocol::BackendId;
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::Profile;

use super::Hosted;
use super::children::agent_composition;
use crate::agent::{AgentBackend, Composition};
use crate::agent_definitions::{
    AgentDefinition, DefinitionLookup, RETIRED_DEFAULT_NAME, is_default_name,
};
use crate::server::floti_agents::FAMILY;
use crate::workspace::worktrees::{Made, Repository};

/// What a person is doing with an agent, which is what a refusal tells them
/// to do instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Starting {
    /// Starting a new session as it, from a launcher.
    Launch,
    /// Reopening a conversation as it, from History.
    Resume,
}

impl Starting {
    /// The end of a refusal: what the person can do instead.
    pub(super) fn instead(self) -> &'static str {
        match self {
            Self::Launch => "Start without the agent, or pick another; nothing was started.",
            Self::Resume => "Resume without the agent; nothing was started.",
        }
    }
}

/// What a launch as an agent adds to its plan.
pub(super) struct AsAgent {
    /// The agent's name, as the session's row records it.
    pub agent: String,
    /// The file it was read from, as the host spells it; `None` for the
    /// default the host makes from its backends and an agent served from
    /// memory.
    pub file: Option<String>,
    /// The agent's composition on the launch's backend.
    pub composition: Composition,
    /// The agent tool families its launch is given none of.
    pub families_off: Vec<&'static str>,
}

impl Hosted {
    /// The agent `name` as its file reads now, for a session a person starts
    /// or resumes as it.
    ///
    /// # Errors
    ///
    /// A sentence naming the agent when it is gone or its file cannot be
    /// loaded, ending with what the person can do instead. Nothing has
    /// started then.
    pub(super) fn person_agent(
        &self,
        name: &str,
        starting: Starting,
    ) -> Result<AgentDefinition, String> {
        self.agent_now(name)
            .map_err(|problem| format!("{problem} {}", starting.instead()))
    }

    /// Why the agent `name` cannot be composed now, as a sentence naming it:
    /// it is gone, or its file cannot be loaded. `None` when it can.
    pub(super) fn agent_problem(&self, name: &str) -> Option<String> {
        self.agent_now(name).err()
    }

    /// The agent `name` as its file reads now, or why it cannot be composed,
    /// as a sentence naming it.
    pub(super) fn agent_now(&self, name: &str) -> Result<AgentDefinition, String> {
        match self.agent_source().lookup(name) {
            DefinitionLookup::Found(agent) => Ok(*agent),
            DefinitionLookup::Bad(bad) => {
                Err(format!("{} cannot be loaded: {}.", bad.file, bad.problem))
            }
            // The default is what a start without an agent gets: there is
            // always one.
            DefinitionLookup::Missing if is_default_name(name) => Ok(self.built_in_default_agent()),
            DefinitionLookup::Missing => Err(format!(
                "{} is no longer among this host's agents: it was renamed or deleted.",
                name.trim()
            )),
        }
    }

    /// The file in the agents folder `agent` was read from, as the host
    /// spells it: `None` for the default the host makes from its backends,
    /// and for an agent a host serves from memory with no folder.
    pub(super) fn agent_file(&self, agent: &AgentDefinition) -> Option<String> {
        let source = self.agent_source();
        let dir = source.folder()?.dir();
        let mut names = vec![format!("{}.md", agent.name)];
        if is_default_name(&agent.name) {
            names.push(format!("{RETIRED_DEFAULT_NAME}.md"));
        }
        names
            .into_iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
            .map(|path| path.display().to_string())
    }

    /// Refuses a session as `agent` on `backend`, the one a conversation ran
    /// on, when the agent does not allow it: the check a start's choice of
    /// backend makes ([`Hosted::backend_for`]).
    ///
    /// # Errors
    ///
    /// A sentence naming the agent and the backend, ending with what the
    /// person can do instead.
    pub(super) fn agent_allows(
        &self,
        agent: &AgentDefinition,
        backend: &BackendId,
        mode: Option<PermissionMode>,
        starting: Starting,
    ) -> Result<(), String> {
        self.backend_for(agent, Some(backend.as_str()), mode)
            .map(|_| ())
            .map_err(|refusal| format!("{refusal} {}", starting.instead()))
    }

    /// What a person's launch of `backend` in `mode` takes from `agent`: its
    /// composition over `profile`'s, and floti-agents left out when it may
    /// not start agents.
    ///
    /// # Errors
    ///
    /// Why `backend` cannot run the agent, as a fragment of a sentence.
    pub(super) fn as_agent(
        &self,
        agent: &AgentDefinition,
        backend: &dyn AgentBackend,
        profile: Option<&Profile>,
        mode: Option<PermissionMode>,
    ) -> Result<AsAgent, String> {
        let composition = agent_composition(agent, backend, profile, mode)?;
        Ok(AsAgent {
            agent: agent.name.clone(),
            file: self.agent_file(agent),
            composition,
            families_off: if agent.can_start {
                Vec::new()
            } else {
                vec![FAMILY]
            },
        })
    }

    /// The agent worktree a session called `name` that a person starts as
    /// `agent` in `folder` works in, made from the commit `folder` has checked
    /// out and named after the session (made unique as a child's is).
    ///
    /// # Errors
    ///
    /// A sentence when `folder` is not in a git repository, has no commit
    /// checked out, or git refuses. Nothing is left behind then.
    pub(super) fn person_worktree(
        &self,
        agent: &AgentDefinition,
        folder: &Path,
        name: &str,
    ) -> Result<Made, String> {
        let nothing = |problem: String| format!("{problem} Nothing was started.");
        let Some(repository) = Repository::find(folder).map_err(nothing)? else {
            return Err(format!(
                "{} works in a worktree of its own, and {} is not in a git repository. Pick a folder in one, or start without the agent; nothing was started.",
                agent.name,
                folder.display()
            ));
        };
        repository.make(folder, name).map_err(nothing)
    }
}
