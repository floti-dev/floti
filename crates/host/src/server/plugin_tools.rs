//! `floti-plugin-<id>`: each plugin's tool server, served to the host's
//! sessions at `/mcp/plugin/<id>` ([`HostPluginTools`]).
//!
//! A plugin whose background module runs in the host's engine, on,
//! error-free and with its tools on, is served from its manifest with no
//! client attached: the provenance line then the manifest's instructions,
//! and each tool's name, description, input schema and `readOnlyHint` as the
//! manifest declares them. Each call goes to the module's handler through
//! [`crate::plugins::Registry::call_tool`], naming the session (the id
//! minted at its launch, its name and its folder), and dropping the call
//! cancels it there. The list of plugins is read again at every change the
//! registry publishes: a plugin turned off, removed or with its tools
//! switched off loses its route at once (404) and is given to no later
//! launch, and is given again once it is back.
//!
//! A tool listed read-only is turned into an allow rule by the listener for
//! the launches given it.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use floti_protocol::channel::from_payload;
use floti_protocol::channels::agent_plugin::AgentPluginCaller;
use floti_protocol::channels::plugins::PluginInfo;

use crate::agent_tools::{
    CallToolResult, Caller, ContentBlock, FamilySource, JsonObject, Tool, ToolAnnotations,
    ToolFamily, ToolFuture,
};
use crate::plugins::WeakRegistry;
use crate::plugins::background::{ToolAnswer, ToolCall, ToolCaller};
use crate::plugins::call::CancelToken;

/// Where a plugin's server is served.
const ROUTE_PREFIX: &str = "/mcp/plugin/";

/// The answer to a call from a token whose session has not started yet.
const NO_SESSION_YET: &str = "Floti cannot tell which session this call is from yet - it is still starting. Try again in a moment.";

/// One plugin tool server, checked and ready to list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginServer {
    /// Its name as a session lists it.
    pub name: String,
    /// The plugin's id.
    pub plugin: String,
    /// `/mcp/plugin/<plugin>`.
    pub route: String,
    pub instructions: String,
    /// Its tools, each read-only one marked with `readOnlyHint`.
    pub tools: Vec<Tool>,
}

impl PluginServer {
    /// The server the host serves for `plugin`, worded from its manifest:
    /// for a plugin on, error-free and with its tools on; `None` otherwise.
    pub fn hosted(plugin: &PluginInfo) -> Option<Self> {
        if !tools_served(plugin) {
            return None;
        }
        let id = plugin.id.clone()?;
        let agent = plugin.agent.as_ref()?;
        let tools = agent
            .tools
            .iter()
            .map(|tool| {
                // The manifest reader has held every schema to an object.
                let schema: JsonObject = from_payload(&tool.input_schema).unwrap_or_default();
                listing(
                    tool.name.clone(),
                    tool.description.clone(),
                    schema,
                    tool.read_only,
                )
            })
            .collect();
        Some(Self {
            name: agent.server.clone(),
            route: format!("{ROUTE_PREFIX}{id}"),
            instructions: plugin_instructions(&plugin.name, agent.instructions.as_deref()),
            plugin: id,
            tools,
        })
    }
}

/// Whether `plugin`'s tools are served at all: it is on, error-free and has
/// its tools on.
fn tools_served(plugin: &PluginInfo) -> bool {
    plugin.enabled
        && plugin.error.is_none()
        && plugin.agent.as_ref().is_some_and(|agent| agent.enabled)
}

/// The words a session reads first about a plugin's server: where its tools
/// come from, then the plugin's own instructions.
fn plugin_instructions(name: &str, instructions: Option<&str>) -> String {
    let provenance = format!(
        "These tools come from \"{name}\", a plugin the user added to Floti, the app hosting \
         this session. The plugin answers every call; Floti passes it on."
    );
    match instructions {
        Some(own) => format!("{provenance}\n\n{own}"),
        None => provenance,
    }
}

/// One tool as a session lists it, marked with `readOnlyHint` when it
/// changes nothing.
fn listing(name: String, description: String, schema: JsonObject, read_only: bool) -> Tool {
    let listing = Tool::new(name, description, schema);
    if read_only {
        listing.with_annotations(ToolAnnotations::new().read_only(true))
    } else {
        listing
    }
}

fn problem(body: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(body.to_owned())])
}

/// How a call's session is named to the plugin, by the session's row id.
type CallerOf = Box<dyn Fn(u64) -> Option<AgentPluginCaller> + Send + Sync>;

type Changed = Arc<dyn Fn() + Send + Sync>;

/// The answer to a call that reached a host whose plugins have stopped.
const REGISTRY_GONE: &str = "The host is shutting down.";

/// The tool servers of the plugins whose background module the host runs,
/// as its registry last listed them, and how a call reaches one.
pub(crate) struct HostPluginTools {
    servers: Mutex<Vec<Arc<PluginServer>>>,
    /// Set once the registry is built: it is built after this is.
    registry: OnceLock<WeakRegistry>,
    caller: CallerOf,
    /// Told, with no lock of this held, each time the servers changed.
    changed: Mutex<Option<Changed>>,
}

