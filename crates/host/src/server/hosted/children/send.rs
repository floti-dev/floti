//! Sending: what `agents_send` and `floti agent send` do. The caller is its
//! child's user for the one turn: the text arrives in the child's terminal as
//! one bracketed paste followed by the backend's submit key, as if it had been
//! typed there.
//!
//! **Only the caller's own children.** A name is resolved among the caller's
//! children, and anything else is not found. A child that has ended takes
//! nothing, and a command child, which takes no turns, is refused too.
//!
//! **Every message is queued, and typed when the child can take it.** A send
//! puts its text at the end of the child's queue in floti.db
//! ([`super::queue`]) and answers at once. The queue is delivered in order,
//! one message at a time, at the first moment the child can take one with
//! nothing typed over a person ([`super::queue::hold`]): its turn has ended
//! (and the turn the last message gave it too), no person has typed into its
//! terminal since (their keys come first, and may be half a line in the
//! prompt), and no dialog is open (Floti answers none). An interrupt does not
//! wait for the turn to end, and still waits for the rest.
//!
//! **When a message is delivered.** A send tries at once, and answers that the
//! text was sent when it was typed then; otherwise it answers that the text is
//! queued, and why, and the parent hears delivered news when it is typed. The
//! queue is tried again at each moment that can change the answer: the
//! activity reader seeing a child's activity move or its turn end
//! (`observe_activity`), a host starting, and the turn a message gave lapsing
//! ([`UNSEEN_TURN_GRACE`]). Nothing is read from the child's screen. A message
//! whose child ends before it can be delivered is dropped: no delivered news
//! comes for it.
//!
//! **A turn nothing saw.** The next message waits for the turn the last one
//! gave to end. A text the agent answers without a turn (one it takes as a
//! command of its own) changes none of its signals, so that turn is taken as
//! over once [`UNSEEN_TURN_GRACE`] has passed with no sign of it; a turn in
//! progress then still holds the queue by its own activity.
//!
//! **The keys are the backend's.** What opens and closes a paste, what
//! submits it, what stops a turn and how long to wait between the end of the
//! paste and the submit key are declared by the child's adapter
//! ([`AgentInput`]); nothing here names an agent. A backend that declares no
//! input (a plain terminal or command) takes no turns.
//!
//! **The pause is a timer.** An agent that reads the paste and the key after it
//! together does not submit on that key, so the submit key is pressed only
//! after the adapter's pause. With `interrupt` the stop key is pressed first,
//! and the text follows after the same pause, so that the stop is read apart
//! from the paste's opening.
//!
//! **Typing is one turn at a time.** Deliveries to one child are taken in
//! turn, so the paste and submit of one never come between another's. A send
//! holds the child's turn from before it queues its text until it answers, so
//! a pass a moment starts meanwhile never types that text behind its back and
//! the answer says truly whether it was sent or why it waits.

use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::sleep;

use super::inbox::{self, EventKind, Facts};
use super::queue::{self, Hold, Moment, Queued};
use super::{not_found, off_runtime, rows};
use crate::agent::AgentInput;
use crate::server::hosted::Hosted;
use crate::session::{ReportState, Session, SessionId, activity_of_report};
use floti_protocol::channels::sessions::SessionActivity;

/// What `agents_send` was asked: the child, the text and whether to stop the
/// child's turn in progress first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SendChild {
    pub child: String,
    pub text: String,
    pub interrupt: bool,
}

/// What a send did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildSent {
    /// The child's name.
    pub name: String,
    /// Whether the stop key is pressed first.
    pub interrupted: bool,
    /// Why the text waits in the child's queue; `None` when it was typed
    /// before the send answered.
    pub queued: Option<QueuedSend>,
}

/// A send whose text waits in its child's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueuedSend {
    /// How many messages sent to the child before it wait too.
    pub ahead: usize,
    /// Why the first of them cannot be typed now.
    pub why: Hold,
}

/// Where the bytes of a turn go: a child's terminal.
pub(super) trait Keys {
    /// Presses `bytes`.
    ///
    /// # Errors
    ///
    /// A sentence when the terminal takes no more.
    fn press(&self, bytes: &[u8]) -> Result<(), String>;
}

impl Keys for Session {
    fn press(&self, bytes: &[u8]) -> Result<(), String> {
        self.type_for_parent(bytes).map_err(|err| err.to_string())
    }
}

