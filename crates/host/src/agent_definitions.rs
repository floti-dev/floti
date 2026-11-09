//! Agents: the named definitions a session that starts a child of its own
//! may start it as, such as a lead, a worker or a reviewer.
//!
//! An agent says which backends may run the child (in order of preference),
//! the model and effort for each, the permission mode, the tools it may use
//! without asking and the ones it may never use, whether it may start
//! children of its own, whether it works in a worktree of its own, and the
//! profile it runs under. Its body is instructions, composed into the child's
//! own and never typed. Rules are written once, in the spelling
//! [`ToolRules`] documents, and each backend's adapter translates them or
//! refuses.
//!
//! The parsed file is an [`AgentDefinition`], so that it never reads as one
//! more `agent` beside the backends' `agent` module. A running one is a
//! session of that agent, and a session another session started is its
//! child.
//!
//! Agents come from a [`DefinitionSource`] that is read on every start and
//! every listing, so a change to an agent reaches the next child and no
//! running one. The host's source is [`FolderDefinitions`]: one `<name>.md`
//! file per agent in the agents folder of its config folder
//! ([`folder::agents_folder`]), which is the user's to edit. A host that finds
//! the folder still called `roles` moves it to `agents` at start
//! ([`folder::prepare_agents_folder`]). The shipped `reviewer`, `worker` and
//! `lead` are written there at host start only when neither folder exists
//! ([`seed`]). The agent every start without one falls back to
//! ([`default_definition`]) is made from the backends the host has, unless a
//! file named `default.md` replaces it, or, where there is none, an
//! `agent.md` from before agents were called agents, when the default was
//! called `agent` ([`RETIRED_DEFAULT_NAME`]). A child's row from then naming
//! `agent` is the default's.
//!
//! A file that cannot be an agent is never skipped quietly: it is named with
//! its problem in every listing, and a start naming it is refused naming the
//! file and the problem, because a typo must never drop a rule.
//!
//! Clients list, read and edit the folder over `agents:*` through
//! [`editing`], which parses a text as a start would before it writes it and
//! keeps what it replaces in floti.db ([`versions`]), and the folder is
//! watched while one listens for its changes ([`watch`]).
//!
//! [`ToolRules`]: crate::agent::ToolRules

pub mod editing;
pub mod folder;
pub mod parse;
pub mod seed;
pub mod versions;
pub mod watch;

use std::collections::BTreeMap;
use std::sync::Arc;

use floti_protocol::BackendId;
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::EffortLevel;

use crate::agent::{AgentBackend, ToolRules};

pub use folder::{FolderDefinitions, agents_folder};
pub use parse::DefinitionProblem;

/// The name of the agent a start without one gets.
pub const DEFAULT_AGENT_NAME: &str = "default";

/// What the default agent was called before agents were called agents. A
/// child's row written then may still carry it, and an `agent.md` written
/// then is the default where there is no `default.md`.
pub const RETIRED_DEFAULT_NAME: &str = "agent";

/// Whether `name` is the default agent's, by its name or by the one it had
/// before ([`RETIRED_DEFAULT_NAME`]), whatever the case.
pub fn is_default_name(name: &str) -> bool {
    let name = name.trim();
    name.eq_ignore_ascii_case(DEFAULT_AGENT_NAME) || name.eq_ignore_ascii_case(RETIRED_DEFAULT_NAME)
}

/// One agent, as its file defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    /// What it is called, and what its children are named by default.
    pub name: String,
    /// What it is for, in a sentence.
    pub description: String,
    /// The backends that may run its children, in the order they are tried.
    /// Empty means every installed agent backend except a plain command line,
    /// in the host's order.
    pub backends: Vec<BackendId>,
    /// The model each backend runs it with, keyed by backend id.
    pub models: BTreeMap<String, String>,
    /// The effort each backend runs it with, keyed by backend id. Only a
    /// backend whose agent has an effort setting can be given one.
    pub efforts: BTreeMap<String, EffortLevel>,
    /// The permission mode its children run in; auto where the backend has it
    /// when none is named. A parent launched in plan or dont-ask holds its
    /// children to that, and only one launched in bypass-permissions passes
    /// that on.
    pub permission: Option<PermissionMode>,
    /// Tools its children use without asking.
    pub allowed_tools: Vec<String>,
    /// Tools its children never use.
    pub disallowed_tools: Vec<String>,
    /// Whether its children may start children of their own.
    pub can_start: bool,
    /// The name of the profile its children run under, when it names one.
    pub profile: Option<String>,
    /// Whether its children work in a worktree of their own.
    pub worktree: bool,
    /// For a plain command line: the command to run.
    pub command: Option<String>,
    /// For a plain command line: how a task is submitted to it.
    pub submit: Option<String>,
    /// For a plain command line: how it is stopped.
    pub stop: Option<String>,
    /// Instructions composed into its children's own, never typed.
    pub instructions: String,
    /// The lines its file has that Floti no longer uses
    /// ([`parse::RETIRED_FIELDS`]), in the file's order: accepted and
    /// ignored, and named by the listing so the user can delete them.
    pub unused_lines: Vec<String>,
}

