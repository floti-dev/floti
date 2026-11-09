//! Children: the sessions an agent starts, lists and stops through the
//! floti-agents tools, and what binds their lifetimes to their parents'.
//!
//! **Everything here works with no client attached.** A child is started by
//! the host for a session that asked, not by a client; nothing is attached to
//! it, it opens no tab, and a client quitting reaches it only through its
//! parent.
//!
//! **A start is judged before anything runs.** In this order: the role, the
//! backend the role allows and the host has, the folder (tracked, on the
//! path it leads to), the profile, the permission mode (the role's, else
//! auto, held to the parent's only where the parent was launched in plan or
//! dont-ask, and bypass-permissions only under a parent launched in it), the
//! backend's ability to enforce every rule, then the limits
//! read afresh from the settings. Only a start that passes all of it spawns a
//! process, and only then is a worktree made, so a refused start writes
//! nothing. Starts are taken one at a time, so two calls cannot both pass a
//! limit that only one fits under, or choose the same worktree name.
//!
//! **A command child is a child that is no agent** ([`commands`]): a plain
//! command the caller runs on the user's shell with `floti agent run`. It is
//! named, waited on, stopped and ended with its parent as an agent child is,
//! and counts toward none of the agent limits.
//!
//! **A resume is a start of an ended child.** It names one of the caller's
//! children, resolved among the caller's own rows (ended ones included), and
//! reopens it in its recorded folder through its backend's resume with the
//! task as its next turn. It is refused while the child runs, when anything
//! but the task is asked for, and when its folder, backend or conversation
//! cannot be reopened; then it is judged against the same limits and counts as
//! one more start and one more running child.
//!
//! **A child's row carries its links** (parent row, parent conversation,
//! name, role, outlive, can_start), written when the session is. Counts and
//! names come from those rows, so they hold across a host restart, and a
//! parent that is resumed in its conversation finds its outliving children
//! again.
//!
//! **Lifetimes follow the parent.** When a session ends, however it ends, the
//! children it did not start to outlive it end, and theirs with them
//! ([`end_children_at_exit`]); a host that starts after parents ended while
//! it was away ends the children they left ([`Hosted::end_orphans`]).

mod commands;
mod compose;
mod folders;
mod inbox;
mod news;
mod notices;
mod queue;
mod read;
mod rows;
mod send;
mod startup;
mod wait;

pub(crate) use commands::RunCommand;
use inbox::StoppedBy;
#[cfg(test)]
pub(crate) use inbox::{EventKind, Facts, News};
use notices::StartLimit;
pub(crate) use read::{ChildRead, ReadChild};
pub(crate) use send::{ChildSent, SendChild, deliver_at_each_moment};
pub(crate) use startup::after_each_launch;
pub(crate) use wait::{Holding, WaitEnd, WaitFor, WaitRequest, Waited};

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use floti_protocol::BackendId;
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::Profile;
use tokio::sync::broadcast::{self, error::RecvError};

use super::{Hosted, Plan, lock};
use crate::agent::guidance::ChildInstructions;
use crate::agent::{AgentBackend, AgentToolServer, Composition};
use crate::agent_cli::{SESSIONS_URL_VAR, TOKEN_VAR, URL_VAR};
use crate::roles::{BadRole, DEFAULT_ROLE, NoRoles, Role, RoleLookup, RoleSource, default_role};
use crate::server::floti_agents::FAMILY;
use crate::server::records::{self, ExitNotice};
use crate::server::session_tools::SESSIONS_FAMILY;
use crate::session::{ParentLink, SessionId, SessionOrigin, TermSize};
use crate::workspace::files::paths::resolves_inside;
use crate::workspace::settings;
use crate::workspace::worktrees::{self, Repository};

/// How long ending a child waits for it to be reported ended.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// The size a child's terminal starts at: no client is attached to it, and the
/// first terminal to report a size resizes it (`Session::resize`).
const CHILD_SIZE: (u16, u16) = (120, 32);

/// What the host keeps to start children: where roles come from, where the
/// `floti` executable is, and the one lock that takes starts one at a time.
pub(super) struct Children {
    roles: RwLock<Arc<dyn RoleSource>>,
    /// The folder holding the `floti` executable, put first on a session's
    /// PATH so that `floti agent` is found there.
    floti_dir: RwLock<Option<PathBuf>>,
    /// The PATH the host's sessions inherit.
    inherited_path: Option<OsString>,
    starting: Mutex<()>,
    /// One lock for each child being typed into, held for a whole turn.
    typing: Mutex<HashMap<SessionId, Arc<tokio::sync::Mutex<()>>>>,
    /// What the children are doing, as news for their parents, and what a
    /// wait sleeps on.
    news: news::Desk,
}

impl Children {
    /// Children running when the host started are taken from the keeper
    /// with `adopted`: their first pass is where they were, not news.
    pub(super) fn adopt(&self, adopted: impl IntoIterator<Item = SessionId>) {
        self.news.adopt(adopted);
    }

    /// The host has started: the notices of parents that are no longer
    /// running go.
    pub(super) fn clear_unowned_notices(&self, store: &crate::store::Store) {
        if let Err(err) = notices::remove_unowned(store) {
            log_line!("floti host: {err}");
        }
    }

    pub(super) fn new(inherited_env: &[(OsString, OsString)]) -> Self {
        Self {
            news: news::Desk::new(),
            roles: RwLock::new(Arc::new(NoRoles)),
            floti_dir: RwLock::new(
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(PathBuf::from)),
            ),
            inherited_path: inherited_env
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| value.clone()),
            starting: Mutex::new(()),
            typing: Mutex::new(HashMap::new()),
        }
    }

    /// The lock that takes sends to session `id` one turn at a time. A lock
    /// nobody holds is forgotten the next time one is asked for.
    fn typing_lock(&self, id: SessionId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = lock(&self.typing);
        locks.retain(|held, turn| *held == id || Arc::strong_count(turn) > 1);
        Arc::clone(locks.entry(id).or_default())
    }

    /// `path` (the PATH the session would have) with the folder of the `floti`
    /// executable first.
    fn path_with_floti(&self, path: Option<&OsString>) -> Option<OsString> {
        let dir = self
            .floti_dir
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        let rest = path.map(std::env::split_paths).into_iter().flatten();
        std::env::join_paths(std::iter::once(dir).chain(rest)).ok()
    }

    fn roles(&self) -> Arc<dyn RoleSource> {
        Arc::clone(
            &self
                .roles
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// What `agents_start` asks for.
#[derive(Debug, Clone, Default)]
pub(crate) struct StartChild {
    pub task: String,
    pub role: Option<String>,
    pub backend: Option<String>,
    pub folder: Option<String>,
    pub profile: Option<String>,
    pub name: Option<String>,
    /// The one-word name of the worktree to make for it. A role that asks for
    /// a worktree makes one named after the child when this is `None`.
    pub worktree: Option<String>,
    pub outlive: bool,
    pub can_start: Option<bool>,
    /// The name of one of the caller's ended children to reopen. `task` is
    /// then its next turn, and nothing else about it can be asked for.
    pub resume: Option<String>,
}

/// What `agents_start` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildStarted {
    pub name: String,
    pub backend: String,
    pub folder: String,
    pub branch: Option<String>,
    /// The name of the worktree made for it, when one was.
    pub worktree: Option<String>,
    /// The name it runs Remote Control under, when it does.
    pub remote_control: Option<String>,
    /// Its parent runs Remote Control and its backend has none: the name of
    /// that backend, for the answer to say the child runs without.
    pub no_remote_control: Option<String>,
}

/// What closing a session ends beyond itself, and what it leaves running.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Closing {
    /// The rows of the running agent-started sessions that end with it, each
    /// with the name its parent calls it.
    pub ending: Vec<(u64, String)>,
    /// The rows of the running ones started to outlive their parent, named
    /// the same way.
    pub outliving: Vec<(u64, String)>,
}