/// Types one turn: the stop key and its pause when `interrupt`, then `text` as
/// one paste, the pause, and the submit key.
///
/// # Errors
///
/// A sentence when `interrupt` is asked of an agent with no stop key (before
/// any key is pressed), or when the terminal takes no more.
pub(super) async fn type_turn(
    keys: &impl Keys,
    input: &AgentInput,
    text: &str,
    interrupt: bool,
) -> Result<(), String> {
    let stop = match (&input.stop, interrupt) {
        (Some(stop), true) => Some(stop),
        (None, true) => return Err(NO_STOP_KEY.to_owned()),
        (_, false) => None,
    };
    if let Some(stop) = stop {
        keys.press(stop)?;
        sleep(input.pause).await;
    }
    keys.press(&input.paste(text))?;
    sleep(input.pause).await;
    keys.press(&input.submit)
}

/// The refusal of an interrupt to an agent that has no stop key.
const NO_STOP_KEY: &str = "That agent has no key that stops a turn.";

/// How long the turn a message gave may go with no sign of it before it is
/// taken as over. An agent shows a turn it began within a moment of the
/// submit key; one that shows none in this long took the text without one.
pub(super) const UNSEEN_TURN_GRACE: Duration = Duration::from_secs(10);

/// After `grace`, takes the turn a message gave as over (`lapse`, which says
/// whether it was still in hand) and, when it was, marks the moment
/// (`moment`), so that the queue is tried again.
async fn lapse_unseen_turn(grace: Duration, lapse: impl FnOnce() -> bool, moment: impl FnOnce()) {
    sleep(grace).await;
    if lapse() {
        moment();
    }
}

/// A child that can be sent to: its terminal, its name and how its backend
/// takes a turn.
struct Target {
    session: Arc<Session>,
    name: String,
    input: AgentInput,
}

/// A running child whose queue is being delivered: its terminal, how it
/// takes a turn, whether its backend keeps records of its sessions, and
/// whether anything tells its turns.
struct Recipient {
    session: Arc<Session>,
    input: AgentInput,
    records: bool,
    signals: bool,
}

/// What a look at a child's queue found.
enum Next {
    /// Nothing waits for it.
    Empty,
    /// It has ended or takes no turns: what waited for it was dropped.
    Gone,
    /// The first message waits, for this reason.
    Held(Hold),
    /// The first message can be typed now.
    Deliver(Recipient, Queued),
}

/// What one delivery of a child's queue did.
#[derive(Debug, Default)]
struct Delivery {
    /// The messages typed, in order.
    delivered: Vec<i64>,
    /// Why the first message left waits, when one does.
    held: Option<Hold>,
}

impl Hosted {
    /// Queues `request.text` for the caller's child `request.child` and
    /// delivers its queue as far as it can be now.
    ///
    /// # Errors
    ///
    /// A sentence when the name is not one of the caller's children, the
    /// child has ended, is a command child, its backend takes no typed turns
    /// or has no stop key for an interrupt, or the text is empty.
    pub(crate) async fn send_to_child(
        self: &Arc<Self>,
        parent: SessionId,
        request: SendChild,
    ) -> Result<ChildSent, String> {
        if request.text.trim().is_empty() {
            return Err("Give the text to send: text is the child's next turn.".to_owned());
        }
        let target = {
            let hosted = Arc::clone(self);
            let child = request.child.clone();
            off_runtime(move || hosted.target_of(parent, &child)).await?
        };
        if request.interrupt && target.input.stop.is_none() {
            return Err(format!("{} {NO_STOP_KEY}", target.name));
        }
        let child = target.session.id();
        // Held from before the text is queued until the send has answered:
        // a pass that a moment starts meanwhile cannot type it, so the
        // answer says what became of it.
        let turn = self.children.typing_lock(child);
        let _one_at_a_time = turn.lock().await;
        let id = {
            let hosted = Arc::clone(self);
            let text = request.text.clone();
            off_runtime(move || {
                queue::push(
                    &hosted.ledger.store(),
                    child,
                    parent,
                    &text,
                    request.interrupt,
                )
                .map_err(|err| err.to_string())
            })
            .await?
        };
        let delivery = self.deliver_locked(child, Some(id)).await;
        let queued = if delivery.delivered.contains(&id) {
            None
        } else {
            let hosted = Arc::clone(self);
            let ahead = off_runtime(move || {
                queue::waiting(&hosted.ledger.store(), child)
                    .map(|waiting| waiting.iter().take_while(|queued| queued.id != id).count())
                    .map_err(|err| err.to_string())
            })
            .await?;
            Some(QueuedSend {
                ahead,
                why: delivery.held.unwrap_or(Hold::Working),
            })
        };
        Ok(ChildSent {
            name: target.name,
            interrupted: request.interrupt,
            queued,
        })
    }

