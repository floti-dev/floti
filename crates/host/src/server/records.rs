//! The host's reads and writes of session rows beyond the spawn and exit the
//! session module records: reconciling what a crash or an update left
//! running, recording a deliberate stop, and reading rows back as the
//! protocol's [`SessionRecord`].
//!
//! All of it is today's `sessions` table, whose `backend` column names the
//! agent backend each session ran on ([`recorded_backend`]).

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::BackendId;
use floti_protocol::channels::common::{EndedBy, PermissionMode, SessionRecord, SessionStatus};
use floti_protocol::channels::sessions::{HistorySession, Profile};
use rusqlite::{OptionalExtension, Row, params};
use tokio::sync::broadcast;

use crate::agent::claude_code::session_title_from;
use crate::agent_tools::KeptGrant;
use crate::history::index::AGENT_TITLES;
use crate::session::{NewSessionRow, SessionExit, SessionId, SessionLedger};
use crate::store::Store;
use crate::{Error, Result};

/// "Now" in the store's timestamp format, as today's app writes it.
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

/// The columns [`read_record`] reads, in its order.
///
/// The parent's name is read from its row (its label, else its name): a
/// parent whose row is gone names nothing, as a session with none does.
const RECORD_COLUMNS: &str = "id, name, label, cwd, branch, project_path, profile_id, argv, \
     claude_session_id, status, started_at, ended_at, duration_ms, exit_code, backend, \
     parent_session_id, \
     (SELECT coalesce(nullif(parent.label, ''), parent.name) FROM sessions AS parent \
      WHERE parent.id = sessions.parent_session_id), \
     agent, stopped, command, outlive, permission_mode, title, stopped_by, kept";

/// The backend a row ran on, from its `backend` column.
///
/// A row with none was written by today's app or before the column existed.
/// Today's app runs only Claude Code, and before the column only Claude Code
/// recorded a conversation id (a shell session has none to reopen), so such a
/// row is Claude Code's. That keeps a conversation started before the column
/// restorable through `claude --resume`, as it was in today's app.
fn recorded_backend(column: Option<String>) -> BackendId {
    column
        .filter(|id| !id.trim().is_empty())
        .map_or_else(BackendId::claude_code, BackendId::new)
}

/// A session that ended, numbered for the clients listening on
/// `session:exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitNotice {
    pub seq: u64,
    pub session: SessionId,
}

/// The ledger the host's sessions write to: the store, plus a notice to the
/// connections when a session ends.
///
/// Once the host is stopping its sessions' rows are settled, so it is
/// frozen: an exit that follows writes nothing. A plain stop has already
/// recorded every row as exited; a stop for an update leaves them running
/// for the next start to find lost.
pub(crate) struct HostLedger {
    store: Arc<Mutex<Store>>,
    /// What was lost, a crash's or an update's at the start and a keeper's
    /// since, until a client answers the offer.
    offer: Mutex<Option<Vec<LostRow>>>,
    frozen: AtomicBool,
    next_seq: AtomicU64,
    exits: broadcast::Sender<ExitNotice>,
    /// What each command child last printed, taken from its screen as it went
    /// ([`HostLedger::last_lines_of`]).
    last_lines: Mutex<KeptLines>,
}

/// The most command children whose last lines are kept: the oldest are let go
/// first, so the host holds a bounded amount however many commands it runs.
const KEPT_COMMANDS: usize = 256;

/// The last lines of the command children that have ended, newest last.
#[derive(Default)]
struct KeptLines {
    lines: HashMap<SessionId, Vec<String>>,
    order: VecDeque<SessionId>,
}

impl KeptLines {
    fn keep(&mut self, id: SessionId, lines: Vec<String>) {
        if self.lines.insert(id, lines).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > KEPT_COMMANDS {
            if let Some(oldest) = self.order.pop_front() {
                self.lines.remove(&oldest);
            }
        }
    }
}