impl HostPluginTools {
    /// Serves nothing until [`Self::installed`] is told a list; names each
    /// call's session with `caller`.
    pub fn new(caller: impl Fn(u64) -> Option<AgentPluginCaller> + Send + Sync + 'static) -> Self {
        Self {
            servers: Mutex::new(Vec::new()),
            registry: OnceLock::new(),
            caller: Box::new(caller),
            changed: Mutex::new(None),
        }
    }

    /// Calls are answered by `registry` from now on.
    pub fn answer_with(&self, registry: WeakRegistry) {
        let _ = self.registry.set(registry);
    }

    /// Calls `changed` from now on each time the servers change, replacing
    /// whatever was called before.
    pub fn on_change(&self, changed: impl Fn() + Send + Sync + 'static) {
        *lock(&self.changed) = Some(Arc::new(changed));
    }

    /// The registry's list now: each plugin it runs a background module for,
    /// on, error-free and with its tools on, is served from here on, and
    /// every other is gone at once for the sessions given it.
    pub fn installed(&self, list: &[PluginInfo]) {
        let servers: Vec<PluginServer> = list.iter().filter_map(PluginServer::hosted).collect();
        {
            let mut now = lock(&self.servers);
            let same = now.len() == servers.len()
                && now.iter().zip(&servers).all(|(was, is)| **was == *is);
            if same {
                return;
            }
            *now = servers.into_iter().map(Arc::new).collect();
        }
        let changed = lock(&self.changed).clone();
        if let Some(changed) = changed {
            changed();
        }
    }

    /// Hands one call of `tool` with `arguments`, made by session `row`'s
    /// agent, to `plugin`'s background module, and answers what its handler
    /// answered. Dropping the future cancels the call there.
    async fn call(
        &self,
        row: u64,
        plugin: &str,
        tool: &str,
        arguments: JsonObject,
    ) -> CallToolResult {
        let Some(caller) = (self.caller)(row) else {
            return problem(NO_SESSION_YET);
        };
        let Some(registry) = self.registry.get().and_then(WeakRegistry::upgrade) else {
            return problem(REGISTRY_GONE);
        };
        let call = ToolCall {
            plugin: plugin.to_owned(),
            tool: tool.to_owned(),
            args: serde_json::Value::Object(arguments),
            session: ToolCaller {
                id: caller.id,
                name: caller.label,
                cwd: caller.folder,
            },
        };
        let cancel = CancelOnDrop(CancelToken::new());
        hosted_result(registry.call_tool(call, cancel.0.clone()).await)
    }
}

/// Cancels its token when dropped: the session no longer waits on the call.
struct CancelOnDrop(CancelToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// What the agent reads for a module's answer: its text, or the sentence it
/// failed with.
fn hosted_result(answer: ToolAnswer) -> CallToolResult {
    match answer {
        Ok(serde_json::Value::String(text)) => {
            CallToolResult::success(vec![ContentBlock::text(text)])
        }
        Ok(other) => CallToolResult::success(vec![ContentBlock::text(other.to_string())]),
        Err(message) => problem(&message),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every server the host serves for its own plugins now, as families.
pub(crate) struct HostPluginFamilies {
    tools: Arc<HostPluginTools>,
}

impl HostPluginFamilies {
    pub fn new(tools: Arc<HostPluginTools>) -> Self {
        Self { tools }
    }
}

impl FamilySource for HostPluginFamilies {
    fn families(&self) -> Vec<Arc<dyn ToolFamily>> {
        lock(&self.tools.servers)
            .iter()
            .map(|server| {
                Arc::new(HostPluginFamily {
                    server: Arc::clone(server),
                    tools: Arc::clone(&self.tools),
                }) as Arc<dyn ToolFamily>
            })
            .collect()
    }
}

/// One plugin's server, as its manifest declares it now.
struct HostPluginFamily {
    server: Arc<PluginServer>,
    tools: Arc<HostPluginTools>,
}

impl ToolFamily for HostPluginFamily {
    fn name(&self) -> &str {
        &self.server.name
    }

    fn route(&self) -> &str {
        &self.server.route
    }

    fn instructions(&self) -> &str {
        &self.server.instructions
    }

    /// On while it is listed: a plugin off, removed or with its tools off
    /// is not.
    fn is_on(&self) -> bool {
        true
    }

    fn tools(&self) -> Vec<Tool> {
        self.server.tools.clone()
    }

    fn call<'a>(&'a self, caller: Caller, name: &'a str, arguments: JsonObject) -> ToolFuture<'a> {
        Box::pin(async move {
            let Some(row) = caller
                .session
                .and_then(|session| u64::try_from(session.0).ok())
            else {
                return problem(NO_SESSION_YET);
            };
            self.tools
                .call(row, &self.server.plugin, name, arguments)
                .await
        })
    }
}