/// One running child, as `agents_list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildEntry {
    pub name: String,
    /// The role an agent runs in; none for a command child.
    pub role: Option<String>,
    /// The command line a command child runs, where an agent has its role.
    pub command: Option<String>,
    pub backend: String,
    pub state: String,
    pub since: String,
    pub folder: String,
    pub branch: Option<String>,
    pub outlive: bool,
    pub can_start: bool,
    /// Whether it works in a worktree Floti made for it.
    pub worktree: bool,
}

/// One role, as `agents_roles` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoleEntry {
    pub name: String,
    pub description: String,
    pub worktree: bool,
    pub can_start: bool,
    /// The installed backends it can run on, in the role's order.
    pub backends: Vec<String>,
    /// The lines its file has that Floti no longer uses.
    pub unused_lines: Vec<String>,
}

/// What `agents_roles` answers: the roles there are now, then the files that
/// are not roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RolesListed {
    pub roles: Vec<RoleEntry>,
    pub bad: Vec<BadRole>,
}

/// What is left of the limits a session starts children under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LimitsLeft {
    pub children_per_parent: u32,
    pub children_left: u32,
    pub starts_per_parent: u32,
    pub starts_left: u32,
    pub sessions_per_host: u32,
    pub host_left: u32,
    /// The most command children a parent runs at once, counted apart from
    /// every agent limit.
    pub commands_per_parent: u32,
    /// How many more command children the parent runs.
    pub commands_left: u32,
    pub depth: u32,
    /// Why the session cannot start children at all, when it cannot.
    pub cannot_start: Option<String>,
}

/// What `agents_list` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildrenListed {
    pub children: Vec<ChildEntry>,
    pub limits: LimitsLeft,
}

/// What `agents_stop` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildStopped {
    pub name: String,
    /// The names of the agents below it that ended with it.
    pub also: Vec<String>,
    /// Whether it stays under its parent as a stopped row until it is
    /// resumed or a person removes it.
    pub kept: bool,
}

impl Hosted {
    /// Reads roles from `roles` from now on.
    pub(crate) fn use_roles(&self, roles: Arc<dyn RoleSource>) {
        *self
            .children
            .roles
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = roles;
    }

