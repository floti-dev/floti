//! The agents a host's sessions start children as: `agents:list`,
//! `agents:read`, `agents:write`, `agents:delete`, `agents:restore`,
//! `agents:addShipped` and `agents:changed`.
//!
//! An agent is a `<name>.md` file in the agents folder of the host's config
//! folder, read afresh by every start. A client sees and edits the agents of
//! the host it asks: a host on another computer answers with its own folder.
//!
//! - A write is parsed by the parser a start uses before anything is
//!   written: text the parser refuses is refused with its problems, each
//!   naming the field and line it concerns, and the file is left as it was.
//!   A write or a delete whose `expectedHash` is not the file's hash now is
//!   refused as a conflict, as `content:write` is, so nothing a client did
//!   not read is written over or deleted.
//! - Replacing, renaming or deleting a file first keeps its old text in the
//!   host's data folder, never in its config folder, and `agents:restore`
//!   puts back the newest, which is a client's Undo.
//! - A host whose config folder is another host's to write (a development or
//!   check run) lists, reads and watches, and refuses every change with a
//!   sentence saying so.
//! - `agents:changed` is sent to every subscriber while one listens, for any
//!   change to the folder, the host's own and an outside editor's alike.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::channel::Catalogue;
use crate::channels::common::PermissionMode;
use crate::channels::sessions::EffortLevel;

pub fn declare(catalogue: &mut Catalogue) {
    catalogue
        .request::<ListAgents>()
        .request::<ReadAgent>()
        .request::<WriteAgent>()
        .request::<DeleteAgent>()
        .request::<RestoreAgent>()
        .request::<AddShippedAgent>()
        .event::<AgentsChanged>();
}

request_channel!(
    /// Every agent the host has now, the default first, then the files in its
    /// agents folder that are not agents, and the shipped agents whose file
    /// the folder does not have.
    ListAgents = "agents:list", () => AgentsListing
);
request_channel!(
    /// One agent's file: its whole text, its hash, each value as written, the
    /// fields as parsed and the body. A file that is not an agent still
    /// answers its text and values, with its problems.
    ReadAgent = "agents:read", AgentName => AgentFile
);
request_channel!(
    /// Creates or replaces an agent's file with a whole text, or renames one
    /// to it, once the text parses as an agent and the file is what the
    /// client read.
    WriteAgent = "agents:write", WriteAgentRequest => AgentChange
);
request_channel!(
    /// Removes an agent's file, keeping its text first, once the file is
    /// what the client read.
    DeleteAgent = "agents:delete", DeleteAgentRequest => AgentChange
);
request_channel!(
    /// Undoes the newest change kept for an agent: puts back the text a
    /// replace, a rename or a delete kept, or removes the file a create made.
    /// Refused, changing nothing, when the file changed since.
    RestoreAgent = "agents:restore", AgentName => AgentChange
);
request_channel!(
    /// Writes a shipped agent the folder does not have, never over a file
    /// that is there.
    AddShippedAgent = "agents:addShipped", AgentName => AgentChange
);
event_channel!(
    /// The agents folder changed: a file came, went or has other text.
    AgentsChanged = "agents:changed", event AgentsChange
);

/// Every agent a host has at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentsListing {
    /// The agents folder, as the host spells it.
    pub folder: String,
    /// Why this host changes nothing in its agents folder, as a sentence;
    /// null when it may.
    pub read_only: Option<String>,
    /// The agents, the default first, then by name.
    pub agents: Vec<AgentSummary>,
    /// The files in the folder that are not agents, by name.
    pub bad: Vec<BadAgentFile>,
    /// The shipped agents the host would add, by name: those whose file the
    /// folder does not have and that `agents` does not list already.
    pub shipped: Vec<ShippedAgent>,
}

/// One agent, as a listing shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub name: String,
    pub description: String,
    /// The file it is read from, such as `reviewer.md`; null for the default
    /// the host makes from its backends, and for a shipped agent a host
    /// serves while it has no agents folder.
    pub file: Option<String>,
    /// The file's hash, as `agents:read` gives it; null where there is no
    /// file.
    pub hash: Option<String>,
    /// The installed backends it can run on, in the agent's order.
    pub backends: Vec<String>,
    pub worktree: bool,
    pub can_start: bool,
    /// The lines its file has that Floti no longer reads, in the file's
    /// order.
    pub unused_lines: Vec<AgentUnusedLine>,
    /// Its fields, as a start reads them: what a launcher fills its pickers
    /// with when a person starts a session as it.
    pub fields: AgentFields,
    /// Why none of the host's backends can run it, as a sentence, when
    /// `backends` is empty: none it lists is installed, or none that is can
    /// enforce its rules. Null when one can.
    pub cannot_run: Option<String>,
}

