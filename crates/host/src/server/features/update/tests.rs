//! The host's updates as its clients hear them: what `update:state` starts
//! from and says as checks run, `update:check` answered by the background
//! check or at once, and `update:install` followed through a run file the
//! test writes in place of `floti update`, on Tokio's paused clock. Nothing
//! reaches the network or starts a process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use floti_protocol::Knowable;
use floti_protocol::channel::{EventChannel, from_payload};
use floti_protocol::channels::update::{
    UpdateProgress, UpdateState, UpdateStatus, UpdateUnavailable,
};
use jiff::Timestamp;
use tokio::sync::broadcast;

use super::{ENDED_UNSAID, NOT_STARTED, STARTUP_WAIT, Start, Updates};
use crate::server::events::{Events, Publication};
use crate::testkit::{PATIENCE, TestDirs};
use crate::update::background::{self, Checked, Told};
use crate::update::run_file::{ENDED_FILE, EndedRecord, RUN_FILE, RunRecord};
use crate::update::{STARTING_LINE, channel};
use crate::version::FLOTI_VERSION;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

const T0: i64 = 1_800_000_000;
const UPDATE: u32 = 7001;
const WAITING: &str = "Waiting for Floti to quit; it is updated once it has.";

/// What a start does to the data folder, as `floti update` would.
type OnStart = Box<dyn FnMut(&Path) + Send>;

/// The processes the fixture knows of, and the starts it was asked for.
#[derive(Default)]
struct World {
    alive: Mutex<HashMap<u32, Knowable<bool>>>,
    started: Mutex<Vec<PathBuf>>,
    /// What a start does to the data folder, as `floti update` would.
    on_start: Mutex<Option<OnStart>>,
}

struct Fixture {
    updates: Updates,
    published: broadcast::Receiver<Publication>,
    world: Arc<World>,
    dirs: TestDirs,
}

/// Updates over a data folder `prepare` fills, for a host that keeps the
/// floti at `binary` up to date, or none with why from `read_only`.
fn fixture(binary: Option<&str>, read_only: bool, prepare: impl FnOnce(&TestDirs)) -> Fixture {
    let dirs = TestDirs::new();
    prepare(&dirs);
    let events = Arc::new(Events::new());
    let published = events.observe();
    let world = Arc::new(World::default());
    let starting = Arc::clone(&world);
    let data = dirs.data.clone();
    let start: Start = Box::new(move |binary| {
        starting.started.lock().unwrap().push(binary.to_path_buf());
        if let Some(on_start) = starting.on_start.lock().unwrap().as_mut() {
            on_start(&data);
        }
        Ok(())
    });
    let asking = Arc::clone(&world);
    let updates = Updates::with(
        events,
        &dirs.data,
        binary.map(|name| dirs.folder("bin").join(name)),
        read_only,
        start,
        Box::new(move |pid| {
            asking
                .alive
                .lock()
                .unwrap()
                .get(&pid)
                .cloned()
                .unwrap_or(Knowable::Unknown)
        }),
    )
    .unwrap();
    Fixture {
        updates,
        published,
        world,
        dirs,
    }
}

fn standalone() -> Fixture {
    fixture(Some("floti.exe"), false, |_| {})
}

/// What `floti update` writes while it runs: its process and its last line.
fn write_run(data: &Path, pid: u32, line: &str, waiting_for_app: bool) {
    let record = RunRecord {
        pid,
        line: line.to_owned(),
        waiting_for_app,
    };
    std::fs::write(data.join(RUN_FILE), serde_json::to_vec(&record).unwrap()).unwrap();
}

/// What `floti update` leaves when it ends: how, and no run file.
fn write_ended(data: &Path, pid: u32, ok: bool, line: &str) {
    let record = EndedRecord {
        pid,
        at: Timestamp::now().to_string(),
        ok,
        line: line.to_owned(),
    };
    std::fs::write(data.join(ENDED_FILE), serde_json::to_vec(&record).unwrap()).unwrap();
    let _ = std::fs::remove_file(data.join(RUN_FILE));
}

impl Fixture {
    fn data(&self) -> &Path {
        &self.dirs.data
    }

    fn alive(&self, pid: u32, alive: bool) {
        self.world
            .alive
            .lock()
            .unwrap()
            .insert(pid, Knowable::Known(alive));
    }

    fn on_start(&self, act: impl FnMut(&Path) + Send + 'static) {
        *self.world.on_start.lock().unwrap() = Some(Box::new(act));
    }

    fn starts(&self) -> usize {
        self.world.started.lock().unwrap().len()
    }

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

