//! The held-file retry with its pauses recorded rather than slept, and each
//! OS's classification, on every OS.

use std::io;
use std::path::Path;
use std::time::Duration;

use super::windows::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
use super::*;
use crate::Error;

/// What a run did: how often it tried and the pauses it asked for, which
/// pass no real time.
struct Run {
    tries: usize,
    slept: Vec<Duration>,
}

impl Run {
    fn waited(&self) -> Duration {
        self.slept.iter().sum()
    }
}

/// A rename through [`retry`], with Windows' classification, that fails with
/// Windows error `held` `fails` times, then answers `last`.
fn rename(fails: usize, held: i32, last: fn() -> io::Result<()>) -> (io::Result<()>, Run) {
    let mut run = Run {
        tries: 0,
        slept: Vec::new(),
    };
    let tries = &mut run.tries;
    let slept = &mut run.slept;
    let result = retry(
        &|err: &io::Error| windows::Native.is_held(err),
        &mut || {
            *tries += 1;
            if *tries <= fails {
                Err(io::Error::from_raw_os_error(held))
            } else {
                last()
            }
        },
        &mut |pause| slept.push(pause),
    );
    (result, run)
}

/// The removal of a folder through [`remove_held`], with `classification`,
/// refused `fails` times on `Uninstall Helm.exe` with Windows error `held`,
/// then answering `last`.
fn removal(
    classification: &dyn HeldFiles,
    fails: usize,
    held: i32,
    last: fn() -> crate::Result<()>,
) -> (crate::Result<()>, Run) {
    let mut run = Run {
        tries: 0,
        slept: Vec::new(),
    };
    let tries = &mut run.tries;
    let slept = &mut run.slept;
    let result = remove_held(
        classification,
        &mut || {
            *tries += 1;
            if *tries <= fails {
                Err(Error::path(
                    "remove",
                    Path::new(r"C:\Programs\Helm\Uninstall Helm.exe"),
                    io::Error::from_raw_os_error(held),
                ))
            } else {
                last()
            }
        },
        &mut |pause| slept.push(pause),
    );
    (result, run)
}

#[test]
fn an_operation_that_works_at_once_waits_for_nothing() {
    let (result, run) = rename(0, ERROR_SHARING_VIOLATION, || Ok(()));
    assert!(result.is_ok());
    assert_eq!((run.tries, run.waited()), (1, Duration::ZERO));
}

#[test]
fn a_held_file_is_tried_again_with_longer_pauses_until_it_is_let_go() {
    for held in [ERROR_SHARING_VIOLATION, ERROR_ACCESS_DENIED] {
        let (result, run) = rename(4, held, || Ok(()));
        assert!(result.is_ok(), "code {held}");
        assert_eq!(run.tries, 5);
        assert_eq!(
            run.slept,
            [25, 50, 100, 200].map(Duration::from_millis),
            "each pause twice the last"
        );
    }
}

#[test]
fn a_file_held_past_the_patience_fails_with_the_operations_own_error() {
    let (result, run) = rename(usize::MAX, ERROR_SHARING_VIOLATION, || Ok(()));
    let err = result.unwrap_err();
    assert_eq!(err.raw_os_error(), Some(ERROR_SHARING_VIOLATION));
    assert_eq!(
        err.to_string(),
        io::Error::from_raw_os_error(ERROR_SHARING_VIOLATION).to_string(),
        "the same message a single try gives"
    );
    assert_eq!(run.waited(), HELD_PATIENCE, "pauses add up to the patience");
    assert!(run.slept.iter().all(|pause| *pause <= LONGEST_PAUSE));
    assert_eq!(
        run.tries,
        run.slept.len() + 1,
        "one last try after the last pause"
    );
}

#[test]
fn any_other_failure_is_not_tried_again() {
    let (result, run) = rename(0, ERROR_SHARING_VIOLATION, || {
        Err(io::Error::from(io::ErrorKind::NotFound))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!((run.tries, run.waited()), (1, Duration::ZERO));
}

#[test]
fn a_held_failure_then_another_stops_at_the_other() {
    let (result, run) = rename(2, ERROR_SHARING_VIOLATION, || {
        Err(io::Error::from(io::ErrorKind::NotFound))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(run.tries, 3);
    assert_eq!(run.slept, [25, 50].map(Duration::from_millis));
}

#[test]
fn a_removal_refused_on_a_held_file_is_tried_again_until_it_goes() {
    for held in [ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION] {
        let (result, run) = removal(&windows::Native, 3, held, || Ok(()));
        assert!(result.is_ok(), "code {held}");
        assert_eq!(run.tries, 4);
        assert_eq!(run.slept, [25, 50, 100].map(Duration::from_millis));
    }
}

#[test]
fn a_removal_held_past_the_patience_fails_with_the_line_a_single_try_gives() {
    let (result, run) = removal(&windows::Native, usize::MAX, ERROR_ACCESS_DENIED, || Ok(()));
    let single = Error::path(
        "remove",
        Path::new(r"C:\Programs\Helm\Uninstall Helm.exe"),
        io::Error::from_raw_os_error(ERROR_ACCESS_DENIED),
    );
    assert_eq!(result.unwrap_err().to_string(), single.to_string());
    assert_eq!(run.waited(), HELD_PATIENCE);
    assert_eq!(run.tries, run.slept.len() + 1);
}

#[test]
fn a_removal_refused_for_another_reason_is_not_tried_again() {
    let (result, run) = removal(&windows::Native, 0, ERROR_ACCESS_DENIED, || {
        Err(Error::Refused(
            "a link that could not be removed".to_owned(),
        ))
    });
    assert!(matches!(result, Err(Error::Refused(_))));
    assert_eq!((run.tries, run.waited()), (1, Duration::ZERO));
}

#[test]
fn macos_and_linux_hold_no_file_so_nothing_is_tried_again() {
    for code in [ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION] {
        assert!(!posix::Native.is_held(&io::Error::from_raw_os_error(code)));
        let (result, run) = removal(&posix::Native, 1, code, || Ok(()));
        assert!(result.is_err(), "code {code}");
        assert_eq!((run.tries, run.waited()), (1, Duration::ZERO));
    }
}

#[test]
fn windows_counts_a_sharing_violation_and_access_denied_as_held_and_nothing_else() {
    let held = |code| windows::Native.is_held(&io::Error::from_raw_os_error(code));
    assert!(held(ERROR_SHARING_VIOLATION));
    assert!(held(ERROR_ACCESS_DENIED));
    assert!(!held(2), "file not found is not held");
    assert!(!windows::Native.is_held(&io::Error::other("no code")));
}
