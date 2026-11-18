//! What a host can say about itself with no window: the part of
//! `desktop_status` the host answers, and what `floti doctor` reports of
//! the host it reaches on this computer.
//!
//! [`host_status`] puts it together from what the host already serves:
//! `app:info` (its version and folders), `setup:status` (each agent CLI's
//! version against its tested range, and its sign-in as the backend's own
//! check judges it: an artefact existing, or Unknown, with nothing under the
//! credential paths opened), `plugins:list` (each plugin's load state and
//! error), `settings:read` (the switches and the five agent limits) and, in
//! the host itself, its agent tools listener (the families on and its
//! address). The host builds it in-process for `desktop_status`; `floti
//! doctor` asks the running host for the same answers over its connection
//! and builds it here too, so the two never say a thing two ways.
//!
//! [`lines`] is how it reads as text, one fact a line. Nothing in it is a
//! secret: no token, key or credential is among its sources.

use floti_protocol::Knowable;
use floti_protocol::channels::plugins::{PluginBackgroundState, PluginInfo};
use floti_protocol::channels::workspace::{BackendStatus, HostSettings, SetupStatus};
use serde::{Deserialize, Serialize};

/// What a host says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// The host's floti version.
    pub version: String,
    /// The protocol version it speaks.
    pub protocol: String,
    pub folders: StatusFolders,
    /// Each agent backend that has a CLI to find.
    pub agents: Vec<AgentCliStatus>,
    pub plugins: Vec<PluginStatus>,
    /// Its agent tools listener; absent where the answer did not come from
    /// inside the host (`floti doctor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_families: Option<ToolFamiliesStatus>,
    pub limits: AgentLimits,
}

/// Where the host keeps what it keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusFolders {
    pub data: String,
    /// The user's own files (agents, templates); absent where it was not
    /// told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    pub logs: String,
    pub host_log: String,
}

/// One agent backend's CLI, as setup finds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// What it answered its version command with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The first version it was tested with, and the first past the range;
    /// empty where the backend has no range.
    pub tested_from: String,
    pub tested_below: String,
    /// Its version is in the tested range.
    pub tested: bool,
    /// Whether it is signed in: known from an artefact existing, or Unknown.
    pub signed_in: Knowable<bool>,
    /// What the sign-in was judged from.
    pub sign_in_signal: String,
    /// Why it could not be found or asked, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// One plugin folder the user registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub path: String,
    /// The user's switch.
    pub on: bool,
    /// Its folder read and its manifest passed.
    pub loaded: bool,
    /// Why it did not load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether sessions are offered its tools; absent for a plugin with none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_on: Option<bool>,
    /// Its background module's state, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<PartStatus>,
    /// Its service's state, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<PartStatus>,
}

/// What one running part of a plugin is doing, and its last error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartStatus {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The host's agent tools listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFamiliesStatus {
    /// The families switched on now, plugins' among them.
    pub on: Vec<String>,
    /// Where it listens, while it holds a port; it binds at the first launch
    /// given a family and lets go when nothing holds a token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// What sessions may start, as Settings sets it: whether they may at all
/// (Floti host › Tools) and the limits (Floti host › Agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLimits {
    /// Sessions can start agents.
    pub sessions_can_start_agents: bool,
    pub children_per_parent: u32,
    pub sessions_per_host: u32,
    pub depth: u32,
    pub starts_per_parent: u32,
    pub commands_per_parent: u32,
}

/// What [`host_status`] is built from.
pub struct StatusParts<'a> {
    pub version: &'a str,
    pub protocol: String,
    pub folders: StatusFolders,
    pub setup: &'a SetupStatus,
    /// What each backend is called, by its id; an id it has no name for is
    /// called by the id.
    pub backend_name: &'a dyn Fn(&str) -> Option<String>,
    pub plugins: &'a [PluginInfo],
    pub settings: &'a HostSettings,
    pub tool_families: Option<ToolFamiliesStatus>,
}