    /// Delivers child `child`'s queue, in order, for as long as it can take
    /// the next message. The message `quiet` (the one a send is about to
    /// answer for) gives no delivered news when it is typed: its send says
    /// so. A child that has ended, or no longer takes turns, has what waited
    /// for it dropped.
    async fn deliver_to(self: &Arc<Self>, child: SessionId, quiet: Option<i64>) -> Delivery {
        let turn = self.children.typing_lock(child);
        let _one_at_a_time = turn.lock().await;
        self.deliver_locked(child, quiet).await
    }

    /// [`Self::deliver_to`], by a caller that holds child `child`'s typing
    /// lock.
    async fn deliver_locked(self: &Arc<Self>, child: SessionId, quiet: Option<i64>) -> Delivery {
        let mut delivery = Delivery::default();
        // The records as of now, not as of the last pass: a prompt that
        // appeared since is seen before anything is typed, and a pass that
        // saw a turn end has spent what it spent.
        self.read_activity_now().await;
        loop {
            let next = {
                let hosted = Arc::clone(self);
                off_runtime(move || Ok(hosted.next_delivery(child))).await
            };
            let (recipient, message) = match next {
                Ok(Next::Deliver(recipient, message)) => (recipient, message),
                Ok(Next::Held(hold)) => {
                    delivery.held = Some(hold);
                    return delivery;
                }
                Ok(Next::Empty | Next::Gone) => return delivery,
                Err(err) => {
                    log_line!("floti host: {err}");
                    return delivery;
                }
            };
            // Marked before the first key: an idle that began after this
            // moment is the end of the turn the message gives.
            let given = recipient.session.give_a_turn();
            let session = Arc::downgrade(&recipient.session);
            let hosted = Arc::downgrade(self);
            tokio::spawn(lapse_unseen_turn(
                UNSEEN_TURN_GRACE,
                move || {
                    session
                        .upgrade()
                        .is_some_and(|session| session.lapse_given_turn(given))
                },
                move || {
                    if let Some(hosted) = hosted.upgrade() {
                        hosted.children.news.queue_moment();
                    }
                },
            ));
            let typed = type_turn(
                &*recipient.session,
                &recipient.input,
                &message.text,
                message.interrupt,
            )
            .await;
            let hosted = Arc::clone(self);
            let news = quiet != Some(message.id);
            let id = message.id;
            let failed = typed.is_err();
            let _ = off_runtime(move || {
                hosted.delivered_one(
                    child,
                    &message,
                    typed.as_ref().err().map(String::as_str),
                    news,
                );
                Ok(())
            })
            .await;
            if failed {
                return delivery;
            }
            delivery.delivered.push(id);
        }
    }

    /// What child `child`'s queue holds first, and whether it can be typed.
    fn next_delivery(&self, child: SessionId) -> Next {
        let store = self.ledger.store();
        let waiting = match queue::waiting(&store, child) {
            Ok(waiting) => waiting,
            Err(err) => {
                log_line!("floti host: {err}");
                return Next::Empty;
            }
        };
        let Some(first) = waiting.into_iter().next() else {
            return Next::Empty;
        };
        let recipient = self.sessions.get(child).and_then(|session| {
            let backend = crate::server::records::read_record(&store, child)
                .ok()
                .and_then(|record| self.find_backend(&record.backend).cloned())?;
            let input = backend.input()?;
            let records = backend.status().is_some();
            let signals = records || session.reported_status().is_some();
            Some(Recipient {
                session,
                input,
                records,
                signals,
            })
        });
        let Some(recipient) = recipient else {
            if let Err(err) = queue::drop_waiting(&store, child) {
                log_line!("floti host: {err}");
            }
            return Next::Gone;
        };
        drop(store);
        let session = &recipient.session;
        let reported = session.reported_status();
        // A backend's records are read by the pass, and win over a report, as
        // the pass has them. A program's own report is the session's as it
        // stands, newer than any pass, so it is read here.
        let activity = match &reported {
            Some(status) if !recipient.records => activity_of_report(status),
            _ => None,
        }
        .or_else(|| self.children.news.activity_of(child));
        let moment = Moment {
            person_typed: session.person_typed(),
            at_a_prompt: matches!(
                &reported,
                Some(status) if status.report.state == ReportState::Blocked
            ),
            activity,
            begun: activity.is_some() || self.children.news.has_begun(child),
            signals: recipient.signals || activity.is_some(),
            given_a_turn: session.given_a_turn(),
        };
        match queue::hold(&moment, first.interrupt) {
            Some(hold) => Next::Held(hold),
            None => Next::Deliver(recipient, first),
        }
    }

