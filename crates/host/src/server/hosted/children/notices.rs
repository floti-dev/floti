//! Standing notices: what the host keeps for a person who is not looking.
//!
//! A notice stands on the host while it holds, with no client attached:
//!
//! - *needs you*: a child is waiting on a person. It is read off the activity
//!   pass ([`super::news`]), so it stands while the activity says waiting, goes
//!   when the child stops waiting or ends, and is rebuilt from the activity
//!   after a restart. It is kept in memory and is never dismissed;
//! - *limit*: a start a limit refused. A row in `notices`, kept across a
//!   restart until a client dismisses it or its parent ends. A new refusal by
//!   the same limit for the same parent replaces the old one;
//! - *error*: an agent child that ended in error, could not start, or
//!   reported an error while it ran on, its `cause` saying which. A row in
//!   `notices` kept the same way, which also goes when its cause does: a
//!   reported error when the child reports working or idle again, and every
//!   error of a child when that child is resumed. A command child's exit is
//!   its parent's business alone: the parent hears the code and the last
//!   lines as news, and no notice stands for it.
//!
//! The list goes to every client on `notices:changed` (the features module
//! serves it); dismissing is for every client, and so is allowing more starts.

use std::io;

use floti_protocol::channels::sessions::{NoticeCause, NoticeKind, NoticeLimit, StandingNotice};
use rusqlite::params;

use super::rows;
use crate::server::hosted::Hosted;
use crate::session::{SessionId, allow_starts};
use crate::store::Store;
use crate::workspace::settings;
use crate::{Error, Result};

/// The id of the notice that child `child` needs a person.
fn needs_you_id(child: SessionId) -> String {
    format!("needs-you-{}", child.0)
}

/// A session's id as the protocol says it.
fn id_number(id: SessionId) -> u64 {
    u64::try_from(id.0).unwrap_or_default()
}

/// The id of stored notice `row`.
fn stored_id(row: i64) -> String {
    format!("notice-{row}")
}

/// The row a stored notice's id names.
fn stored_row(id: &str) -> Option<i64> {
    id.strip_prefix("notice-")?.parse().ok()
}

/// The notice that child `child` of `parent` waits on `text`.
pub(super) fn needs_you(
    child: SessionId,
    child_name: &str,
    parent: SessionId,
    parent_name: &str,
    text: &str,
    since: String,
) -> StandingNotice {
    StandingNotice {
        id: needs_you_id(child),
        kind: NoticeKind::NeedsYou,
        parent_id: id_number(parent),
        parent_name: parent_name.to_owned(),
        child_id: Some(id_number(child)),
        child_name: Some(child_name.to_owned()),
        text: text.to_owned(),
        since,
        limit: None,
        allow_more: None,
        cause: None,
    }
}

/// The word a notice's row keeps in `cause`.
fn cause_word(cause: NoticeCause) -> &'static str {
    match cause {
        NoticeCause::Ended => "ended",
        NoticeCause::Launch => "launch",
        NoticeCause::Report => "report",
    }
}

/// What an error notice's row says in `cause`, or for a row written before
/// the cause was kept, what it was then: a launch that failed names no child,
/// and one that names a child ended in error.
fn cause_of(word: Option<&str>, child: Option<i64>) -> NoticeCause {
    match word {
        Some("launch") => NoticeCause::Launch,
        Some("report") => NoticeCause::Report,
        Some("ended") => NoticeCause::Ended,
        _ if child.is_none() => NoticeCause::Launch,
        _ => NoticeCause::Ended,
    }
}

/// What a refused start leaves.
pub(super) struct Limit<'a> {
    pub limit: StartLimit,
    pub text: &'a str,
    /// For the starts limit, how many starts Allow adds.
    pub allow_more: Option<u32>,
}

/// A limit a start can be refused by: every one there is, each one of the
/// protocol's [`NoticeLimit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartLimit {
    Children,
    Sessions,
    Depth,
    Starts,
}

