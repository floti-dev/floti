//! A Windows host hears the user signing out, or the computer shutting down,
//! as Windows tells a process that has loaded user32: through its top-level
//! windows, with `WM_QUERYENDSESSION` and then `WM_ENDSESSION`. Each test
//! plays Windows for one real host running a fake agent in a session: it
//! finds the host's window by its class and the host's process, sends both
//! messages as Windows does (`SendMessageTimeoutW`), and checks that by the
//! time `WM_ENDSESSION` returns, after which Windows may end the process at
//! once, the session is recorded as stopped by Floti, and that the host then
//! exits.
//!
//! A host with no console is the one Floti starts; one with a console is
//! `floti host` run in a terminal. macOS and Linux tell a process with a
//! signal instead (`host.rs` stops a host with SIGTERM), so there no window
//! is found and each test ends at once.

use std::path::Path;
use std::time::{Duration, Instant};

use floti_host::fake_agent;
use floti_host::platform::process_tree::{Native as NativeTree, ProcessTree};
use floti_host::platform::startup::{self, SessionMessage};
use floti_host::testkit::{
    FakeAgent, HostProcess, PATIENCE, TestClient, TestDirs, record_test_host,
};
use floti_protocol::Knowable;
use floti_protocol::channels::sessions::{SessionList, SessionStart, StartSessionRequest};

use super::console::{Console, console_of};

/// Temporary: main's host has no window of its own for the session ending,
/// so the test is pointed at the one top-level window it has, the
/// notification area's, which a host a desktop started makes.
const WINDOW_CLASS: &str = "FlotiHostNotificationArea";

const TERMINAL: &[&str] = &["terminal"];

/// How long a test waits for `WM_ENDSESSION` to return: longer than the 20 s
/// the host's window waits for its stop, so a send that fails means the
/// window never answered.
const END_SESSION_SEND: Duration = Duration::from_secs(30);

const LOOK_AGAIN: Duration = Duration::from_millis(20);

/// How the host under test was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// With no console, as Floti starts one.
    NoConsole,
    /// Sharing the test's console, as `floti host` in a terminal does.
    Console,
    /// With no console, by a desktop (`--desktop`), so with a notification
    /// area beside its window.
    Desktop,
}

/// Whether this OS tells a process the session is ending through a window.
fn windows_here() -> bool {
    startup::top_level_windows(std::process::id(), WINDOW_CLASS) != Knowable::Unknown
}

/// `floti host` over `dirs`, recording the fake agent's runs, started as
/// `start`.
fn host(dirs: &TestDirs, agent: &FakeAgent, start: Start) -> HostProcess {
    record_test_host(&dirs.floti_endpoint(), &dirs.data);
    let mut command = dirs.floti_command(env!("CARGO_BIN_EXE_floti"));
    command.arg("host");
    let (log_var, log_dir) = agent.log_env();
    command.env(log_var, log_dir);
    if start == Start::Desktop {
        let app = dirs.folder("Floti app");
        command
            .arg("--desktop")
            .arg(app.join("Floti.exe"))
            .arg("--desktop-arg")
            .arg(app.join("resources app"));
    }
    if start != Start::Console {
        startup::without_console(&mut command);
    }
    HostProcess::spawn(command)
}

/// Starts a shell session through `client` with the fake agent running in
/// it, and returns the fake agent's process.
async fn a_session_running_the_fake_agent(
    dirs: &TestDirs,
    agent: &FakeAgent,
    client: &mut TestClient,
) -> u32 {
    let folder = dirs.folder("a project");
    let session = client
        .request::<SessionStart>(&StartSessionRequest {
            cwd: folder.to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
            ..StartSessionRequest::default()
        })
        .await
        .unwrap();
    client.attach(1, session.id).await;
    // The shell's prompt first, so what is typed reaches a shell reading it.
    client.read_stream_until(1, |bytes| !bytes.is_empty()).await;
    client
        .type_into(1, format!("\"{}\"\r", agent.exe().display()).as_bytes())
        .await;
    client
        .read_stream_until(1, |_| !agent.records().is_empty())
        .await;
    agent.records()[0].pid
}