    /// Message `message` has been typed into child `child`, or typing it
    /// failed with `failed`: it waits no more, and unless the send that
    /// queued it is answering for it (`news` false), its parent hears that it
    /// was delivered. One that could not be typed is dropped, since a
    /// terminal that takes no more keys is one that is ending.
    fn delivered_one(&self, child: SessionId, message: &Queued, failed: Option<&str>, news: bool) {
        let store = self.ledger.store();
        if let Some(err) = failed {
            log_line!("floti host: a queued message could not be typed: {err}");
            if let Err(err) = queue::dropped(&store, message.id) {
                log_line!("floti host: {err}");
            }
            return;
        }
        if let Err(err) = queue::delivered(&store, message.id) {
            log_line!("floti host: {err}");
        }
        drop(store);
        if news {
            self.write_news(
                child,
                EventKind::Delivered,
                &Facts {
                    state: message.interrupt.then(|| inbox::INTERRUPTED.to_owned()),
                    detail: Some(message.text.trim().to_owned()),
                    exit_code: None,
                },
            );
        }
    }

    /// Delivers every child's queue as far as it can be now, each child's on
    /// its own so that one being typed into holds up no other.
    pub(crate) async fn deliver_queued(self: &Arc<Self>) {
        let hosted = Arc::clone(self);
        let children = off_runtime(move || {
            queue::children_waiting(&hosted.ledger.store()).map_err(|err| err.to_string())
        })
        .await;
        match children {
            Ok(children) => {
                for child in children {
                    let hosted = Arc::clone(self);
                    tokio::spawn(async move {
                        hosted.deliver_to(child, None).await;
                    });
                }
            }
            Err(err) => log_line!("floti host: {err}"),
        }
    }

    /// The moments a queued message may have become deliverable.
    pub(crate) fn queue_moments(&self) -> watch::Receiver<u64> {
        self.children.news.queue_moments()
    }

    /// The running child of `parent` called `name`, with its backend's input.
    fn target_of(&self, parent: SessionId, name: &str) -> Result<Target, String> {
        let (child, session) = {
            let store = self.ledger.store();
            let facts = rows::parent_facts(&store, parent).map_err(|err| err.to_string())?;
            let children = rows::children_of(&store, parent, facts.conversation_id.as_deref())
                .map_err(|err| err.to_string())?;
            let asked = name.trim();
            let Some(found) = children
                .iter()
                .rev()
                .find(|child| child.name.eq_ignore_ascii_case(asked))
            else {
                let running: Vec<&rows::ChildRow> = children
                    .iter()
                    .filter(|child| child.running && self.sessions.get(child.id).is_some())
                    .collect();
                return Err(not_found(name, &running));
            };
            if found.is_command() {
                return Err(format!(
                    "{} is a command child, and a command child takes no turns, so nothing was sent. \
                     Wait on it for its exit code and last lines, or stop it.",
                    found.name
                ));
            }
            let session = self.sessions.get(found.id).filter(|_| found.running);
            (
                (found.name.clone(), found.backend.clone()),
                session.ok_or_else(|| {
                    format!(
                        "{} has ended, so it takes nothing more. Resume it with agents_start \
                         resume to give it another turn.",
                        found.name
                    )
                })?,
            )
        };
        let (name, backend) = child;
        let input = self
            .find_backend(&floti_protocol::BackendId::new(backend))
            .and_then(|backend| backend.input())
            .ok_or_else(|| format!("{name} takes no typed turns: it is not an agent."))?;
        Ok(Target {
            session,
            name,
            input,
        })
    }
}

/// Delivers the queues at each moment one may have become deliverable, and
/// once when it starts, for what was queued before a host restart. It ends
/// with the host.
pub(crate) async fn deliver_at_each_moment(
    mut moments: watch::Receiver<u64>,
    hosted: Weak<Hosted>,
) {
    moments.mark_changed();
    while moments.changed().await.is_ok() {
        let Some(hosted) = hosted.upgrade() else {
            return;
        };
        hosted.deliver_queued().await;
    }
}

