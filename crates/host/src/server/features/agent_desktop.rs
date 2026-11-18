//! The floti-desktop tools' relay: `agentDesktop:call` and
//! `agentDesktop:answer`.
//!
//! The host serves floti-desktop to its sessions, but Floti's views are a
//! window's. So each call is carried to one attached client that declared
//! `desktop` and listens on `agentDesktop:call`, and the tool answers with
//! what that client answers.
//!
//! **Which window.** Of the clients listening, the call goes to the one
//! showing the caller's tab, where a child shown inside its parent's tab
//! counts as shown there (the caller or a session above it is on that
//! client's screen, as it last said on `session:focus`); else one on the
//! device that started the caller (for a child, the nearest session above it
//! that a device started); else the one most recently in front. Ties go to
//! the one most recently in front, and then to the one attached longest.
//!
//! **No timers.** A call ends with its answer, with the client it was sent to
//! leaving or no longer listening (it fails at once, saying no window is
//! attached), or with the agent hanging up, which forgets it. A call made
//! while no window is attached fails at once the same way.
//!
//! **Only the client a call was sent to answers it.** An answer naming a call
//! sent to another client is refused, and the call goes on waiting for its
//! own client.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::channel::Router;
use floti_protocol::channels::agent_desktop::{
    AgentDesktopAnswer, AgentDesktopCalls, AgentDesktopOutcome, AgentDesktopToolAnswer,
    AgentDesktopToolCall, listens_for_desktop_calls,
};
use floti_protocol::{ClientChannels, Payload};
use tokio::sync::oneshot;

use super::sessions::Focus;
use super::{Caller, Feature};
use crate::server::events::Events;
use crate::session::ClientId;

/// The fingerprint of the device an attached client's connection proved.
type DeviceOf = Box<dyn Fn(ClientId) -> Option<String> + Send + Sync>;

/// Who made a call, as the choice of window reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DesktopCaller {
    /// The caller's session, then each session above it, nearest first.
    pub lineage: Vec<u64>,
    /// The fingerprint of the device that started the caller, or for a
    /// child the nearest session above it that a device started; none for a
    /// session no device started.
    pub starter: Option<String>,
}

impl DesktopCaller {
    /// The session whose agent made the call.
    pub fn session(&self) -> u64 {
        self.lineage.first().copied().unwrap_or_default()
    }
}

/// The relay: which clients can take a call, and the calls waiting on one.
pub(crate) struct DesktopRelay {
    events: Arc<Events>,
    focus: Arc<Mutex<Focus>>,
    device_of: DeviceOf,
    state: Mutex<Relay>,
}

#[derive(Default)]
struct Relay {
    /// The clients a call may go to, the longest attached first.
    windows: BTreeSet<ClientId>,
    /// The number of the last call.
    last_call: u64,
    pending: HashMap<u64, Pending>,
}

/// A call sent to a client and not answered yet.
struct Pending {
    client: ClientId,
    answer: oneshot::Sender<AgentDesktopOutcome>,
}

impl DesktopRelay {
    /// Serves `agentDesktop:call` one client at a time on `events`, choosing
    /// a client by what `focus` holds and the device `device_of` names for
    /// it.
    pub fn new(
        events: &Arc<Events>,
        focus: Arc<Mutex<Focus>>,
        device_of: impl Fn(ClientId) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        events.serve_per_client::<AgentDesktopCalls>();
        Self {
            events: Arc::clone(events),
            focus,
            device_of: Box::new(device_of),
            state: Mutex::new(Relay::default()),
        }
    }

    /// Whether a call made now would go to a client.
    pub fn has_window(&self) -> bool {
        !self.state().windows.is_empty()
    }

