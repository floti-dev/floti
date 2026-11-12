//! The host's own updates: `update:state`, what the host's check last found
//! and what an update it runs has reached; `update:check`, a check now; and
//! `update:install`, which runs `floti update` on the host's computer, the
//! desktop app's step included.
//!
//! The host is the one that contacts the release: it reads the signed
//! manifest at most once a day while its `updateCheck` setting is on, and
//! when a client asks. A client contacts nothing itself; it learns of a
//! newer release here, and compares the channel's newest version with its
//! own.
//!
//! `floti update` started here replaces floti and the desktop app on the
//! host's computer. It ends no session: the host relaunches through its
//! session keeper, and the app is replaced once it has quit. A desktop on
//! that computer quits by itself when the update says it is waiting for it
//! (`UpdateProgress::waiting_for_app`), keeping its sessions running, and
//! the update opens it again once it is replaced.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::channel::Catalogue;

pub fn declare(catalogue: &mut Catalogue) {
    catalogue
        .event::<UpdateStatus>()
        .request::<UpdateCheckNow>()
        .request::<UpdateInstall>();
}

event_channel! {
    /// What the host knows about updates now, whole on every change: a
    /// check starting or ending, and each step of an update it runs.
    UpdateStatus = "update:state", state UpdateState
}

request_channel! {
    /// Checks now, whatever the `updateCheck` setting says and however
    /// recently the host last did, and answers once the check has ended.
    /// When the channel has a newer floti, the check puts it in place as
    /// the background check does, and the host then relaunches on it, its
    /// sessions running on. A host that never checks
    /// ([`UpdateState::unavailable`]) answers its state without checking.
    UpdateCheckNow = "update:check", () => UpdateState
}

request_channel! {
    /// Runs `floti update` on the host's computer, for floti and the desktop
    /// app, and answers once it has started. It outlives the host and every
    /// client. Its steps are told on `update:state`. Starts nothing when one
    /// the host started still runs; refused with why when the host never
    /// updates ([`UpdateState::unavailable`]) or it could not be started.
    UpdateInstall = "update:install", () => ()
}

/// What the host knows about updates now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// The floti this host runs.
    pub running: String,
    /// The channel it follows: `stable` unless `floti update --channel`
    /// chose another.
    pub channel: String,
    /// Why this host neither checks nor updates; null when it does.
    pub unavailable: Option<UpdateUnavailable>,
    /// The channel's newest version, as the last check that read the
    /// manifest found it; null when none has.
    pub newest: Option<String>,
    /// When that check read the manifest, by the host's clock, as RFC 3339
    /// text; null when none has.
    pub checked_at: Option<String>,
    /// Why the last check failed, as one sentence; null when it did not, or
    /// none has run since the host started.
    pub failed: Option<String>,
    /// Whether a check runs now.
    pub checking: bool,
    /// The `floti update` this host started, while it runs.
    pub installing: Option<UpdateProgress>,
    /// Why the last `floti update` this host started failed, as the line it
    /// ended with; null when it did not, until the next one starts.
    pub install_failed: Option<String>,
}

/// Why a host neither checks for updates nor updates anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum UpdateUnavailable {
    /// Its config folder is another host's to write: a development run or
    /// a check, which never touches what the installed host reads.
    ReadOnly,
    /// It does not run floti installed on its own: a development build, a
    /// floti inside a portable app, or one started from anywhere else.
    NotStandalone,
}

/// Where a `floti update` the host started has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// The last line it said, as a sentence.
    pub line: String,
    /// Whether everything is downloaded and checked and it waits for the
    /// desktop app to quit, to replace it.
    pub waiting_for_app: bool,
}