    /// Puts `exe`'s folder on the PATH of the sessions that start from now
    /// on.
    pub(crate) fn use_floti(&self, exe: &std::path::Path) {
        if let Some(dir) = exe.parent() {
            *self
                .children
                .floti_dir
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.to_path_buf());
        }
    }

    /// Adds to a session's environment what its shell needs for `floti agent`
    /// and `floti session title`: its token, the address of each of the two
    /// families it was given, and the PATH with the `floti` executable's
    /// folder first. Nothing for a launch given neither family.
    pub(super) fn give_agent_env(
        &self,
        env: &mut Vec<(OsString, OsString)>,
        servers: &[AgentToolServer],
    ) {
        let agents = servers.iter().find(|server| server.name == FAMILY);
        let sessions = servers.iter().find(|server| server.name == SESSIONS_FAMILY);
        // One token per launch, whichever family it is read from.
        let Some(token) = agents.or(sessions).map(|server| &server.token) else {
            return;
        };
        let path = env
            .iter()
            .rev()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.clone())
            .or_else(|| self.children.inherited_path.clone());
        set_env(env, TOKEN_VAR, OsString::from(token));
        if let Some(agents) = agents {
            set_env(env, URL_VAR, OsString::from(&agents.url));
        }
        if let Some(sessions) = sessions {
            set_env(env, SESSIONS_URL_VAR, OsString::from(&sessions.url));
        }
        if let Some(path) = self.children.path_with_floti(path.as_ref()) {
            set_env(env, "PATH", path);
        }
    }

    /// Starts a child of `parent` on `request`, or says why it cannot: the
    /// sentence a model reads.
    ///
    /// # Errors
    ///
    /// A sentence naming what was refused. Nothing has started then.
    pub(crate) fn start_child(
        &self,
        parent: SessionId,
        request: StartChild,
    ) -> Result<ChildStarted, String> {
        // One start at a time: the limits are counted and then spent.
        let _starting = lock(&self.children.starting);
        if self.sessions.get(parent).is_none() {
            return Err("Your session has ended, so it starts no more agents.".to_owned());
        }
        let facts =
            rows::parent_facts(&self.ledger.store(), parent).map_err(|err| err.to_string())?;
        if request.resume.is_some() {
            return self.resume_child(parent, &facts, request);
        }

        let role = self.role_for(request.role.as_deref())?;
        // A backend is chosen for the mode the role names. The auto a child
        // gets when it names none is each backend's own, so it rules none out.
        let role_mode = child_mode(role.permission, facts.permission_mode, None);
        let backend = self.backend_for(&role, request.backend.as_deref(), role_mode)?;
        let base = PathBuf::from(&facts.cwd);
        let folder = folders::child_folder(
            &self.ledger.shared_store(),
            &base,
            request.folder.as_deref(),
        )?;
        // A worktree is asked for by name or by the role. Anywhere in a
        // repository a start must also leave alone a worktree another session
        // works in.
        let wants_worktree = request.worktree.is_some() || role.worktree;
        let repository = if wants_worktree {
            Some(Repository::of(&folder)?)
        } else {
            Repository::of(&folder).ok()
        };
        if let Some(repository) = &repository {
            self.refuse_shared_worktree(repository, &folder, parent)?;
        }
        let profile = {
            let store = self.ledger.store();
            let profiles = rows::profiles(&store).map_err(|err| err.to_string())?;
            match (request.profile.as_deref(), role.profile.as_deref()) {
                (Some(named), _) => Some(folders::profile_named(&profiles, named)?),
                (None, Some(named)) => Some(
                    folders::profile_named(&profiles, named).map_err(|problem| {
                        format!(
                            "The role file {}.md names a profile that cannot be used: {problem} No agent was started.",
                            role.name
                        )
                    })?,
                ),
                (None, None) => {
                    folders::profile_holding(&store, &folder).map_err(|err| err.to_string())?
                }
            }
        };

        let mode = child_mode(
            role.permission
                .or_else(|| profile.as_ref().and_then(|profile| profile.permission_mode)),
            facts.permission_mode,
            backend.unattended_mode(),
        );
        let mut composition =
            compose::role_composition(&role, backend.as_ref(), profile.as_ref(), mode)
                .map_err(|reason| format!("{reason}, so no agent was started."))?;
        composition.unattended = true;
        // What the child is told of the session that started it. An agent with
        // no MCP tools has no `agents_report` and is not told of it.
        composition.child = Some(ChildInstructions {
            parent: rows::label_of(&self.ledger.store(), parent)
                .ok()
                .flatten()
                .unwrap_or_default(),
            role: role.name.clone(),
            reports: backend.tool_support().mcp_tools,
        });

        let can_start = request.can_start.unwrap_or(role.can_start);
        let (name, limits_error) = {
            let store = self.ledger.store();
            let settings = settings::read(store.connection()).map_err(|err| err.to_string())?;
            let existing = rows::children_of(&store, parent, facts.conversation_id.as_deref())
                .map_err(|err| err.to_string())?;
            let on_host = self.agents_on_host(&store)?;
            // Command children are no agents: they fill no agent limit.
            let agents: Vec<&rows::ChildRow> = existing
                .iter()
                .filter(|child| !child.is_command())
                .collect();
            let running: Vec<&rows::ChildRow> = agents
                .iter()
                .copied()
                .filter(|child| child.running && self.sessions.get(child.id).is_some())
                .collect();
            let limits_error =
                judge(&settings, &facts, &running, agents.len(), &agents, &on_host).err();
            let wanted = request.name.as_deref().unwrap_or(&role.name);
            (
                unique_child_name(wanted, existing.iter().map(|child| child.name.as_str())),
                limits_error,
            )
        };
        if let Some(refusal) = limits_error {
            return Err(self.refused(parent, refusal));
        }

        // Everything is judged: only now is anything written.
        let made = match (&repository, wants_worktree) {
            (Some(repository), true) => {
                Some(repository.make(&folder, request.worktree.as_deref().unwrap_or(&name))?)
            }
            _ => None,
        };
        let project_path = self.known_project(&folder.display().to_string());
        let run_folder = made
            .as_ref()
            .map_or_else(|| folder.clone(), |made| made.folder.clone());
        let outlive = request.outlive;
        let backend_id = backend.id().clone();
        let keeps_records = backend.status().is_some();
        let remote_control = compose::inherited_remote_control(
            facts.remote_control_name.as_deref(),
            &name,
            backend.as_ref(),
        );
        let no_remote_control = remote_control
            .backend_has_none
            .then(|| backend.name().to_owned());
        let launched = self.run(
            Plan {
                backend,
                folder: run_folder,
                name: name.clone(),
                prompt: Some(request.task),
                composition,
                resume: None,
                origin: SessionOrigin {
                    project_path,
                    profile_id: profile.as_ref().and_then(|p| i64::try_from(p.id).ok()),
                    permission_mode: mode.map(records::permission_mode_name),
                    branch: None,
                    remote_control_name: remote_control.name.clone(),
                    parent: Some(ParentLink {
                        session: parent,
                        conversation_id: facts.conversation_id.clone(),
                        name: name.clone(),
                        role: Some(role.name.clone()),
                        command: None,
                        outlive,
                        can_start,
                        worktree: made.is_some(),
                    }),
                },
            },
            TermSize::new(CHILD_SIZE.0, CHILD_SIZE.1),
            None,
        );
        let launched = match launched {
            Ok(launched) => launched,
            Err(reason) => {
                // A worktree made for a child that never started holds no work.
                if let Some(Err(err)) = made.as_ref().map(worktrees::Made::discard) {
                    log_line!("floti host: {err}");
                }
                self.launch_failed(
                    &rows::Link {
                        name: name.clone(),
                        parent,
                        conversation: facts.conversation_id.clone(),
                    },
                    &reason,
                );
                return Err(reason);
            }
        };

        self.end_with_gone_parent(parent, outlive, launched.session.id)?;
        self.child_launched(launched.session.id, keeps_records);
        Ok(ChildStarted {
            name,
            backend: backend_id_text(&backend_id),
            folder: launched.session.cwd,
            branch: launched.session.branch,
            worktree: made.map(|made| made.name),
            remote_control: remote_control.name,
            no_remote_control,
        })
    }

    /// A limit refused a start of `parent`: the notice it leaves stands for
    /// the person. Answers the sentence the refusal is told in.
    fn refused(&self, parent: SessionId, refusal: Refusal) -> String {
        self.limit_refused(
            parent,
            notices::Limit {
                limit: refusal.limit,
                text: &refusal.text,
                allow_more: refusal.allow_more,
            },
        )
    }

    /// The parent may have ended while a child started, after the exit notice
    /// that would have ended the child had already been handled: the child
    /// that was not to outlive it ends now.
    fn end_with_gone_parent(
        &self,
        parent: SessionId,
        outlive: bool,
        child: u64,
    ) -> Result<(), String> {
        if outlive || self.parent_runs(parent) {
            return Ok(());
        }
        if let Some(session) = super::session_id(child).and_then(|id| self.sessions.get(id)) {
            self.note_stopped(session.id(), StoppedBy::Floti, false);
            if let Err(err) = session.end_now() {
                log_line!("floti host: {err}");
            }
        }
        Err(
            "Your session ended while the agent was starting, so the agent ended with it."
                .to_owned(),
        )
    }

    /// Reopens `parent`'s ended child named by `request.resume` in the folder
    /// it worked in, through its backend's own resume of its conversation,
    /// with the task as its next turn and its role's instructions composed
    /// again. It is judged as a start is, against the same limits, and is one
    /// more start and one more running child. It keeps its name, parent link,
    /// outlive and can_start; nothing else about it can be asked for.
    ///
    /// # Errors
    ///
    /// A sentence naming what was refused. Nothing has started then.
    fn resume_child(
        &self,
        parent: SessionId,
        facts: &rows::ParentFacts,
        request: StartChild,
    ) -> Result<ChildStarted, String> {
        let wanted = request.resume.as_deref().unwrap_or_default().trim();
        let also: Vec<&str> = [
            ("role", &request.role),
            ("backend", &request.backend),
            ("folder", &request.folder),
            ("profile", &request.profile),
            ("worktree", &request.worktree),
            ("name", &request.name),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.is_some().then_some(key))
        .collect();
        if !also.is_empty() {
            return Err(format!(
                "A resumed agent keeps its own role, backend, folder, profile, worktree and name, so resume takes only the task, and you also gave {}. No agent was started.",
                also.join(", ")
            ));
        }

        let (settings, existing, on_host) = {
            let store = self.ledger.store();
            (
                settings::read(store.connection()).map_err(|err| err.to_string())?,
                rows::children_of(&store, parent, facts.conversation_id.as_deref())
                    .map_err(|err| err.to_string())?,
                self.agents_on_host(&store)?,
            )
        };
        let agents: Vec<&rows::ChildRow> = existing
            .iter()
            .filter(|child| !child.is_command())
            .collect();
        let running: Vec<&rows::ChildRow> = agents
            .iter()
            .copied()
            .filter(|child| child.running && self.sessions.get(child.id).is_some())
            .collect();
        // The newest row of that name is the child: a resume adds a row under
        // the same name.
        let Some(child) = existing
            .iter()
            .rev()
            .find(|child| child.name.eq_ignore_ascii_case(wanted))
        else {
            return Err(not_found(wanted, &running));
        };
        let name = &child.name;
        if child.is_command() {
            return Err(format!(
                "{name} is a command child, and a command is not resumed. Run it again with floti agent run. No agent was started."
            ));
        }
        if running.iter().any(|runs| runs.id == child.id) {
            return Err(format!(
                "{name} is still running, so it cannot be resumed. Wait for news from it, or stop it first. No agent was started."
            ));
        }
        let Some(conversation_id) = child.conversation_id.clone() else {
            return Err(format!(
                "Floti never learned which conversation {name} had, so it cannot be resumed. No agent was started."
            ));
        };
        let backend_id = BackendId::new(&child.backend);
        let Some(backend) = self.find_backend(&backend_id).map(Arc::clone) else {
            return Err(format!(
                "{name} ran on `{}`, which this Floti host does not have, so it cannot be resumed. No agent was started.",
                child.backend
            ));
        };
        if backend.resumer().is_none() {
            return Err(format!(
                "{} cannot reopen a conversation, so {name} cannot be resumed. No agent was started.",
                backend.name()
            ));
        }
        let recorded = PathBuf::from(&child.cwd);
        if !recorded.is_dir() {
            return Err(format!(
                "{} is no longer on disk, so {name} cannot be resumed. No agent was started.",
                recorded.display()
            ));
        }
        let folder = folders::child_folder(&self.ledger.shared_store(), &recorded, None)
            .map_err(|problem| format!("{problem} {name} was not resumed."))?;
        if let Ok(repository) = Repository::of(&folder) {
            self.refuse_shared_worktree(&repository, &folder, parent)
                .map_err(|problem| format!("{name} was not resumed: {problem}"))?;
        }

        let profile = child
            .profile_id
            .and_then(|id| u64::try_from(id).ok())
            .and_then(|id| {
                records::read_profile(&self.ledger.store(), id)
                    .ok()
                    .flatten()
            });
        let mode = child_mode(
            child.permission_mode,
            facts.permission_mode,
            backend.unattended_mode(),
        );
        let mut composition = super::composition_of(profile.as_ref());
        composition.permission_mode = mode;
        if let Some(role) = child.role.as_deref() {
            let (reopened, warning) =
                self.reopen_in_role(role, backend.as_ref(), profile.as_ref(), mode)?;
            if let Some(reopened) = reopened {
                composition = reopened;
            }
            if let Some(warning) = warning {
                log_line!("floti host: {name}: {warning}");
            }
        }
        composition.unattended = true;
        // A reopened child is told again of the session that started it.
        composition.child = Some(ChildInstructions {
            parent: rows::label_of(&self.ledger.store(), parent)
                .ok()
                .flatten()
                .unwrap_or_default(),
            role: child
                .role
                .clone()
                .unwrap_or_else(|| DEFAULT_ROLE.to_owned()),
            reports: backend.tool_support().mcp_tools,
        });
        judge(&settings, facts, &running, agents.len(), &agents, &on_host)
            .map_err(|refusal| self.refused(parent, refusal))?;

        // Everything is judged: only now does anything run.
        let remote_control = compose::inherited_remote_control(
            facts.remote_control_name.as_deref(),
            name,
            backend.as_ref(),
        );
        let no_remote_control = remote_control
            .backend_has_none
            .then(|| backend.name().to_owned());
        let keeps_records = backend.status().is_some();
        let launched = self.run(
            Plan {
                backend,
                folder: folder.clone(),
                name: name.clone(),
                prompt: Some(request.task),
                composition,
                resume: Some(conversation_id),
                origin: SessionOrigin {
                    project_path: self.known_project(&folder.display().to_string()),
                    profile_id: profile.as_ref().and_then(|p| i64::try_from(p.id).ok()),
                    permission_mode: mode.map(records::permission_mode_name),
                    branch: None,
                    remote_control_name: remote_control.name.clone(),
                    parent: Some(ParentLink {
                        session: parent,
                        conversation_id: facts.conversation_id.clone(),
                        name: name.clone(),
                        role: child.role.clone(),
                        command: None,
                        outlive: child.outlive,
                        can_start: child.can_start,
                        worktree: child.worktree,
                    }),
                },
            },
            TermSize::new(CHILD_SIZE.0, CHILD_SIZE.1),
            None,
        );
        let launched = match launched {
            Ok(launched) => launched,
            Err(reason) => {
                // The parent is told in the answer, and again as news: a
                // wait it is in hears that the agent never ran.
                self.launch_failed(
                    &rows::Link {
                        name: name.clone(),
                        parent,
                        conversation: facts.conversation_id.clone(),
                    },
                    &reason,
                );
                return Err(reason);
            }
        };
        self.end_with_gone_parent(parent, child.outlive, launched.session.id)?;
        self.child_launched(launched.session.id, keeps_records);
        // Resumed, it is its running row: the stopped rows of its name are
        // no longer kept.
        let earlier: Vec<SessionId> = existing
            .iter()
            .filter(|row| row.name.eq_ignore_ascii_case(name))
            .map(|row| row.id)
            .collect();
        self.unkeep(&earlier);
        Ok(ChildStarted {
            name: name.clone(),
            backend: backend_id_text(&backend_id),
            folder: launched.session.cwd,
            branch: launched.session.branch,
            worktree: child
                .worktree
                .then(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()))
                .flatten(),
            remote_control: remote_control.name,
            no_remote_control,
        })
    }

    /// Refuses a start in a folder inside an agent worktree that a running
    /// session other than `caller` works in: one checkout, one agent.
    fn refuse_shared_worktree(
        &self,
        repository: &Repository,
        folder: &Path,
        caller: SessionId,
    ) -> Result<(), String> {
        let Some(root) = repository.worktree_holding(folder) else {
            return Ok(());
        };
        let running =
            rows::running_sessions(&self.ledger.store()).map_err(|err| err.to_string())?;
        let holder = running
            .into_iter()
            .filter(|session| session.id != caller && self.sessions.get(session.id).is_some())
            .find(|session| resolves_inside(&root, Path::new(&session.cwd)));
        match holder {
            Some(holder) => Err(format!(
                "{} is inside the worktree {}, where the agent {} works, so no agent starts there. Start yours in a worktree of its own: name one with worktree.",
                folder.display(),
                root.display(),
                holder.label
            )),
            None => Ok(()),
        }
    }

    /// `parent`'s running children and what is left of its limits.
    ///
    /// # Errors
    ///
    /// A sentence when the host's record cannot be read.
    pub(crate) fn list_children(&self, parent: SessionId) -> Result<ChildrenListed, String> {
        let store = self.ledger.store();
        let facts = rows::parent_facts(&store, parent).map_err(|err| err.to_string())?;
        let settings = settings::read(store.connection()).map_err(|err| err.to_string())?;
        let existing = rows::children_of(&store, parent, facts.conversation_id.as_deref())
            .map_err(|err| err.to_string())?;
        let on_host = self.agents_on_host(&store)?;
        // Command children are listed with their command, but they are no
        // agents and fill no agent limit.
        let agents: Vec<&rows::ChildRow> = existing
            .iter()
            .filter(|child| !child.is_command())
            .collect();
        let mut children = Vec::new();
        for child in existing.iter() {
            let Some(session) = self.sessions.get(child.id).filter(|_| child.running) else {
                continue;
            };
            let state = session
                .reported_status()
                .map_or_else(|| "running".to_owned(), |status| state_word(&status));
            children.push(ChildEntry {
                name: child.name.clone(),
                role: child.role.clone(),
                command: child.command.clone(),
                backend: child.backend.clone(),
                state,
                since: child.started_at.clone(),
                folder: child.cwd.clone(),
                branch: child.branch.clone(),
                outlive: child.outlive,
                can_start: child.can_start,
                worktree: child.worktree,
            });
        }
        let commands = children
            .iter()
            .filter(|child| child.command.is_some())
            .count();
        let running = u32::try_from(children.len() - commands).unwrap_or(u32::MAX);
        let commands = u32::try_from(commands).unwrap_or(u32::MAX);
        let made = u32::try_from(agents.len()).unwrap_or(u32::MAX);
        let on_host = u32::try_from(on_host.len()).unwrap_or(u32::MAX);
        let starts_allowed = settings
            .agent_starts_per_parent
            .saturating_add(facts.extra_starts);
        Ok(ChildrenListed {
            children,
            limits: LimitsLeft {
                children_per_parent: settings.agent_children_per_parent,
                children_left: settings.agent_children_per_parent.saturating_sub(running),
                starts_per_parent: starts_allowed,
                starts_left: starts_allowed.saturating_sub(made),
                sessions_per_host: settings.agent_sessions_per_host,
                host_left: settings.agent_sessions_per_host.saturating_sub(on_host),
                commands_per_parent: settings.agent_commands_per_parent,
                commands_left: settings.agent_commands_per_parent.saturating_sub(commands),
                depth: settings.agent_depth,
                cannot_start: depth_refusal(&settings, &facts),
            },
        })
    }

    /// Ends `parent`'s running child called `name` and every agent below it.
    /// With `keep` the child stays under its parent as a stopped row, in every
    /// client, until it is resumed or a person removes it; without, it leaves
    /// them at once. The agents below it are never kept. A name that is not
    /// one of `parent`'s running children is not found: nothing else is
    /// looked at.
    ///
    /// # Errors
    ///
    /// A sentence when `name` is not one of `parent`'s running children.
    pub(crate) fn stop_child(
        &self,
        parent: SessionId,
        name: &str,
        keep: bool,
    ) -> Result<ChildStopped, String> {
        let (target, below) = {
            let store = self.ledger.store();
            let facts = rows::parent_facts(&store, parent).map_err(|err| err.to_string())?;
            let existing = rows::children_of(&store, parent, facts.conversation_id.as_deref())
                .map_err(|err| err.to_string())?;
            let running: Vec<&rows::ChildRow> = existing
                .iter()
                .filter(|child| child.running && self.sessions.get(child.id).is_some())
                .collect();
            let Some(target) = running
                .iter()
                .find(|child| child.name.eq_ignore_ascii_case(name.trim()))
            else {
                return Err(not_found(name, &running));
            };
            let below = rows::descendants(&store, target.id).map_err(|err| err.to_string())?;
            (target.id, below)
        };
        let target_name = self.child_name(target).unwrap_or_else(|| name.to_owned());
        let mut ending: Vec<(SessionId, String)> = below
            .into_iter()
            .filter(|child| child.running)
            .map(|child| (child.id, child.name))
            .collect();
        let also: Vec<String> = ending.iter().map(|(_, name)| name.clone()).collect();
        ending.push((target, target_name.clone()));
        let sessions: Vec<_> = ending
            .iter()
            .filter_map(|(id, _)| self.sessions.get(*id))
            .collect();
        // The agent named is stopped by its parent; the ones it started end
        // because it did, which is Floti's doing.
        for session in &sessions {
            if session.id() == target {
                self.note_stopped(session.id(), StoppedBy::Parent, keep);
            } else {
                self.note_stopped(session.id(), StoppedBy::Floti, false);
            }
        }
        for session in &sessions {
            if let Err(err) = session.end_now() {
                log_line!("floti host: {err}");
            }
        }
        for session in &sessions {
            session.wait_exit(STOP_WAIT);
        }
        // A person's stop that came first decides whether it is kept.
        let kept = records::is_kept(&self.ledger.store(), target).unwrap_or(keep);
        Ok(ChildStopped {
            name: target_name,
            also,
            kept,
        })
    }

    /// What closing session `id` ends beyond itself and what keeps running,
    /// for the question that asks before it does. Nothing is ended here.
    pub(crate) fn closing(&self, id: u64) -> Closing {
        let plan = super::session_id(id)
            .map(|id| rows::close_plan(&self.ledger.store(), id))
            .transpose()
            .unwrap_or_else(|err| {
                log_line!("floti host: {err}");
                None
            })
            .unwrap_or_default();
        let row_id = |id: SessionId| u64::try_from(id.0).ok();
        Closing {
            ending: plan
                .ending
                .into_iter()
                .filter_map(|(id, name)| Some((row_id(id)?, name)))
                .collect(),
            outliving: plan
                .outliving
                .into_iter()
                .filter_map(|(id, name)| Some((row_id(id)?, name)))
                .collect(),
        }
    }

    /// Ends the children of `parent` that were not started to outlive it.
    pub(crate) fn end_children_of(&self, parent: SessionId) {
        let children = match rows::ending_with(&self.ledger.store(), parent) {
            Ok(children) => children,
            Err(err) => {
                log_line!("floti host: {err}");
                return;
            }
        };
        for session in children.iter().filter_map(|id| self.sessions.get(*id)) {
            self.note_stopped(session.id(), StoppedBy::Floti, false);
            if let Err(err) = session.end_now() {
                log_line!("floti host: {err}");
            }
        }
    }

    /// Ends every running child whose parent is not running and which was not
    /// started to outlive it, then the children of those, until none is left:
    /// the children a parent left when it ended while no host was there to end
    /// them.
    pub(crate) fn end_orphans(&self) {
        // A chain is as deep as the depth limit allows, which is bounded.
        for _ in 0..16 {
            let orphans = match rows::orphans(&self.ledger.store()) {
                Ok(orphans) => orphans,
                Err(err) => {
                    log_line!("floti host: {err}");
                    return;
                }
            };
            let sessions: Vec<_> = orphans
                .iter()
                .filter_map(|id| self.sessions.get(*id))
                .collect();
            if sessions.is_empty() {
                return;
            }
            for session in &sessions {
                self.note_stopped(session.id(), StoppedBy::Floti, false);
                if let Err(err) = session.end_now() {
                    log_line!("floti host: {err}");
                }
            }
            for session in &sessions {
                session.wait_exit(STOP_WAIT);
            }
        }
    }

    /// Stops keeping each of `ids` that is a kept child, and moves the
    /// clients' listing so every device's tree loses it. Answers how many
    /// were kept.
    pub(crate) fn unkeep(&self, ids: &[SessionId]) -> usize {
        match records::clear_kept(&self.ledger.store(), ids) {
            Ok(0) => 0,
            Ok(cleared) => {
                self.kept_moved();
                cleared
            }
            Err(err) => {
                log_line!("floti host: {err}");
                0
            }
        }
    }

    /// A person is stopping session `id` from a client, on any device: its
    /// record says so, and when it is an agent's child, its parent is told
    /// and it is kept, so the person sees it stopped under its parent.
    pub(super) fn note_person_stop(&self, id: SessionId) {
        self.note_stopped(id, StoppedBy::Person, true);
    }

    /// `session` is in `conversation_id`, which an earlier session was in when
    /// it started children that still run: they are `session`'s again.
    pub(super) fn readopt_children(&self, session: SessionId, conversation_id: &str) {
        match rows::readopt(&self.ledger.store(), session, conversation_id) {
            Ok(adopted) if !adopted.is_empty() => {
                log_line!(
                    "floti host: session {session} was given back {} agent(s) it had started",
                    adopted.len()
                );
            }
            Ok(_) => {}
            Err(err) => log_line!("floti host: {err}"),
        }
    }

    /// The profile that turns the agents family off for a launch, when its
    /// profile does: the families to leave out of its grant.
    pub(super) fn families_off_for(&self, profile_id: Option<i64>) -> Vec<&'static str> {
        let off = profile_id
            .and_then(|id| u64::try_from(id).ok())
            .and_then(|id| {
                records::read_profile(&self.ledger.store(), id)
                    .ok()
                    .flatten()
            })
            .is_some_and(|profile| profile.session_agents == Some(false));
        if off { vec![FAMILY] } else { Vec::new() }
    }

    /// Whether profile `profile_id` turns Remote Control on for the sessions
    /// launched from it.
    pub(super) fn profile_remote_control(&self, profile_id: Option<i64>) -> bool {
        profile_id
            .and_then(|id| u64::try_from(id).ok())
            .and_then(|id| {
                records::read_profile(&self.ledger.store(), id)
                    .ok()
                    .flatten()
            })
            .is_some_and(|profile| profile.remote_control == Some(true))
    }

    fn parent_runs(&self, parent: SessionId) -> bool {
        self.sessions.get(parent).is_some()
            && rows::is_running(&self.ledger.store(), parent).unwrap_or(false)
    }

    fn child_name(&self, id: SessionId) -> Option<String> {
        self.ledger
            .store()
            .connection()
            .query_row(
                "SELECT child_name FROM sessions WHERE id = ?1",
                [id.0],
                |row| row.get(0),
            )
            .ok()
    }

    /// The agent-started sessions running on this host now.
    fn agents_on_host(&self, store: &crate::store::Store) -> Result<Vec<SessionId>, String> {
        Ok(rows::agent_sessions_running(store)
            .map_err(|err| err.to_string())?
            .into_iter()
            .filter(|id| self.sessions.get(*id).is_some())
            .collect())
    }

    /// The role a start asks for, read now: the default role when it names
    /// none.
    fn role_for(&self, asked: Option<&str>) -> Result<Role, String> {
        let roles = self.children.roles();
        let name = asked.unwrap_or(DEFAULT_ROLE);
        match roles.lookup(name) {
            RoleLookup::Found(role) => Ok(*role),
            RoleLookup::Bad(bad) => Err(bad.refusal()),
            RoleLookup::Missing if name.eq_ignore_ascii_case(DEFAULT_ROLE) => {
                Ok(default_role(&self.backends))
            }
            RoleLookup::Missing => {
                let mut names = vec![DEFAULT_ROLE.to_owned()];
                names.extend(
                    roles
                        .listing()
                        .roles
                        .into_iter()
                        .map(|role| role.name)
                        .filter(|known| !known.eq_ignore_ascii_case(DEFAULT_ROLE)),
                );
                Err(format!(
                    "There is no role named {name}. The roles are: {}.",
                    names.join(", ")
                ))
            }
        }
    }

    /// What a lost child is reopened with: its role as the role's file reads
    /// now. A role whose file cannot be used refuses the reopen, naming the
    /// file; a role that is gone reopens the child without it and says so
    /// (`None` for the composition).
    ///
    /// # Errors
    ///
    /// A sentence naming the role file, or the rule the backend now cannot
    /// enforce for it. Nothing has started then.
    pub(super) fn reopen_in_role(
        &self,
        role_name: &str,
        backend: &dyn AgentBackend,
        profile: Option<&Profile>,
        mode: Option<PermissionMode>,
    ) -> Result<(Option<Composition>, Option<String>), String> {
        let role = match self.children.roles().lookup(role_name) {
            RoleLookup::Found(role) => *role,
            RoleLookup::Bad(bad) => return Err(bad.reopen_refusal()),
            RoleLookup::Missing if role_name.eq_ignore_ascii_case(DEFAULT_ROLE) => {
                default_role(&self.backends)
            }
            RoleLookup::Missing => {
                return Ok((
                    None,
                    Some(format!(
                        "The role {role_name} no longer exists, so the session reopened without its instructions and rules."
                    )),
                ));
            }
        };
        let composition = compose::role_composition(&role, backend, profile, mode)
            .map_err(|reason| format!("{reason}, so the session was not reopened."))?;
        Ok((Some(composition), None))
    }

    /// Every role there is now and every file that is not one, for
    /// `agents_roles`. The default role comes first, from `agent.md` when
    /// that is a role. Each role lists the installed backends it can run on
    /// for a caller in `caller`'s mode: those whose adapter enforces its
    /// permission and tool rules (the permission the role names, held back by
    /// the caller's mode as a start holds it back), takes an effort where the role gives one and takes
    /// instructions where it has a body.
    ///
    /// # Errors
    ///
    /// A sentence when the caller's record cannot be read.
    pub(crate) fn list_roles(&self, caller: SessionId) -> Result<RolesListed, String> {
        let facts =
            rows::parent_facts(&self.ledger.store(), caller).map_err(|err| err.to_string())?;
        let found = self.children.roles().listing();
        let mut roles = found.roles;
        let default = match roles
            .iter()
            .position(|role| role.name.eq_ignore_ascii_case(DEFAULT_ROLE))
        {
            Some(at) => roles.remove(at),
            None if found
                .bad
                .iter()
                .any(|bad| bad.file.eq_ignore_ascii_case(&format!("{DEFAULT_ROLE}.md"))) =>
            {
                // A broken `agent.md` leaves no default role to list, and a
                // start without a role is refused naming it.
                return Ok(RolesListed {
                    roles: roles
                        .iter()
                        .map(|role| self.role_entry(role, facts.permission_mode))
                        .collect(),
                    bad: found.bad,
                });
            }
            None => default_role(&self.backends),
        };
        let mut entries = vec![self.role_entry(&default, facts.permission_mode)];
        entries.extend(
            roles
                .iter()
                .map(|role| self.role_entry(role, facts.permission_mode)),
        );
        Ok(RolesListed {
            roles: entries,
            bad: found.bad,
        })
    }

    fn role_entry(&self, role: &Role, caller_mode: Option<PermissionMode>) -> RoleEntry {
        // As a start chooses its backend: for the mode the role names.
        let mode = child_mode(role.permission, caller_mode, None);
        RoleEntry {
            name: role.name.clone(),
            description: role.description.clone(),
            worktree: role.worktree,
            can_start: role.can_start,
            backends: role
                .backends_on(&self.backends)
                .into_iter()
                .filter(|backend| role.refusal_on(backend.as_ref(), mode).is_none())
                .map(|backend| backend_id_text(backend.id()))
                .collect(),
            unused_lines: role.unused_lines.clone(),
        }
    }

    /// The backend a child runs on: the one asked for when the role lists it
    /// and the host has it, else the first the role lists that the host has
    /// and can run it, which is the first `agents_roles` lists. When none of
    /// those can run it, the first one's reason is the refusal.
    fn backend_for(
        &self,
        role: &Role,
        asked: Option<&str>,
        mode: Option<PermissionMode>,
    ) -> Result<Arc<dyn AgentBackend>, String> {
        let allowed = role.backends_on(&self.backends);
        let installed: Vec<String> = allowed
            .iter()
            .map(|backend| backend_id_text(backend.id()))
            .collect();
        match asked {
            Some(asked) => allowed
                .iter()
                .find(|backend| backend.id().as_str().eq_ignore_ascii_case(asked))
                .map(|backend| Arc::clone(backend))
                .ok_or_else(|| {
                    format!(
                        "The role {} does not allow the backend {asked} on this host. The installed backends it allows: {}.",
                        role.name,
                        or_none(&installed)
                    )
                }),
            None => {
                if let Some(runs) = allowed
                    .iter()
                    .find(|backend| role.refusal_on(backend.as_ref(), mode).is_none())
                {
                    return Ok(Arc::clone(runs));
                }
                match allowed.first() {
                    Some(first) => Err(format!(
                        "{}, so no agent was started.",
                        role.refusal_on(first.as_ref(), mode)
                            .unwrap_or_else(|| "The role cannot run".to_owned())
                    )),
                    None => {
                        let listed: Vec<String> =
                            role.backends.iter().map(backend_id_text).collect();
                        Err(format!(
                            "None of the backends the role {} lists is installed on this host (it lists: {}).",
                            role.name,
                            or_none(&listed)
                        ))
                    }
                }
            }
        }
    }
}