    /// Carries one call of `tool` with `arguments`, made by `caller`'s
    /// agent, to the window chosen for it, and answers what it answers.
    /// Dropping the future forgets the call.
    pub async fn call(
        &self,
        caller: &DesktopCaller,
        tool: &str,
        arguments: Payload,
    ) -> AgentDesktopOutcome {
        let (call, client, answered) = {
            let mut state = self.state();
            let Some(client) = self.choose(&state.windows, caller) else {
                return AgentDesktopOutcome::no_window();
            };
            state.last_call += 1;
            let call_id = state.last_call;
            let (answer, answered) = oneshot::channel();
            state.pending.insert(call_id, Pending { client, answer });
            let call = AgentDesktopToolCall {
                call_id,
                session: caller.session(),
                tool: tool.to_owned(),
                arguments,
            };
            (call, client, answered)
        };
        let forget = Forget {
            relay: self,
            call_id: call.call_id,
        };
        // Sent once it is pending: a client leaving from here on fails it.
        if let Err(err) = self.events.emit_to::<AgentDesktopCalls>(client, &call) {
            return AgentDesktopOutcome::Failed {
                message: err.to_string(),
            };
        }
        let outcome = answered
            .await
            .unwrap_or_else(|_| AgentDesktopOutcome::no_window());
        drop(forget);
        outcome
    }

    /// The window a call from `caller` goes to, of `windows`: the one
    /// showing the caller's tab, else one on the device that started it,
    /// else the one most recently in front; ties to the most recently in
    /// front, then to the longest attached.
    fn choose(&self, windows: &BTreeSet<ClientId>, caller: &DesktopCaller) -> Option<ClientId> {
        let focus = lock(&self.focus);
        let rank = |client: ClientId| {
            let shows = focus.shows_any(client, &caller.lineage);
            let started = caller
                .starter
                .as_ref()
                .is_some_and(|starter| (self.device_of)(client).as_ref() == Some(starter));
            (shows, started, focus.fronted(client))
        };
        let mut best: Option<(ClientId, (bool, bool, u64))> = None;
        for &client in windows {
            let ranked = rank(client);
            // Strictly better only, so a full tie keeps the longest attached.
            if best.is_none_or(|(_, top)| ranked > top) {
                best = Some((client, ranked));
            }
        }
        best.map(|(client, _)| client)
    }

    /// `client`'s answer to one call: refused when the call is not waiting
    /// or was sent to another client.
    fn answered(&self, client: ClientId, answer: AgentDesktopToolAnswer) -> Result<(), String> {
        let mut state = self.state();
        let call_id = answer.call_id;
        match state.pending.get(&call_id) {
            None => Err(format!(
                "Desktop tool call {call_id} is not waiting for an answer: it was answered \
                 already, its session stopped waiting, or it was never made."
            )),
            Some(pending) if pending.client != client => Err(format!(
                "Desktop tool call {call_id} was sent to another client, so this one cannot \
                 answer it."
            )),
            Some(_) => {
                if let Some(pending) = state.pending.remove(&call_id) {
                    let _ = pending.answer.send(answer.outcome);
                }
                Ok(())
            }
        }
    }

    /// `client` takes calls from now on, or no longer does; when it no
    /// longer does, every call waiting on it fails at once.
    fn listening(&self, client: ClientId, listening: bool) {
        let mut state = self.state();
        if listening {
            state.windows.insert(client);
            return;
        }
        state.windows.remove(&client);
        let gone: Vec<u64> = state
            .pending
            .iter()
            .filter(|(_, pending)| pending.client == client)
            .map(|(call_id, _)| *call_id)
            .collect();
        for call_id in gone {
            if let Some(pending) = state.pending.remove(&call_id) {
                let _ = pending.answer.send(AgentDesktopOutcome::no_window());
            }
        }
    }

    fn state(&self) -> MutexGuard<'_, Relay> {
        lock(&self.state)
    }
}

/// Forgets a call when its caller stops waiting, answered or not.
struct Forget<'a> {
    relay: &'a DesktopRelay,
    call_id: u64,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        self.relay.state().pending.remove(&self.call_id);
    }
}

