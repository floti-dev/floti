//! The floti-desktop tools' relay: `agentDesktop:*`.
//!
//! The host serves floti-desktop to its sessions, but Floti's views (a file's
//! tab, Files, a project page, History, Settings) are a window's. So a tool
//! call travels host to client as an event on `agentDesktop:call`, and the
//! client answers it on `agentDesktop:answer`. Both need the `desktop`
//! capability: a client that did not declare it is never sent a call and
//! cannot answer one.
//!
//! A call names its tool and carries its arguments as JSON, so every tool of
//! the family travels the same way and the window dispatches by the tool's
//! name. What `desktop_open` carries is a [`DesktopOpen`]: the host has
//! already resolved its path against the caller's folder and checked it is
//! under a folder it knows, so the window opens what it is told.
//!
//! The host picks one client per call, from the clients that declared
//! `desktop` and listen on `agentDesktop:call` ([`listens_for_desktop_calls`]).
//! With none attached, the tool answers that no window is attached
//! ([`AgentDesktopOutcome::no_window`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::channel::{Catalogue, EventChannel};
use crate::delivery::ClientChannels;
use crate::message::Payload;

pub fn declare(catalogue: &mut Catalogue) {
    catalogue
        .event::<AgentDesktopCalls>()
        .request::<AgentDesktopAnswer>()
        .payload::<DesktopOpen>();
}

event_channel!(
    /// A floti-desktop tool call from a session's agent, for the client to
    /// carry out in its window.
    AgentDesktopCalls = "agentDesktop:call",
    event AgentDesktopToolCall,
    needs Desktop
);

request_channel!(
    /// The client's answer to one floti-desktop tool call.
    AgentDesktopAnswer = "agentDesktop:answer",
    AgentDesktopToolAnswer => (),
    needs Desktop
);

/// One floti-desktop tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDesktopToolCall {
    /// Chosen by the host; the answer names it.
    #[ts(type = "number")]
    pub call_id: u64,
    /// The id of the hosted session whose agent called the tool.
    #[ts(type = "number")]
    pub session: u64,
    /// The tool's name, such as `desktop_open`.
    pub tool: String,
    /// The tool's arguments as the host passes them on: for `desktop_open`
    /// a [`DesktopOpen`].
    #[schemars(with = "serde_json::Value")]
    #[ts(type = "unknown")]
    pub arguments: Payload,
}

/// The client's answer to a floti-desktop tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentDesktopToolAnswer {
    /// The `callId` of the call it answers.
    #[ts(type = "number")]
    pub call_id: u64,
    pub outcome: AgentDesktopOutcome,
}

/// What a floti-desktop tool call came to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AgentDesktopOutcome {
    /// The tool ran; `value` is its result for the agent, an MCP tool result.
    Done {
        #[schemars(with = "serde_json::Value")]
        #[ts(type = "unknown")]
        value: Payload,
    },
    /// The tool could not run; `message` is the sentence the agent reads.
    Failed { message: String },
}

impl AgentDesktopOutcome {
    /// The answer to a call made while no client with Floti's window is
    /// attached.
    pub fn no_window() -> Self {
        Self::Failed {
            message: "No window is attached: Floti's window is not connected to the host this \
                      session runs in, so there is nothing to open it in. Open Floti, then try \
                      again."
                .into(),
        }
    }
}

/// What `desktop_open` asks the window to show, with every path already
/// resolved, made absolute and checked by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "view", rename_all = "camelCase")]
pub enum DesktopOpen {
    /// A file's tab beside the caller's, scrolled to `line` when one is
    /// given. `root` is the known folder holding it.
    #[serde(rename_all = "camelCase")]
    File {
        root: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        line: Option<u32>,
    },
    /// The Files view on `root`, the known folder holding `path`, with the
    /// folder `path` revealed.
    #[serde(rename_all = "camelCase")]
    Folder { root: String, path: String },
    /// A project's page beside the caller's tab: `path` is the project's
    /// folder as discovery knows it.
    #[serde(rename_all = "camelCase")]
    Project { path: String },
    /// The History palette with `search` typed into it.
    #[serde(rename_all = "camelCase")]
    History { search: String },
    /// A Settings tab beside the caller's, on `section` when one is given.
    #[serde(rename_all = "camelCase")]
    Settings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        section: Option<String>,
    },
}

/// Whether `client` takes floti-desktop calls: it declared `desktop` and
/// listens on `agentDesktop:call`.
pub fn listens_for_desktop_calls(client: &ClientChannels<'_>) -> bool {
    !client.subscriptions_to(AgentDesktopCalls::NAME).is_empty()
}