impl HostLedger {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        let (exits, _) = broadcast::channel(256);
        Self {
            store,
            offer: Mutex::new(None),
            frozen: AtomicBool::new(false),
            next_seq: AtomicU64::new(1),
            exits,
            last_lines: Mutex::new(KeptLines::default()),
        }
    }

    /// The last lines session `id` printed, kept when it ended as a command
    /// child, so that its parent can be told them and read them later. Empty
    /// for any other session, and for one the host no longer keeps.
    pub fn last_lines_of(&self, id: SessionId) -> Vec<String> {
        self.last_lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lines
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether session `id`'s row says it runs a command for the session that
    /// started it.
    fn runs_command(&self, id: SessionId) -> bool {
        self.store()
            .connection()
            .query_row(
                "SELECT command IS NOT NULL FROM sessions WHERE id = ?1",
                [id.0],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store itself, for a feature that keeps its own rows in it.
    pub fn shared_store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
    }

    /// Exits from now on, for one connection.
    pub fn exits(&self) -> broadcast::Receiver<ExitNotice> {
        self.exits.subscribe()
    }

    /// The restore offer: what has been lost and not yet answered for.
    pub fn offer(&self) -> MutexGuard<'_, Option<Vec<LostRow>>> {
        self.offer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds `lost` to the restore offer, which is made again if it had been
    /// answered.
    pub fn offer_lost(&self, lost: Vec<LostRow>) {
        if lost.is_empty() {
            return;
        }
        let mut offer = self.offer();
        let rows = offer.get_or_insert_with(Vec::new);
        rows.extend(lost);
        rows.sort_by_key(|row| row.id);
        rows.dedup_by_key(|row| row.id);
    }

    /// Records nothing more: the rows are settled for this stop.
    pub fn freeze(&self) {
        self.frozen.store(true, Ordering::SeqCst);
    }
}

impl SessionLedger for HostLedger {
    fn started(&self, row: &NewSessionRow) -> Result<SessionId> {
        if self.frozen.load(Ordering::SeqCst) {
            return Err(Error::Refused(
                "The Floti host is stopping, so it starts no more sessions.".to_owned(),
            ));
        }
        self.store.as_ref().started(row)
    }

    fn finished(&self, id: SessionId, exit: &SessionExit) -> Result<()> {
        let written = if self.frozen.load(Ordering::SeqCst) {
            Ok(())
        } else {
            self.store.as_ref().finished(id, exit)
        };
        self.notify(id);
        written
    }

    /// Only a command child's last lines are kept: no other session's screen
    /// is read by anything.
    fn screen_ended(&self, id: SessionId, last_lines: Vec<String>) {
        if last_lines.is_empty() || !self.runs_command(id) {
            return;
        }
        self.last_lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keep(id, last_lines);
    }

    /// The host is ending the session: its row says so before its program
    /// exits. Once the host is stopping the rows are settled, and a stop for
    /// an update leaves them for the next start to find lost.
    fn ending(&self, id: SessionId) {
        if !self.frozen.load(Ordering::SeqCst) {
            self.store.as_ref().ending(id);
        }
    }

    /// The session's keeper went away with it: its row is lost and offered
    /// for restore, as a crash's are at the next start.
    fn lost(&self, id: SessionId) -> Result<()> {
        let written = if self.frozen.load(Ordering::SeqCst) {
            Ok(())
        } else {
            record_lost(&self.store(), id).map(|lost| self.offer_lost(lost.into_iter().collect()))
        };
        self.notify(id);
        written
    }
}

impl HostLedger {
    fn notify(&self, id: SessionId) {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        // Nobody listening is not a failure.
        let _ = self.exits.send(ExitNotice { seq, session: id });
    }
}

/// A row a crash or an update left running, now `lost`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LostRow {
    pub id: SessionId,
    /// What its tab said: the label, or the name when it has none.
    pub name: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub profile_id: Option<i64>,
    pub project_path: Option<String>,
    /// The conversation it was last in: the one it moved to after a clear,
    /// or the one it started in.
    pub conversation_id: Option<String>,
    /// The permission mode it was launched with, which it reopens in.
    pub permission_mode: Option<PermissionMode>,
    /// The backend it ran on, which reopens it ([`recorded_backend`]).
    pub backend: BackendId,
    /// The agent it was started as, when another session started it: the
    /// agent's file as it is when the session reopens is what composes it
    /// again.
    pub agent: Option<String>,
    /// The name it ran Remote Control under, when it did: the line a child
    /// inherited from its parent, which it runs under again.
    pub remote_control_name: Option<String>,
}