impl Feature for Arc<DesktopRelay> {
    fn requests<'a>(&'a self, Caller { client, .. }: Caller, router: &mut Router<'a>) {
        router.handle::<AgentDesktopAnswer>(move |answer| self.answered(client, answer));
    }

    fn subscriptions_changed(&self, client: ClientId, channels: &ClientChannels<'_>) {
        self.listening(client, listens_for_desktop_calls(channels));
    }

    fn client_left(&self, client: ClientId) {
        self.listening(client, false);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use floti_protocol::EventChannel as _;
    use floti_protocol::channel::from_payload;

    use super::*;

    const FIRST: ClientId = ClientId(1);
    const SECOND: ClientId = ClientId(2);
    const THIRD: ClientId = ClientId(3);

    struct World {
        events: Arc<Events>,
        focus: Arc<Mutex<Focus>>,
        relay: Arc<DesktopRelay>,
    }

    /// A relay whose clients are on the devices `devices` names, by client
    /// number.
    fn world(devices: &'static [(u64, &'static str)]) -> World {
        let events = Arc::new(Events::new());
        let focus = Arc::new(Mutex::new(Focus::default()));
        let relay = Arc::new(DesktopRelay::new(&events, Arc::clone(&focus), |client| {
            devices
                .iter()
                .find(|(number, _)| *number == client.0)
                .map(|(_, device)| (*device).to_owned())
        }));
        World {
            events,
            focus,
            relay,
        }
    }

    fn caller(lineage: &[u64], starter: Option<&str>) -> DesktopCaller {
        DesktopCaller {
            lineage: lineage.to_vec(),
            starter: starter.map(str::to_owned),
        }
    }

    fn done(text: &str) -> AgentDesktopOutcome {
        AgentDesktopOutcome::Done {
            value: Payload::Text(text.to_owned()),
        }
    }

    fn chosen(world: &World, caller: &DesktopCaller) -> Option<ClientId> {
        let state = world.relay.state();
        world.relay.choose(&state.windows, caller)
    }

    /// The next call sent to `client`'s connection.
    async fn sent(
        listening: &mut tokio::sync::broadcast::Receiver<crate::server::events::Publication>,
        client: ClientId,
    ) -> AgentDesktopToolCall {
        loop {
            let published = listening.recv().await.expect("a publication");
            if published.channel == AgentDesktopCalls::NAME && published.is_for(client) {
                return from_payload(&published.payload).expect("a call");
            }
        }
    }

    #[tokio::test]
    async fn a_call_with_no_window_fails_at_once_saying_no_window_is_attached() {
        let world = world(&[]);
        assert!(!world.relay.has_window());
        let outcome = world
            .relay
            .call(&caller(&[3], None), "desktop_open", Payload::Null)
            .await;
        assert_eq!(outcome, AgentDesktopOutcome::no_window());
        let AgentDesktopOutcome::Failed { message } = outcome else {
            unreachable!()
        };
        assert!(message.contains("No window is attached"), "{message}");
    }

    #[test]
    fn the_window_showing_the_callers_tab_is_chosen_whichever_was_in_front_last() {
        let world = world(&[(1, "laptop"), (2, "desktop")]);
        world.relay.listening(FIRST, true);
        world.relay.listening(SECOND, true);
        {
            let mut focus = lock(&world.focus);
            focus.set(FIRST, [7], Some(false));
            focus.set(SECOND, [9], Some(true));
        }
        assert_eq!(chosen(&world, &caller(&[7], Some("desktop"))), Some(FIRST));
        // A child shown inside its parent's tab is shown where the parent is.
        assert_eq!(chosen(&world, &caller(&[12, 7], None)), Some(FIRST));
        // Shown in both: the one in front last.
        lock(&world.focus).set(SECOND, [9, 7], Some(true));
        assert_eq!(chosen(&world, &caller(&[7], None)), Some(SECOND));
    }

    #[test]
    fn with_the_callers_tab_shown_nowhere_the_starting_device_then_the_one_in_front_last_is_chosen()
    {
        let world = world(&[(1, "laptop"), (2, "desktop"), (3, "desktop")]);
        for client in [FIRST, SECOND, THIRD] {
            world.relay.listening(client, true);
        }
        {
            let mut focus = lock(&world.focus);
            focus.set(SECOND, [], Some(true));
            focus.set(THIRD, [], Some(true));
            focus.set(FIRST, [], Some(true));
        }
        // Started on the laptop: its window, though another came to the front.
        assert_eq!(chosen(&world, &caller(&[5], Some("laptop"))), Some(FIRST));
        // Two windows on the starting device: the one in front last.
        assert_eq!(chosen(&world, &caller(&[5], Some("desktop"))), Some(THIRD));
        // No device started it: the one in front last of all.
        assert_eq!(chosen(&world, &caller(&[5], None)), Some(FIRST));
        // Nobody ever in front and nothing else to go by: the longest attached.
        let quiet = world_with_three_quiet_windows();
        assert_eq!(chosen(&quiet, &caller(&[5], None)), Some(FIRST));
    }

    fn world_with_three_quiet_windows() -> World {
        let world = world(&[]);
        for client in [THIRD, FIRST, SECOND] {
            world.relay.listening(client, true);
        }
        world
    }

    #[tokio::test]
    async fn a_call_goes_to_the_chosen_window_naming_its_session_and_takes_its_answer() {
        let world = world(&[]);
        let mut listening = world.events.listen(SECOND);
        world.relay.listening(FIRST, true);
        world.relay.listening(SECOND, true);
        lock(&world.focus).set(SECOND, [7], Some(true));
        let calling = {
            let relay = Arc::clone(&world.relay);
            tokio::spawn(async move {
                relay
                    .call(
                        &caller(&[7], None),
                        "desktop_open",
                        Payload::Text("args".into()),
                    )
                    .await
            })
        };
        let call = sent(&mut listening, SECOND).await;
        assert_eq!(call.session, 7);
        assert_eq!(call.tool, "desktop_open");
        assert_eq!(call.arguments, Payload::Text("args".into()));
        // The window the call did not go to cannot answer it.
        let refused = world
            .relay
            .answered(
                FIRST,
                AgentDesktopToolAnswer {
                    call_id: call.call_id,
                    outcome: done("forged"),
                },
            )
            .unwrap_err();
        assert!(refused.contains("sent to another client"), "{refused}");
        world
            .relay
            .answered(
                SECOND,
                AgentDesktopToolAnswer {
                    call_id: call.call_id,
                    outcome: done("opened"),
                },
            )
            .unwrap();
        assert_eq!(calling.await.unwrap(), done("opened"));
        // Answered once: a second answer is refused.
        assert!(
            world
                .relay
                .answered(
                    SECOND,
                    AgentDesktopToolAnswer {
                        call_id: call.call_id,
                        outcome: done("again"),
                    },
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn calls_waiting_on_a_window_that_leaves_or_stops_listening_fail_at_once() {
        for leaves in [true, false] {
            let world = world(&[]);
            let mut listening = world.events.listen(FIRST);
            world.relay.listening(FIRST, true);
            let calling = {
                let relay = Arc::clone(&world.relay);
                tokio::spawn(async move {
                    relay
                        .call(&caller(&[1], None), "desktop_open", Payload::Null)
                        .await
                })
            };
            let _call = sent(&mut listening, FIRST).await;
            if leaves {
                world.relay.client_left(FIRST);
            } else {
                world.relay.listening(FIRST, false);
            }
            assert_eq!(calling.await.unwrap(), AgentDesktopOutcome::no_window());
            assert!(!world.relay.has_window());
            assert!(world.relay.state().pending.is_empty());
        }
    }

    #[tokio::test]
    async fn a_caller_that_stops_waiting_leaves_nothing_pending() {
        let world = world(&[]);
        let mut listening = world.events.listen(FIRST);
        world.relay.listening(FIRST, true);
        let calling = {
            let relay = Arc::clone(&world.relay);
            tokio::spawn(async move {
                relay
                    .call(&caller(&[1], None), "desktop_open", Payload::Null)
                    .await
            })
        };
        let call = sent(&mut listening, FIRST).await;
        calling.abort();
        assert!(calling.await.unwrap_err().is_cancelled());
        assert!(world.relay.state().pending.is_empty());
        assert!(
            world
                .relay
                .answered(
                    FIRST,
                    AgentDesktopToolAnswer {
                        call_id: call.call_id,
                        outcome: done("late"),
                    },
                )
                .is_err()
        );
    }
}