impl StartLimit {
    /// The word a notice's row keeps in `limit_kind`.
    fn word(self) -> &'static str {
        match self {
            Self::Children => "children",
            Self::Sessions => "sessions",
            Self::Depth => "depth",
            Self::Starts => "starts",
        }
    }

    /// The limit a row's word names, or none for a word it is not.
    fn of(word: &str) -> Option<Self> {
        Some(match word {
            "children" => Self::Children,
            "sessions" => Self::Sessions,
            "depth" => Self::Depth,
            "starts" => Self::Starts,
            _ => return None,
        })
    }
}

impl From<StartLimit> for NoticeLimit {
    fn from(limit: StartLimit) -> Self {
        match limit {
            StartLimit::Children => Self::Children,
            StartLimit::Sessions => Self::Sessions,
            StartLimit::Depth => Self::Depth,
            StartLimit::Starts => Self::Starts,
        }
    }
}

/// Records that `parent`'s start was refused by `refusal`, in place of the
/// notice the same limit left for the same parent before.
pub(super) fn record_limit(
    store: &Store,
    parent: SessionId,
    parent_name: &str,
    refusal: &Limit<'_>,
) -> Result<()> {
    let word = refusal.limit.word();
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE kind = 'limit' AND parent_session_id = ?1 \
             AND limit_kind = ?2",
            params![parent.0, word],
        )
        .map_err(sql("replace a limit notice"))?;
    store
        .connection()
        .execute(
            "INSERT INTO notices (kind, parent_session_id, parent_name, text, limit_kind, \
             allow_more) VALUES ('limit', ?1, ?2, ?3, ?4, ?5)",
            params![
                parent.0,
                parent_name,
                refusal.text,
                word,
                refusal.allow_more.map(i64::from)
            ],
        )
        .map_err(sql("record a limit notice"))?;
    Ok(())
}

/// What an error notice says: the child it is about and why it stands.
#[derive(Debug, Clone, Copy)]
pub(super) struct ChildError<'a> {
    pub child: Option<SessionId>,
    pub child_name: &'a str,
    pub text: &'a str,
    pub cause: NoticeCause,
}

/// Records that a child of `parent` ended in error, never started or reported
/// an error, in place of an earlier notice about the same child.
pub(super) fn record_error(
    store: &Store,
    parent: SessionId,
    parent_name: &str,
    error: ChildError<'_>,
) -> Result<()> {
    let ChildError {
        child,
        child_name,
        text,
        cause,
    } = error;
    let child = child.map(|child| child.0);
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE kind = 'error' AND parent_session_id = ?1 \
             AND child_name = ?2 AND child_session_id IS ?3",
            params![parent.0, child_name, child],
        )
        .map_err(sql("replace an error notice"))?;
    store
        .connection()
        .execute(
            "INSERT INTO notices (kind, parent_session_id, parent_name, child_session_id, \
             child_name, text, cause) VALUES ('error', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                parent.0,
                parent_name,
                child,
                child_name,
                text,
                cause_word(cause)
            ],
        )
        .map_err(sql("record an error notice"))?;
    Ok(())
}