/// Sets `name` to `value` in `env`, replacing a variable of that name however
/// it is cased.
fn set_env(env: &mut Vec<(OsString, OsString)>, name: &str, value: OsString) {
    env.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
    env.push((OsString::from(name), value));
}

fn backend_id_text(id: &BackendId) -> String {
    id.as_str().to_owned()
}

fn or_none(list: &[String]) -> String {
    if list.is_empty() {
        "none".to_owned()
    } else {
        list.join(", ")
    }
}

/// The word for what a program last reported about itself.
fn state_word(status: &crate::session::ReportedStatus) -> String {
    format!("{:?}", status.report.state).to_lowercase()
}

/// The mode a child runs in: `requested` (its role's, else its profile's),
/// else `unattended` (the backend's [`AgentBackend::unattended_mode`]: auto
/// where it has one), held back by what the host knows of the mode `parent`
/// was launched in.
///
/// A child has nobody at its terminal, so a prompt it raises is never
/// answered. Its parent's mode caps it only where the parent was launched to
/// be held back: a parent launched in plan or don't ask gives its children no
/// more than that (from the strictest: plan, don't ask, manual, accept edits,
/// auto, bypass permissions), and bypass permissions reaches a child only
/// when its parent was launched in it; a child asking for it under any other
/// parent gets what a child asking for nothing gets. A parent launched in no
/// mode or in manual does not hold its children to manual: a session started
/// in a tab is launched with none whatever it runs in now, and the agent's
/// records do not say. The user's own deny rules still win, in the agent.
pub(super) fn child_mode(
    requested: Option<PermissionMode>,
    parent: Option<PermissionMode>,
    unattended: Option<PermissionMode>,
) -> Option<PermissionMode> {
    let wanted = requested.or(unattended);
    match parent {
        Some(ceiling @ (PermissionMode::Plan | PermissionMode::DontAsk)) => match wanted {
            Some(mode) if strictness(mode) <= strictness(ceiling) => Some(mode),
            _ => Some(ceiling),
        },
        Some(PermissionMode::BypassPermissions) => wanted,
        _ if wanted == Some(PermissionMode::BypassPermissions) => unattended,
        _ => wanted,
    }
}

