//! The host's agents, for its clients: `agents:list`, `agents:read`,
//! `agents:write`, `agents:delete`, `agents:restore`, `agents:addShipped`
//! and `agents:changed`.
//!
//! The agents are the files the host's starts read afresh
//! ([`FolderDefinitions`]), so a client sees the host it asks, and a change a
//! client makes reaches the next start with nothing cached in between. The
//! files are read and written by [`AgentEditor`], which keeps what it
//! replaces in floti.db. A host whose config folder is another host's to
//! write lists, reads and watches, and changes nothing there.
//!
//! The folder is watched only while a client listens on `agents:changed`,
//! and every listening client is told each change, the host's own and an
//! outside editor's alike ([`AgentsWatch`]).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::channel::Router;
use floti_protocol::channels::agents::{
    AddShippedAgent, AgentChange, AgentSummary, AgentUnusedLine, AgentsChange, AgentsChanged,
    AgentsListing, BadAgentFile, DeleteAgent, ListAgents, ReadAgent, RestoreAgent, ShippedAgent,
    WriteAgent,
};
use floti_protocol::{ClientChannels, EventChannel as _};

use super::config::off_the_workers;
use super::{Caller, Feature, FeatureContext};
use crate::agent_definitions::editing::{
    self, AgentEditor, FileState, READ_ONLY, canonical, unused_lines,
};
use crate::agent_definitions::folder::NOT_TEXT;
use crate::agent_definitions::parse::{DefinitionProblem, parse_agent, read_agent_file};
use crate::agent_definitions::seed::SHIPPED_AGENTS;
use crate::agent_definitions::watch::AgentsWatch;
use crate::agent_definitions::{
    AgentDefinition, DEFAULT_AGENT_NAME, FolderDefinitions, is_default_name,
};
use crate::server::events::Events;
use crate::server::hosted::Hosted;
use crate::session::ClientId;
use crate::store::Store;

/// What a host whose agents come from no folder says: only a test's host is
/// one.
const NO_FOLDER: &str =
    "This host reads its agents from no folder, so it has none to list or change.";

/// Who listens on `agents:changed`, and the watch kept while anyone does.
#[derive(Default)]
struct Watching {
    listeners: BTreeSet<ClientId>,
    watch: Option<AgentsWatch>,
}

pub(crate) struct Agents {
    hosted: Arc<Hosted>,
    store: Arc<Mutex<Store>>,
    events: Arc<Events>,
    /// Whether the host's config folder is another host's to write.
    read_only: bool,
    /// Where the watch runs; `None` outside a runtime, where nothing is
    /// watched.
    runtime: Option<tokio::runtime::Handle>,
    watching: Mutex<Watching>,
}

impl Agents {
    pub fn new(context: &FeatureContext) -> crate::Result<Self> {
        context.events.serve_events::<AgentsChanged>();
        Ok(Self {
            hosted: Arc::clone(&context.hosted),
            store: context.hosted.ledger().shared_store(),
            events: Arc::clone(&context.events),
            read_only: context.workspace.run.config_read_only,
            runtime: tokio::runtime::Handle::try_current().ok(),
            watching: Mutex::new(Watching::default()),
        })
    }

    /// `work` on the folder the host reads its agents from now.
    fn with_folder<T>(
        &self,
        work: impl FnOnce(&FolderDefinitions) -> Result<T, String>,
    ) -> Result<T, String> {
        let source = self.hosted.agent_source();
        let folder = source.folder().ok_or_else(|| NO_FOLDER.to_owned())?;
        work(folder)
    }

