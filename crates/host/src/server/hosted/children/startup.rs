//! A child that has not begun its first turn a minute after it was launched.
//! Nobody is at its terminal, so a prompt it stopped on before its first turn
//! (a folder its agent has not been told to trust, a sign-in) is never
//! answered: the child is needs-you news for its parent, and a needs-you
//! notice for the person, until it begins.
//!
//! **Begun** is what the agent and its program say, as the activity reader
//! sees them: the backend's own record of the session exists (for Claude Code
//! `<config>/sessions/<pid>.json`, which it writes once its start-up dialogs
//! are behind it), its program has sent an OSC 7501 report, or its activity is
//! known. Only a child whose backend keeps records is watched: one that keeps
//! none cannot be told from one that has begun.
//!
//! **The minute** is Tokio's clock: each launch is announced with the instant
//! it is due, and [`after_each_launch`] sleeps until then and has the overdue
//! children looked at, so a test steps it.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::time::Instant;

use crate::session::SessionId;

/// How long after its launch a child has to begin its first turn.
pub(crate) const STARTUP_GRACE: Duration = Duration::from_secs(60);

/// What the parent is told of a child that has not begun, after "needs you:".
pub(super) const NOT_STARTED: &str =
    "it has not started its task, and its terminal may be waiting on a prompt";

/// What the person's notice says of it.
pub(super) const NOT_STARTED_NOTICE: &str =
    "has not started its task: its terminal may be waiting on a prompt";

/// The children launched that have not begun their first turn.
#[derive(Debug)]
pub(super) struct Startups {
    /// How long a child has to begin: [`STARTUP_GRACE`] but in a test that
    /// cannot step the clock of the processes it runs.
    grace: Duration,
    /// Each child not yet overdue, with when it is due.
    due: HashMap<SessionId, Instant>,
    /// Children found overdue that have not begun since.
    stalled: HashSet<SessionId>,
}

impl Default for Startups {
    fn default() -> Self {
        Self {
            grace: STARTUP_GRACE,
            due: HashMap::new(),
            stalled: HashSet::new(),
        }
    }
}

impl Startups {
    /// A child launched from now on has `grace` to begin.
    pub fn use_grace(&mut self, grace: Duration) {
        self.grace = grace;
    }

    /// Child `id` was launched `at`. When it is due to have begun.
    pub fn launched(&mut self, id: SessionId, at: Instant) -> Instant {
        let due = at + self.grace;
        self.stalled.remove(&id);
        self.due.insert(id, due);
        due
    }

    /// Child `id` has begun. Whether it had been found overdue.
    pub fn begun(&mut self, id: SessionId) -> bool {
        self.due.remove(&id);
        self.stalled.remove(&id)
    }

    /// Whether child `id` was found overdue and has not begun since.
    pub fn stalled(&self, id: SessionId) -> bool {
        self.stalled.contains(&id)
    }

    /// The children due by `now` that have not begun, in id order, each given
    /// once: from then on they are stalled until they begin.
    pub fn overdue(&mut self, now: Instant) -> Vec<SessionId> {
        let mut overdue: Vec<SessionId> = self
            .due
            .iter()
            .filter(|(_, due)| **due <= now)
            .map(|(id, _)| *id)
            .collect();
        overdue.sort_unstable();
        for id in &overdue {
            self.due.remove(id);
            self.stalled.insert(*id);
        }
        overdue
    }

    /// Lets go of child `id`, which has ended.
    pub fn forget(&mut self, id: SessionId) {
        self.due.remove(&id);
        self.stalled.remove(&id);
    }
}