/// The status `parts` say, every list in a stable order: agents by id,
/// plugins as the user added them.
pub fn host_status(parts: &StatusParts<'_>) -> HostStatus {
    let settings = parts.settings;
    HostStatus {
        version: parts.version.to_owned(),
        protocol: parts.protocol.clone(),
        folders: parts.folders.clone(),
        agents: parts
            .setup
            .backends
            .iter()
            .map(|(id, status)| {
                agent_cli(
                    id.as_str(),
                    (parts.backend_name)(id.as_str()).unwrap_or_else(|| id.as_str().to_owned()),
                    status,
                )
            })
            .collect(),
        plugins: parts.plugins.iter().map(plugin).collect(),
        tool_families: parts.tool_families.clone(),
        limits: AgentLimits {
            sessions_can_start_agents: settings.session_agents,
            children_per_parent: settings.agent_children_per_parent,
            sessions_per_host: settings.agent_sessions_per_host,
            depth: settings.agent_depth,
            starts_per_parent: settings.agent_starts_per_parent,
            commands_per_parent: settings.agent_commands_per_parent,
        },
    }
}

fn agent_cli(id: &str, name: String, status: &BackendStatus) -> AgentCliStatus {
    AgentCliStatus {
        id: id.to_owned(),
        name,
        installed: status.installed,
        path: status.path.clone(),
        version: status.semver.clone().or_else(|| status.version.clone()),
        tested_from: status.tested_range.min.clone(),
        tested_below: status.tested_range.max.clone(),
        tested: status.tested,
        signed_in: status.signed_in.clone(),
        sign_in_signal: status.sign_in_signal.clone(),
        problem: status.error.clone(),
    }
}

fn plugin(info: &PluginInfo) -> PluginStatus {
    PluginStatus {
        id: info.id.clone(),
        name: info.name.clone(),
        path: info.path.clone(),
        on: info.enabled,
        loaded: info.error.is_none(),
        error: info.error.clone(),
        tools_on: info.agent.as_ref().map(|agent| agent.enabled),
        background: info.background.as_ref().map(|background| PartStatus {
            state: background_state(background.state).to_owned(),
            error: background.error.clone(),
        }),
        service: info.service.as_ref().map(|service| PartStatus {
            state: enum_word(&service.state),
            error: service.error.clone(),
        }),
    }
}

fn background_state(state: PluginBackgroundState) -> &'static str {
    match state {
        PluginBackgroundState::Stopped => "stopped",
        PluginBackgroundState::Starting => "starting",
        PluginBackgroundState::Running => "running",
        PluginBackgroundState::Crashed => "crashed",
    }
}

/// A unit enum's variant as its JSON spells it.
fn enum_word(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        _ => "unknown".to_owned(),
    }
}

/// `status` as text, one fact a line: versions, folders, each agent CLI,
/// each plugin, the tool families and the limits.
pub fn lines(status: &HostStatus) -> Vec<String> {
    let mut lines = vec![format!(
        "Floti host {}, protocol {}.",
        status.version, status.protocol
    )];
    let folders = &status.folders;
    lines.push(format!(
        "Folders: data {}{}, logs {} (host log {}).",
        folders.data,
        folders
            .config
            .as_deref()
            .map(|config| format!(", config {config}"))
            .unwrap_or_default(),
        folders.logs,
        folders.host_log
    ));
    if status.agents.is_empty() {
        lines.push("Agent CLIs: none to find.".to_owned());
    }
    lines.extend(status.agents.iter().map(agent_line));
    if status.plugins.is_empty() {
        lines.push("Plugins: none added.".to_owned());
    }
    lines.extend(status.plugins.iter().map(plugin_line));
    if let Some(families) = &status.tool_families {
        let on = if families.on.is_empty() {
            "none".to_owned()
        } else {
            families.on.join(", ")
        };
        let address = families.address.as_deref().map_or_else(
            || "holds no port now (it binds at the next launch given a family)".to_owned(),
            |address| format!("listens on {address}"),
        );
        lines.push(format!(
            "Agent tool families on: {on}; the listener {address}."
        ));
    }
    let limits = &status.limits;
    lines.push(format!(
        "Agents: sessions can start agents {}; at most {} children per parent, {} agent sessions on this host, {} levels deep, {} starts and {} commands per parent.",
        if limits.sessions_can_start_agents { "on" } else { "off" },
        limits.children_per_parent,
        limits.sessions_per_host,
        limits.depth,
        limits.starts_per_parent,
        limits.commands_per_parent,
    ));
    lines
}