/// The notices the database keeps, oldest first.
pub(super) fn stored(store: &Store) -> Result<Vec<StandingNotice>> {
    let mut select = store
        .connection()
        .prepare(
            "SELECT id, kind, parent_session_id, parent_name, child_session_id, child_name, \
             text, limit_kind, allow_more, since, cause FROM notices ORDER BY id",
        )
        .map_err(sql("read the notices"))?;
    let rows = select
        .query_map([], |row| {
            let kind: String = row.get(1)?;
            let limit: Option<String> = row.get(7)?;
            let child: Option<i64> = row.get(4)?;
            let cause: Option<String> = row.get(10)?;
            let limited = kind == "limit";
            Ok(StandingNotice {
                id: stored_id(row.get(0)?),
                kind: if limited {
                    NoticeKind::Limit
                } else {
                    NoticeKind::Error
                },
                parent_id: id_number(SessionId(row.get(2)?)),
                parent_name: row.get(3)?,
                child_id: child.map(|id| id_number(SessionId(id))),
                child_name: row.get(5)?,
                text: row.get(6)?,
                since: row.get(9)?,
                limit: limit
                    .as_deref()
                    .and_then(StartLimit::of)
                    .map(NoticeLimit::from),
                allow_more: row
                    .get::<_, Option<i64>>(8)?
                    .and_then(|more| u32::try_from(more).ok()),
                cause: (!limited).then(|| cause_of(cause.as_deref(), child)),
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("read the notices"))?;
    Ok(rows)
}

/// Removes the notices under `parent`, which has ended. Whether any went.
pub(super) fn remove_for_parent(store: &Store, parent: SessionId) -> Result<bool> {
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE parent_session_id = ?1",
            [parent.0],
        )
        .map(|removed| removed > 0)
        .map_err(sql("remove the notices of a session that ended"))
}

/// Removes the error child `child` reported while it ran on, which it no
/// longer reports. Whether one went.
pub(super) fn remove_report_of(store: &Store, child: SessionId) -> Result<bool> {
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE kind = 'error' AND cause = 'report' \
             AND child_session_id = ?1",
            [child.0],
        )
        .map(|removed| removed > 0)
        .map_err(sql("remove the error a child no longer reports"))
}

/// Removes the errors of `parent`'s child called `child_name`, which has been
/// resumed: whatever it ended, failed to start or reported with is behind it.
/// Names match as a resume finds them, whatever their case. Whether any went.
pub(super) fn remove_for_resumed(
    store: &Store,
    parent: SessionId,
    child_name: &str,
) -> Result<bool> {
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE kind = 'error' AND parent_session_id = ?1 \
             AND child_name = ?2 COLLATE NOCASE",
            params![parent.0, child_name],
        )
        .map(|removed| removed > 0)
        .map_err(sql("remove the errors of a child that was resumed"))
}

/// Removes the notices whose parent is not running: the parents that ended
/// while no host was there to say so.
pub(super) fn remove_unowned(store: &Store) -> Result<()> {
    store
        .connection()
        .execute(
            "DELETE FROM notices WHERE parent_session_id NOT IN \
             (SELECT id FROM sessions WHERE status = 'running')",
            [],
        )
        .map(|_| ())
        .map_err(sql("remove the notices of sessions that ended"))
}

impl Hosted {
    /// The notices standing now: the children that need a person, then the
    /// refusals and errors the host has kept.
    ///
    /// # Errors
    ///
    /// A sentence when the host's record cannot be read.
    pub(crate) fn standing_notices(&self) -> std::result::Result<Vec<StandingNotice>, String> {
        let mut list = self.children.news.needs_you_notices();
        list.extend(stored(&self.ledger.store()).map_err(|err| err.to_string())?);
        Ok(list)
    }

