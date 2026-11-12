//! `update:state`, `update:check` and `update:install`: the host's own
//! updates, told to its clients.
//!
//! - `update:state` carries what the host knows, whole on every change:
//!   the floti it runs and the channel it follows, what the last check that
//!   read the manifest found (kept in the data folder across restarts,
//!   [`background::kept`]), why the last check failed, whether one runs,
//!   and where a `floti update` on this computer has got to.
//! - Only a host that keeps floti up to date checks
//!   ([`HostRun::updates`](crate::server::HostRun::updates)): the background
//!   check ([`background::keep_up_to_date`]) tells each check here as it
//!   starts and ends, and answers each `update:check` once a check has
//!   ended. Every other host says why on `update:state`
//!   ([`UpdateUnavailable`]), answers `update:check` with that, checking
//!   nothing, and refuses `update:install`. While an update runs, a check
//!   asked for is answered at once, and one the background check could not
//!   make because of it is not told as a failure.
//! - `update:install` starts `<floti> update --from-desktop` apart
//!   ([`ProcessTree::start_apart`]): it outlives this host, which it asks to
//!   relaunch once the desktop app is replaced, holds nothing of the host's
//!   open, and leaves nothing for a host relaunched by `exec` to reap.
//! - The host follows an update through the run file every update keeps in
//!   the data folder ([`run_file`]), looking at it every [`FOLLOW_EVERY`]:
//!   each step and the wait for the app, then how it ended. So a host that
//!   starts while an update runs (relaunched, or after a crash) reports it
//!   too, and an Update pressed meanwhile starts nothing. An update whose
//!   process ended with its run file still there failed; one the host
//!   started that makes no run file within [`STARTUP_WAIT`] did not start.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use floti_protocol::Knowable;
use floti_protocol::channel::{Later, Router};
use floti_protocol::channels::update::{
    UpdateCheckNow, UpdateInstall, UpdateProgress, UpdateState, UpdateStatus, UpdateUnavailable,
};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};

use super::{Caller, Feature};
use crate::platform::process_tree::{self, ProcessTree};
use crate::server::events::Events;
use crate::update::background::{self, Ask, Checked, Told};
use crate::update::run_file::{self, Look};
use crate::update::{APP_POLL, STARTING_LINE, channel};
use crate::version::FLOTI_VERSION;

/// How often the run file is looked at while an update runs: as often as
/// the update looks for the app it waits on.
pub const FOLLOW_EVERY: Duration = APP_POLL;

/// How long an update the host started has to make its run file.
pub const STARTUP_WAIT: Duration = Duration::from_secs(30);

/// What an update whose process ended with its run file still there failed
/// with.
pub const ENDED_UNSAID: &str = "floti update ended before saying it was done.";

/// What an update the host started that made no run file failed with.
pub const NOT_STARTED: &str = "floti update did not start.";

/// Starts `<floti> update --from-desktop` from the floti at the path given.
pub(crate) type Start = Box<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

/// Whether a process is alive.
pub(crate) type IsAlive = Box<dyn Fn(u32) -> Knowable<bool> + Send + Sync>;

/// The host's updates: the state its clients are told, the asks for a check
/// now, and the `floti update` it starts. A clone is the same updates.
#[derive(Clone)]
pub(crate) struct Updates {
    shared: Arc<Shared>,
}

struct Shared {
    events: Arc<Events>,
    state: Mutex<UpdateState>,
    /// The floti this host keeps up to date; `None` when it does not.
    binary: Option<PathBuf>,
    data: PathBuf,
    asks: mpsc::UnboundedSender<Ask>,
    /// The asks' receiving end, until the background check takes it.
    asked: Mutex<Option<mpsc::UnboundedReceiver<Ask>>>,
    start: Start,
    is_alive: IsAlive,
}

impl Updates {
    /// Serves `update:state` on `events`, from what a check kept in `data`.
    /// `binary` is the floti this host keeps up to date, or `None`, with why
    /// from `config_read_only`.
    pub fn new(
        events: Arc<Events>,
        data: &Path,
        binary: Option<PathBuf>,
        config_read_only: bool,
    ) -> crate::Result<Self> {
        Self::with(
            events,
            data,
            binary,
            config_read_only,
            Box::new(start_native),
            Box::new(|pid| process_tree::Native.is_alive(pid)),
        )
    }

