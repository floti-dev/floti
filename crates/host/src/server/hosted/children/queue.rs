//! The queue: what a parent has sent its child that the child has not been
//! given yet, kept in floti.db beside the inbox so it outlasts a host restart.
//!
//! **Keyed by the child's row.** A host that takes its sessions back from the
//! keeper keeps their rows, so what was queued before a restart is delivered
//! after it. A resumed child is a new row and is sent afresh.
//!
//! **In the order sent.** A message waits until every one sent to the same
//! child before it has been delivered; the queue's id is that order.
//!
//! **When a child can take a message** is [`hold`], over what the host already
//! reads of the child ([`Moment`]): a person's keys, a dialog, its activity and
//! whether the turn a message gave it has ended. Nothing on its screen is read.

use floti_protocol::channels::sessions::SessionActivity;
use rusqlite::params;

use crate::session::SessionId;
use crate::store::Store;
use crate::{Error, Result};

/// One message that waits for its child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Queued {
    pub id: i64,
    pub text: String,
    /// The child's turn in progress is stopped first.
    pub interrupt: bool,
}

/// Puts `text` at the end of `child`'s queue, sent by `parent`, and the id it
/// was given.
pub(super) fn push(
    store: &Store,
    child: SessionId,
    parent: SessionId,
    text: &str,
    interrupt: bool,
) -> Result<i64> {
    store
        .connection()
        .query_row(
            "INSERT INTO agent_messages (child_session_id, parent_session_id, text, interrupt) \
             VALUES (?1, ?2, ?3, ?4) RETURNING id",
            params![child.0, parent.0, text, interrupt],
            |row| row.get(0),
        )
        .map_err(sql("queue a message for an agent"))
}

/// What waits for `child`, oldest first.
pub(super) fn waiting(store: &Store, child: SessionId) -> Result<Vec<Queued>> {
    let mut select = store
        .connection()
        .prepare(
            "SELECT id, text, interrupt FROM agent_messages \
             WHERE child_session_id = ?1 AND delivered_at IS NULL AND dropped_at IS NULL \
             ORDER BY id",
        )
        .map_err(sql("read an agent's queued messages"))?;
    select
        .query_map([child.0], |row| {
            Ok(Queued {
                id: row.get(0)?,
                text: row.get(1)?,
                interrupt: row.get(2)?,
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("read an agent's queued messages"))
}

/// Every child something waits for, in the order its oldest was sent.
pub(super) fn children_waiting(store: &Store) -> Result<Vec<SessionId>> {
    let mut select = store
        .connection()
        .prepare(
            "SELECT child_session_id FROM agent_messages \
             WHERE delivered_at IS NULL AND dropped_at IS NULL \
             GROUP BY child_session_id ORDER BY MIN(id)",
        )
        .map_err(sql("read the agents with queued messages"))?;
    select
        .query_map([], |row| row.get(0).map(SessionId))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(sql("read the agents with queued messages"))
}

/// Message `id` has been typed into its child.
pub(super) fn delivered(store: &Store, id: i64) -> Result<()> {
    store
        .connection()
        .execute(
            "UPDATE agent_messages SET delivered_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?1 AND delivered_at IS NULL",
            [id],
        )
        .map(drop)
        .map_err(sql("mark an agent's message delivered"))
}

/// Message `id` is never to be delivered: its child could not take it.
pub(super) fn dropped(store: &Store, id: i64) -> Result<()> {
    store
        .connection()
        .execute(
            "UPDATE agent_messages SET dropped_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?1 AND delivered_at IS NULL AND dropped_at IS NULL",
            [id],
        )
        .map(drop)
        .map_err(sql("drop an agent's message"))
}

/// Everything waiting for `child` is never to be delivered: it has ended.
/// How many there were.
pub(super) fn drop_waiting(store: &Store, child: SessionId) -> Result<usize> {
    store
        .connection()
        .execute(
            "UPDATE agent_messages SET dropped_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE child_session_id = ?1 AND delivered_at IS NULL AND dropped_at IS NULL",
            [child.0],
        )
        .map_err(sql("drop an ended agent's messages"))
}

/// What the host knows of a child at the moment a message could be typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Moment {
    /// A person has typed into its terminal since its last turn ended.
    pub person_typed: bool,
    /// It waits on a permission prompt or any other dialog: its backend's
    /// records say so, or its program reported `blocked`.
    pub at_a_prompt: bool,
    /// Its activity at the last pass.
    pub activity: Option<SessionActivity>,
    /// Anything tells its turns: its backend keeps records of its sessions,
    /// or its program reports its status. With neither, there is no turn to
    /// wait for.
    pub signals: bool,
    /// A message was typed into it and the turn that gave it has not ended.
    pub given_a_turn: bool,
}

/// Why a message waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold {
    /// A person has typed in the child's terminal since its last turn.
    Person,
    /// The child waits on a prompt only a person answers.
    Prompt,
    /// The child has not begun its first turn.
    NotStarted,
    /// The child is in a turn.
    Working,
}

impl Hold {
    /// The word a program reads.
    pub fn word(self) -> &'static str {
        match self {
            Self::Person => "person_typed",
            Self::Prompt => "prompt",
            Self::NotStarted => "not_started",
            Self::Working => "working",
        }
    }

    /// Why, as a sentence about `name` says it.
    pub fn said(self, name: &str) -> String {
        match self {
            Self::Person => format!(
                "a person has typed in {name}'s terminal since its last turn, and their keys come first"
            ),
            Self::Prompt => format!("{name} is waiting on a prompt a person must answer"),
            Self::NotStarted => format!("{name} has not begun its first turn yet"),
            Self::Working => format!("{name} is in a turn"),
        }
    }
}

