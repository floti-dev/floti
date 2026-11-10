//! Payload types more than one domain uses.
//!
//! Each mirrors its type in `packages/core/src/types.ts`; where that type is
//! Claude-specific, the mirror is backend-neutral and says which backend it is
//! about.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::backend::BackendId;
use crate::channel::Catalogue;
use crate::channels::sessions::EffortLevel;

pub fn declare(catalogue: &mut Catalogue) {
    catalogue
        .payload::<SessionRecord>()
        .payload::<PermissionMode>();
}

/// How a hosted session ended, or that it has not.
///
/// `lost` is a row that was still `running` when the process that owned it
/// went away: a crash, a kill, a power cut. Stamping an end time later would
/// invent a duration nobody measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    Exited,
    Lost,
}

/// One hosted agent process, from spawn to exit.
// Its serde derives are `remote = "Self"`, which makes them this type's own
// functions, and the `Serialize` and `Deserialize` impls below wrap them: a
// record is written and read with `agent` and the retired `role` holding one
// value, whichever of the two the other side sent. `schemars(rename)` keeps
// the schema's name, which `remote` would otherwise take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", remote = "Self")]
#[schemars(rename = "SessionRecord")]
pub struct SessionRecord {
    #[ts(type = "number")]
    pub id: u64,
    /// The name handed to the agent, so the session can be found in its own
    /// resume list later. Never rewritten; a rename writes `label`.
    pub name: String,
    /// What Floti calls it on screen, or null for "use `name`".
    pub label: Option<String>,
    /// What the session is doing, in its agent's words (`session_title`): one
    /// line of at most 60 characters, shown beside its name. A name the user
    /// gives wins over it. Absent until the agent sets one, and from a host
    /// before protocol 2.2, which reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    pub cwd: String,
    /// The branch `cwd` was on when this was spawned. Null for a folder that
    /// is not a repository, a detached HEAD, or a read that failed.
    pub branch: Option<String>,
    /// The discovered project it was launched against, if it was one.
    pub project_path: Option<String>,
    /// The profile it was launched from, if it was one. The session outlives
    /// a deleted profile.
    #[ts(type = "number | null")]
    pub profile_id: Option<u64>,
    /// Arguments after the executable, as spawned. A command child's holds the
    /// shell program it ran through first, then its arguments.
    pub argv: Vec<String>,
    /// The agent's own id for the conversation this session is having, or
    /// null. Assigned at launch where the backend lets Floti pick it (Claude
    /// Code's `--session-id`); a resumed session carries the id it resumed.
    pub conversation_id: Option<String>,
    /// The agent backend running it. Claude Code is `claude-code`.
    pub backend: BackendId,
    pub status: SessionStatus,
    /// ISO 8601.
    pub started_at: String,
    /// ISO 8601, or null while running and for a lost session.
    pub ended_at: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    /// Null while running, and for a session whose exit code was never
    /// observed. The code as the OS reports it, wrapped to a signed 32-bit
    /// value: Windows' 0xC0000005 reads -1073741819, as PowerShell's
    /// `$LASTEXITCODE` says.
    pub exit_code: Option<i32>,
    /// The session that started this one with an agent tool, as its row id,
    /// or null for a session a person started. A child that outlived its
    /// parent keeps naming it. Absent from a host before protocol 2.1, which
    /// reads as null.
    #[serde(default)]
    #[ts(type = "number | null", optional = nullable)]
    pub parent_id: Option<u64>,
    /// What the parent is called on screen (its label, else its name), or
    /// null for a session with no parent.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub parent_name: Option<String>,
    /// The agent the session was started as (a lead, a worker, a reviewer),
    /// or null for a session started as none: one a person started, and a
    /// command child. Absent from a host before protocol 2.3, which says it
    /// as `role`; a record read from one has it here all the same.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub agent: Option<String>,
    /// Retired: the same as `agent`, under the name it had before protocol
    /// 2.3. Still written with `agent`'s value so that a client before 2.3
    /// reads what it did, and read in place of `agent` from one that sends
    /// only this. It leaves the protocol at the next major version.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub role: Option<String>,
    /// The command line a command child runs, as its parent gave it after
    /// `--`, or null for every other session. A command child's exit code is
    /// `exit_code`, once it has exited. Absent from a host before protocol
    /// 2.1, which reads as null.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub command: Option<String>,
    /// Whether the host ended the session itself - a client's Stop or End, an
    /// agent's stop, its parent ending, a quit that ends it - and not the
    /// program, which exited on its own. Marked before the process exits, so
    /// a finished child reads "Stopped" or "Done". Absent reads false, as it
    /// does from a host before protocol 2.1.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub stopped: bool,
    /// How the session ended: its program exited by itself, or who stopped
    /// it. Set on every session that has exited; absent while it runs, for a
    /// lost session, and from a host before protocol 2.2, which reads as not
    /// known (`stopped` still says whether the host ended it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ended_by: Option<EndedBy>,
    /// Whether this child, stopped, stays under its parent in every client's
    /// Sessions tree and family view: its parent stopped it with `keep`, or a
    /// person stopped it. Any other ended child leaves them. Cleared when the
    /// child is resumed and when a person removes it (`session:remove`). Set
    /// only on a child that has exited; absent reads false, as it does from a
    /// host before protocol 2.2.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub kept: bool,
    /// What this child last told the session that started it with
    /// `agents_report`, until its next turn starts. Absent when it has told
    /// nothing since, for a session with no parent, and from a host before
    /// this field, which reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_report: Option<ChildReport>,
    /// Whether this child was started to keep running when its parent ends.
    /// Absent reads false, as it does for a session with no parent and from a
    /// host before protocol 2.1.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub outlive: bool,
    /// The permission mode the session was launched in, or absent for a
    /// session launched with none and from a host before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<PermissionMode>,
    /// What the session was launched with beyond its agent, profile and
    /// permission mode, which have fields of their own (protocol 2.3): the
    /// sessions pane's Launched as. Absent for a plain command line, a row
    /// written before it, and from a host before 2.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub launched_as: Option<LaunchedAs>,
}

