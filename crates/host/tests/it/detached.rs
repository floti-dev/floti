//! Starting a program detached (`ProcessTree::start_detached`), as a
//! relaunch and `floti connect` start the host: the environment it gets is
//! this process's, less each variable it was told to leave out, on every OS.
//! `floti connect` leaves the SSH variables out this way, so the host it
//! starts over ssh does not take itself for an SSH session's.

use std::path::Path;
use std::time::{Duration, Instant};

use floti_host::platform::process_tree::{Native as NativeTree, ProcessTree};
use floti_host::testkit::{PATIENCE, TestDirs};

#[test]
fn a_program_started_detached_lacks_the_variables_named_and_keeps_the_others() {
    // Both are set for every test by the runner, cargo's and nextest's alike.
    let removed = "CARGO_MANIFEST_DIR";
    let kept = "PATH";
    assert!(
        std::env::var_os(removed).is_some(),
        "the test runner sets {removed}"
    );
    let ours = std::env::var(kept).expect("this test's PATH");
    let dirs = TestDirs::new();
    let report = dirs.root().join("report.json");
    let report_arg = report.to_str().expect("a Unicode path").to_owned();

    let mut detached = NativeTree
        .start_detached(
            Path::new(env!("CARGO_BIN_EXE_floti-fake-program")),
            &[
                "--show-env",
                removed,
                "--show-env",
                kept,
                "--report-file",
                &report_arg,
            ],
            &[removed],
        )
        .expect("start the program detached");
    let deadline = Instant::now() + PATIENCE;
    let code = loop {
        if let Some(code) = detached.process.exited() {
            break code;
        }
        assert!(
            Instant::now() < deadline,
            "the program did not exit within {PATIENCE:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(code, Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).expect("the program's report"))
            .expect("a JSON report");
    assert_eq!(report["env"][removed], serde_json::Value::Null, "{report}");
    assert_eq!(report["env"][kept], serde_json::json!(ours), "{report}");
}