    /// As [`Self::new`], starting `floti update` with `start` and telling a
    /// process alive with `is_alive`.
    pub fn with(
        events: Arc<Events>,
        data: &Path,
        binary: Option<PathBuf>,
        config_read_only: bool,
        start: Start,
        is_alive: IsAlive,
    ) -> crate::Result<Self> {
        let kept = background::kept(data);
        let state = UpdateState {
            running: FLOTI_VERSION.to_owned(),
            channel: channel::remembered(data),
            unavailable: binary
                .is_none()
                .then(|| background::unavailable(config_read_only)),
            newest: kept.as_ref().and_then(|kept| kept.newest.clone()),
            checked_at: kept.map(|kept| kept.at.to_string()),
            failed: None,
            checking: false,
            installing: None,
            install_failed: None,
        };
        events.serve_state::<UpdateStatus, _>(&state)?;
        let (asks, asked) = mpsc::unbounded_channel();
        Ok(Self {
            shared: Arc::new(Shared {
                events,
                state: Mutex::new(state),
                binary,
                data: data.to_path_buf(),
                asks,
                asked: Mutex::new(Some(asked)),
                start,
                is_alive,
            }),
        })
    }

    /// The asks for a check now, for the background check to answer: given
    /// once, and only by a host that checks.
    pub fn take_asks(&self) -> Option<mpsc::UnboundedReceiver<Ask>> {
        self.shared.binary.as_ref()?;
        lock(&self.shared.asked).take()
    }

    /// What tells clients each check the background check `told`.
    pub fn teller(&self) -> impl Fn(Told) + Send + 'static {
        let shared = Arc::clone(&self.shared);
        move |told| shared.told(told)
    }

    /// Follows the update running on this computer as the host starts, when
    /// there is one: one this host's predecessor started, or one run in a
    /// terminal. Only a host that updates.
    pub fn follow_running(&self) {
        if self.shared.binary.is_none() {
            return;
        }
        let mut state = lock(&self.shared.state);
        if state.installing.is_none() {
            self.shared.adopt(&mut state);
        }
    }

    /// What clients are told now.
    #[cfg(test)]
    pub fn state(&self) -> UpdateState {
        self.shared.snapshot()
    }
}

impl Feature for Updates {
    fn requests<'a>(&'a self, _caller: Caller, router: &mut Router<'a>) {
        router
            .handle_later::<UpdateCheckNow>(|()| Ok(self.shared.check_now()))
            .handle::<UpdateInstall>(|()| self.shared.install());
    }
}

impl Shared {
    fn snapshot(&self) -> UpdateState {
        lock(&self.state).clone()
    }

    /// Changes the state by `edit` and tells clients the whole of it, when
    /// it changed.
    fn change(&self, edit: impl FnOnce(&mut UpdateState)) {
        let mut state = lock(&self.state);
        self.publish(&mut state, edit);
    }

    fn publish(
        &self,
        state: &mut MutexGuard<'_, UpdateState>,
        edit: impl FnOnce(&mut UpdateState),
    ) {
        let before = (**state).clone();
        edit(state);
        if **state == before {
            return;
        }
        // Under the state's lock, so clients hear the changes in order.
        if let Err(err) = self.events.set_state::<UpdateStatus, _>(&**state) {
            log_line!("floti host: the update state could not be told: {err}");
        }
    }

    fn told(&self, told: Told) {
        self.change(|state| match told {
            Told::Checking => state.checking = true,
            Told::Checked { at, checked } => {
                state.checking = false;
                match checked {
                    Checked::UpToDate { newest } | Checked::Replaced { version: newest } => {
                        state.newest = Some(newest);
                        state.checked_at = Some(at.to_string());
                        state.failed = None;
                    }
                    // Kept off the update running, not a failure of the release.
                    Checked::Failed { .. } if state.installing.is_some() => {}
                    Checked::Failed { why, soon } => {
                        // One the release failed did read the manifest.
                        if !soon {
                            state.checked_at = Some(at.to_string());
                        }
                        state.failed = Some(why);
                    }
                }
            }
        });
    }

    /// A check now, answered with the state once it has ended; at once, and
    /// checking nothing, on a host that never checks, while an update runs,
    /// or once the check loop has ended for good.
    fn check_now(self: &Arc<Self>) -> Later<UpdateState> {
        let shared = Arc::clone(self);
        let installing = lock(&self.state).installing.is_some();
        let answered = self.binary.as_ref().filter(|_| !installing).and_then(|_| {
            let (answer, answered) = oneshot::channel();
            self.asks.send(answer).ok().map(|()| answered)
        });
        Box::pin(async move {
            if let Some(answered) = answered {
                // A check loop that ended drops the ask: the state is the
                // answer then too.
                let _ = answered.await;
            }
            Ok(shared.snapshot())
        })
    }