/// Why a message that does (`interrupt`) or does not stop the child's turn
/// cannot be typed at `moment`, or `None` when it can.
///
/// A person's keys and a dialog hold every message, an interrupt too: nothing
/// is typed over a person or into a prompt. A child whose turns something
/// tells takes a message once it has begun and is at rest: idle, or stopped
/// on an error its program reported, and not still in the turn the last
/// message gave it. An interrupt does not wait for the turn to end, since
/// stopping it is what it asks. A child that nothing tells the turns of takes
/// each message as soon as no person has typed.
pub(super) fn hold(moment: &Moment, interrupt: bool) -> Option<Hold> {
    if moment.person_typed {
        return Some(Hold::Person);
    }
    if moment.at_a_prompt || moment.activity == Some(SessionActivity::Waiting) {
        return Some(Hold::Prompt);
    }
    if !moment.signals {
        return None;
    }
    if moment.activity.is_none() {
        return Some(Hold::NotStarted);
    }
    if interrupt {
        return None;
    }
    if moment.given_a_turn {
        return Some(Hold::Working);
    }
    match moment.activity {
        Some(SessionActivity::Idle | SessionActivity::Error) => None,
        _ => Some(Hold::Working),
    }
}

fn sql(action: &'static str) -> impl Fn(rusqlite::Error) -> Error {
    move |err| Error::io(action, std::io::Error::other(err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TestDirs;

    fn store() -> (TestDirs, Store) {
        let dirs = TestDirs::new();
        let store = Store::open(&dirs.data).expect("open the store");
        (dirs, store)
    }

    const CHILD: SessionId = SessionId(5);
    const PARENT: SessionId = SessionId(1);

    #[test]
    fn messages_wait_in_the_order_sent_for_their_own_child_only() {
        let (_dirs, store) = store();
        push(&store, CHILD, PARENT, "first", false).unwrap();
        push(&store, SessionId(6), PARENT, "other child", false).unwrap();
        push(&store, CHILD, PARENT, "second", true).unwrap();
        let texts: Vec<(String, bool)> = waiting(&store, CHILD)
            .unwrap()
            .into_iter()
            .map(|queued| (queued.text, queued.interrupt))
            .collect();
        assert_eq!(
            texts,
            [("first".to_owned(), false), ("second".to_owned(), true)]
        );
        assert_eq!(
            children_waiting(&store).unwrap(),
            [CHILD, SessionId(6)],
            "by the oldest message each has waiting"
        );
    }

    #[test]
    fn a_delivered_or_dropped_message_waits_no_more_and_a_child_that_ended_has_nothing_waiting() {
        let (_dirs, store) = store();
        let first = push(&store, CHILD, PARENT, "first", false).unwrap();
        let second = push(&store, CHILD, PARENT, "second", false).unwrap();
        push(&store, CHILD, PARENT, "third", false).unwrap();
        delivered(&store, first).unwrap();
        dropped(&store, second).unwrap();
        // Delivered once is delivered; a delivered one is not dropped after.
        delivered(&store, first).unwrap();
        dropped(&store, first).unwrap();
        let left = waiting(&store, CHILD).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "third");
        assert_eq!(drop_waiting(&store, CHILD).unwrap(), 1);
        assert!(waiting(&store, CHILD).unwrap().is_empty());
        assert!(children_waiting(&store).unwrap().is_empty());
    }

    #[test]
    fn the_queue_is_kept_in_the_database_and_read_back_when_it_is_opened_again() {
        let dirs = TestDirs::new();
        {
            let store = Store::open(&dirs.data).unwrap();
            push(&store, CHILD, PARENT, "across a restart", false).unwrap();
        }
        let store = Store::open(&dirs.data).unwrap();
        assert_eq!(waiting(&store, CHILD).unwrap()[0].text, "across a restart");
    }

    fn at_rest() -> Moment {
        Moment {
            person_typed: false,
            at_a_prompt: false,
            activity: Some(SessionActivity::Idle),
            signals: true,
            given_a_turn: false,
        }
    }

    #[test]
    fn a_child_at_rest_that_nobody_typed_into_takes_a_message() {
        assert_eq!(hold(&at_rest(), false), None);
        assert_eq!(hold(&at_rest(), true), None);
        let errored = Moment {
            activity: Some(SessionActivity::Error),
            ..at_rest()
        };
        assert_eq!(hold(&errored, false), None);
    }

    #[test]
    fn a_persons_keys_and_a_prompt_hold_every_message_an_interrupt_too() {
        for interrupt in [false, true] {
            let typed = Moment {
                person_typed: true,
                ..at_rest()
            };
            assert_eq!(hold(&typed, interrupt), Some(Hold::Person));
            let blocked = Moment {
                at_a_prompt: true,
                ..at_rest()
            };
            assert_eq!(hold(&blocked, interrupt), Some(Hold::Prompt));
            let waiting = Moment {
                activity: Some(SessionActivity::Waiting),
                ..at_rest()
            };
            assert_eq!(hold(&waiting, interrupt), Some(Hold::Prompt));
            // A person's keys come first even for a child nothing tells the
            // turns of.
            let silent = Moment {
                person_typed: true,
                signals: false,
                activity: None,
                ..at_rest()
            };
            assert_eq!(hold(&silent, interrupt), Some(Hold::Person));
        }
    }

    #[test]
    fn a_working_child_takes_a_message_when_its_turn_ends_and_an_interrupt_at_once() {
        for activity in [SessionActivity::Busy, SessionActivity::Shell] {
            let working = Moment {
                activity: Some(activity),
                ..at_rest()
            };
            assert_eq!(hold(&working, false), Some(Hold::Working));
            assert_eq!(hold(&working, true), None);
        }
        // Idle again, but the turn the last message gave it has not ended.
        let given = Moment {
            given_a_turn: true,
            ..at_rest()
        };
        assert_eq!(hold(&given, false), Some(Hold::Working));
        assert_eq!(hold(&given, true), None);
    }

    #[test]
    fn a_child_that_has_not_begun_waits_and_one_nothing_tells_the_turns_of_takes_it_at_once() {
        let unstarted = Moment {
            activity: None,
            ..at_rest()
        };
        assert_eq!(hold(&unstarted, false), Some(Hold::NotStarted));
        assert_eq!(hold(&unstarted, true), Some(Hold::NotStarted));
        let silent = Moment {
            activity: None,
            signals: false,
            given_a_turn: true,
            ..at_rest()
        };
        assert_eq!(hold(&silent, false), None);
    }
}