/// What an update to `lost` returns, in [`lost_row`]'s order.
const LOST_COLUMNS: &str = "id, name, label, cwd, branch, profile_id, \
     coalesce(last_claude_session_id, claude_session_id), backend, project_path, permission_mode, \
     agent, remote_control_name";

fn lost_row(row: &Row<'_>) -> rusqlite::Result<LostRow> {
    let name: String = row.get(1)?;
    let label: Option<String> = row.get(2)?;
    Ok(LostRow {
        id: SessionId(row.get(0)?),
        name: label
            .filter(|label| !label.trim().is_empty())
            .unwrap_or(name),
        cwd: row.get(3)?,
        branch: row.get(4)?,
        profile_id: row.get(5)?,
        conversation_id: row.get(6)?,
        backend: recorded_backend(row.get(7)?),
        project_path: row.get(8)?,
        permission_mode: named(row.get(9)?),
        agent: row
            .get::<_, Option<String>>(10)?
            .filter(|agent| !agent.trim().is_empty()),
        remote_control_name: row
            .get::<_, Option<String>>(11)?
            .filter(|name| !name.trim().is_empty()),
    })
}

/// Records every row still `running` as `lost`, as the start after a crash
/// or an update does, but those in `held`, which the session keeper still
/// runs, and returns them oldest first. Stamping an end time would invent a
/// duration nobody measured, so none is written.
pub(crate) fn reconcile_running(store: &Store, held: &[SessionId]) -> Result<Vec<LostRow>> {
    let held = serde_json::to_string(&held.iter().map(|id| id.0).collect::<Vec<_>>())
        .map_err(|err| Error::io("list the sessions still held", io::Error::other(err)))?;
    let mut statement = store
        .connection()
        .prepare(&format!(
            "UPDATE sessions SET status = 'lost' WHERE status = 'running' \
             AND id NOT IN (SELECT value FROM json_each(?1)) RETURNING {LOST_COLUMNS}"
        ))
        .map_err(sql("reconcile the sessions the last run left running"))?;
    let mut lost = statement
        .query_map(params![held], lost_row)
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("reconcile the sessions the last run left running"))?;
    lost.sort_by_key(|row| row.id);
    Ok(lost)
}

/// Records session `id` as `lost`, when it is still running.
pub(crate) fn record_lost(store: &Store, id: SessionId) -> Result<Option<LostRow>> {
    store
        .connection()
        .query_row(
            &format!(
                "UPDATE sessions SET status = 'lost' WHERE id = ?1 AND status = 'running' \
                 RETURNING {LOST_COLUMNS}"
            ),
            params![id.0],
            lost_row,
        )
        .optional()
        .map_err(sql("record the lost session"))
}

/// The ids of every row still `running`.
pub(crate) fn running_ids(store: &Store) -> Result<Vec<SessionId>> {
    let mut statement = store
        .connection()
        .prepare("SELECT id FROM sessions WHERE status = 'running'")
        .map_err(sql("read the running sessions"))?;
    statement
        .query_map([], |row| row.get(0).map(SessionId))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("read the running sessions"))
}

/// Who ended a session that did not exit on its own, as its row keeps it
/// (`stopped_by`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoppedBy {
    /// The session that started it, with `agents_stop`.
    Parent,
    /// A person, from any device: End or Stop, or a quit that ends it.
    Person,
    /// Floti: its parent ended, or the host was stopped.
    Floti,
}

impl StoppedBy {
    /// The word stored.
    pub fn word(self) -> &'static str {
        match self {
            Self::Parent => "parent",
            Self::Person => "person",
            Self::Floti => "floti",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        [Self::Parent, Self::Person, Self::Floti]
            .into_iter()
            .find(|by| by.word() == word)
    }