fn agent_line(agent: &AgentCliStatus) -> String {
    let range = if agent.tested_from.is_empty() {
        "no tested range".to_owned()
    } else {
        format!(
            "tested from {} up to but not including {}",
            agent.tested_from, agent.tested_below
        )
    };
    let found = match (&agent.version, agent.installed) {
        (Some(version), true) => format!(
            "{version}{}, {} ({range})",
            agent
                .path
                .as_deref()
                .map(|path| format!(" at {path}"))
                .unwrap_or_default(),
            if agent.tested {
                "in the tested range"
            } else {
                "outside the tested range"
            }
        ),
        _ => format!(
            "not installed: {}",
            agent.problem.as_deref().map_or("it was not found", clause)
        ),
    };
    let signed_in = match agent.signed_in {
        Knowable::Known(true) => "signed in",
        Knowable::Known(false) => "not signed in",
        Knowable::Unknown => "sign-in unknown",
    };
    format!(
        "Agent CLI {}: {found}; {signed_in} ({}).",
        agent.name,
        clause(&agent.sign_in_signal)
    )
}

/// A sentence as a clause inside another: without its closing stop.
fn clause(sentence: &str) -> &str {
    sentence.trim().trim_end_matches('.')
}

fn plugin_line(plugin: &PluginStatus) -> String {
    let mut said = vec![if plugin.on { "on" } else { "off" }.to_owned()];
    match &plugin.error {
        Some(error) => said.push(format!("did not load: {}", clause(error))),
        None => said.push("loaded".to_owned()),
    }
    if let Some(tools) = plugin.tools_on {
        said.push(format!("tools {}", if tools { "on" } else { "off" }));
    }
    for (part, status) in [
        ("background", &plugin.background),
        ("service", &plugin.service),
    ] {
        if let Some(status) = status {
            said.push(match &status.error {
                Some(error) => format!("{part} {}: {}", status.state, clause(error)),
                None => format!("{part} {}", status.state),
            });
        }
    }
    let id = plugin
        .id
        .as_deref()
        .map(|id| format!(" ({id})"))
        .unwrap_or_default();
    format!("Plugin {}{id}: {}.", plugin.name, said.join(", "))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use floti_protocol::BackendId;
    use floti_protocol::channels::workspace::TestedRange;
    use serde_json::json;

    use super::*;
    use crate::workspace::settings::defaults;

    fn claude(signed_in: Knowable<bool>, tested: bool) -> BackendStatus {
        BackendStatus {
            installed: true,
            path: Some("C:\\Program Files\\claude.exe".to_owned()),
            source: None,
            version: Some("2.1.225 (Claude Code)".to_owned()),
            semver: Some("2.1.225".to_owned()),
            tested,
            tested_range: TestedRange {
                min: "2.1.200".to_owned(),
                max: "2.2.0".to_owned(),
            },
            config_dir: None,
            config_dir_exists: true,
            signed_in,
            sign_in_signal: ".credentials.json exists".to_owned(),
            error: None,
        }
    }

    fn plugins() -> Vec<PluginInfo> {
        let loaded: PluginInfo = serde_json::from_value(json!({
            "path": "C:\\plugins\\trackr", "id": "trackr", "name": "Trackr", "version": "1.0.0",
            "description": null, "enabled": true, "error": null, "warnings": [], "revision": 1,
            "icon": null, "rail": null, "panels": {}, "tabs": {}, "pageStrip": false,
            "background": { "url": "floti-plugin://trackr/bg.js", "state": "crashed", "error": "boom" },
            "commands": [], "settings": [], "settingValues": {}, "network": [], "secrets": [],
            "exec": [], "service": null, "runsPrograms": false,
            "agent": { "enabled": false, "server": "floti-plugin-trackr", "instructions": null, "tools": [] },
            "startsSessions": false, "seesSessions": false, "status": null, "badge": null
        }))
        .expect("a plugin");
        let mut failed = loaded.clone();
        failed.path = "C:\\plugins\\broken".to_owned();
        failed.id = None;
        failed.name = "broken".to_owned();
        failed.enabled = false;
        failed.error = Some("There is no floti-plugin.json.".to_owned());
        failed.background = None;
        failed.agent = None;
        vec![loaded, failed]
    }

    fn status(families: Option<ToolFamiliesStatus>) -> HostStatus {
        let setup = SetupStatus {
            backends: BTreeMap::from([(
                BackendId::new("claude-code"),
                claude(Knowable::Unknown, true),
            )]),
        };
        let mut settings = defaults();
        settings.session_agents = false;
        settings.agent_depth = 2;
        let plugins = plugins();
        host_status(&StatusParts {
            version: "1.2.3",
            protocol: "4.3".to_owned(),
            folders: StatusFolders {
                data: "D:\\data dir".to_owned(),
                config: Some("C:\\config dir".to_owned()),
                logs: "D:\\logs dir".to_owned(),
                host_log: "D:\\logs dir\\host.log".to_owned(),
            },
            setup: &setup,
            backend_name: &|id| (id == "claude-code").then(|| "Claude Code".to_owned()),
            plugins: &plugins,
            settings: &settings,
            tool_families: families,
        })
    }

    #[test]
    fn it_takes_versions_folders_agents_plugins_and_limits_from_what_the_host_serves() {
        let status = status(None);
        assert_eq!(status.version, "1.2.3");
        assert_eq!(status.agents.len(), 1);
        let agent = &status.agents[0];
        assert_eq!(
            (agent.name.as_str(), agent.version.as_deref()),
            ("Claude Code", Some("2.1.225"))
        );
        assert_eq!(agent.signed_in, Knowable::Unknown);
        assert_eq!(status.plugins[0].tools_on, Some(false));
        assert_eq!(
            status.plugins[0].background,
            Some(PartStatus {
                state: "crashed".to_owned(),
                error: Some("boom".to_owned())
            })
        );
        assert!(!status.plugins[1].loaded);
        assert!(!status.limits.sessions_can_start_agents);
        assert_eq!(status.limits.depth, 2);
        // No listener outside the host: the JSON has no key for it.
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("toolFamilies").is_none(), "{value}");
        assert_eq!(value["agents"][0]["signedIn"]["state"], "unknown");
    }

    #[test]
    fn its_lines_say_each_fact_once_and_an_unbound_listener_says_when_it_binds() {
        let text = lines(&status(Some(ToolFamiliesStatus {
            on: vec!["floti-desktop".to_owned(), "floti-agents".to_owned()],
            address: None,
        })))
        .join("\n");
        for said in [
            "Floti host 1.2.3, protocol 4.3.",
            "config C:\\config dir",
            "host log D:\\logs dir\\host.log",
            "Agent CLI Claude Code: 2.1.225 at C:\\Program Files\\claude.exe, in the tested range (tested from 2.1.200 up to but not including 2.2.0); sign-in unknown (.credentials.json exists).",
            "Plugin Trackr (trackr): on, loaded, tools off, background crashed: boom.",
            "Plugin broken: off, did not load: There is no floti-plugin.json.",
            "Agent tool families on: floti-desktop, floti-agents; the listener holds no port now",
            "sessions can start agents off",
            "2 levels deep",
        ] {
            assert!(text.contains(said), "{said}\n---\n{text}");
        }
        let bound = lines(&status(Some(ToolFamiliesStatus {
            on: Vec::new(),
            address: Some("127.0.0.1:4100".to_owned()),
        })))
        .join("\n");
        assert!(
            bound.contains("Agent tool families on: none; the listener listens on 127.0.0.1:4100."),
            "{bound}"
        );
        // Outside the host there is no line for the listener at all.
        assert!(!lines(&status(None)).join("\n").contains("tool families"));
    }

    #[test]
    fn an_agent_cli_not_found_says_why() {
        let mut missing = claude(Knowable::Known(false), false);
        missing.installed = false;
        missing.version = None;
        missing.semver = None;
        missing.path = None;
        missing.error = Some("Claude Code was not found on PATH.".to_owned());
        let line = agent_line(&agent_cli(
            "claude-code",
            "Claude Code".to_owned(),
            &missing,
        ));
        assert_eq!(
            line,
            "Agent CLI Claude Code: not installed: Claude Code was not found on PATH; not signed in (.credentials.json exists)."
        );
    }
}