/// What a session was launched with, as its launch composed it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct LaunchedAs {
    /// The agent file it was composed from, as the host spells its path;
    /// null for no agent, the default the host makes from its backends, and
    /// an agent a host serves with no agents folder.
    pub agent_file: Option<String>,
    /// The model it runs with, or null for the backend's own.
    pub model: Option<String>,
    /// The effort it runs with, or null for the backend's own.
    pub effort: Option<EffortLevel>,
    /// The tools its agent lets it use without asking.
    pub allowed_tools: Vec<String>,
    /// The tools its agent never lets it use.
    pub disallowed_tools: Vec<String>,
    /// The agent tool families its launch was given, such as `floti-agents`.
    pub tools: Vec<String>,
    /// Whether it works in an agent worktree Floti made for it.
    pub worktree: bool,
}

impl SessionRecord {
    /// This record with `agent` and the retired `role` holding the same
    /// value: `agent`'s, else `role`'s from a side before protocol 2.3.
    fn with_one_agent(mut self) -> Self {
        if self.agent.is_none() {
            self.agent.clone_from(&self.role);
        } else if self.role != self.agent {
            self.role.clone_from(&self.agent);
        }
        self
    }
}

impl Serialize for SessionRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.role == self.agent {
            SessionRecord::serialize(self, serializer)
        } else {
            SessionRecord::serialize(&self.clone().with_one_agent(), serializer)
        }
    }
}

impl<'de> Deserialize<'de> for SessionRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SessionRecord::deserialize(deserializer).map(SessionRecord::with_one_agent)
    }
}

/// How a session that has exited came to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum EndedBy {
    /// Its program exited by itself; `exit_code` is its own.
    Exited,
    /// The session that started it stopped it (`agents_stop`,
    /// `floti agent stop`).
    Parent,
    /// A person stopped it, from any device: a tab's End or Stop, or a quit
    /// that ends what the device started.
    Person,
    /// Floti ended it: the session that started it ended, or the host was
    /// stopped.
    Floti,
}

/// A child's report to its parent: the words it was sent with, as its card
/// shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChildReport {
    /// `done`, `stuck` or `note`.
    pub state: String,
    /// The report's text, up to 4000 characters.
    pub text: String,
    /// ISO 8601.
    pub at: String,
}

/// How a session asks before it acts. The values are the ones the launcher
/// offers; each backend maps them to its own flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    AcceptEdits,
    Auto,
    BypassPermissions,
    Manual,
    DontAsk,
    Plan,
}
