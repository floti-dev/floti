//! A host Floti starts on Windows has no console, so no window opens for it
//! at sign-in or at a start, and none can be closed to stop it: a relaunched
//! host and one `floti connect` started. The host the desktop app starts is
//! checked beside the app's launcher (`launcher-console.floti.test.ts`).
//!
//! Whether a process has a console is asked from outside it, by
//! `floti-fake-gui --console-of=<pid>`, a program with no console of its own
//! that asks Windows to attach it to that process's console
//! (`AttachConsole`). A console counts hidden or not: `CREATE_NO_WINDOW`
//! gives a console program a console all the same, with its window hidden,
//! which is how the host started itself before. The first test shows the
//! probe finds that one, and the one a host started with no flags shares,
//! so the others failing to find one means there is none.
//!
//! Every host here runs on the test's own folders and is asked to stop by
//! the test, or by the guard that holds it when the test fails first.
#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use floti_host::platform::socket::Endpoint;
use floti_host::server::LOCK_FILE;
use floti_host::setup::{START_ON_CONNECT_FILE, START_ON_CONNECT_LINE};
use floti_host::testkit::{PATIENCE, TestClient, TestDirs, record_test_host};
use floti_protocol::channels::host::{HostStop, HostStopReason, HostStopRequest};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const LOOK_AGAIN: Duration = Duration::from_millis(20);

/// What the probe found of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Console {
    /// It has a console, hidden or not.
    Has,
    /// It has none.
    None,
}

/// Whether process `pid` has a console, asked by `floti-fake-gui`.
fn console_of(pid: u32) -> Console {
    let output = Command::new(env!("CARGO_BIN_EXE_floti-fake-gui"))
        .arg(format!("--console-of={pid}"))
        .stdin(Stdio::null())
        .output()
        .expect("run the console probe");
    match output.status.code() {
        Some(0) => Console::Has,
        Some(1) => Console::None,
        _ => panic!(
            "the probe could not tell whether process {pid} has a console: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// `floti <args>` over the test's folders.
fn floti(dirs: &TestDirs, args: &[&str]) -> Command {
    let mut command = dirs.floti_command(env!("CARGO_BIN_EXE_floti"));
    command.args(args);
    command
}

/// The process ids of every `floti host` start the log of the host over
/// `dirs` names, in order.
fn started_hosts(dirs: &TestDirs) -> Vec<u32> {
    dirs.host_log()
        .lines()
        .filter(|line| line.starts_with("floti host ") && line.contains(" started at "))
        .filter_map(|line| line.rsplit_once(", process ")?.1.trim().parse().ok())
        .collect()
}

/// Waits until `done` holds, failing after [`PATIENCE`] naming `what`.
async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what} within {PATIENCE:?}");
        tokio::time::sleep(LOOK_AGAIN).await;
    }
}

/// The host over the test's folders, however it was started: asked to stop
/// by the test, or, when the test fails first, as the guard is dropped.
struct HostOnTestFolders {
    endpoint: Endpoint,
    data: std::path::PathBuf,
    stopped: bool,
}

impl HostOnTestFolders {
    fn new(dirs: &TestDirs) -> Self {
        let endpoint = dirs.floti_endpoint();
        record_test_host(&endpoint, &dirs.data);
        Self {
            endpoint,
            data: dirs.data.clone(),
            stopped: false,
        }
    }

    /// Asks it to stop for `reason` over a connection that waits until it
    /// answers.
    async fn ask(&self, reason: HostStopReason) {
        let mut client = TestClient::connect(&self.endpoint, &[]).await;
        let _ = client
            .request::<HostStop>(&HostStopRequest { reason })
            .await;
    }

    /// Stops it, and waits for it to let go of its lock, which it holds for
    /// as long as it runs.
    async fn stop(mut self) {
        self.ask(HostStopReason::Stop).await;
        self.stopped = true;
        let lock = std::fs::File::options()
            .read(true)
            .open(self.data.join(LOCK_FILE))
            .expect("the host's lock file");
        until("the host to exit", || lock.try_lock().is_ok()).await;
    }
}

