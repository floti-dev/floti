//! The background update's schedule, with time stepped: the schedule as a
//! function of the times kept, and the loop on Tokio's paused clock, with a
//! fake check that records when it ran.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};
use tokio::sync::{mpsc, oneshot, watch};

use super::*;
use crate::testkit::TestDirs;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

const T0: i64 = 1_800_000_000;
const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;

#[test]
fn with_the_setting_off_nothing_is_checked_whatever_was_kept() {
    assert_eq!(next(false, None, None, at(T0)), Next::Off);
    assert_eq!(next(false, Some(at(T0 - 2 * DAY)), None, at(T0)), Next::Off);
}

#[test]
fn a_host_that_never_checked_checks_now_and_then_once_a_day() {
    assert_eq!(next(true, None, None, at(T0)), Next::Now);
    let checked = Some(at(T0));
    assert_eq!(
        next(true, checked, None, at(T0 + HOUR)),
        Next::At(at(T0 + DAY))
    );
    assert_eq!(
        next(true, checked, None, at(T0 + DAY - 1)),
        Next::At(at(T0 + DAY))
    );
    assert_eq!(next(true, checked, None, at(T0 + DAY)), Next::Now);
    assert_eq!(next(true, checked, None, at(T0 + 3 * DAY)), Next::Now);
}

#[test]
fn a_check_that_could_not_reach_the_release_is_tried_again_after_an_hour() {
    let checked = Some(at(T0 - 2 * DAY));
    let tried = Some(at(T0));
    assert_eq!(
        next(true, checked, tried, at(T0 + 1)),
        Next::At(at(T0 + HOUR))
    );
    assert_eq!(next(true, checked, tried, at(T0 + HOUR)), Next::Now);
    assert_eq!(next(true, None, tried, at(T0 + 1)), Next::At(at(T0 + HOUR)));
}

#[test]
fn a_time_kept_in_the_future_holds_updates_off_for_a_day_at_most() {
    let ahead = Some(at(T0 + 365 * DAY));
    assert_eq!(next(true, ahead, None, at(T0)), Next::At(at(T0 + DAY)));
}