    /// As a sentence says it.
    pub fn said(self) -> &'static str {
        match self {
            Self::Parent => "its parent",
            Self::Person => "a person",
            Self::Floti => "Floti",
        }
    }

    /// As a session's record says it.
    fn ended_by(self) -> EndedBy {
        match self {
            Self::Parent => EndedBy::Parent,
            Self::Person => EndedBy::Person,
            Self::Floti => EndedBy::Floti,
        }
    }
}

/// Who ended a row `stopped` marks, from its `stopped_by`. A stop nobody
/// named is a person's: every other route names itself before it ends the
/// session.
pub(crate) fn stopped_by_of(word: Option<&str>) -> StoppedBy {
    word.and_then(StoppedBy::from_word)
        .unwrap_or(StoppedBy::Person)
}

/// Records that `by` is about to end session `id`, while it still runs, and,
/// for a child, whether it is kept: whether it stays under its parent as a
/// stopped row once it has ended. The first to say wins, for both: a person's
/// Stop on a child its parent is already stopping is the parent's, and keeps
/// it only if the parent did. A row that is no longer running is left as it
/// is, and a session with no parent is never kept.
pub(crate) fn record_stopping(
    store: &Store,
    id: SessionId,
    by: StoppedBy,
    keep: bool,
) -> Result<()> {
    store
        .connection()
        .execute(
            "UPDATE sessions SET stopped_by = ?2, kept = (?3 AND parent_session_id IS NOT NULL) \
             WHERE id = ?1 AND status = 'running' AND stopped_by IS NULL",
            params![id.0, by.word(), keep],
        )
        .map_err(sql("record who is stopping the session"))?;
    Ok(())
}