    /// Counts the changes to the standing notices, for the feature that sends
    /// them to the clients to wait on.
    pub(crate) fn notices_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.children.news.notices_changes()
    }

    /// A client dismisses notice `id`, for every client. A notice that is
    /// gone, and a needs-you notice, which goes by itself when its child stops
    /// waiting, are left as they are.
    ///
    /// # Errors
    ///
    /// A sentence when the host's record cannot be written.
    pub(crate) fn dismiss_notice(&self, id: &str) -> std::result::Result<(), String> {
        let Some(row) = stored_row(id) else {
            return Ok(());
        };
        let removed = self
            .ledger
            .store()
            .connection()
            .execute("DELETE FROM notices WHERE id = ?1", [row])
            .map_err(|err| sql("dismiss a notice")(err).to_string())?;
        if removed > 0 {
            self.children.news.notices_moved();
        }
        Ok(())
    }

    /// A client lets `parent` start as many more children as the starts
    /// setting says, after its starts limit refused it; the starts notice
    /// goes. No other session's allowance changes. Without a starts notice
    /// standing for `parent` nothing is added: two clients pressing Allow
    /// together add the starts once.
    ///
    /// Answers how many more starts `parent` may make now.
    ///
    /// # Errors
    ///
    /// A sentence when the host's record cannot be read or written.
    pub(crate) fn allow_more_starts(&self, parent: u64) -> std::result::Result<u32, String> {
        let parent = SessionId(i64::try_from(parent).map_err(|err| err.to_string())?);
        let added = {
            let store = self.ledger.store();
            let standing: bool = store
                .connection()
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM notices WHERE kind = 'limit' \
                     AND parent_session_id = ?1 AND limit_kind = 'starts')",
                    [parent.0],
                    |row| row.get(0),
                )
                .map_err(|err| sql("read a starts notice")(err).to_string())?;
            if !standing {
                return Ok(0);
            }
            let more = settings::read(store.connection())
                .map_err(|err| err.to_string())?
                .agent_starts_per_parent;
            allow_starts(&store, parent, more).map_err(|err| err.to_string())?;
            store
                .connection()
                .execute(
                    "DELETE FROM notices WHERE kind = 'limit' AND parent_session_id = ?1 \
                     AND limit_kind = 'starts'",
                    [parent.0],
                )
                .map_err(|err| sql("remove a starts notice")(err).to_string())?;
            more
        };
        self.children.news.notices_moved();
        Ok(added)
    }

    /// `limit` refused a start of `parent`: the notice stands until it is
    /// dismissed, the parent is allowed more, or the parent ends. Answers the
    /// refusal's sentence.
    pub(super) fn limit_refused(&self, parent: SessionId, refusal: Limit<'_>) -> String {
        let store = self.ledger.store();
        let name = rows::label_of(&store, parent)
            .ok()
            .flatten()
            .unwrap_or_default();
        if let Err(err) = record_limit(&store, parent, &name, &refusal) {
            log_line!("floti host: {err}");
        }
        drop(store);
        self.children.news.notices_moved();
        refusal.text.to_owned()
    }

    /// An agent child of `link`'s parent ended in error, never started or
    /// reported an error: the notice stands until it is dismissed, the parent
    /// ends or its cause goes.
    pub(super) fn error_noticed(
        &self,
        link: &rows::Link,
        child: Option<SessionId>,
        text: &str,
        cause: NoticeCause,
    ) {
        let store = self.ledger.store();
        let parent = rows::label_of(&store, link.parent)
            .ok()
            .flatten()
            .unwrap_or_default();
        let error = ChildError {
            child,
            child_name: &link.name,
            text,
            cause,
        };
        if let Err(err) = record_error(&store, link.parent, &parent, error) {
            log_line!("floti host: {err}");
        }
        drop(store);
        self.children.news.notices_moved();
    }

    /// Child `child` reports working or idle again after it reported an
    /// error: that error's notice goes.
    pub(super) fn notices_of_recovered(&self, child: SessionId) {
        match remove_report_of(&self.ledger.store(), child) {
            Ok(true) => self.children.news.notices_moved(),
            Ok(false) => {}
            Err(err) => log_line!("floti host: {err}"),
        }
    }

    /// `parent`'s child called `child_name` has been resumed: the errors that
    /// stood for it go.
    pub(super) fn notices_of_resumed(&self, parent: SessionId, child_name: &str) {
        match remove_for_resumed(&self.ledger.store(), parent, child_name) {
            Ok(true) => self.children.news.notices_moved(),
            Ok(false) => {}
            Err(err) => log_line!("floti host: {err}"),
        }
    }

    /// Session `session` has ended: the notices under it go with it.
    pub(super) fn notices_of_ended(&self, session: SessionId) {
        match remove_for_parent(&self.ledger.store(), session) {
            Ok(true) => self.children.news.notices_moved(),
            Ok(false) => {}
            Err(err) => log_line!("floti host: {err}"),
        }
    }
}