impl Drop for HostOnTestFolders {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let endpoint = self.endpoint.clone();
        // On a thread of its own: this may run on the test's runtime.
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async {
                    let mut client = TestClient::connect(&endpoint, &[]).await;
                    let _ = client
                        .request::<HostStop>(&HostStopRequest {
                            reason: HostStopReason::Stop,
                        })
                        .await;
                });
            }
        })
        .join();
    }
}

/// Waits for `child` to exit, killing it and failing after [`PATIENCE`].
async fn exited(child: &mut Child, what: &str) -> Option<i32> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = child.try_wait().expect("poll the process") {
            return status.code();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not exit within {PATIENCE:?}");
        }
        tokio::time::sleep(LOOK_AGAIN).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_probe_finds_a_console_in_a_host_started_with_no_flags_or_with_create_no_window() {
    for (how, flags) in [("no flags", 0), ("CREATE_NO_WINDOW", CREATE_NO_WINDOW)] {
        let dirs = TestDirs::new();
        let host = HostOnTestFolders::new(&dirs);
        let mut child = floti(&dirs, &["host"])
            .creation_flags(flags)
            .spawn()
            .expect("run floti host");
        // It answers, so it is past every start-up step that could exit.
        drop(TestClient::connect(&host.endpoint, &[]).await);
        assert_eq!(console_of(child.id()), Console::Has, "started with {how}");
        host.stop().await;
        exited(&mut child, "floti host").await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relaunched_host_has_no_console() {
    let dirs = TestDirs::new();
    let host = HostOnTestFolders::new(&dirs);
    let mut first = floti(&dirs, &["host"]).spawn().expect("run floti host");
    drop(TestClient::connect(&host.endpoint, &[]).await);

    // What every update ends with: the host starts floti again in its place.
    host.ask(HostStopReason::Relaunch).await;
    exited(&mut first, "the host relaunching").await;
    until("the relaunched host to start", || {
        started_hosts(&dirs).len() == 2
    })
    .await;
    let relaunched = started_hosts(&dirs)[1];
    assert_ne!(relaunched, first.id());
    drop(TestClient::connect(&host.endpoint, &[]).await);

    assert_eq!(console_of(relaunched), Console::None, "{}", dirs.host_log());
    host.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_floti_connect_started_has_no_console() {
    let dirs = TestDirs::new();
    // Where nothing runs, `floti connect` starts the host when setup left
    // this record.
    std::fs::create_dir_all(&dirs.data).unwrap();
    std::fs::write(dirs.data.join(START_ON_CONNECT_FILE), START_ON_CONNECT_LINE).unwrap();
    let host = HostOnTestFolders::new(&dirs);
    let mut connect = floti(&dirs, &["connect"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("run floti connect");

    until("the host floti connect started to start", || {
        !started_hosts(&dirs).is_empty()
    })
    .await;
    drop(TestClient::connect(&host.endpoint, &[]).await);
    let started = started_hosts(&dirs)[0];
    assert_ne!(started, connect.id());
    assert_eq!(console_of(started), Console::None, "{}", dirs.host_log());

    // Its connection ends when its input does; the host runs on.
    drop(connect.stdin.take());
    exited(&mut connect, "floti connect").await;
    host.stop().await;
}

/// The probe's own answer for a program with no console of its own, and
/// for a process that has ended, so a wrong pid is never read as "none".
#[test]
fn the_probe_finds_no_console_in_a_program_with_none_and_cannot_tell_for_one_that_ended() {
    let mut gui = Command::new(env!("CARGO_BIN_EXE_floti-fake-gui"))
        .arg("--fake-sleep=600000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("run the fake GUI program");
    assert_eq!(console_of(gui.id()), Console::None);
    let pid = gui.id();
    gui.kill().unwrap();
    gui.wait().unwrap();
    let ended = Command::new(env!("CARGO_BIN_EXE_floti-fake-gui"))
        .arg(format!("--console-of={pid}"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(ended.status.code(), Some(2), "{ended:?}");
}
