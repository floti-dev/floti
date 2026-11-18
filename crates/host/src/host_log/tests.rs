//! The log's rotation, on a log in a test folder with a small limit, and a
//! stand-in for standard error that counts where it was pointed: these tests
//! never point the test process's own standard error anywhere.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    HIDDEN, HostLog, KEPT_LOGS, LOG_FILE, LOG_LIMIT, TAIL_MOST, TEST_LOG_LIMIT_MARKER,
    hide_secrets, log_limit, tail,
};
use crate::platform::private_files::{Native as NativeFiles, PrivateFiles};
use crate::platform::standard_error::StandardError;
use crate::testkit::TestDirs;

const LIMIT: u64 = 100;

/// Standard error as these tests see it: how many times it was pointed at a
/// file.
#[derive(Clone, Default)]
struct Pointed(Arc<AtomicUsize>);

impl Pointed {
    fn times(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl StandardError for Pointed {
    fn point_at(&self, _file: &File) -> std::io::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn numbered(folder: &Path, n: u32) -> PathBuf {
    folder.join(format!("{LOG_FILE}.{n}"))
}

/// What standard error would write: `bytes` appended to the log.
fn write(log: &HostLog, bytes: usize) {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(log.path())
        .unwrap();
    file.write_all(&vec![b'x'; bytes]).unwrap();
}

/// The names in `folder`, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_new_log_is_made_owner_only_and_standard_error_is_pointed_at_it() {
    let dirs = TestDirs::new();
    let folder = dirs.logs.join("not made yet");
    let pointed = Pointed::default();
    let log = HostLog::open(&folder, LIMIT, pointed.clone()).unwrap();
    assert_eq!(log.path(), folder.join(LOG_FILE));
    assert!(NativeFiles.owner_only(log.path()).unwrap());
    assert_eq!(pointed.times(), 1);
}

#[test]
fn a_log_past_its_limit_at_start_is_rotated_once_the_host_owns_it_keeping_three_older_files() {
    let dirs = TestDirs::new();
    let folder = &dirs.logs;
    fs::write(folder.join(LOG_FILE), "the last run\n".repeat(10)).unwrap();
    for n in 1..=KEPT_LOGS {
        fs::write(numbered(folder, n), format!("rotated {n}\n")).unwrap();
    }
    let pointed = Pointed::default();
    let log = HostLog::open(folder, LIMIT, pointed.clone()).unwrap();

    // Another host may still be writing to it until this one holds its data
    // folder.
    assert!(!log.rotate_if_full().unwrap());
    assert_eq!(pointed.times(), 1);

    assert!(log.own().unwrap());
    assert_eq!(pointed.times(), 2);
    assert_eq!(fs::read_to_string(log.path()).unwrap(), "");
    assert_eq!(
        fs::read_to_string(numbered(folder, 1)).unwrap(),
        "the last run\n".repeat(10)
    );
    assert_eq!(
        fs::read_to_string(numbered(folder, 2)).unwrap(),
        "rotated 1\n"
    );
    assert_eq!(
        fs::read_to_string(numbered(folder, 3)).unwrap(),
        "rotated 2\n"
    );
    assert_eq!(
        names(folder),
        ["host.log", "host.log.1", "host.log.2", "host.log.3"]
    );
}

#[test]
fn writes_past_the_limit_rotate_the_log_and_only_three_older_files_are_kept() {
    let dirs = TestDirs::new();
    let folder = &dirs.logs;
    let pointed = Pointed::default();
    let log = HostLog::open(folder, LIMIT, pointed.clone()).unwrap();
    assert!(!log.own().unwrap());

    write(&log, 60);
    assert!(!log.rotate_if_full().unwrap(), "under the limit");
    for round in 1..=6 {
        write(&log, 60);
        assert!(log.rotate_if_full().unwrap(), "round {round}");
        assert_eq!(fs::metadata(log.path()).unwrap().len(), 0);
        write(&log, 0);
        assert!(!log.rotate_if_full().unwrap(), "round {round}, empty");
        write(&log, 60);
    }
    assert_eq!(pointed.times(), 1 + 6);
    assert_eq!(
        names(folder),
        ["host.log", "host.log.1", "host.log.2", "host.log.3"]
    );
    for n in 1..=KEPT_LOGS {
        let size = fs::metadata(numbered(folder, n)).unwrap().len();
        assert_eq!(size, 120, "host.log.{n}");
    }
}

#[test]
fn a_log_under_its_limit_is_left_as_it_is() {
    let dirs = TestDirs::new();
    fs::write(dirs.logs.join(LOG_FILE), "short\n").unwrap();
    let log = HostLog::open(&dirs.logs, LIMIT, Pointed::default()).unwrap();
    assert!(!log.own().unwrap());
    assert_eq!(fs::read_to_string(log.path()).unwrap(), "short\n");
    assert_eq!(names(&dirs.logs), ["host.log"]);
}

#[test]
fn the_limit_is_five_mebibytes_unless_a_test_build_is_given_one() {
    let dirs = TestDirs::new();
    assert_eq!(log_limit(&dirs.data), LOG_LIMIT);
    assert_eq!(LOG_LIMIT, 5 * 1024 * 1024);
    fs::write(dirs.data.join(TEST_LOG_LIMIT_MARKER), "300").unwrap();
    assert_eq!(log_limit(&dirs.data), 300);
}

#[test]
fn the_start_line_stamps_utc_and_a_local_offset_never_reaches_it() {
    let started: jiff::Timestamp = "2026-11-16T02:51:13.5Z".parse().unwrap();
    let line = super::start_line(started);
    assert!(
        line.contains(" started at 2026-11-16T02:51:13.5Z, process "),
        "{line}"
    );
    // The same moment given as Mountain time is still stamped in UTC.
    let local: jiff::Timestamp = "2026-11-15T19:51:13.5-07:00".parse().unwrap();
    assert_eq!(super::start_line(local), line);
}

#[test]
fn a_tail_answers_the_latest_lines_oldest_first_reading_back_across_chunks() {
    let dirs = TestDirs::new();
    let path = dirs.folder("logs dir").join(LOG_FILE);
    // Lines long enough that the last 500 span several 64 KiB chunks.
    let line = |n: usize| format!("line {n:04} {}", "x".repeat(300));
    let text: String = (0..2000).map(|n| format!("{}\r\n", line(n))).collect();
    fs::write(&path, text).unwrap();
    assert_eq!(
        tail(&path, 3, None).unwrap().unwrap(),
        [line(1997), line(1998), line(1999)]
    );
    // Never more than 500, and every line whole across a chunk's edge.
    let most = tail(&path, 10_000, None).unwrap().unwrap();
    assert_eq!(most.len(), TAIL_MOST);
    assert_eq!(most, (1500..2000).map(line).collect::<Vec<_>>());
}

#[test]
fn a_tail_with_a_word_keeps_only_lines_holding_it_in_any_case_and_reads_to_the_start() {
    let dirs = TestDirs::new();
    let path = dirs.folder("logs dir").join(LOG_FILE);
    fs::write(
        &path,
        "first Plugin trackr loaded\n\nsecond nothing\nthird plugin x crashed\nlast",
    )
    .unwrap();
    assert_eq!(
        tail(&path, 10, Some(" PLUGIN ")).unwrap().unwrap(),
        ["first Plugin trackr loaded", "third plugin x crashed"]
    );
    assert_eq!(
        tail(&path, 10, None).unwrap().unwrap(),
        [
            "first Plugin trackr loaded",
            "second nothing",
            "third plugin x crashed",
            "last"
        ]
    );
    assert_eq!(tail(&path, 0, None).unwrap().unwrap(), Vec::<String>::new());
    // No log yet: nothing, not an error.
    assert_eq!(
        tail(&dirs.folder("empty").join(LOG_FILE), 5, None).unwrap(),
        None
    );
}

#[test]
fn secrets_are_hidden_and_a_fingerprint_stays() {
    let token = "0123456789abcdef".repeat(4);
    assert_eq!(
        hide_secrets(&format!("granted {token} to session 4")),
        format!("granted {HIDDEN} to session 4")
    );
    assert_eq!(
        hide_secrets("Authorization: Bearer abc.DEF-123 sent"),
        format!("Authorization: Bearer {HIDDEN} sent")
    );
    assert_eq!(
        hide_secrets("authorization: bearer x"),
        format!("authorization: bearer {HIDDEN}")
    );
    let plain = "device 1a2b3c4d5e6f7a8b attached, \u{fc}n\u{ef}code kept";
    assert!(matches!(hide_secrets(plain), std::borrow::Cow::Borrowed(_)));
    assert_eq!(hide_secrets("Bearer "), "Bearer ");
}

#[test]
fn known_key_shapes_and_a_bearer_with_a_colon_are_hidden() {
    for (line, said) in [
        ("Bearer: abc.def next", format!("Bearer: {HIDDEN} next")),
        (
            "authorization bearer:xyz",
            format!("authorization bearer:{HIDDEN}"),
        ),
        (
            "key sk-ant-api03-AbC_d-9 was set",
            format!("key sk-ant-{HIDDEN} was set"),
        ),
        ("GH_TOKEN=ghp_A1b2C3", format!("GH_TOKEN=ghp_{HIDDEN}")),
        (
            "(gho_x) (ghu_y) (ghs_z)",
            format!("(gho_{HIDDEN}) (ghu_{HIDDEN}) (ghs_{HIDDEN})"),
        ),
        (
            "pat github_pat_11AB_cd, then",
            format!("pat github_pat_{HIDDEN}, then"),
        ),
    ] {
        assert_eq!(hide_secrets(line), said, "{line}");
    }
    // A prefix inside a word, or with nothing after it, is no key; a bearer
    // with neither a space nor a colon after it is a word.
    for kept in ["mysk-ant-x", "the ghp_ prefix", "bearers carry", "Bearer:"] {
        assert_eq!(hide_secrets(kept), kept);
    }
}