    /// Starts `floti update --from-desktop`, unless an update runs on this
    /// computer already: that one is followed instead.
    fn install(self: &Arc<Self>) -> Result<(), String> {
        let mut state = lock(&self.state);
        let Some(binary) = &self.binary else {
            return Err(refusal(
                state
                    .unavailable
                    .unwrap_or(UpdateUnavailable::NotStandalone),
            ));
        };
        if state.installing.is_some() || self.adopt(&mut state) {
            return Ok(());
        }
        // Before the start, so an update that ends at once is still its.
        let since = Timestamp::now();
        (self.start)(binary).map_err(|err| {
            format!(
                "floti update could not be started from {}: {err}.",
                binary.display()
            )
        })?;
        log_line!("floti host: started floti update for a client.");
        self.publish(&mut state, |state| {
            state.installing = Some(UpdateProgress {
                line: STARTING_LINE.to_owned(),
                waiting_for_app: false,
            });
            state.install_failed = None;
        });
        drop(state);
        tokio::spawn(follow(Arc::clone(self), None, Some(since)));
        Ok(())
    }

    /// Follows the update the run file says runs, when its process is not
    /// known to have ended. Whether there was one.
    fn adopt(self: &Arc<Self>, state: &mut MutexGuard<'_, UpdateState>) -> bool {
        let Look::Running(run) = run_file::look(&self.data) else {
            return false;
        };
        if matches!((self.is_alive)(run.pid), Knowable::Known(false)) {
            return false;
        }
        log_line!(
            "floti host: following the floti update already running (process {}).",
            run.pid
        );
        self.publish(state, |state| {
            state.installing = Some(UpdateProgress {
                line: run.line,
                waiting_for_app: run.waiting_for_app,
            });
            state.install_failed = None;
        });
        tokio::spawn(follow(Arc::clone(self), Some(run.pid), None));
        true
    }

    /// Where the update has got to, when it changed.
    fn progress(&self, line: String, waiting_for_app: bool) {
        self.change(|state| {
            state.installing = Some(UpdateProgress {
                line,
                waiting_for_app,
            });
        });
    }

    /// The update has ended: well, or with `failed`.
    fn finish(&self, failed: Option<String>) {
        match &failed {
            None => log_line!("floti host: floti update has finished."),
            Some(why) => log_line!("floti host: floti update failed: {why}"),
        }
        self.change(|state| {
            state.installing = None;
            state.install_failed = failed;
        });
    }
}

/// Follows an update through the run file until it ends: `followed`, the
/// process it found running, or, with none, the one the host started at
/// `since`.
async fn follow(shared: Arc<Shared>, mut followed: Option<u32>, since: Option<Timestamp>) {
    let started = tokio::time::Instant::now();
    let outcome = |pid: u32| -> Option<String> {
        run_file::ended(&shared.data)
            .filter(|ended| ended.pid == pid && !ended.ok)
            .map(|ended| ended.line)
    };
    loop {
        match run_file::look(&shared.data) {
            Look::Running(run) => {
                let ended = matches!((shared.is_alive)(run.pid), Knowable::Known(false));
                match followed {
                    // The one followed ended, and another runs now.
                    Some(pid) if pid != run.pid => return shared.finish(outcome(pid)),
                    Some(_) if ended => return shared.finish(Some(ENDED_UNSAID.to_owned())),
                    // A leftover the update started is about to take over.
                    None if ended => {}
                    _ => {
                        followed = Some(run.pid);
                        shared.progress(run.line, run.waiting_for_app);
                    }
                }
            }
            Look::Absent => match followed {
                Some(pid) => return shared.finish(outcome(pid)),
                None => {
                    // One that ended before its run file was ever seen.
                    if let Some(ended) = run_file::ended(&shared.data).filter(|ended| {
                        since.is_some_and(|since| ended.ended_at().is_some_and(|at| at >= since))
                    }) {
                        return shared.finish((!ended.ok).then_some(ended.line));
                    }
                    if started.elapsed() >= STARTUP_WAIT {
                        return shared.finish(Some(NOT_STARTED.to_owned()));
                    }
                }
            },
            Look::Unreadable => {}
        }
        tokio::time::sleep(FOLLOW_EVERY).await;
    }
}

/// Why a host that does not update refuses `update:install`.
fn refusal(why: UpdateUnavailable) -> String {
    match why {
        UpdateUnavailable::ReadOnly => "This host runs on another host's config folder, as a development run or a check does, so it neither checks for updates nor updates anything.".to_owned(),
        UpdateUnavailable::NotStandalone => "This host does not run floti installed on its own, so floti update does not update it.".to_owned(),
    }
}

/// Starts `<binary> update --from-desktop` apart from this process.
fn start_native(binary: &Path) -> std::io::Result<()> {
    process_tree::Native.start_apart(binary, &["update", "--from-desktop"])
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