fn strictness(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::Plan => 0,
        PermissionMode::DontAsk => 1,
        PermissionMode::Manual => 2,
        PermissionMode::AcceptEdits => 3,
        PermissionMode::Auto => 4,
        PermissionMode::BypassPermissions => 5,
    }
}

/// A child's name: `wanted` made into one word, and made unique among the
/// names `taken` by a number after it (`agent`, `agent-2`).
fn unique_child_name<'a>(wanted: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: Vec<String> = taken.into_iter().map(str::to_lowercase).collect();
    let word: String = wanted
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    let base = if word.is_empty() {
        DEFAULT_ROLE.to_owned()
    } else {
        word
    };
    if !taken.contains(&base.to_lowercase()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.contains(&candidate.to_lowercase()))
        .unwrap_or(base)
}

/// What a name that is none of the caller's running children is answered.
fn not_found(name: &str, running: &[&rows::ChildRow]) -> String {
    let names: Vec<&str> = running.iter().map(|child| child.name.as_str()).collect();
    if names.is_empty() {
        format!("No agent of yours is named {name}: you have no running agents.")
    } else {
        format!(
            "No agent of yours is named {name}. Your running agents are: {}.",
            names.join(", ")
        )
    }
}

/// Why a session at this depth cannot start children, when it cannot.
fn depth_refusal(
    settings: &floti_protocol::channels::workspace::HostSettings,
    facts: &rows::ParentFacts,
) -> Option<String> {
    let limit = settings.agent_depth;
    if facts.depth >= limit {
        return Some(format!(
            "The depth limit of {limit} holds: you are {} (depth {}), and agents nest no deeper than {limit}. Do the work yourself.",
            facts.child_name.as_deref().map_or_else(
                || "a session".to_owned(),
                |name| format!("the agent {name}")
            ),
            facts.depth
        ));
    }
    if facts.depth >= 1 && !facts.can_start {
        return Some(format!(
            "The depth limit of {limit} holds: you are an agent that was started without can_start, so you cannot start agents of your own. Do the work yourself."
        ));
    }
    None
}