    /// `work` with the folder's editor, which keeps what it replaces in the
    /// store. A change made is measured at once by a watch that is on.
    fn edit<T>(
        &self,
        work: impl FnOnce(&AgentEditor<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_folder(|folder| {
            let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            work(&AgentEditor::new(
                folder,
                store.connection(),
                self.read_only,
            ))
        })
    }

    fn change(
        &self,
        work: impl FnOnce(&AgentEditor<'_>) -> Result<AgentChange, String>,
    ) -> Result<AgentChange, String> {
        let change = self.edit(work)?;
        if change.changed
            && let Some(watch) = &self.watching().watch
        {
            watch.poke();
        }
        Ok(change)
    }

    fn watching(&self) -> MutexGuard<'_, Watching> {
        self.watching.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every agent the host has now.
    fn list(&self) -> Result<AgentsListing, String> {
        self.with_folder(|folder| {
            let mut listing = AgentsListing {
                folder: folder.dir().display().to_string(),
                read_only: self.read_only.then(|| READ_ONLY.to_owned()),
                agents: Vec::new(),
                bad: Vec::new(),
                shipped: Vec::new(),
            };
            if folder.serves_shipped() {
                listing.agents.push(self.summary(
                    &self.hosted.built_in_default_agent(),
                    None,
                    Vec::new(),
                ));
                for agent in shipped_agents() {
                    listing.agents.push(self.summary(&agent, None, Vec::new()));
                }
                listing.shipped = shipped_agents().iter().map(shipped_entry).collect();
                return Ok(listing);
            }
            let names = folder.file_names().map_err(|err| {
                format!(
                    "Floti could not read the agents folder {}: {err}",
                    folder.dir().display()
                )
            })?;
            // The default first, from its file when it has one, as a start
            // without an agent reads it. A file that is not an agent leaves
            // no default to list.
            if names.iter().any(|name| is_default_name(name)) {
                let file = folder
                    .file_of(DEFAULT_AGENT_NAME)
                    .unwrap_or_else(|| format!("{DEFAULT_AGENT_NAME}.md"));
                self.push_file(folder, DEFAULT_AGENT_NAME, &file, &mut listing);
            } else {
                listing.agents.push(self.summary(
                    &self.hosted.built_in_default_agent(),
                    None,
                    Vec::new(),
                ));
            }
            for name in names.iter().filter(|name| !is_default_name(name)) {
                self.push_file(folder, name, &format!("{name}.md"), &mut listing);
            }
            listing.shipped = shipped_agents()
                .iter()
                .filter(|agent| !names.contains(&agent.name))
                .map(shipped_entry)
                .collect();
            Ok(listing)
        })
    }

    /// The agent file `file` of `name`, onto the listing as an agent or as a
    /// file that is not one.
    fn push_file(
        &self,
        folder: &FolderDefinitions,
        name: &str,
        file: &str,
        listing: &mut AgentsListing,
    ) {
        let name = canonical(name);
        let bad = |hash: Option<String>, problems: &[DefinitionProblem]| BadAgentFile {
            name: name.to_owned(),
            file: file.to_owned(),
            hash,
            problems: problems.iter().map(editing::problem).collect(),
        };
        match FileState::of(&folder.dir().join(file)) {
            // Gone since the folder was read.
            Ok(FileState::Absent) => {}
            Err(err) => listing.bad.push(bad(
                None,
                &[DefinitionProblem::whole(format!(
                    "it could not be read: {err}"
                ))],
            )),
            Ok(FileState::Present { text: None, hash }) => listing
                .bad
                .push(bad(Some(hash), &[DefinitionProblem::whole(NOT_TEXT)])),
            Ok(FileState::Present {
                text: Some(text),
                hash,
            }) => {
                let reading = read_agent_file(name, &text);
                match &reading.agent {
                    Ok(agent) => {
                        let unused = unused_lines(agent, &reading);
                        listing.agents.push(self.summary(
                            agent,
                            Some((file.to_owned(), hash)),
                            unused,
                        ));
                    }
                    Err(problems) => listing.bad.push(bad(Some(hash), problems)),
                }
            }
        }
    }

    fn summary(
        &self,
        agent: &AgentDefinition,
        file: Option<(String, String)>,
        unused_lines: Vec<AgentUnusedLine>,
    ) -> AgentSummary {
        let (file, hash) = file.unzip();
        AgentSummary {
            name: agent.name.clone(),
            description: agent.description.clone(),
            file,
            hash,
            backends: self.hosted.agent_runs_on(agent),
            worktree: agent.worktree,
            can_start: agent.can_start,
            unused_lines,
        }
    }

    /// `client` listens on `agents:changed` now, or no longer: the folder is
    /// watched while anyone does.
    fn listening(&self, client: ClientId, listens: bool) {
        let mut watching = self.watching();
        if listens {
            watching.listeners.insert(client);
        } else {
            watching.listeners.remove(&client);
        }
        if watching.listeners.is_empty() {
            watching.watch = None;
            return;
        }
        if watching.watch.is_some() {
            return;
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        let source = self.hosted.agent_source();
        let Some(folder) = source.folder() else {
            return;
        };
        let events = Arc::clone(&self.events);
        watching.watch = Some(AgentsWatch::start(
            folder.dir(),
            runtime,
            Arc::new(move |names| {
                if let Err(err) = events.emit::<AgentsChanged>(&AgentsChange { names }) {
                    log_line!("floti host: {err}");
                }
            }),
        ));
    }
}

/// The shipped agents, parsed, by name.
fn shipped_agents() -> Vec<AgentDefinition> {
    SHIPPED_AGENTS
        .iter()
        .filter_map(|(file, text)| parse_agent(file.strip_suffix(".md")?, text).ok())
        .collect()
}

fn shipped_entry(agent: &AgentDefinition) -> ShippedAgent {
    ShippedAgent {
        name: agent.name.clone(),
        description: agent.description.clone(),
    }
}

impl Feature for Agents {
    fn requests<'a>(&'a self, _caller: Caller, router: &mut Router<'a>) {
        router
            .handle::<ListAgents>(|()| off_the_workers(|| self.list()))
            .handle::<ReadAgent>(|request| {
                off_the_workers(|| self.edit(|editor| editor.read(&request.name)))
            })
            .handle::<WriteAgent>(|request| {
                off_the_workers(|| self.change(|editor| editor.write(&request)))
            })
            .handle::<DeleteAgent>(|request| {
                off_the_workers(|| self.change(|editor| editor.delete(&request)))
            })
            .handle::<RestoreAgent>(|request| {
                off_the_workers(|| self.change(|editor| editor.restore(&request.name)))
            })
            .handle::<AddShippedAgent>(|request| {
                off_the_workers(|| self.change(|editor| editor.add_shipped(&request.name)))
            });
    }

    fn subscriptions_changed(&self, client: ClientId, channels: &ClientChannels<'_>) {
        let listens = !channels.subscriptions_to(AgentsChanged::NAME).is_empty();
        self.listening(client, listens);
    }

    fn client_left(&self, client: ClientId) {
        self.listening(client, false);
    }
}