    /// Waits, for at most `patience` on the paused clock, until a state
    /// told meets `wanted`, and answers it.
    async fn until(
        &mut self,
        patience: std::time::Duration,
        wanted: impl Fn(&UpdateState) -> bool,
    ) -> UpdateState {
        let waiting = async {
            loop {
                let published = self.published.recv().await.expect("the events go on");
                if published.channel != <UpdateStatus as EventChannel>::NAME {
                    continue;
                }
                let state: UpdateState = from_payload(&published.payload).unwrap();
                if wanted(&state) {
                    return state;
                }
            }
        };
        tokio::time::timeout(patience, waiting)
            .await
            .expect("the state was told within the patience")
    }

    async fn until_ended(&mut self) -> UpdateState {
        self.until(PATIENCE, |state| state.installing.is_none())
            .await
    }
}

fn progress(state: &UpdateState) -> Option<(String, bool)> {
    state.installing.as_ref().map(
        |UpdateProgress {
             line,
             waiting_for_app,
         }| (line.clone(), *waiting_for_app),
    )
}

#[test]
fn a_host_that_keeps_floti_up_to_date_starts_from_what_its_last_check_kept() {
    let fx = fixture(Some("floti.exe"), false, |dirs| {
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

#[tokio::test(start_paused = true)]
async fn a_development_run_says_why_answers_a_check_at_once_and_refuses_to_update() {
    let fx = fixture(None, true, |_| {});
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
    assert_eq!(fx.starts(), 0);
}

#[tokio::test(start_paused = true)]
async fn any_other_floti_is_named_as_not_installed_on_its_own_and_follows_nothing() {
    let fx = fixture(None, false, |dirs| {
        write_run(&dirs.data, UPDATE, "Downloading.", false)
    });
    fx.alive(UPDATE, true);
    assert_eq!(
        fx.updates.state().unavailable,
        Some(UpdateUnavailable::NotStandalone)
    );
    fx.updates.follow_running();
    assert_eq!(fx.updates.state().installing, None);
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
    let failed = Some("could not download update.json.".to_owned());
    assert_eq!(
        told,
        vec![
            (true, None, None, None),
            (false, Some("9.1.0".into()), when(T0), None),
            (true, Some("9.1.0".into()), when(T0), None),
            // A check that could not reach the release keeps what the last
            // one found and says why.
            (false, Some("9.1.0".into()), when(T0), failed.clone()),
            (true, Some("9.1.0".into()), when(T0), failed),
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn an_update_s_steps_are_told_in_order_with_the_wait_for_the_app_marked() {
    let mut fx = standalone();
    fx.alive(UPDATE, true);
    fx.on_start(|data| write_run(data, UPDATE, STARTING_LINE, false));
    fx.updates.shared.install().unwrap();
    let step = |line: &str, waiting: bool| Some((line.to_owned(), waiting));
    assert_eq!(progress(&fx.updates.state()), step(STARTING_LINE, false));
    assert_eq!(fx.starts(), 1);

    write_run(
        fx.data(),
        UPDATE,
        "Downloading Floti-9.1.0-setup.exe.",
        false,
    );
    fx.until(PATIENCE, |state| {
        progress(state) == step("Downloading Floti-9.1.0-setup.exe.", false)
    })
    .await;
    write_run(fx.data(), UPDATE, WAITING, true);
    fx.until(PATIENCE, |state| progress(state) == step(WAITING, true))
        .await;
    write_ended(fx.data(), UPDATE, true, "floti update has finished.");
    let last = fx.until_ended().await;
    assert_eq!(last.install_failed, None);
}

#[tokio::test(start_paused = true)]
async fn an_update_that_stopped_says_its_last_line_until_the_next_one_starts() {
    let mut fx = standalone();
    fx.alive(UPDATE, true);
    fx.on_start(|data| write_run(data, UPDATE, STARTING_LINE, false));
    fx.updates.shared.install().unwrap();
    write_ended(
        fx.data(),
        UPDATE,
        false,
        "Floti-9.1.0-setup.exe does not match its signature. Nothing was updated.",
    );
    let last = fx.until_ended().await;
    assert_eq!(
        last.install_failed,
        Some("Floti-9.1.0-setup.exe does not match its signature. Nothing was updated.".into())
    );
    // The next one starts with no failure standing.
    fx.updates.shared.install().unwrap();
    assert_eq!(fx.updates.state().install_failed, None);
    assert_eq!(fx.starts(), 2);
    write_ended(fx.data(), UPDATE, true, "floti update has finished.");
    fx.until_ended().await;
}

#[tokio::test(start_paused = true)]
async fn an_update_that_ended_before_its_run_file_was_seen_is_told_from_how_it_ended() {
    let mut fx = standalone();
    fx.on_start(|data| {
        write_ended(
            data,
            UPDATE,
            false,
            "another floti update is running (process 9). Nothing was updated.",
        )
    });
    fx.updates.shared.install().unwrap();
    let last = fx.until_ended().await;
    assert_eq!(
        last.install_failed,
        Some("another floti update is running (process 9). Nothing was updated.".into())
    );
}

#[tokio::test(start_paused = true)]
async fn an_update_whose_process_ended_with_its_run_file_left_failed() {
    let mut fx = standalone();
    fx.alive(UPDATE, true);
    fx.on_start(|data| write_run(data, UPDATE, "Downloading floti 9.1.0 from stable.", false));
    fx.updates.shared.install().unwrap();
    fx.until(PATIENCE, |state| {
        progress(state).is_some_and(|(line, _)| line.starts_with("Downloading"))
    })
    .await;
    fx.alive(UPDATE, false);
    let last = fx.until_ended().await;
    assert_eq!(last.install_failed, Some(ENDED_UNSAID.into()));
}

#[tokio::test(start_paused = true)]
async fn an_update_that_never_makes_its_run_file_did_not_start() {
    let mut fx = standalone();
    fx.updates.shared.install().unwrap();
    let last = fx
        .until(STARTUP_WAIT + PATIENCE, |state| state.installing.is_none())
        .await;
    assert_eq!(last.install_failed, Some(NOT_STARTED.into()));
}

#[tokio::test(start_paused = true)]
async fn a_host_that_starts_while_an_update_runs_reports_it_and_starts_no_second_one() {
    let mut fx = fixture(Some("floti.exe"), false, |dirs| {
        write_run(&dirs.data, UPDATE, WAITING, true);
    });
    fx.alive(UPDATE, true);
    fx.updates.follow_running();
    assert_eq!(
        progress(&fx.updates.state()),
        Some((WAITING.to_owned(), true)),
        "the wait for the app is told at once, so a desktop quits"
    );
    // Update pressed meanwhile starts nothing.
    fx.updates.shared.install().unwrap();
    assert_eq!(fx.starts(), 0);
    // A check asked for is answered at once, checking nothing.
    let asks = fx.updates.take_asks().unwrap();
    let answered = fx.updates.shared.check_now().await.unwrap();
    assert!(answered.installing.is_some());
    drop(asks);

    write_ended(fx.data(), UPDATE, true, "floti update has finished.");
    let last = fx.until_ended().await;
    assert_eq!(last.install_failed, None);
}

#[tokio::test(start_paused = true)]
async fn an_update_pressed_while_one_from_a_terminal_runs_follows_that_one() {
    let mut fx = standalone();
    write_run(
        fx.data(),
        UPDATE,
        "Downloading floti 9.1.0 from stable.",
        false,
    );
    fx.alive(UPDATE, true);
    fx.updates.shared.install().unwrap();
    assert_eq!(fx.starts(), 0);
    assert_eq!(
        progress(&fx.updates.state()),
        Some(("Downloading floti 9.1.0 from stable.".to_owned(), false))
    );
    write_ended(fx.data(), UPDATE, true, "floti update has finished.");
    fx.until_ended().await;
}

#[tokio::test(start_paused = true)]
async fn a_leftover_run_file_is_no_update_running() {
    let fx = fixture(Some("floti.exe"), false, |dirs| {
        write_run(&dirs.data, UPDATE, "Downloading.", false);
    });
    fx.alive(UPDATE, false);
    fx.updates.follow_running();
    assert_eq!(fx.updates.state().installing, None);
}

#[tokio::test(start_paused = true)]
async fn a_check_the_running_update_kept_off_is_not_told_as_a_failure() {
    let mut fx = standalone();
    fx.alive(UPDATE, true);
    write_run(fx.data(), UPDATE, "Downloading.", false);
    fx.updates.follow_running();
    fx.updates.teller()(Told::Checked {
        at: at(T0),
        checked: Checked::Failed {
            why: "another floti update is running (process 7001). Nothing was updated.".into(),
            soon: true,
        },
    });
    assert_eq!(fx.updates.state().failed, None);
    write_ended(fx.data(), UPDATE, true, "floti update has finished.");
    fx.until_ended().await;
}

#[tokio::test(start_paused = true)]
async fn an_update_that_could_not_be_started_is_refused_with_why() {
    let dirs = TestDirs::new();
    let start: Start = Box::new(|_| Err(std::io::Error::other("no such file")));
    let updates = Updates::with(
        Arc::new(Events::new()),
        &dirs.data,
        Some(dirs.folder("bin").join("floti.exe")),
        false,
        start,
        Box::new(|_| Knowable::Unknown),
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