/// A start a limit refused: which limit, and the sentence that names it and
/// what holds it.
struct Refusal {
    limit: StartLimit,
    text: String,
    /// For the starts limit, how many starts allowing more adds.
    allow_more: Option<u32>,
}

impl Refusal {
    fn new(limit: StartLimit, text: String) -> Self {
        Self {
            limit,
            text,
            allow_more: None,
        }
    }
}

impl From<Refusal> for String {
    fn from(refusal: Refusal) -> Self {
        refusal.text
    }
}

/// Whether a start is within every limit; the limit and a sentence naming it
/// and what holds it when it is not.
fn judge(
    settings: &floti_protocol::channels::workspace::HostSettings,
    facts: &rows::ParentFacts,
    running: &[&rows::ChildRow],
    starts_made: usize,
    made: &[&rows::ChildRow],
    host_running: &[SessionId],
) -> Result<(), Refusal> {
    if let Some(refusal) = depth_refusal(settings, facts) {
        return Err(Refusal::new(StartLimit::Depth, refusal));
    }
    let per_parent = settings.agent_children_per_parent as usize;
    if running.len() >= per_parent {
        let names: Vec<&str> = running.iter().map(|child| child.name.as_str()).collect();
        return Err(Refusal::new(
            StartLimit::Children,
            format!(
                "The limit of {per_parent} running agents per parent holds: you have {} running ({}). Wait for news from them and start again.",
                running.len(),
                names.join(", ")
            ),
        ));
    }
    let per_host = settings.agent_sessions_per_host as usize;
    if host_running.len() >= per_host {
        let mine: Vec<&str> = running.iter().map(|child| child.name.as_str()).collect();
        let yours = if mine.is_empty() {
            "none of them yours".to_owned()
        } else {
            format!("{} of them yours ({})", mine.len(), mine.join(", "))
        };
        return Err(Refusal::new(
            StartLimit::Sessions,
            format!(
                "The limit of {per_host} agent sessions running on this host holds: {} are running, {yours}. Wait for news and start again.",
                host_running.len()
            ),
        ));
    }
    // What the parent was allowed beyond the setting counts with it.
    let per_life =
        (settings.agent_starts_per_parent as usize).saturating_add(facts.extra_starts as usize);
    if starts_made >= per_life {
        let names: Vec<&str> = made.iter().map(|child| child.name.as_str()).collect();
        return Err(Refusal {
            allow_more: Some(settings.agent_starts_per_parent),
            ..Refusal::new(
                StartLimit::Starts,
                format!(
                    "The limit of {per_life} starts per parent holds: you have made {starts_made} starts ({}). Wait for news and start again.",
                    names.join(", ")
                ),
            )
        });
    }
    Ok(())
}