#[test]
fn the_time_a_check_reached_the_manifest_is_kept_in_the_data_folder() {
    let dirs = TestDirs::new();
    assert_eq!(last_checked(&dirs.data), None);
    remember_checked(&dirs.data, at(T0), Some("2.5.0")).unwrap();
    assert_eq!(last_checked(&dirs.data), Some(at(T0)));
    assert_eq!(
        kept(&dirs.data),
        Some(Kept {
            at: at(T0),
            newest: Some("2.5.0".into())
        })
    );
    remember_checked(&dirs.data, at(T0 + 1), None).unwrap();
    assert_eq!(
        kept(&dirs.data),
        Some(Kept {
            at: at(T0 + 1),
            newest: None
        })
    );
    // What an earlier floti kept, with no version, still says when.
    std::fs::write(
        dirs.data.join(CHECKED_FILE),
        format!(r#"{{"at":"{}"}}"#, at(T0)),
    )
    .unwrap();
    assert_eq!(last_checked(&dirs.data), Some(at(T0)));
    std::fs::write(dirs.data.join(CHECKED_FILE), "not json").unwrap();
    assert_eq!(last_checked(&dirs.data), None);
}

#[test]
fn the_network_and_the_disk_are_tried_again_soon_and_a_bad_release_is_not() {
    let soon = [
        UpdateError::Network {
            file: "update.json".into(),
            url: "https://floti.dev/update.json".into(),
            reason: "offline".into(),
        },
        UpdateError::CutOff {
            file: "floti".into(),
            reason: "reset".into(),
        },
        UpdateError::Disk {
            reason: "full".into(),
        },
        UpdateError::Replace {
            reason: "in use".into(),
        },
    ];
    let later = [
        UpdateError::BadSignature {
            file: "floti".into(),
        },
        UpdateError::WrongSha256 {
            file: "floti".into(),
        },
        UpdateError::Manifest("no schema".into()),
    ];
    assert!(soon.iter().all(retries_soon));
    assert!(!later.iter().any(retries_soon));
}

fn up_to_date() -> Checked {
    Checked::UpToDate {
        newest: "2.0.0".into(),
    }
}

/// The loop on Tokio's paused clock: a fake time that moves with it, and a
/// check that answers from a script and says when it ran and what time was
/// kept when it did. What the loop tells is recorded, and asks for a check
/// now go in through `ask`.
struct Run {
    on: watch::Sender<bool>,
    asks: mpsc::UnboundedSender<Ask>,
    ran: mpsc::UnboundedReceiver<(Timestamp, Option<Timestamp>)>,
    told: Arc<Mutex<Vec<Told>>>,
    ended: tokio::task::JoinHandle<Ended>,
    dirs: TestDirs,
}

impl Run {
    fn start(on: bool, answers: Vec<Checked>) -> Self {
        let dirs = TestDirs::new();
        let (setting, watched) = watch::channel(on);
        let (asks, asked) = mpsc::unbounded_channel();
        let (said, ran) = mpsc::unbounded_channel();
        let told = Arc::new(Mutex::new(Vec::new()));
        let telling = Arc::clone(&told);
        let start = tokio::time::Instant::now();
        let clock = move || {
            let elapsed = SignedDuration::try_from(start.elapsed()).unwrap();
            at(T0).checked_add(elapsed).unwrap()
        };
        let answers = Arc::new(Mutex::new(VecDeque::from(answers)));
        let data = dirs.data.clone();
        let check = move || {
            said.send((clock(), last_checked(&data))).unwrap();
            answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(up_to_date)
        };
        let ended = tokio::spawn(keep_up_to_date(
            dirs.data.clone(),
            watched,
            asked,
            clock,
            check,
            move |said| telling.lock().unwrap().push(said),
        ));
        Self {
            on: setting,
            asks,
            ran,
            told,
            ended,
            dirs,
        }
    }

    /// Asks for a check now: settles once a check has answered it.
    fn ask(&self) -> oneshot::Receiver<()> {
        let (answer, answered) = oneshot::channel();
        self.asks.send(answer).unwrap();
        answered
    }

    fn told(&self) -> Vec<Told> {
        self.told.lock().unwrap().clone()
    }

    /// When the next check ran, and what time was kept then.
    async fn next_check(&mut self) -> (Timestamp, Option<Timestamp>) {
        self.ran.recv().await.expect("a check")
    }

    /// Lets `wait` pass on the paused clock.
    async fn pass(&self, wait: SignedDuration) {
        tokio::time::sleep(Duration::try_from(wait).unwrap()).await;
    }
}

#[tokio::test(start_paused = true)]
async fn it_checks_at_once_then_a_day_after_each_check_that_reached_the_manifest() {
    let mut run = Run::start(true, vec![up_to_date(), up_to_date()]);
    assert_eq!(run.next_check().await, (at(T0), None));
    assert_eq!(run.next_check().await, (at(T0 + DAY), Some(at(T0))));
    assert_eq!(
        run.next_check().await,
        (at(T0 + 2 * DAY), Some(at(T0 + DAY)))
    );
}

#[tokio::test(start_paused = true)]
async fn with_the_setting_off_it_waits_for_it_and_checks_once_it_is_turned_on() {
    let mut run = Run::start(false, vec![]);
    run.pass(SignedDuration::from_hours(10 * 24)).await;
    assert!(
        run.ran.try_recv().is_err(),
        "nothing is checked while it is off"
    );
    run.on.send_replace(true);
    assert_eq!(run.next_check().await.0, at(T0 + 10 * DAY));
}

#[tokio::test(start_paused = true)]
async fn turning_it_off_between_checks_holds_the_next_one_until_it_is_on_again() {
    let mut run = Run::start(true, vec![]);
    assert_eq!(run.next_check().await.0, at(T0));
    run.on.send_replace(false);
    run.pass(SignedDuration::from_hours(3 * 24)).await;
    assert!(run.ran.try_recv().is_err());
    run.on.send_replace(true);
    assert_eq!(run.next_check().await.0, at(T0 + 3 * DAY));
}

#[tokio::test(start_paused = true)]
async fn a_check_that_could_not_reach_the_release_is_tried_again_an_hour_later_and_kept_no_time() {
    let mut run = Run::start(
        true,
        vec![Checked::Failed {
            why: "offline".into(),
            soon: true,
        }],
    );
    assert_eq!(run.next_check().await, (at(T0), None));
    assert_eq!(run.next_check().await, (at(T0 + HOUR), None));
}

#[tokio::test(start_paused = true)]
async fn a_release_that_failed_its_checks_is_looked_at_again_the_next_day() {
    let mut run = Run::start(
        true,
        vec![Checked::Failed {
            why: "floti does not match its signature.".into(),
            soon: false,
        }],
    );
    assert_eq!(run.next_check().await, (at(T0), None));
    assert_eq!(run.next_check().await, (at(T0 + DAY), Some(at(T0))));
}

#[tokio::test(start_paused = true)]
async fn a_newer_floti_in_place_ends_it_for_the_host_to_relaunch() {
    let mut run = Run::start(
        true,
        vec![Checked::Replaced {
            version: "999.0.0".into(),
        }],
    );
    assert_eq!(run.next_check().await.0, at(T0));
    assert_eq!((&mut run.ended).await.unwrap(), Ended::Relaunch);
    assert_eq!(
        last_checked(&run.dirs.data),
        Some(at(T0)),
        "the new host does not look again within the day"
    );
}

#[tokio::test(start_paused = true)]
async fn it_ends_with_the_settings() {
    let run = Run::start(false, vec![]);
    drop(run.on);
    assert_eq!(run.ended.await.unwrap(), Ended::Stopping);
}

#[test]
fn only_floti_installed_on_its_own_with_a_config_folder_of_its_own_is_kept_up_to_date() {
    let dirs = TestDirs::new();
    let mut env = dirs.user_env();
    // Where floti installed on its own goes on Windows; macOS and Linux
    // put it under the home.
    env.vars.insert(
        "LOCALAPPDATA".into(),
        dirs.folder("local app data").into_os_string(),
    );
    let standalone = standalone::Native.binary(&env).expect("a standalone place");
    std::fs::create_dir_all(standalone.parent().unwrap()).unwrap();
    std::fs::write(&standalone, b"floti").unwrap();
    let elsewhere = dirs.folder("target").join("floti");
    std::fs::write(&elsewhere, b"floti").unwrap();

    assert_eq!(
        eligible(&env, Some(&standalone), false),
        Some(standalone.clone())
    );
    assert_eq!(eligible(&env, Some(&standalone), true), None);
    assert_eq!(eligible(&env, Some(&elsewhere), false), None);
    assert_eq!(eligible(&env, None, false), None);
}

#[tokio::test(start_paused = true)]
async fn an_ask_checks_at_once_with_the_setting_off_and_is_answered_once_the_check_ended() {
    let mut run = Run::start(false, vec![up_to_date()]);
    run.pass(SignedDuration::from_hours(5)).await;
    let answered = run.ask();
    assert_eq!(run.next_check().await.0, at(T0 + 5 * HOUR));
    answered.await.expect("the ask is answered");
    assert_eq!(
        run.told(),
        vec![
            Told::Checking,
            Told::Checked {
                at: at(T0 + 5 * HOUR),
                checked: up_to_date()
            }
        ]
    );
    assert_eq!(
        kept(&run.dirs.data).and_then(|kept| kept.newest),
        Some("2.0.0".into())
    );
    // With the setting off, nothing more is checked by itself.
    run.pass(SignedDuration::from_hours(3 * 24)).await;
    assert!(run.ran.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn an_ask_between_daily_checks_checks_at_once() {
    let mut run = Run::start(true, vec![up_to_date(), up_to_date()]);
    assert_eq!(run.next_check().await.0, at(T0));
    run.pass(SignedDuration::from_hours(2)).await;
    let answered = run.ask();
    assert_eq!(run.next_check().await.0, at(T0 + 2 * HOUR));
    answered.await.expect("the ask is answered");
}

#[tokio::test(start_paused = true)]
async fn a_failed_check_is_told_with_why_and_still_answers_the_ask() {
    let failed = Checked::Failed {
        why: "could not download update.json.".into(),
        soon: true,
    };
    let mut run = Run::start(false, vec![failed.clone()]);
    let answered = run.ask();
    run.next_check().await;
    answered.await.expect("the ask is answered");
    assert_eq!(
        run.told(),
        vec![
            Told::Checking,
            Told::Checked {
                at: at(T0),
                checked: failed
            }
        ]
    );
    assert_eq!(kept(&run.dirs.data), None, "nothing was reached");
}

#[tokio::test(start_paused = true)]
async fn a_newer_floti_found_by_an_ask_is_told_before_the_host_relaunches() {
    let replaced = Checked::Replaced {
        version: "2.1.0".into(),
    };
    let mut run = Run::start(false, vec![replaced.clone()]);
    let answered = run.ask();
    run.next_check().await;
    answered.await.expect("the ask is answered");
    assert_eq!((&mut run.ended).await.unwrap(), Ended::Relaunch);
    assert_eq!(
        run.told().last(),
        Some(&Told::Checked {
            at: at(T0),
            checked: replaced
        })
    );
}

#[tokio::test(start_paused = true)]
async fn with_every_asker_gone_it_keeps_its_schedule() {
    let mut run = Run::start(true, vec![up_to_date(), up_to_date()]);
    let (closed, _) = mpsc::unbounded_channel();
    drop(std::mem::replace(&mut run.asks, closed));
    assert_eq!(run.next_check().await.0, at(T0));
    assert_eq!(run.next_check().await.0, at(T0 + DAY));
}

#[test]
fn a_read_only_config_folder_is_named_apart_from_any_other_floti() {
    assert_eq!(unavailable(true), UpdateUnavailable::ReadOnly);
    assert_eq!(unavailable(false), UpdateUnavailable::NotStandalone);
}
