//! `update:state`, `update:check` and `update:install`: the host's own
//! updates, told to its clients.
//!
//! - `update:state` carries what the host knows, whole on every change:
//!   the floti it runs and the channel it follows, what the last check that
//!   read the manifest found (kept in the data folder across restarts,
//!   [`background::kept`]), why the last check failed, whether one runs,
//!   and where a `floti update` it started has got to.
//! - Only a host that keeps floti up to date checks
//!   ([`HostRun::updates`](crate::server::HostRun::updates)): the background
//!   check ([`background::keep_up_to_date`]) tells each check here as it
//!   starts and ends, and answers each `update:check` once a check has
//!   ended. Every other host says why on `update:state`
//!   ([`UpdateUnavailable`]), answers `update:check` with that, checking
//!   nothing, and refuses `update:install`.
//! - `update:install` starts `<floti> update --from-desktop` as a lasting
//!   root that is never this process's child
//!   ([`ProcessTree::prepare_orphan_root`]), so it outlives this host, which
//!   it asks to relaunch once the desktop app is replaced, and leaves no
//!   child behind a host relaunched by `exec`. The host reads each JSON line
//!   it says ([`Said`]) into `update:state` and its log, and learns how it
//!   ended from its last line; output that ends with no such line is a
//!   failure, said with the last line it wrote on standard error.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use floti_protocol::channel::{Later, Router};
use floti_protocol::channels::update::{
    UpdateCheckNow, UpdateInstall, UpdateProgress, UpdateState, UpdateStatus, UpdateUnavailable,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::{Caller, Feature};
use crate::platform::process_tree::{self, ProcessTree};
use crate::server::events::Events;
use crate::update::background::{self, Ask, Checked, Told};
use crate::update::{Said, channel};
use crate::version::FLOTI_VERSION;

/// What `update:state` says a `floti update` just started is doing.
pub const STARTING_LINE: &str = "Starting floti update.";

/// The output of a `floti update` the host started.
pub(crate) struct Started {
    pub stdout: Pin<Box<dyn AsyncRead + Send>>,
    pub stderr: Pin<Box<dyn AsyncRead + Send>>,
}

/// Starts `<floti> update --from-desktop` from the floti at the path given.
pub(crate) type Start = Box<dyn Fn(&Path) -> std::io::Result<Started> + Send + Sync>;

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
    asks: mpsc::UnboundedSender<Ask>,
    /// The asks' receiving end, until the background check takes it.
    asked: Mutex<Option<mpsc::UnboundedReceiver<Ask>>>,
    start: Start,
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
        Self::with_start(
            events,
            data,
            binary,
            config_read_only,
            Box::new(start_native),
        )
    }

    /// As [`Self::new`], starting `floti update` with `start`.
    pub fn with_start(
        events: Arc<Events>,
        data: &Path,
        binary: Option<PathBuf>,
        config_read_only: bool,
        start: Start,
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
                asks,
                asked: Mutex::new(Some(asked)),
                start,
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

    /// Changes the state by `edit` and tells clients the whole of it.
    fn change(&self, edit: impl FnOnce(&mut UpdateState)) {
        let mut state = lock(&self.state);
        self.publish(&mut state, edit);
    }

    fn publish(
        &self,
        state: &mut MutexGuard<'_, UpdateState>,
        edit: impl FnOnce(&mut UpdateState),
    ) {
        edit(state);
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
    /// checking nothing, on a host that never checks or whose check has
    /// ended for good.
    fn check_now(self: &Arc<Self>) -> Later<UpdateState> {
        let shared = Arc::clone(self);
        let answered = self.binary.as_ref().and_then(|_| {
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

    /// Starts `floti update --from-desktop`, unless one this host started
    /// still runs.
    fn install(self: &Arc<Self>) -> Result<(), String> {
        let mut state = lock(&self.state);
        let Some(binary) = &self.binary else {
            return Err(refusal(
                state
                    .unavailable
                    .unwrap_or(UpdateUnavailable::NotStandalone),
            ));
        };
        if state.installing.is_some() {
            return Ok(());
        }
        let started = (self.start)(binary).map_err(|err| {
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
        tokio::spawn(follow(Arc::clone(self), started));
        Ok(())
    }
}

/// Reads what `floti update` says until it ends, into the state and the log.
async fn follow(shared: Arc<Shared>, started: Started) {
    let Started { stdout, mut stderr } = started;
    let reading = {
        let shared = Arc::clone(&shared);
        async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut ended = None;
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(said) = serde_json::from_str::<Said>(&line) else {
                    log_line!("floti host: floti update said: {line}");
                    continue;
                };
                log_line!("floti host: floti update: {}", said.line);
                match said.ok {
                    Some(ok) => ended = Some((ok, said.line)),
                    None => shared.change(|state| {
                        state.installing = Some(UpdateProgress {
                            line: said.line,
                            waiting_for_app: said.waiting_for_app,
                        });
                    }),
                }
            }
            ended
        }
    };
    let errors = async move {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text).await;
        text
    };
    let (ended, errors) = tokio::join!(reading, errors);
    let failed = match ended {
        Some((true, _)) => None,
        Some((false, line)) => Some(line),
        None => Some(
            last_line(&errors)
                .map(|line| {
                    line.strip_prefix("floti update: ")
                        .unwrap_or(line)
                        .to_owned()
                })
                .unwrap_or_else(|| "floti update ended before saying it was done.".to_owned()),
        ),
    };
    match &failed {
        None => log_line!("floti host: floti update has finished."),
        Some(why) => log_line!("floti host: floti update failed: {why}"),
    }
    shared.change(|state| {
        state.installing = None;
        state.install_failed = failed;
    });
}

/// The last line of `text` with anything in it.
fn last_line(text: &[u8]) -> Option<&str> {
    std::str::from_utf8(text)
        .ok()?
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map(str::trim)
}

/// Why a host that does not update refuses `update:install`.
fn refusal(why: UpdateUnavailable) -> String {
    match why {
        UpdateUnavailable::ReadOnly => "This host runs on another host's config folder, as a development run or a check does, so it neither checks for updates nor updates anything.".to_owned(),
        UpdateUnavailable::NotStandalone => "This host does not run floti installed on its own, so floti update does not update it.".to_owned(),
    }
}

/// Starts `<binary> update --from-desktop` as a lasting root that is never
/// this process's child, its output piped here.
fn start_native(binary: &Path) -> std::io::Result<Started> {
    let mut command = std::process::Command::new(binary);
    command
        .args(["update", "--from-desktop"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process_tree::Native.prepare_orphan_root(&mut command);
    let mut child = tokio::process::Command::from(command).spawn()?;
    let not_piped = || std::io::Error::other("its output was not piped");
    let stdout = child.stdout.take().ok_or_else(not_piped)?;
    let stderr = child.stderr.take().ok_or_else(not_piped)?;
    // What was spawned is the intermediate that exits at once on macOS and
    // Linux, and the update itself on Windows: reaped either way.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(Started {
        stdout: Box::pin(stdout),
        stderr: Box::pin(stderr),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