impl AgentDefinition {
    /// An agent `name` that lets `backends` run its children and asks for
    /// nothing else.
    pub fn new(name: impl Into<String>, backends: impl IntoIterator<Item = BackendId>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            backends: backends.into_iter().collect(),
            models: BTreeMap::new(),
            efforts: BTreeMap::new(),
            permission: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            can_start: false,
            profile: None,
            worktree: false,
            command: None,
            submit: None,
            stop: None,
            instructions: String::new(),
            unused_lines: Vec::new(),
        }
    }

    /// The model the agent gives for `backend`.
    pub fn model_for(&self, backend: &BackendId) -> Option<&str> {
        keyed(&self.models, backend).map(String::as_str)
    }

    /// The effort the agent gives for `backend`.
    pub fn effort_for(&self, backend: &BackendId) -> Option<EffortLevel> {
        keyed(&self.efforts, backend).copied()
    }

    /// The agent's rules for a launch in `mode`, written once.
    pub fn rules(&self, mode: Option<PermissionMode>) -> ToolRules {
        ToolRules {
            permission_mode: mode,
            allowed_tools: self.allowed_tools.clone(),
            disallowed_tools: self.disallowed_tools.clone(),
        }
    }

    /// The backends of `host` that may run this agent's children, in the
    /// order the agent prefers them: the ones it lists that the host has, or
    /// every agent backend but a plain command line when it lists none.
    pub fn backends_on<'a>(
        &self,
        host: &'a [Arc<dyn AgentBackend>],
    ) -> Vec<&'a Arc<dyn AgentBackend>> {
        if self.backends.is_empty() {
            return host
                .iter()
                .filter(|backend| !is_plain_command(backend.as_ref()))
                .collect();
        }
        self.backends
            .iter()
            .filter_map(|id| {
                host.iter()
                    .find(|backend| backend.id().as_str().eq_ignore_ascii_case(id.as_str()))
            })
            .collect()
    }

    /// Why `backend` cannot run this agent's children in `mode`, as a
    /// fragment of a sentence, or `None` when it can: its adapter enforces
    /// the agent's permission and tool rules, it has an effort setting where
    /// the agent gives it an effort, and it takes instructions where the
    /// agent has a body. A plain command line takes no task, so none can.
    pub fn refusal_on(
        &self,
        backend: &dyn AgentBackend,
        mode: Option<PermissionMode>,
    ) -> Option<String> {
        if is_plain_command(backend) {
            return Some(format!(
                "{} runs a plain command line, which takes no task",
                backend.id()
            ));
        }
        if let Err(err) = backend.check_rules(&self.rules(mode)) {
            return Some(err.to_string());
        }
        if self.effort_for(backend.id()).is_some() && !backend.takes_effort() {
            return Some(format!(
                "The agent {} gives an effort for {}, which has no effort setting",
                self.name,
                backend.id()
            ));
        }
        if !self.instructions.is_empty() && !backend.takes_instructions() {
            return Some(format!(
                "The agent {} has instructions, which {} cannot take",
                self.name,
                backend.id()
            ));
        }
        None
    }
}

/// The value keyed by `backend`, whatever the case of the key.
fn keyed<'a, V>(map: &'a BTreeMap<String, V>, backend: &BackendId) -> Option<&'a V> {
    map.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(backend.as_str()))
        .map(|(_, value)| value)
}

