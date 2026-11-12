//! The run file as a lock and a record, on a real data folder.

use floti_protocol::Knowable;
use jiff::Timestamp;

use super::*;
use crate::testkit::TestDirs;

const ME: u32 = 4100;
const OTHER: u32 = 4200;

fn alive(answer: Knowable<bool>) -> impl Fn(u32) -> Knowable<bool> {
    move |_| answer.clone()
}

#[test]
fn a_second_update_refuses_while_the_first_runs_and_takes_over_once_it_has_ended() {
    let dirs = TestDirs::new();
    let first = RunLock::take(
        &dirs.data,
        OTHER,
        "Starting.",
        &alive(Knowable::Known(true)),
    )
    .unwrap();
    let refused =
        RunLock::take(&dirs.data, ME, "Starting.", &alive(Knowable::Known(true))).unwrap_err();
    assert!(
        matches!(refused, UpdateError::AlreadyRunning { pid: OTHER }),
        "{refused:?}"
    );
    // Unknown counts as running.
    let refused =
        RunLock::take(&dirs.data, ME, "Starting.", &alive(Knowable::Unknown)).unwrap_err();
    assert!(
        matches!(refused, UpdateError::AlreadyRunning { pid: OTHER }),
        "{refused:?}"
    );
    // A run file whose process has ended is a leftover.
    std::mem::forget(first);
    let taken = RunLock::take(&dirs.data, ME, "Starting.", &alive(Knowable::Known(false))).unwrap();
    assert_eq!(
        look(&dirs.data),
        Look::Running(RunRecord {
            pid: ME,
            line: "Starting.".into(),
            waiting_for_app: false
        })
    );
    drop(taken);
    assert_eq!(
        look(&dirs.data),
        Look::Absent,
        "a dropped lock removes its file"
    );
}

#[test]
fn a_run_file_that_is_no_record_is_taken_over() {
    let dirs = TestDirs::new();
    std::fs::write(dirs.data.join(RUN_FILE), "not json").unwrap();
    assert_eq!(look(&dirs.data), Look::Unreadable);
    let taken = RunLock::take(&dirs.data, ME, "Starting.", &alive(Knowable::Known(true)));
    assert!(taken.is_ok(), "{taken:?}");
}

#[test]
fn each_step_is_kept_and_the_end_says_how_it_went() {
    let dirs = TestDirs::new();
    let mut lock =
        RunLock::take(&dirs.data, ME, "Starting.", &alive(Knowable::Known(true))).unwrap();
    lock.step("Waiting for Helm to quit; it is updated once it has.", true);
    assert_eq!(
        look(&dirs.data),
        Look::Running(RunRecord {
            pid: ME,
            line: "Waiting for Helm to quit; it is updated once it has.".into(),
            waiting_for_app: true
        })
    );
    let at = Timestamp::from_second(1_800_000_000).unwrap();
    lock.end(at, Err("Helm could not be updated. Nothing was updated."));
    assert_eq!(look(&dirs.data), Look::Absent);
    let ended = ended(&dirs.data).unwrap();
    assert_eq!(
        ended,
        EndedRecord {
            pid: ME,
            at: at.to_string(),
            ok: false,
            line: "Helm could not be updated. Nothing was updated.".into()
        }
    );
    assert_eq!(ended.ended_at(), Some(at));
}
