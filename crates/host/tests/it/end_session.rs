//! A Windows host hears the user signing out, the computer shutting down or
//! Restart Manager closing it as Windows tells a process that has loaded
//! user32: through its top-level windows, with `WM_QUERYENDSESSION` and then
//! `WM_ENDSESSION`. Windows then ends the host, its keeper and its sessions'
//! processes in no set order, and a host still alive while its sessions die
//! would record each as exited, so none would be offered for restore.
//!
//! Each test plays Windows for one real host with a fake agent running in a
//! shell session. It finds the host's window by its class and the host's
//! process and sends both messages as Windows does (`SendMessageTimeoutW`).
//! When `WM_ENDSESSION` returns, after which Windows may end anything, the
//! session's shell and the fake agent must be gone already, ended by the host
//! with its records frozen, so a window that returns early fails every time.
//! It checks that the session's row is still `running`, that the host exits,
//! and that the next host offers the session for restore.
//!
//! A host with no console is the one Floti starts; one with a console is
//! `floti host` run in a terminal. macOS and Linux tell a process with a
//! signal instead (`host.rs` stops a host with SIGTERM), so there no window
//! is found and each test ends at once.

use std::path::Path;
use std::time::{Duration, Instant};

use floti_host::fake_agent;
use floti_host::platform::process_table::{self, ProcessTable};
use floti_host::platform::process_tree::{Native as NativeTree, ProcessTree};
use floti_host::platform::startup::{self, SessionEnd, SessionMessage};
use floti_host::testkit::{
    FakeAgent, HostProcess, PATIENCE, TestClient, TestDirs, record_test_host,
};
use floti_protocol::Knowable;
use floti_protocol::channels::sessions::{
    SessionList, SessionRestorable, SessionStart, StartSessionRequest,
};

use super::console::{Console, console_of};

/// The class of the window the host hears the session ending through.
const WINDOW_CLASS: &str = startup::END_SESSION_WINDOW_CLASS;

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

/// A shell session with the fake agent running in it.
struct Session {
    id: u64,
    /// The session's shell, its root process.
    shell: u32,
    /// The fake agent, running in the shell.
    fake: u32,
}

/// Starts a shell session through `client` and runs the fake agent in it.
async fn a_session_running_the_fake_agent(
    dirs: &TestDirs,
    agent: &FakeAgent,
    client: &mut TestClient,
) -> Session {
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
    // It records itself as it starts and prints nothing until it reads a
    // line, so its record is looked for rather than its output.
    let deadline = Instant::now() + PATIENCE;
    let fake = loop {
        if let Some(record) = agent.records().first() {
            break record.pid;
        }
        assert!(
            Instant::now() < deadline,
            "the fake agent did not start within {PATIENCE:?}"
        );
        tokio::time::sleep(LOOK_AGAIN).await;
    };
    let shell = parent_of(fake).expect("the fake agent is in the process table");
    Session {
        id: session.id,
        shell,
        fake,
    }
}

/// The process that started process `pid`, as the process table says.
fn parent_of(pid: u32) -> Option<u32> {
    process_table::Native
        .processes()
        .expect("read the process table")
        .into_iter()
        .find(|row| row.pid == pid)
        .map(|row| row.parent_pid)
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

/// A host started as `start` with a session running is sent the session
/// ending for `why`, as Windows sends it, and says it stopped for `said`.
async fn the_session_ends(start: Start, why: SessionEnd, said: &str) {
    if !windows_here() {
        return;
    }
    let dirs = TestDirs::new();
    let agent = fake_agent!(&dirs);
    let host = host(&dirs, &agent, start);
    let host_pid = host.id();
    let endpoint = dirs.floti_endpoint();
    let mut client = TestClient::connect(&endpoint, TERMINAL).await;
    let session = a_session_running_the_fake_agent(&dirs, &agent, &mut client).await;
    let expected = if start == Start::Console {
        Console::Has
    } else {
        Console::None
    };
    assert_eq!(console_of(host_pid), expected, "{start:?}");

    let windows = startup::top_level_windows(host_pid, WINDOW_CLASS);
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
            startup::top_level_windows(host_pid, WINDOW_CLASS),
            Knowable::Known(vec![window])
        );
    }

    // Asked whether the session may end, the host says yes and does
    // nothing more; nor does a session that turns out not to end.
    assert_eq!(
        send(window, SessionMessage::Query(why), PATIENCE).await,
        1,
        "the host agrees the session may end"
    );
    send(window, SessionMessage::DoesNotEnd(why), PATIENCE).await;
    assert_eq!(
        client.request::<SessionList>(&()).await.unwrap().len(),
        1,
        "the session runs on"
    );

    send(window, SessionMessage::Ends(why), END_SESSION_SEND).await;
    // From here Windows may end the host at any moment, and ends whatever
    // of the session still runs: the host has ended it already, with its
    // records frozen, so no end of it can be recorded.
    assert_eq!(
        [session.shell, session.fake].map(|pid| NativeTree.is_alive(pid)),
        [Knowable::Known(false), Knowable::Known(false)],
        "the session's shell and the fake agent are gone when WM_ENDSESSION returns"
    );
    assert_eq!(
        rows(&dirs.data),
        [("running".to_owned(), None)],
        "the session's row is left for the next start: {}",
        dirs.host_log()
    );
    drop(client);
    let (stdout, log) = host.exited().await;
    assert!(
        stdout.contains(&format!("Floti host is stopping: {said}.")),
        "{stdout}{log}"
    );
    assert!(
        stdout.contains(
            "Floti host stopped as the session ended; its next start offers its sessions for restore."
        ),
        "{stdout}{log}"
    );

    // The next host, at the next sign-in, offers it for restore.
    let next = self::host(&dirs, &agent, Start::NoConsole);
    let mut client = TestClient::connect(&endpoint, TERMINAL).await;
    let offer = client
        .request::<SessionRestorable>(&())
        .await
        .unwrap()
        .expect("the session is offered for restore");
    assert_eq!(
        offer
            .sessions
            .iter()
            .map(|lost| lost.id)
            .collect::<Vec<_>>(),
        [session.id]
    );
    assert_eq!(rows(&dirs.data), [("lost".to_owned(), None)]);
    next.stop(client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_console_leaves_its_sessions_for_restore_when_the_user_signs_out() {
    the_session_ends(Start::NoConsole, SessionEnd::SignOut, "signing out").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_console_leaves_its_sessions_for_restore_when_the_computer_shuts_down() {
    the_session_ends(
        Start::NoConsole,
        SessionEnd::Shutdown,
        "the computer shutting down",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_console_leaves_its_sessions_for_restore_when_restart_manager_closes_it() {
    the_session_ends(
        Start::NoConsole,
        SessionEnd::CloseApp,
        "Windows asking it to close",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_a_console_leaves_its_sessions_for_restore_when_the_user_signs_out() {
    the_session_ends(Start::Console, SessionEnd::SignOut, "signing out").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_a_desktop_started_hears_a_sign_out_through_its_own_window_whatever_its_icon_does() {
    the_session_ends(Start::Desktop, SessionEnd::SignOut, "signing out").await;
}