/// At each instant `launches` announces a child is due, runs what `look`
/// gives, which looks at every child that is overdue then. `look` gives
/// nothing once the host is gone. Announcements missed while behind need no
/// look of their own: those kept after them come later and are due no
/// sooner, and the look at each finds every child due by then. Ends when the
/// announcements do.
pub(crate) async fn after_each_launch<C, F>(mut launches: broadcast::Receiver<Instant>, look: C)
where
    C: Fn() -> Option<F> + Clone + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    loop {
        match launches.recv().await {
            Ok(due) => {
                let look = look.clone();
                tokio::spawn(async move {
                    tokio::time::sleep_until(due).await;
                    if let Some(looking) = look() {
                        looking.await;
                    }
                });
            }
            Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn a_child_is_overdue_a_minute_after_its_launch_once_and_not_before() {
        let launched = Instant::now();
        let mut startups = Startups::default();
        let due = startups.launched(SessionId(3), launched);
        assert_eq!(due, launched + STARTUP_GRACE);
        startups.launched(SessionId(1), launched + Duration::from_secs(10));

        let second = Duration::from_secs(1);
        assert!(
            startups
                .overdue(launched + STARTUP_GRACE - second)
                .is_empty()
        );
        assert_eq!(startups.overdue(launched + STARTUP_GRACE), [SessionId(3)]);
        assert!(startups.stalled(SessionId(3)));
        // Each is given once.
        assert!(
            startups
                .overdue(launched + STARTUP_GRACE + Duration::from_secs(5))
                .is_empty()
        );
        assert_eq!(
            startups.overdue(launched + STARTUP_GRACE * 2),
            [SessionId(1)]
        );
    }

    #[test]
    fn a_child_that_begins_in_time_is_never_overdue_and_one_that_begins_late_is_stalled_until_then()
    {
        let launched = Instant::now();
        let mut startups = Startups::default();
        startups.launched(SessionId(1), launched);
        startups.launched(SessionId(2), launched);

        assert!(!startups.begun(SessionId(1)), "it was not overdue");
        assert_eq!(startups.overdue(launched + STARTUP_GRACE), [SessionId(2)]);
        assert!(!startups.stalled(SessionId(1)));
        assert!(startups.stalled(SessionId(2)));

        assert!(startups.begun(SessionId(2)), "it had been overdue");
        assert!(!startups.stalled(SessionId(2)));

        // One that ends is let go of.
        startups.launched(SessionId(4), launched);
        startups.forget(SessionId(4));
        assert!(startups.overdue(launched + STARTUP_GRACE).is_empty());
    }

    #[test]
    fn a_grace_given_is_what_a_child_launched_after_it_has() {
        let launched = Instant::now();
        let mut startups = Startups::default();
        startups.launched(SessionId(1), launched);
        startups.use_grace(Duration::ZERO);
        assert_eq!(startups.launched(SessionId(2), launched), launched);
        assert_eq!(startups.overdue(launched), [SessionId(2)]);
    }

    #[tokio::test(start_paused = true)]
    async fn each_launch_is_looked_at_a_minute_after_it() {
        let (announce, launches) = broadcast::channel(4);
        let (looked, mut looks) = mpsc::unbounded_channel();
        tokio::spawn(after_each_launch(launches, move || {
            let looked = looked.clone();
            Some(async move {
                let _ = looked.send(Instant::now());
            })
        }));

        let mut startups = Startups::default();
        let first = Instant::now();
        announce
            .send(startups.launched(SessionId(1), first))
            .unwrap();
        tokio::time::sleep(Duration::from_secs(20)).await;
        let second = Instant::now();
        announce
            .send(startups.launched(SessionId(2), second))
            .unwrap();

        assert_eq!(looks.recv().await.unwrap() - first, STARTUP_GRACE);
        assert_eq!(startups.overdue(Instant::now()), [SessionId(1)]);
        assert_eq!(looks.recv().await.unwrap() - second, STARTUP_GRACE);
        assert_eq!(startups.overdue(Instant::now()), [SessionId(2)]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_host_that_is_gone_is_not_looked_at_and_the_watch_ends_with_its_announcements() {
        let (announce, launches) = broadcast::channel(4);
        let watching = tokio::spawn(after_each_launch(
            launches,
            || None::<std::future::Ready<()>>,
        ));
        announce.send(Instant::now() + STARTUP_GRACE).unwrap();
        tokio::time::sleep(STARTUP_GRACE * 2).await;
        drop(announce);
        watching.await.unwrap();
    }
}