/// Whether a child doing `activity` is still at work for a wait: anything but
/// at rest or waiting on a person, and at rest with a message waiting in its
/// queue (`queued`), which it is yet to be given.
pub(super) fn still_working(activity: Option<SessionActivity>, queued: bool) -> bool {
    match activity {
        Some(SessionActivity::Waiting) => false,
        Some(SessionActivity::Idle | SessionActivity::Error) => queued,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use tokio::time::Instant;

    use super::*;

    /// Keys pressed, each with the instant of the clock it was pressed at.
    #[derive(Default)]
    struct Pressed(Mutex<Vec<(Instant, Vec<u8>)>>);

    impl Keys for Pressed {
        fn press(&self, bytes: &[u8]) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push((Instant::now(), bytes.to_vec()));
            Ok(())
        }
    }

    const PAUSE: Duration = Duration::from_millis(250);

    fn input() -> AgentInput {
        AgentInput::bracketed(b"\r", Some(b"\x1b"), PAUSE)
    }

    #[tokio::test(start_paused = true)]
    async fn the_text_is_one_paste_and_the_submit_key_comes_only_after_the_pause() {
        let keys = Pressed::default();
        let started = Instant::now();
        let input = input();
        let turn = type_turn(&keys, &input, "one\ntwo", false);
        tokio::pin!(turn);

        // Nothing but the paste has been pressed while the pause runs.
        tokio::select! {
            biased;
            result = &mut turn => panic!("finished before the pause: {result:?}"),
            () = tokio::time::sleep(PAUSE - Duration::from_millis(1)) => {}
        }
        {
            let pressed = keys.0.lock().unwrap();
            assert_eq!(pressed.len(), 1);
            assert_eq!(pressed[0].1, b"\x1b[200~one\ntwo\x1b[201~".to_vec());
            assert_eq!(pressed[0].0, started);
        }

        turn.await.unwrap();
        let pressed = keys.0.lock().unwrap();
        assert_eq!(pressed.len(), 2);
        assert_eq!(pressed[1].1, b"\r".to_vec());
        assert_eq!(pressed[1].0 - pressed[0].0, PAUSE);
    }

    #[tokio::test(start_paused = true)]
    async fn an_interrupt_presses_the_stop_key_first_then_pastes_and_submits_each_after_the_pause()
    {
        let keys = Pressed::default();
        type_turn(&keys, &input(), "instead", true).await.unwrap();
        let pressed = keys.0.lock().unwrap();
        let sent: Vec<&[u8]> = pressed.iter().map(|(_, bytes)| bytes.as_slice()).collect();
        assert_eq!(sent, [&b"\x1b"[..], b"\x1b[200~instead\x1b[201~", b"\r"]);
        assert_eq!(pressed[1].0 - pressed[0].0, PAUSE);
        assert_eq!(pressed[2].0 - pressed[1].0, PAUSE);
    }

    #[tokio::test(start_paused = true)]
    async fn an_interrupt_of_an_agent_with_no_stop_key_is_refused_before_any_key_is_pressed() {
        let keys = Pressed::default();
        let mut input = input();
        input.stop = None;
        let refused = type_turn(&keys, &input, "instead", true).await.unwrap_err();
        assert!(refused.contains("no key that stops a turn"), "{refused}");
        assert!(keys.0.lock().unwrap().is_empty());
        // The same agent takes the text without an interrupt.
        type_turn(&keys, &input, "instead", false).await.unwrap();
        assert_eq!(keys.0.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn the_turn_a_message_gave_lapses_after_the_grace_and_marks_a_moment_only_if_still_in_hand()
     {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let moments = Arc::new(AtomicUsize::new(0));
        let lapse = |in_hand: bool| {
            let moments = Arc::clone(&moments);
            tokio::spawn(lapse_unseen_turn(
                UNSEEN_TURN_GRACE,
                move || in_hand,
                move || {
                    moments.fetch_add(1, Ordering::SeqCst);
                },
            ))
        };
        let still = lapse(true);
        let seen = lapse(false);
        tokio::time::advance(UNSEEN_TURN_GRACE - Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(moments.load(Ordering::SeqCst), 0);
        assert!(!still.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        still.await.unwrap();
        seen.await.unwrap();
        // The turn whose end was seen, or that a later message replaced,
        // marks nothing.
        assert_eq!(moments.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_child_at_rest_with_a_message_queued_is_still_at_work_for_a_wait() {
        assert!(!still_working(Some(SessionActivity::Idle), false));
        assert!(!still_working(Some(SessionActivity::Waiting), false));
        assert!(still_working(Some(SessionActivity::Busy), false));
        assert!(still_working(None, false));
        assert!(still_working(Some(SessionActivity::Idle), true));
        assert!(still_working(Some(SessionActivity::Error), true));
        // One that needs a person is the user's, queued message or not.
        assert!(!still_working(Some(SessionActivity::Waiting), true));
    }
}
