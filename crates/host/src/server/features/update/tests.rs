//! The host's updates as its clients hear them: what `update:state` starts
//! from and says as checks run, `update:check` answered by the background
//! check or at once, and `update:install` reading a faked `floti update`'s
//! output. Nothing reaches the network or starts a process.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use floti_protocol::channel::{EventChannel, from_payload};
use floti_protocol::channels::update::{
    UpdateProgress, UpdateState, UpdateStatus, UpdateUnavailable,
};
use jiff::Timestamp;
use tokio::sync::broadcast;

use super::{STARTING_LINE, Start, Started, Updates};
use crate::server::events::{Events, Publication};
use crate::testkit::{PATIENCE, TestDirs};
use crate::update::background::{self, Checked, Told};
use crate::update::{FINISHED_LINE, Said, channel};
use crate::version::FLOTI_VERSION;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

const T0: i64 = 1_800_000_000;

/// What a faked `floti update` writes, and how often one was started.
#[derive(Default)]
struct Script {
    stdout: String,
    stderr: String,
    started: Mutex<Vec<PathBuf>>,
}

struct Fixture {
    updates: Updates,
    published: broadcast::Receiver<Publication>,
    script: Arc<Script>,
    _dirs: TestDirs,
}

/// Updates over a data folder `prepare` fills, for a host that keeps the
/// floti at `binary` up to date, or none with why from `read_only`; a
/// `floti update` started writes what `script` says.
fn fixture(
    binary: Option<&str>,
    read_only: bool,
    script: Script,
    prepare: impl FnOnce(&TestDirs),
) -> Fixture {
    let dirs = TestDirs::new();
    prepare(&dirs);
    let events = Arc::new(Events::new());
    let published = events.observe();
    let script = Arc::new(script);
    let starting = Arc::clone(&script);
    let start: Start = Box::new(move |binary| {
        starting.started.lock().unwrap().push(binary.to_path_buf());
        Ok(Started {
            stdout: Box::pin(std::io::Cursor::new(starting.stdout.clone().into_bytes())),
            stderr: Box::pin(std::io::Cursor::new(starting.stderr.clone().into_bytes())),
        })
    });
    let updates = Updates::with_start(
        events,
        &dirs.data,
        binary.map(|name| dirs.folder("bin").join(name)),
        read_only,
        start,
    )
    .unwrap();
    Fixture {
        updates,
        published,
        script,
        _dirs: dirs,
    }
}

fn standalone() -> Fixture {
    fixture(Some("floti.exe"), false, Script::default(), |_| {})
}

impl Fixture {
    /// Every state told since the last call.
    fn drain(&mut self) -> Vec<UpdateState> {
        let mut states = Vec::new();
        while let Ok(published) = self.published.try_recv() {
            if published.channel == <UpdateStatus as EventChannel>::NAME {
                states.push(from_payload(&published.payload).unwrap());
            }
        }
        states
    }

    /// Every state told from now until the `floti update` started ends,
    /// waiting for at most the shared patience.
    async fn until_installed(&mut self) -> Vec<UpdateState> {
        let waiting = async {
            let mut told = Vec::new();
            loop {
                let published = self.published.recv().await.expect("the events go on");
                if published.channel != <UpdateStatus as EventChannel>::NAME {
                    continue;
                }
                let state: UpdateState = from_payload(&published.payload).unwrap();
                let ended = state.installing.is_none();
                told.push(state);
                if ended {
                    return told;
                }
            }
        };
        tokio::time::timeout(PATIENCE, waiting)
            .await
            .expect("the update ended within the patience")
    }
}

/// What each state says of the update: its line and whether it waits for
/// the app, or nothing once it has ended.
fn progress(told: &[UpdateState]) -> Vec<Option<(String, bool)>> {
    told.iter()
        .map(|state| {
            state.installing.as_ref().map(
                |UpdateProgress {
                     line,
                     waiting_for_app,
                 }| (line.clone(), *waiting_for_app),
            )
        })
        .collect()
}

#[test]
fn a_host_that_keeps_floti_up_to_date_starts_from_what_its_last_check_kept() {
    let fx = fixture(Some("floti.exe"), false, Script::default(), |dirs| {
        background::remember_checked(&dirs.data, at(T0), Some("9.1.0")).unwrap();
        channel::remember(&dirs.data, "beta").unwrap();
    });
    assert_eq!(
        fx.updates.state(),
        UpdateState {
            running: FLOTI_VERSION.to_owned(),
            channel: "beta".into(),
            unavailable: None,
            newest: Some("9.1.0".into()),
            checked_at: Some(at(T0).to_string()),
            failed: None,
            checking: false,
            installing: None,
            install_failed: None,
        }
    );
    assert!(fx.updates.take_asks().is_some());
    assert!(fx.updates.take_asks().is_none(), "the asks are given once");
}