fn sql(action: &'static str) -> impl Fn(rusqlite::Error) -> Error {
    move |err| Error::io(action, io::Error::other(err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TestDirs;

    const LEAD: SessionId = SessionId(10);

    fn store() -> (TestDirs, Store) {
        let dirs = TestDirs::new();
        let store = Store::open(&dirs.data).unwrap();
        (dirs, store)
    }

    fn error<'a>(
        child: Option<i64>,
        child_name: &'a str,
        text: &'a str,
        cause: NoticeCause,
    ) -> ChildError<'a> {
        ChildError {
            child: child.map(SessionId),
            child_name,
            text,
            cause,
        }
    }

    /// The standing errors, as (child name, cause).
    fn errors(store: &Store) -> Vec<(String, Option<NoticeCause>)> {
        stored(store)
            .unwrap()
            .into_iter()
            .map(|notice| (notice.child_name.unwrap_or_default(), notice.cause))
            .collect()
    }

    #[test]
    fn an_error_keeps_its_cause_and_a_newer_error_for_the_same_child_replaces_it() {
        let (_dirs, store) = store();
        let failed = error(
            None,
            "ghost",
            "ghost could not be started: no program.",
            NoticeCause::Launch,
        );
        record_error(&store, LEAD, "lead", failed).unwrap();
        let reported = error(
            Some(11),
            "worker",
            "worker reported an error: it broke.",
            NoticeCause::Report,
        );
        record_error(&store, LEAD, "lead", reported).unwrap();
        assert_eq!(
            errors(&store),
            [
                ("ghost".to_owned(), Some(NoticeCause::Launch)),
                ("worker".to_owned(), Some(NoticeCause::Report)),
            ]
        );
        let ended = error(
            Some(11),
            "worker",
            "worker ended in error: it exited with code 1.",
            NoticeCause::Ended,
        );
        record_error(&store, LEAD, "lead", ended).unwrap();
        assert_eq!(
            errors(&store),
            [
                ("ghost".to_owned(), Some(NoticeCause::Launch)),
                ("worker".to_owned(), Some(NoticeCause::Ended)),
            ]
        );
    }

    #[test]
    fn an_error_written_before_causes_were_kept_reads_as_a_launch_without_a_child_and_an_ending_with_one()
     {
        let (_dirs, store) = store();
        for child in [None, Some(11)] {
            store
                .connection()
                .execute(
                    "INSERT INTO notices (kind, parent_session_id, parent_name, \
                     child_session_id, child_name, text) VALUES ('error', 10, 'lead', ?1, 'w', 'x')",
                    [child],
                )
                .unwrap();
        }
        let causes: Vec<_> = errors(&store).into_iter().map(|(_, cause)| cause).collect();
        assert_eq!(
            causes,
            [Some(NoticeCause::Launch), Some(NoticeCause::Ended)]
        );
    }

    #[test]
    fn a_child_that_works_again_loses_only_its_reported_error() {
        let (_dirs, store) = store();
        let reported = error(Some(11), "a", "a reported an error.", NoticeCause::Report);
        record_error(&store, LEAD, "lead", reported).unwrap();
        let ended = error(Some(12), "b", "b ended in error.", NoticeCause::Ended);
        record_error(&store, LEAD, "lead", ended).unwrap();
        assert!(!remove_report_of(&store, SessionId(12)).unwrap());
        assert!(remove_report_of(&store, SessionId(11)).unwrap());
        assert_eq!(errors(&store), [("b".to_owned(), Some(NoticeCause::Ended))]);
    }

    #[test]
    fn a_resumed_child_loses_every_error_of_its_name_under_its_parent_whatever_the_case() {
        let (_dirs, store) = store();
        let launch = error(
            None,
            "Worker",
            "Worker could not be started.",
            NoticeCause::Launch,
        );
        record_error(&store, LEAD, "lead", launch).unwrap();
        let report = error(
            Some(11),
            "worker",
            "worker reported an error.",
            NoticeCause::Report,
        );
        record_error(&store, LEAD, "lead", report).unwrap();
        let other = error(
            Some(12),
            "other",
            "other ended in error.",
            NoticeCause::Ended,
        );
        record_error(&store, LEAD, "lead", other).unwrap();
        let elsewhere = error(
            Some(13),
            "worker",
            "worker ended in error.",
            NoticeCause::Ended,
        );
        record_error(&store, SessionId(20), "other lead", elsewhere).unwrap();

        assert!(remove_for_resumed(&store, LEAD, "WORKER").unwrap());
        assert_eq!(
            errors(&store),
            [
                ("other".to_owned(), Some(NoticeCause::Ended)),
                ("worker".to_owned(), Some(NoticeCause::Ended)),
            ]
        );
        assert!(!remove_for_resumed(&store, LEAD, "worker").unwrap());
    }
}