/// The stopped children their parents kept whose parent is one of `parents`,
/// oldest first: the ended rows every client draws under a running parent.
pub(crate) fn kept_children_of(store: &Store, parents: &[SessionId]) -> Result<Vec<SessionId>> {
    if parents.is_empty() {
        return Ok(Vec::new());
    }
    let mut select = store
        .connection()
        .prepare(
            "SELECT id, parent_session_id FROM sessions \
             WHERE kept = 1 AND status = 'exited' AND parent_session_id IS NOT NULL ORDER BY id",
        )
        .map_err(sql("read the kept children"))?;
    let rows = select
        .query_map([], |row| {
            Ok((SessionId(row.get(0)?), SessionId(row.get(1)?)))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("read the kept children"))?;
    Ok(rows
        .into_iter()
        .filter(|(_, parent)| parents.contains(parent))
        .map(|(id, _)| id)
        .collect())
}

/// Whether session `id`'s row says it is kept, whatever its state.
pub(crate) fn is_kept(store: &Store, id: SessionId) -> Result<bool> {
    store
        .connection()
        .query_row("SELECT kept FROM sessions WHERE id = ?1", [id.0], |row| {
            row.get(0)
        })
        .optional()
        .map(Option::unwrap_or_default)
        .map_err(sql("read whether a child is kept"))
}

/// Stops keeping each of `ids` that is a kept child: it was resumed, or a
/// person removed it. Answers how many were kept. Nothing else of a row
/// changes, so History and a resume still find it.
pub(crate) fn clear_kept(store: &Store, ids: &[SessionId]) -> Result<usize> {
    let mut statement = store
        .connection()
        .prepare("UPDATE sessions SET kept = 0 WHERE id = ?1 AND kept = 1")
        .map_err(sql("stop keeping a child"))?;
    let mut cleared = 0;
    for id in ids {
        cleared += statement
            .execute([id.0])
            .map_err(sql("stop keeping a child"))?;
    }
    Ok(cleared)
}

/// Records `ids` as exited now, with no exit code, as a deliberate stop does
/// before it ends their processes, so none is offered for restore, and as
/// stopped by `by`, unless someone was already stopping one. A row that is no
/// longer running is left as it is.
pub(crate) fn record_stopped(store: &Store, ids: &[SessionId], by: StoppedBy) -> Result<()> {
    let mut statement = store
        .connection()
        .prepare(&format!(
            "UPDATE sessions SET status = 'exited', stopped = 1, \
             stopped_by = coalesce(stopped_by, ?2), ended_at = {NOW}, \
             duration_ms = CAST((julianday({NOW}) - julianday(started_at)) * 86400000 AS INTEGER) \
             WHERE id = ?1 AND status = 'running'"
        ))
        .map_err(sql("record the stopped sessions"))?;
    for id in ids {
        statement
            .execute(params![id.0, by.word()])
            .map_err(sql("record the stopped sessions"))?;
    }
    Ok(())
}

/// The columns [`profile_row`] reads, in its order.
pub(crate) const PROFILE_COLUMNS: &str = "id, name, root, overlays, access, model, effort, \
     permission_mode, agent, mcp, opening_prompt, pinned_order, created_at, updated_at, \
     session_agents, remote_control";

/// Profile `id` as its row in `profiles` holds it, or `None` when it has
/// been deleted.
pub(crate) fn read_profile(store: &Store, id: u64) -> Result<Option<Profile>> {
    let Ok(id) = i64::try_from(id) else {
        return Ok(None);
    };
    store
        .connection()
        .query_row(
            &format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?1"),
            [id],
            profile_row,
        )
        .optional()
        .map_err(sql("read a profile"))
}

/// A `profiles` row selected with [`PROFILE_COLUMNS`]. A value the row
/// holds that this host does not know, such as an effort level from a later
/// app, reads as not chosen.
pub(crate) fn profile_row(row: &Row<'_>) -> rusqlite::Result<Profile> {
    let list = |index: usize| -> rusqlite::Result<Vec<String>> {
        let text: Option<String> = row.get(index)?;
        Ok(text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default())
    };
    Ok(Profile {
        id: unsigned(row.get(0)?),
        name: row.get(1)?,
        root: row.get(2)?,
        overlays: list(3)?,
        access: list(4)?,
        model: non_empty(row.get(5)?),
        effort: named(row.get(6)?),
        permission_mode: named(row.get(7)?),
        agent: non_empty(row.get(8)?),
        mcp: list(9)?,
        opening_prompt: non_empty(row.get(10)?),
        pinned_order: row
            .get::<_, Option<i64>>(11)?
            .and_then(|order| u32::try_from(order).ok()),
        // Only the choice to turn the agents tools off is kept; anything else
        // follows the host's setting.
        session_agents: row.get::<_, Option<bool>>(14)?.filter(|on| !on),
        // Only the choice to turn Remote Control on is kept.
        remote_control: row.get::<_, Option<bool>>(15)?.filter(|on| *on),
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

/// A value stored under its protocol name, such as `acceptEdits`, or `None`
/// for none or a name this host does not know.
fn named<T: serde::de::DeserializeOwned>(text: Option<String>) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text?)).ok()
}

/// How a permission mode is stored: by its protocol name.
pub(crate) fn permission_mode_name(mode: PermissionMode) -> String {
    serde_json::to_value(mode)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The mode a stored name is, or `None` for a name this host does not know.
pub(crate) fn permission_mode_from_name(text: &str) -> Option<PermissionMode> {
    named(Some(text.to_owned()))
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// Conversation `conversation_id` in the history index, with any name given
/// by hand, or `None` when the index does not have it. History is Claude
/// Code's: its index rows name no backend because only Claude Code records
/// one, so a row is Claude Code's, as [`recorded_backend`] reads a session
/// row with none.
pub(crate) fn read_history_session(
    store: &Store,
    conversation_id: &str,
) -> Result<Option<HistorySession>> {
    store
        .connection()
        .query_row(
            &format!(
                "SELECT h.session_id, h.project, h.prompt_count, h.first_at, h.last_at, \
                 h.first_prompt, h.title_prompt, h.transcript_file, h.transcript_bytes, \
                 h.project_exists, n.name, t.title \
                 FROM history_sessions h \
                 LEFT JOIN history_names n ON n.session_id = lower(h.session_id) \
                 LEFT JOIN {AGENT_TITLES} t ON t.conversation = lower(h.session_id) \
                 WHERE lower(h.session_id) = lower(?1)"
            ),
            [conversation_id],
            |row| {
                let project: String = row.get(1)?;
                let first_prompt: String = row.get(5)?;
                let title_prompt: Option<String> = row.get(6)?;
                let title =
                    session_title_from(&[title_prompt.unwrap_or_else(|| first_prompt.clone())]);
                Ok(HistorySession {
                    session_id: row.get(0)?,
                    backend: BackendId::claude_code(),
                    project_name: last_segment(&project),
                    project,
                    prompt_count: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    first_at: unsigned(row.get(3)?),
                    last_at: unsigned(row.get(4)?),
                    first_prompt,
                    title: title.text,
                    title_fallback: title.fallback,
                    label: non_empty(row.get(10)?),
                    agent_title: non_empty(row.get(11)?),
                    transcript_file: row.get(7)?,
                    transcript_bytes: row.get::<_, Option<i64>>(8)?.map(unsigned),
                    project_exists: row.get::<_, Option<bool>>(9)?.unwrap_or(false),
                    archive: None,
                    archived_messages: None,
                    matched: None,
                })
            },
        )
        .optional()
        .map_err(sql("read a conversation from the history index"))
}

/// The last segment of a recorded folder, whichever separator it was
/// recorded with.
fn last_segment(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_owned()
}

/// Sets session `id`'s label, or clears it with `None`. False when there is
/// no such row.
pub(crate) fn set_label(store: &Store, id: SessionId, label: Option<&str>) -> Result<bool> {
    let changed = store
        .connection()
        .execute(
            "UPDATE sessions SET label = ?2 WHERE id = ?1",
            params![id.0, label],
        )
        .map_err(sql("rename a session"))?;
    Ok(changed > 0)
}

/// Sets session `id`'s title, the one its agent gave it. False when there is
/// no such row.
pub(crate) fn set_title(store: &Store, id: SessionId, title: &str) -> Result<bool> {
    let changed = store
        .connection()
        .execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1",
            params![id.0, title],
        )
        .map_err(sql("title a session"))?;
    Ok(changed > 0)
}

/// Session `id` is in `conversation_id` now, which is not the one it started
/// in: a cleared conversation. A restore reopens this one.
pub(crate) fn record_conversation(
    store: &Store,
    id: SessionId,
    conversation_id: &str,
) -> Result<()> {
    store
        .connection()
        .execute(
            "UPDATE sessions SET last_claude_session_id = ?2 WHERE id = ?1 AND status = 'running'",
            params![id.0, conversation_id],
        )
        .map_err(sql("record a session's conversation"))?;
    Ok(())
}

/// Keeps in session `id`'s row what a host after this one needs to serve its
/// agent tools again: never the token, only its hash.
pub(crate) fn record_agent_tools(store: &Store, id: SessionId, kept: &KeptGrant) -> Result<()> {
    let json = serde_json::to_string(kept)
        .map_err(|err| Error::io("keep a session's agent tools", io::Error::other(err)))?;
    store
        .connection()
        .execute(
            "UPDATE sessions SET agent_tools = ?2 WHERE id = ?1",
            params![id.0, json],
        )
        .map_err(sql("keep a session's agent tools"))?;
    Ok(())
}

/// What session `id`'s row kept of its agent tools: `None` for a launch
/// given none, or a value this host cannot read.
pub(crate) fn kept_agent_tools(store: &Store, id: SessionId) -> Result<Option<KeptGrant>> {
    let json: Option<String> = store
        .connection()
        .query_row(
            "SELECT agent_tools FROM sessions WHERE id = ?1",
            [id.0],
            |row| row.get(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(sql("read a session's agent tools"))?;
    Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
}

/// The conversation session `id` is in now, as far as its row knows.
pub(crate) fn current_conversation(store: &Store, id: SessionId) -> Result<Option<String>> {
    store
        .connection()
        .query_row(
            "SELECT coalesce(last_claude_session_id, claude_session_id) FROM sessions WHERE id = ?1",
            [id.0],
            |row| row.get(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(sql("read a session's conversation"))
}

/// The name of profile `id`, or `None` when it has been deleted.
pub(crate) fn profile_name(store: &Store, id: i64) -> Result<Option<String>> {
    store
        .connection()
        .query_row("SELECT name FROM profiles WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(sql("read a profile"))
}

/// Session `id`'s row.
pub(crate) fn read_record(store: &Store, id: SessionId) -> Result<SessionRecord> {
    store
        .connection()
        .query_row(
            &format!("SELECT {RECORD_COLUMNS} FROM sessions WHERE id = ?1"),
            [id.0],
            record_from,
        )
        .map_err(sql("read a session's row"))
}

fn record_from(row: &Row<'_>) -> rusqlite::Result<SessionRecord> {
    let argv: String = row.get(7)?;
    let status: String = row.get(9)?;
    let agent = row.get::<_, Option<String>>(17)?.and_then(|agent| {
        let agent = agent.trim();
        (!agent.is_empty()).then(|| agent.to_owned())
    });
    Ok(SessionRecord {
        id: unsigned(row.get::<_, i64>(0)?),
        name: row.get(1)?,
        label: row.get(2)?,
        title: non_empty(row.get(22)?),
        cwd: row.get(3)?,
        branch: row.get(4)?,
        project_path: row.get(5)?,
        profile_id: row.get::<_, Option<i64>>(6)?.map(unsigned),
        // A row written by an older app with something unreadable here still
        // lists; its arguments are simply not known.
        argv: serde_json::from_str(&argv).unwrap_or_default(),
        conversation_id: row.get(8)?,
        backend: recorded_backend(row.get(14)?),
        status: match status.as_str() {
            "running" => SessionStatus::Running,
            "exited" => SessionStatus::Exited,
            _ => SessionStatus::Lost,
        },
        started_at: row.get(10)?,
        ended_at: row.get(11)?,
        duration_ms: row.get::<_, Option<i64>>(12)?.map(unsigned),
        // The ledger keeps the code as the OS gave it, a `u32` in an `i64`.
        // Windows reports an NTSTATUS such as 0xC0000005 that way, and the
        // signed value (-1073741819) is what PowerShell's `$LASTEXITCODE`
        // says, so the record keeps the low 32 bits and never clamps.
        exit_code: row.get::<_, Option<i64>>(13)?.map(|code| code as i32),
        parent_id: row.get::<_, Option<i64>>(15)?.map(unsigned),
        parent_name: row.get(16)?,
        // The retired role says the same, for a client before protocol 2.3.
        role: agent.clone(),
        agent,
        stopped: row.get(18)?,
        ended_by: ended_by(
            &status,
            row.get(18)?,
            row.get::<_, Option<String>>(23)?.as_deref(),
        ),
        // Only a child that has exited is drawn as a stopped row; a running
        // or a lost one is drawn as what it is.
        kept: status == "exited" && row.get::<_, bool>(24)?,
        command: row
            .get::<_, Option<String>>(19)?
            .filter(|line| !line.trim().is_empty()),
        last_report: None,
        outlive: row.get(20)?,
        permission_mode: row
            .get::<_, Option<String>>(21)?
            .as_deref()
            .and_then(permission_mode_from_name),
    })
}

/// How a row ended: by itself, or stopped by whoever its row names. Only a
/// row that has exited says; a running or a lost one does not.
fn ended_by(status: &str, stopped: bool, by: Option<&str>) -> Option<EndedBy> {
    (status == "exited").then(|| {
        if stopped {
            stopped_by_of(by).ended_by()
        } else {
            EndedBy::Exited
        }
    })
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests;

fn sql(action: &'static str) -> impl Fn(rusqlite::Error) -> Error {
    move |err| Error::io(action, io::Error::other(err))
}