/// Ends the children of each session as it ends, from the records' exit
/// notices, until the host's sessions are gone. Holds the host weakly.
pub(crate) async fn end_children_at_exit(
    mut exits: broadcast::Receiver<ExitNotice>,
    hosted: Weak<Hosted>,
) {
    loop {
        let received = exits.recv().await;
        let Some(hosted) = hosted.upgrade() else {
            return;
        };
        match received {
            Ok(notice) => {
                let _ = tokio::task::spawn_blocking(move || {
                    // Its parent hears how it ended; then it ends what it
                    // started.
                    hosted.child_ended(notice.session);
                    hosted.end_children_of(notice.session);
                })
                .await;
            }
            // Missed notices: every child whose parent is gone ends.
            Err(RecvError::Lagged(_)) => {
                let _ = tokio::task::spawn_blocking(move || hosted.end_orphans()).await;
            }
            Err(RecvError::Closed) => return,
        }
    }
}

/// Runs `work`, which reads the store and may start processes, off the
/// runtime's workers, for a call of the agents family. A panic in it is a
/// refusal, never a hung call.
///
/// A runtime that is shutting down cancels the work instead of running it.
/// The host is then going, and the call is never answered: a host that goes
/// says nothing to a call it held, as a crashed one cannot, rather than
/// telling its caller to try again. The call is dropped with the runtime.
pub(crate) async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    match settled(tokio::task::spawn_blocking(work).await) {
        Some(settled) => settled,
        None => std::future::pending().await,
    }
}