#[tokio::test]
async fn a_development_run_says_why_answers_a_check_at_once_and_refuses_to_update() {
    let fx = fixture(None, true, Script::default(), |_| {});
    let state = fx.updates.state();
    assert_eq!(state.unavailable, Some(UpdateUnavailable::ReadOnly));
    assert_eq!(state.channel, "stable");
    assert_eq!(state.newest, None);
    assert!(fx.updates.take_asks().is_none());
    let answered = fx.updates.shared.check_now().await.unwrap();
    assert_eq!(answered, state, "nothing was checked");
    let refused = fx.updates.shared.install().unwrap_err();
    assert!(
        refused.contains("another host's config folder"),
        "{refused}"
    );
    assert!(fx.script.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn any_other_floti_is_named_as_not_installed_on_its_own() {
    let fx = fixture(None, false, Script::default(), |_| {});
    assert_eq!(
        fx.updates.state().unavailable,
        Some(UpdateUnavailable::NotStandalone)
    );
    let refused = fx.updates.shared.install().unwrap_err();
    assert!(
        refused.contains("not run floti installed on its own"),
        "{refused}"
    );
}

#[test]
fn each_check_is_told_as_it_starts_and_ends() {
    let mut fx = standalone();
    let tell = fx.updates.teller();
    tell(Told::Checking);
    tell(Told::Checked {
        at: at(T0),
        checked: Checked::UpToDate {
            newest: "9.1.0".into(),
        },
    });
    tell(Told::Checking);
    tell(Told::Checked {
        at: at(T0 + 60),
        checked: Checked::Failed {
            why: "could not download update.json.".into(),
            soon: true,
        },
    });
    tell(Told::Checking);
    tell(Told::Checked {
        at: at(T0 + 120),
        checked: Checked::Replaced {
            version: "9.2.0".into(),
        },
    });
    let told: Vec<_> = fx
        .drain()
        .into_iter()
        .map(|state| (state.checking, state.newest, state.checked_at, state.failed))
        .collect();
    let when = |second: i64| Some(at(second).to_string());
    assert_eq!(
        told,
        vec![
            (true, None, None, None),
            (false, Some("9.1.0".into()), when(T0), None),
            (true, Some("9.1.0".into()), when(T0), None),
            // A check that could not reach the release keeps what the last
            // one found and says why.
            (
                false,
                Some("9.1.0".into()),
                when(T0),
                Some("could not download update.json.".into())
            ),
            (
                true,
                Some("9.1.0".into()),
                when(T0),
                Some("could not download update.json.".into())
            ),
            (false, Some("9.2.0".into()), when(T0 + 120), None),
        ]
    );
}

#[test]
fn a_release_that_failed_its_checks_was_read_and_says_why() {
    let mut fx = standalone();
    fx.updates.teller()(Told::Checked {
        at: at(T0),
        checked: Checked::Failed {
            why: "floti does not match its signature.".into(),
            soon: false,
        },
    });
    let state = fx.drain().pop().unwrap();
    assert_eq!(state.checked_at, Some(at(T0).to_string()));
    assert_eq!(
        state.failed,
        Some("floti does not match its signature.".into())
    );
}

#[tokio::test]
async fn a_check_now_is_answered_once_the_background_check_has_ended() {
    let fx = standalone();
    let mut asks = fx.updates.take_asks().unwrap();
    let tell = fx.updates.teller();
    let answering = tokio::spawn(async move {
        let ask = asks.recv().await.expect("an ask");
        tell(Told::Checking);
        tell(Told::Checked {
            at: at(T0),
            checked: Checked::UpToDate {
                newest: "9.1.0".into(),
            },
        });
        ask.send(()).unwrap();
        asks
    });
    let answered = fx.updates.shared.check_now().await.unwrap();
    assert_eq!(answered.newest, Some("9.1.0".into()));
    assert!(!answered.checking);
    let asks = answering.await.unwrap();
    // With the check loop gone, an ask is answered at once with the state.
    drop(asks);
    let answered = fx.updates.shared.check_now().await.unwrap();
    assert_eq!(answered.newest, Some("9.1.0".into()));
}

fn lines(said: &[String]) -> String {
    said.iter().map(|line| format!("{line}\n")).collect()
}

const WAITING: &str = "Waiting for Helm to quit; it is updated once it has.";

#[tokio::test]
async fn an_update_s_steps_are_told_in_order_with_the_wait_for_the_app_marked() {
    let script = Script {
        stdout: lines(&[
            Said::step("Downloading Helm-9.1.0-setup.exe.", false),
            Said::step(WAITING, true),
            Said::step("Helm is updated to 9.1.0.", false),
            Said::end(FINISHED_LINE, true),
        ]),
        ..Script::default()
    };
    let mut fx = fixture(Some("floti.exe"), false, script, |_| {});
    fx.updates.shared.install().unwrap();
    let told = fx.until_installed().await;
    let step = |line: &str, waiting: bool| Some((line.to_owned(), waiting));
    assert_eq!(
        progress(&told),
        vec![
            step(STARTING_LINE, false),
            step("Downloading Helm-9.1.0-setup.exe.", false),
            step(WAITING, true),
            step("Helm is updated to 9.1.0.", false),
            None,
        ]
    );
    assert_eq!(told.last().unwrap().install_failed, None);
    let started = fx.script.started.lock().unwrap().clone();
    assert_eq!(started.len(), 1);
    assert!(started[0].ends_with("floti.exe"), "{started:?}");
}

#[tokio::test]
async fn an_update_that_stopped_says_its_last_line_until_the_next_one_starts() {
    let script = Script {
        stdout: lines(&[
            Said::step("Downloading Helm-9.1.0-setup.exe.", false),
            Said::end(
                "Helm-9.1.0-setup.exe does not match its signature. Nothing was updated.",
                false,
            ),
        ]),
        stderr: "floti update: Helm-9.1.0-setup.exe does not match its signature. Nothing was updated.\n".into(),
        ..Script::default()
    };
    let mut fx = fixture(Some("floti.exe"), false, script, |_| {});
    fx.updates.shared.install().unwrap();
    let told = fx.until_installed().await;
    assert_eq!(
        told.last().unwrap().install_failed,
        Some("Helm-9.1.0-setup.exe does not match its signature. Nothing was updated.".into())
    );
    // The next one starts with no failure standing.
    fx.updates.shared.install().unwrap();
    let told = fx.until_installed().await;
    assert_eq!(told[0].install_failed, None);
    assert_eq!(fx.script.started.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn output_that_ends_with_no_last_line_is_a_failure_said_from_standard_error() {
    let script = Script {
        stdout: lines(&[Said::step("Downloading floti 9.1.0 from stable.", false)]),
        stderr:
            "thread 'main' panicked at src/update/mod.rs:1:1:\nfloti update: the disk is full\n"
                .into(),
        ..Script::default()
    };
    let mut fx = fixture(Some("floti.exe"), false, script, |_| {});
    fx.updates.shared.install().unwrap();
    let told = fx.until_installed().await;
    assert_eq!(
        told.last().unwrap().install_failed,
        Some("the disk is full".into())
    );

    let mut fx = fixture(Some("floti.exe"), false, Script::default(), |_| {});
    fx.updates.shared.install().unwrap();
    let told = fx.until_installed().await;
    assert_eq!(
        told.last().unwrap().install_failed,
        Some("floti update ended before saying it was done.".into())
    );
}

#[tokio::test]
async fn an_install_asked_while_one_runs_starts_nothing() {
    let mut fx = standalone();
    // Held as running: what the fake wrote is not read until the test
    // lets the runtime turn.
    fx.updates.shared.install().unwrap();
    fx.updates.shared.install().unwrap();
    assert_eq!(fx.script.started.lock().unwrap().len(), 1);
    fx.until_installed().await;
}

#[tokio::test]
async fn an_update_that_could_not_be_started_is_refused_with_why() {
    let dirs = TestDirs::new();
    let start: Start = Box::new(|_| Err(std::io::Error::other("no such file")));
    let updates = Updates::with_start(
        Arc::new(Events::new()),
        &dirs.data,
        Some(dirs.folder("bin").join("floti.exe")),
        false,
        start,
    )
    .unwrap();
    let refused = updates.shared.install().unwrap_err();
    assert!(
        refused.starts_with("floti update could not be started from "),
        "{refused}"
    );
    assert!(refused.ends_with(": no such file."), "{refused}");
    assert_eq!(updates.state().installing, None);
}