/// An agent file that cannot be used, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadDefinition {
    /// The file's name, such as `reviewer.md`.
    pub file: String,
    /// What is wrong with it, as a sentence fragment: every problem, joined.
    pub problem: String,
    /// Every problem, each with the field and line it concerns.
    pub problems: Vec<DefinitionProblem>,
}

impl BadDefinition {
    /// The file `file` with `problems`.
    pub fn new(file: impl Into<String>, problems: Vec<DefinitionProblem>) -> Self {
        Self {
            file: file.into(),
            problem: parse::joined(&problems),
            problems,
        }
    }

    /// The sentence a refused start gives.
    pub fn refusal(&self) -> String {
        format!(
            "The agent file {} cannot be used: {}. Fix it and start again; no agent was started.",
            self.file, self.problem
        )
    }

    /// The sentence a refused reopen gives.
    pub fn reopen_refusal(&self) -> String {
        format!(
            "The agent file {} cannot be used: {}. Fix it and reopen the session again; it was not reopened.",
            self.file, self.problem
        )
    }
}

/// Every agent there is at one moment: the good ones in name order, then the
/// files that are not agents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefinitionListing {
    pub agents: Vec<AgentDefinition>,
    pub bad: Vec<BadDefinition>,
}

/// What looking an agent up by name found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionLookup {
    Found(Box<AgentDefinition>),
    /// The agent's file exists and cannot be used.
    Bad(BadDefinition),
    Missing,
}

/// Where agents come from. Read on every start and every listing.
pub trait DefinitionSource: Send + Sync {
    /// Every agent and every unusable file, as they are now.
    fn listing(&self) -> DefinitionListing;

    /// The agent called `name`, whatever the case, as it is now. Either of
    /// the default's names finds the default.
    fn lookup(&self, name: &str) -> DefinitionLookup {
        let name = if is_default_name(name) {
            DEFAULT_AGENT_NAME
        } else {
            name
        };
        let listing = self.listing();
        if let Some(agent) = listing
            .agents
            .into_iter()
            .find(|agent| agent.name.eq_ignore_ascii_case(name))
        {
            return DefinitionLookup::Found(Box::new(agent));
        }
        listing
            .bad
            .into_iter()
            .find(|bad| bad.file.eq_ignore_ascii_case(&format!("{name}.md")))
            .map_or(DefinitionLookup::Missing, DefinitionLookup::Bad)
    }

    /// The folder the agents are read from, when they are read from one: what
    /// a client lists, reads and edits.
    fn folder(&self) -> Option<&FolderDefinitions> {
        None
    }
}

/// A source that names no agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDefinitions;

impl DefinitionSource for NoDefinitions {
    fn listing(&self) -> DefinitionListing {
        DefinitionListing::default()
    }
}

/// A fixed list of agents.
#[derive(Debug, Clone, Default)]
pub struct FixedDefinitions(pub Vec<AgentDefinition>);

impl DefinitionSource for FixedDefinitions {
    fn listing(&self) -> DefinitionListing {
        DefinitionListing {
            agents: self.0.clone(),
            bad: Vec::new(),
        }
    }
}

/// The agent a start without one gets, `default`: every agent backend the
/// host has except a plain command, in the host's order, with no rules of its
/// own, that may not start children.
pub fn default_definition(backends: &[Arc<dyn AgentBackend>]) -> AgentDefinition {
    let mut agent = AgentDefinition::new(
        DEFAULT_AGENT_NAME,
        backends
            .iter()
            .filter(|backend| !is_plain_command(backend.as_ref()))
            .map(|backend| backend.id().clone()),
    );
    agent.description =
        "What a start without an agent gets: any installed backend, no rules of its own."
            .to_owned();
    agent
}

/// Whether `backend` is a plain command line, which declares nothing beyond
/// running: no resume, no records, no tools, no history. It takes no opening
/// prompt, so it is not an agent a task can be handed to.
pub fn is_plain_command(backend: &dyn AgentBackend) -> bool {
    backend.resumer().is_none()
        && backend.status().is_none()
        && backend.agent_tools().is_none()
        && backend.permissions().is_none()
        && backend.history().is_none()
}

/// Whether `name` can be an agent's name: lowercase letters, digits and
/// hyphens.
pub fn is_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests;