/// Each session's status and who stopped it, from the host's database.
fn rows(data: &Path) -> Vec<(String, Option<String>)> {
    let db = rusqlite::Connection::open(data.join("floti.db")).expect("open floti.db");
    let mut statement = db
        .prepare("SELECT status, stopped_by FROM sessions ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// Sends `window` `message` off the test's runtime, which the send blocks
/// for as long as the window takes, and returns its answer.
async fn send(window: isize, message: SessionMessage, within: Duration) -> isize {
    tokio::task::spawn_blocking(move || startup::send_session_message(window, message, within))
        .await
        .unwrap()
        .unwrap_or_else(|err| panic!("{message:?} was not answered within {within:?}: {err}"))
}

/// Waits until process `pid` has ended.
async fn ended(pid: u32) {
    let deadline = Instant::now() + PATIENCE;
    while NativeTree.is_alive(pid) != Knowable::Known(false) {
        if Instant::now() >= deadline {
            let _ = NativeTree.end_tree(pid, floti_host::platform::process_tree::Ending::Forced);
            panic!("process {pid} was still running {PATIENCE:?} after the host stopped");
        }
        tokio::time::sleep(LOOK_AGAIN).await;
    }
}

/// A host started as `start` with a session running is sent the session
/// ending as Windows sends it, for a sign-out when `sign_out` and a shutdown
/// otherwise, and its stop says it stopped for `said`.
async fn the_session_ends(start: Start, sign_out: bool, said: &str) {
    if !windows_here() {
        return;
    }
    let dirs = TestDirs::new();
    let agent = fake_agent!(&dirs);
    let host = host(&dirs, &agent, start);
    let endpoint = dirs.floti_endpoint();
    let mut client = TestClient::connect(&endpoint, TERMINAL).await;
    let fake = a_session_running_the_fake_agent(&dirs, &agent, &mut client).await;
    let expected = if start == Start::Console {
        Console::Has
    } else {
        Console::None
    };
    assert_eq!(console_of(host.id()), expected, "{start:?}");

    let windows = startup::top_level_windows(host.id(), WINDOW_CLASS);
    let Knowable::Known(windows) = windows else {
        panic!("the host's windows could not be looked for");
    };
    assert_eq!(
        windows.len(),
        1,
        "the host's windows of class {WINDOW_CLASS}"
    );
    let window = windows[0];

    if start == Start::Desktop {
        // Its client leaves with the session running, so the notification
        // area shows the host, and a new one comes, so it hides it again:
        // the window is the same one all along.
        drop(client);
        client = TestClient::connect(&endpoint, TERMINAL).await;
        assert_eq!(
            startup::top_level_windows(host.id(), WINDOW_CLASS),
            Knowable::Known(vec![window])
        );
    }

    // Asked whether the session may end, the host says yes and does
    // nothing more; nor does a session that turns out not to end.
    assert_eq!(
        send(window, SessionMessage::Query { sign_out }, PATIENCE).await,
        1,
        "the host agrees the session may end"
    );
    send(
        window,
        SessionMessage::End {
            ends: false,
            sign_out,
        },
        PATIENCE,
    )
    .await;
    assert_eq!(
        client.request::<SessionList>(&()).await.unwrap().len(),
        1,
        "the session runs on"
    );

    send(
        window,
        SessionMessage::End {
            ends: true,
            sign_out,
        },
        END_SESSION_SEND,
    )
    .await;
    // Windows may end the host from here on: the session is recorded.
    assert_eq!(
        rows(&dirs.data),
        [("exited".to_owned(), Some("floti".to_owned()))],
        "{}",
        dirs.host_log()
    );

    drop(client);
    let (stdout, log) = host.exited().await;
    assert!(
        stdout.contains(&format!("Floti host is stopping: {said}.")),
        "{stdout}{log}"
    );
    assert!(stdout.contains("Floti host stopped."), "{stdout}{log}");
    ended(fake).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_console_records_its_sessions_before_a_sign_out_goes_on_and_then_exits() {
    the_session_ends(Start::NoConsole, true, "signing out").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_console_records_its_sessions_before_a_shutdown_goes_on_and_then_exits() {
    the_session_ends(Start::NoConsole, false, "the computer shutting down").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_a_console_records_its_sessions_before_a_sign_out_goes_on_and_then_exits() {
    the_session_ends(Start::Console, true, "signing out").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_a_desktop_started_hears_a_sign_out_through_its_own_window_whatever_its_icon_does() {
    the_session_ends(Start::Desktop, true, "signing out").await;
}