/// What work run off the runtime gives its call: its own result, a refusal
/// for a panic, or `None` for work the runtime cancelled because it is
/// shutting down. Nothing else cancels it: no one aborts that work.
fn settled<T>(
    joined: Result<Result<T, String>, tokio::task::JoinError>,
) -> Option<Result<T, String>> {
    match joined {
        Ok(done) => Some(done),
        Err(err) if err.is_cancelled() => None,
        Err(_) => Some(Err("Floti could not finish that. Try again.".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PermissionMode::*;

    #[tokio::test]
    async fn work_a_runtime_cancels_is_no_answer_and_a_panic_is_a_refusal() {
        let cancelled = tokio::spawn(std::future::pending::<Result<u8, String>>());
        cancelled.abort();
        assert_eq!(settled(cancelled.await), None);

        let panicked = tokio::task::spawn_blocking(|| -> Result<u8, String> { panic!("in work") });
        assert_eq!(
            settled(panicked.await),
            Some(Err("Floti could not finish that. Try again.".to_owned()))
        );
        let done = tokio::task::spawn_blocking(|| Ok::<u8, String>(7));
        assert_eq!(settled(done.await), Some(Ok(7)));
    }

    /// A runtime that has shut down cancels work it is handed, which is how
    /// [`off_runtime`] tells it is going.
    #[test]
    fn a_runtime_that_has_shut_down_cancels_the_work_it_is_handed() {
        let going = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .expect("a runtime");
        let handle = going.handle().clone();
        going.shutdown_timeout(std::time::Duration::from_secs(5));
        let handed = handle.spawn_blocking(|| Ok::<u8, String>(1));
        let joined = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(handed);
        assert_eq!(settled(joined), None);
    }

    /// Every mode, strictest first.
    const MODES: [PermissionMode; 6] =
        [Plan, DontAsk, Manual, AcceptEdits, Auto, BypassPermissions];

    #[test]
    fn a_child_whose_role_names_no_mode_runs_in_auto_under_a_parent_launched_in_none_or_in_manual()
    {
        for parent in [None, Some(Manual), Some(AcceptEdits), Some(Auto)] {
            assert_eq!(
                child_mode(None, parent, Some(Auto)),
                Some(Auto),
                "{parent:?}"
            );
        }
        // A backend with no auto mode keeps its own default.
        assert_eq!(child_mode(None, None, None), None);
        assert_eq!(child_mode(None, Some(Manual), None), None);
    }

    #[test]
    fn a_child_runs_in_the_mode_its_role_names_under_a_parent_that_holds_it_to_nothing() {
        for parent in [None, Some(Manual), Some(AcceptEdits), Some(Auto)] {
            for asked in [Plan, DontAsk, Manual, AcceptEdits, Auto] {
                assert_eq!(
                    child_mode(Some(asked), parent, Some(Auto)),
                    Some(asked),
                    "{asked:?} under {parent:?}"
                );
            }
        }
    }

    #[test]
    fn a_parent_launched_in_plan_or_dont_ask_gives_its_children_no_more() {
        for ceiling in [Plan, DontAsk] {
            for asked in MODES {
                let expected = if asked == Plan { Plan } else { ceiling };
                assert_eq!(
                    child_mode(Some(asked), Some(ceiling), Some(Auto)),
                    Some(expected),
                    "{asked:?} under {ceiling:?}"
                );
            }
            // The auto a role naming nothing would get is no more either.
            assert_eq!(child_mode(None, Some(ceiling), Some(Auto)), Some(ceiling));
            assert_eq!(child_mode(None, Some(ceiling), None), Some(ceiling));
        }
    }

    #[test]
    fn bypass_permissions_reaches_a_child_only_when_its_parent_was_launched_in_it() {
        assert_eq!(
            child_mode(Some(BypassPermissions), Some(BypassPermissions), Some(Auto)),
            Some(BypassPermissions)
        );
        // Under any other parent it is what naming nothing gives.
        for parent in [None, Some(Manual), Some(AcceptEdits), Some(Auto)] {
            assert_eq!(
                child_mode(Some(BypassPermissions), parent, Some(Auto)),
                Some(Auto),
                "{parent:?}"
            );
            assert_eq!(child_mode(Some(BypassPermissions), parent, None), None);
        }
        // A parent in bypass holds back nothing a child asks for.
        for asked in MODES {
            assert_eq!(
                child_mode(Some(asked), Some(BypassPermissions), Some(Auto)),
                Some(asked)
            );
        }
        assert_eq!(
            child_mode(None, Some(BypassPermissions), Some(Auto)),
            Some(Auto)
        );
    }

    #[test]
    fn names_are_one_word_and_unique_among_the_callers_children() {
        assert_eq!(unique_child_name("agent", []), "agent");
        assert_eq!(unique_child_name("agent", ["agent"]), "agent-2");
        assert_eq!(unique_child_name("agent", ["Agent", "agent-2"]), "agent-3");
        assert_eq!(unique_child_name("code reviewer", []), "code-reviewer");
        assert_eq!(unique_child_name("   ", []), "agent");
    }
}