/// A line an agent file has that Floti no longer reads: accepted and
/// ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentUnusedLine {
    pub field: String,
    /// The file's line, from 1.
    pub line: u32,
}

/// A file in the agents folder that is not an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BadAgentFile {
    /// The agent it would be, by its file's name.
    pub name: String,
    /// The file, such as `reviewer.md`.
    pub file: String,
    /// The file's hash; null when it could not be read.
    pub hash: Option<String>,
    /// Every problem with it.
    pub problems: Vec<AgentProblem>,
}

/// One problem with an agent file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentProblem {
    /// The field it concerns, as the file spells it; null for a problem with
    /// the whole file, such as front matter that does not parse.
    pub field: Option<String>,
    /// The file's line it concerns, from 1; null when it concerns no one
    /// line, such as a file that could not be read.
    pub line: Option<u32>,
    /// What is wrong, as a sentence fragment.
    pub message: String,
}

/// A shipped agent, as the host would write it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShippedAgent {
    pub name: String,
    pub description: String,
}

/// An agent, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentName {
    pub name: String,
}

/// One agent's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentFile {
    pub name: String,
    /// The file, such as `reviewer.md`.
    pub file: String,
    /// Whether the text is the folder's file; false for a shipped agent a
    /// host serves while it has no agents folder.
    pub in_folder: bool,
    /// The whole text.
    pub text: String,
    /// The SHA-256 of the file's bytes, as lowercase hex: what a write or a
    /// delete names as the text it was based on.
    pub hash: String,
    /// Each value of the front matter as written, in the file's order, with
    /// the line its field is on, whether or not the file is an agent.
    pub values: Vec<AgentWrittenValue>,
    /// The fields as a start reads them; null when the file is not an agent.
    pub fields: Option<AgentFields>,
    /// The text after the front matter: the agent's instructions.
    pub body: String,
    /// Every problem with it; empty when it is an agent.
    pub problems: Vec<AgentProblem>,
    pub unused_lines: Vec<AgentUnusedLine>,
}

/// One field's value as the file writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentWrittenValue {
    /// The field, as the file spells it.
    pub field: String,
    /// The line it is on, from 1.
    pub line: u32,
    /// What follows the field's colon, and the indented lines under it with
    /// their common indent taken off, trimmed.
    pub value: String,
}

/// An agent's fields, as a start reads them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentFields {
    pub description: String,
    pub profile: Option<String>,
    /// The backends that may run its children, in order; empty for every
    /// installed agent backend.
    pub backends: Vec<String>,
    /// The model each backend runs it with, by backend id.
    pub models: BTreeMap<String, String>,
    /// The effort each backend runs it with, by backend id.
    pub efforts: BTreeMap<String, EffortLevel>,
    pub permission: Option<PermissionMode>,
    pub worktree: bool,
    pub can_start: bool,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    /// For a plain command line: the command, and the keys that submit a
    /// task and stop it.
    pub command: Option<String>,
    pub submit: Option<String>,
    pub stop: Option<String>,
}

/// A whole text to write under an agent's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WriteAgentRequest {
    pub name: String,
    pub text: String,
    /// The hash `agents:read` gave the text was based on, or null for an
    /// agent the client believes has no file: a file there now is a
    /// conflict. For a rename, the hash of the file it renames.
    #[serde(default)]
    pub expected_hash: Option<String>,
    /// The agent this one is renamed from: its file goes once `name`'s is
    /// written, and its text is kept first. A file already called `name` is
    /// never written over by a rename.
    #[serde(default)]
    pub rename_from: Option<String>,
}

/// An agent's file to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAgentRequest {
    pub name: String,
    /// The hash `agents:read` or `agents:list` gave the file the client
    /// means to delete. A missing or stale one is a conflict and deletes
    /// nothing.
    #[serde(default)]
    pub expected_hash: Option<String>,
}

/// What a write, a delete, a restore or an add came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentChange {
    /// Whether the folder changed. False with no refusal when it already
    /// held what was asked.
    pub changed: bool,
    /// The agent's file as it is now: after the change, or as it stands when
    /// the change was refused; null when there is none.
    pub file: Option<AgentFile>,
    /// Why nothing changed, as a sentence; null when the change was made or
    /// nothing needed to.
    pub refusal: Option<String>,
    /// The parser's problems with the text, when they are why it was
    /// refused.
    pub problems: Vec<AgentProblem>,
    /// Set when the file is not what the client's change was based on.
    pub conflict: Option<AgentConflict>,
}

/// The file on disk, which is not what the client's change was based on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentConflict {
    /// The file's hash now; null when there is no file.
    pub on_disk_hash: Option<String>,
    /// The file's text now; null when there is no file.
    pub on_disk_text: Option<String>,
}

/// What changed in the agents folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentsChange {
    /// The agents whose files came, went or have other text, by the files'
    /// names without `.md`, sorted.
    pub names: Vec<String>,
}
