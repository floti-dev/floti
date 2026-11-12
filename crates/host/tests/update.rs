//! `floti update`, end to end: a copy of the built `floti`, run from a fake
//! bin folder, updating itself and a fake desktop install from a loopback
//! HTTP server that serves a release signed with a key made in the test.
//!
//! The test key and the manifest's address reach the binary only through
//! the testkit's marker in the data folder (`floti-test-update.json`); the
//! desktop app is looked for only in the folder `FLOTI_DESKTOP_DIR` names,
//! which a build with the testkit reads. No host runs for the test's data
//! folder, so on Windows, where installing the app stops the host, there
//! is nothing to ask before it.
//! Everything is under the test's `TestDirs`. On Windows the release's
//! installer is the fake program, which records how it was run.
//!
//! The host's own background update is here too: a real `floti host` run
//! from a copy of the built floti put where floti installed on its own goes
//! (under the test's home and local app data), relaunching on the same
//! build served as a newer release, with its sessions in a real keeper.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use floti_host::fake_agent;
use floti_host::keeper::TEST_KEEPER_LINK_VERSION;
use floti_host::platform::installs::{DESKTOP_DIR_VAR, FakeDesktop, Native};
use floti_host::platform::process_tree::{Native as NativeTree, ProcessTree};
use floti_host::platform::standalone::{self, Standalone};
use floti_host::sha256::hex_digest;
use floti_host::store::Store;
use floti_host::testkit::{FakeAgent, PATIENCE, TestClient, TestDirs, record_test_host, run_floti};
use floti_host::update::background;
use floti_host::update::signature::TEST_UPDATE_MARKER;
use floti_host::version::{cpu_name, os_name};
use floti_host::workspace::settings;
use floti_protocol::channels::common::SessionStatus;
use floti_protocol::channels::host::{HostStop, HostStopReason, HostStopRequest};
use floti_protocol::channels::sessions::{
    SessionList, SessionRestorable, SessionStart, StartSessionRequest,
};
use floti_protocol::channels::workspace::{HostSettingsPatch, WriteSettings};
use floti_protocol::{KEEPER_LINK_VERSION, Knowable};

const NEWER: &str = "999.0.0";
/// The fake desktop install's version, where the OS keeps one in the
/// install (macOS); elsewhere it is unknown, which counts as older.
const OLDER: &str = "1.0.0";
const RECORD_VAR: &str = "FLOTI_FAKE_PROGRAM_RECORD";

#[derive(Clone)]
enum Served {
    Body(Vec<u8>),
    /// Announces the whole body, sends the first bytes, and closes.
    CutAfter(Vec<u8>, usize),
}

/// An HTTP/1.1 server on a loopback port, serving fixed bodies by path, one
/// connection at a time, until it is dropped.
struct Server {
    address: SocketAddr,
    served: Arc<Mutex<HashMap<String, Served>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let address = listener.local_addr().expect("its address");
        let served: Arc<Mutex<HashMap<String, Served>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let served = Arc::clone(&served);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        answer(stream, &served);
                    }
                }
            })
        };
        Self {
            address,
            served,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn serve(&self, path: &str, served: Served) {
        self.served.lock().unwrap().insert(path.to_owned(), served);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept so the loop sees the stop.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn answer(stream: TcpStream, served: &Mutex<HashMap<String, Served>>) {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {}
        }
    }
    let path = request.split(' ').nth(1).unwrap_or_default().to_owned();
    let found = served.lock().unwrap().get(&path).cloned();
    let mut stream = reader.into_inner();
    let (status, body, sent) = match found {
        None => ("404 Not Found", Vec::new(), 0),
        Some(Served::Body(body)) => {
            let length = body.len();
            ("200 OK", body, length)
        }
        Some(Served::CutAfter(body, kept)) => ("200 OK", body, kept),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body[..sent]);
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// A minisign key made for the test.
struct Signer {
    pair: minisign::KeyPair,
}

impl Signer {
    fn new() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("make a key pair"),
        }
    }

    fn public_text(&self) -> String {
        self.pair.pk.to_box().expect("box the key").into_string()
    }

    fn sign(&self, bytes: &[u8], comment: &str) -> Vec<u8> {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(bytes),
            Some(comment),
            None,
        )
        .expect("sign")
        .into_string()
        .into_bytes()
    }
}

/// A fake bin folder holding a copy of the built floti, a fake desktop
/// install, and a server with a signed release on it.
struct World {
    server: Server,
    signer: Signer,
    floti: PathBuf,
    apps: PathBuf,
    record: PathBuf,
    new_floti: Vec<u8>,
    floti_file: String,
    desktop_file: String,
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let bin = dirs.folder("bin dir");
        let floti = bin.join(format!("floti{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(env!("CARGO_BIN_EXE_floti"), &floti).expect("copy the built floti");
        let apps = dirs.folder("apps dir");
        Native.fake_install(&apps, OLDER);
        let record = dirs.root().join("installer runs.jsonl");

        let server = Server::start();
        let signer = Signer::new();
        let target = format!("{}-{}", os_name(), cpu_name());
        let folder = format!("/download/v{NEWER}/");
        let floti_file = format!("floti-{NEWER}-{target}{}", std::env::consts::EXE_SUFFIX);
        let new_floti = format!("floti {NEWER} for {target}").into_bytes();
        let (desktop_file, desktop) = Native.fake_release(
            &dirs.folder("release work"),
            NEWER,
            Path::new(env!("CARGO_BIN_EXE_floti-fake-program")),
        );
        let manifest = serde_json::json!({
            "schema": 1,
            "channels": {"stable": {
                "version": NEWER,
                "url": server.url(&folder),
                "floti": {&target: {"file": floti_file, "sha256": hex_digest(&new_floti)}},
                "desktop": {&target: {"file": desktop_file, "sha256": hex_digest(&desktop)}},
            }}
        })
        .to_string()
        .into_bytes();

        let world = Self {
            server,
            signer,
            floti,
            apps,
            record,
            new_floti,
            floti_file,
            desktop_file,
            dirs,
        };
        world.serve_signed("/update.json", &manifest, "floti update.json");
        world.serve_signed(
            &format!("{folder}{}", world.floti_file),
            &world.new_floti,
            &format!("floti {NEWER} {}", world.floti_file),
        );
        world.serve_signed(
            &format!("{folder}{}", world.desktop_file),
            &desktop,
            &format!("floti {NEWER} {}", world.desktop_file),
        );
        let marker = serde_json::json!({
            "manifest": world.server.url("/update.json"),
            "key": world.signer.public_text(),
        });
        std::fs::write(world.dirs.data.join(TEST_UPDATE_MARKER), marker.to_string())
            .expect("write the test's update marker");
        world
    }

    fn serve_signed(&self, path: &str, bytes: &[u8], comment: &str) {
        self.server.serve(
            &format!("{path}.minisig"),
            Served::Body(self.signer.sign(bytes, comment)),
        );
        self.server.serve(path, Served::Body(bytes.to_vec()));
    }

    fn file_path(&self, file: &str) -> String {
        format!("/download/v{NEWER}/{file}")
    }

    async fn update(&self) -> std::process::Output {
        let mut command = self.dirs.floti_command(&self.floti);
        command
            .arg("update")
            .env(DESKTOP_DIR_VAR, &self.apps)
            .env(RECORD_VAR, &self.record);
        run_floti(command).await
    }

    fn floti_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.floti).expect("read the installed floti")
    }

    fn desktop_updated(&self) -> bool {
        Native.was_updated(&self.apps, NEWER, &self.record)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn floti_update_replaces_floti_then_the_desktop_app_from_a_signed_release() {
    let world = World::new();
    let output = world.update().await;
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert_eq!(world.floti_bytes(), world.new_floti, "{stdout}");
    assert!(world.desktop_updated(), "{stdout}\n{stderr}");
    assert!(stdout.contains(&format!("to {NEWER}.")), "{stdout}");
    let staging = world.dirs.data.join("update");
    assert!(!staging.exists(), "the download is removed once installed");
}

#[tokio::test]
async fn a_bad_signature_stops_floti_update_with_one_line_and_nothing_replaced() {
    let world = World::new();
    let before = world.floti_bytes();
    world.server.serve(
        &format!("{}.minisig", world.file_path(&world.desktop_file)),
        Served::Body(world.signer.sign(
            b"another file",
            &format!("floti {NEWER} {}", world.desktop_file),
        )),
    );
    let output = world.update().await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim_end(),
        format!(
            "floti update: {} does not match its signature. Nothing was updated.",
            world.desktop_file
        )
    );
    assert_eq!(world.floti_bytes(), before);
    assert!(!world.desktop_updated());
}

#[tokio::test]
async fn a_cut_off_download_stops_floti_update_and_leaves_floti_as_it_was() {
    let world = World::new();
    let before = world.floti_bytes();
    world.server.serve(
        &world.file_path(&world.floti_file),
        Served::CutAfter(world.new_floti.clone(), 4),
    );
    let output = world.update().await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with(&format!(
            "floti update: the download of {} stopped before its end",
            world.floti_file
        )),
        "{stderr}"
    );
    assert_eq!(world.floti_bytes(), before);
    let mut leftovers = Vec::new();
    for entry in std::fs::read_dir(world.floti.parent().unwrap()).unwrap() {
        leftovers.push(entry.unwrap().file_name());
    }
    assert_eq!(leftovers.len(), 1, "nothing beside floti: {leftovers:?}");
}

// ---------------------------------------------------------------------------
// The host's own update: a `floti host` run from floti installed on its own
// (a copy of the built floti under the test's home and local app data)
// finds a newer release of that same build on the loopback server once its
// `updateCheck` setting is turned on, puts it in place, and relaunches on it
// through its session keeper.
// ---------------------------------------------------------------------------

/// Patience for what waits on the host downloading and checking a whole
/// floti build, which an unoptimised test build hashes slowly.
const UPDATE_PATIENCE: Duration = Duration::from_secs(PATIENCE.as_secs() * 3);

/// A host running floti installed on its own, a release of the same build
/// on the loopback server, and the fake agent for its sessions.
struct HostWorld {
    server: Server,
    signer: Signer,
    standalone: PathBuf,
    local_app_data: PathBuf,
    endpoint: floti_host::platform::socket::Endpoint,
    agent: FakeAgent,
    host: Option<Child>,
    dirs: TestDirs,
}

impl HostWorld {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let local_app_data = dirs.folder("local app data");
        let mut env = dirs.user_env();
        env.vars.insert(
            "LOCALAPPDATA".into(),
            local_app_data.clone().into_os_string(),
        );
        let standalone = standalone::Native
            .binary(&env)
            .expect("a place for floti installed on its own");
        std::fs::create_dir_all(standalone.parent().unwrap()).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_floti"), &standalone).expect("install the built floti");
        let agent = fake_agent!(&dirs);
        let endpoint = dirs.floti_endpoint();
        let world = Self {
            server: Server::start(),
            signer: Signer::new(),
            standalone,
            local_app_data,
            endpoint,
            agent,
            host: None,
            dirs,
        };
        world.publish();
        world.turn_update_check(false);
        world
    }

    /// The same build as a newer release on the server, signed with the
    /// test's key, which the host's data folder names with the server.
    fn publish(&self) {
        let target = format!("{}-{}", os_name(), cpu_name());
        let folder = format!("/download/v{NEWER}/");
        let file = format!("floti-{NEWER}-{target}{}", std::env::consts::EXE_SUFFIX);
        let floti = std::fs::read(env!("CARGO_BIN_EXE_floti")).expect("read the built floti");
        let manifest = serde_json::json!({
            "schema": 1,
            "channels": {"stable": {
                "version": NEWER,
                "url": self.server.url(&folder),
                "floti": {&target: {"file": file, "sha256": hex_digest(&floti)}},
            }}
        })
        .to_string()
        .into_bytes();
        self.serve_signed("/update.json", &manifest, "floti update.json");
        self.serve_signed(
            &format!("{folder}{file}"),
            &floti,
            &format!("floti {NEWER} {file}"),
        );
        let marker = serde_json::json!({
            "manifest": self.server.url("/update.json"),
            "key": self.signer.public_text(),
        });
        std::fs::write(self.dirs.data.join(TEST_UPDATE_MARKER), marker.to_string())
            .expect("write the test's update marker");
    }

    fn serve_signed(&self, path: &str, bytes: &[u8], comment: &str) {
        self.server.serve(
            &format!("{path}.minisig"),
            Served::Body(self.signer.sign(bytes, comment)),
        );
        self.server.serve(path, Served::Body(bytes.to_vec()));
    }

    /// Writes the setting into the host's store before any host runs.
    fn turn_update_check(&self, on: bool) {
        let mut store = Store::open(&self.dirs.data).expect("open the store");
        settings::write(
            store.connection_mut(),
            &HostSettingsPatch {
                update_check: Some(on),
                ..HostSettingsPatch::default()
            },
        )
        .expect("write the setting");
    }

    /// `floti host --desktop`, run from floti installed on its own: a host
    /// that exits by itself once idle, so one a failed test leaves running
    /// ends once its sessions have.
    fn start_host(&mut self) {
        record_test_host(&self.endpoint, &self.dirs.data);
        let app = self.dirs.folder("Helm app");
        let (log_var, log_dir) = self.agent.log_env();
        let mut command = self.dirs.floti_command(&self.standalone);
        command
            .arg("host")
            .arg("--desktop")
            .arg(app.join("Helm.exe"))
            .arg("--desktop-arg")
            .arg(app.join("resources app"))
            .env("LOCALAPPDATA", &self.local_app_data)
            .env(log_var, log_dir);
        self.host = Some(command.spawn().expect("run floti host"));
    }

    async fn client(&self) -> TestClient {
        TestClient::connect(&self.endpoint, TERMINAL).await
    }

    /// Starts a session whose shell runs the fake agent in `folder`: its
    /// id, and the fake agent's pid.
    async fn session(&self, client: &mut TestClient, folder: &Path) -> (u64, u32) {
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
        client.read_stream_until(1, |bytes| !bytes.is_empty()).await;
        let line = format!("\"{}\" --fake-tick=100\r", self.agent.exe().display());
        client.type_into(1, line.as_bytes()).await;
        let name = folder.file_name().unwrap().to_owned();
        let mut pid = None;
        until("the fake agent ran", PATIENCE, || {
            pid = self
                .agent
                .records()
                .into_iter()
                .find(|record| Path::new(&record.cwd).file_name() == Some(name.as_os_str()))
                .map(|record| record.pid);
            pid.is_some()
        })
        .await;
        (session.id, pid.unwrap())
    }

    /// The session keeper the first host started, as its log names it.
    fn keeper_pid(&self) -> u32 {
        let log = self.dirs.host_log();
        log.lines()
            .find_map(|line| line.strip_prefix("floti host: started a session keeper, process "))
            .and_then(|rest| rest.trim_end_matches('.').parse().ok())
            .unwrap_or_else(|| panic!("the host started a session keeper:\n{log}"))
    }

    /// Turns the setting on through the host, as Settings does, and waits
    /// until the host has stopped to start the new floti in its place.
    async fn update_now(&self, client: &mut TestClient) {
        client
            .request::<WriteSettings>(&HostSettingsPatch {
                update_check: Some(true),
                ..HostSettingsPatch::default()
            })
            .await
            .expect("the setting is written");
        until(
            "the host to start the new floti in its place",
            UPDATE_PATIENCE,
            || {
                // Written by the old host once it has let go of everything,
                // just before the new floti starts.
                self.dirs.host_log().contains("floti host: starting ")
            },
        )
        .await;
    }

    /// Stops the host over `client`, which keeps it attached until then: a
    /// host a desktop started stops by itself once nothing runs and no
    /// client is attached.
    async fn stop(&mut self, mut client: TestClient) {
        let _ = client
            .request::<HostStop>(&HostStopRequest {
                reason: HostStopReason::Stop,
            })
            .await;
        drop(client);
        if let Some(mut child) = self.host.take() {
            // On macOS and Linux the relaunched host is this process still.
            let deadline = Instant::now() + PATIENCE;
            while child.try_wait().expect("poll the host").is_none() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for HostWorld {
    fn drop(&mut self) {
        if let Some(mut child) = self.host.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

const TERMINAL: &[&str] = &["terminal"];

/// Waits until `done` holds, failing after `patience` naming `what`.
async fn until(what: &str, patience: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + patience;
    while !done() {
        assert!(Instant::now() < deadline, "{what} within {patience:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn alive(pid: u32) -> bool {
    NativeTree.is_alive(pid) != Knowable::Known(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_host_updates_itself_in_the_background_and_its_sessions_run_on_through_the_relaunch() {
    let mut world = HostWorld::new();
    world.start_host();
    let mut client = world.client().await;
    let folder = world.dirs.folder("a project");
    let (session, pid) = world.session(&mut client, &folder).await;
    let keeper = world.keeper_pid();

    world.update_now(&mut client).await;
    drop(client);
    assert!(alive(pid) && alive(keeper), "the relaunch ended nothing");

    let mut client = world.client().await;
    let listed = client.request::<SessionList>(&()).await.unwrap();
    assert_eq!(listed.iter().map(|s| s.id).collect::<Vec<_>>(), [session]);
    assert_eq!(listed[0].status, SessionStatus::Running);
    assert_eq!(
        client.request::<SessionRestorable>(&()).await.unwrap(),
        None,
        "nothing is offered back: nothing ended"
    );
    // The session is live in the new host.
    client.attach(2, session).await;
    client.type_into(2, b"after\r").await;
    client
        .read_stream_until(2, |bytes| {
            String::from_utf8_lossy(bytes).contains("echo: after")
        })
        .await;
    let log = world.dirs.host_log();
    assert!(log.contains(&format!("to {NEWER}.")), "{log}");
    assert!(log.contains("took back the session keeper"), "{log}");
    assert!(
        background::last_checked(&world.dirs.data).is_some(),
        "the new host does not look again within the day"
    );

    // A stop after the relaunch ends the keeper and its session at once:
    // on macOS and Linux the relaunched host is the same process, which
    // must not be left holding an ended keeper it never reaps.
    world.stop(client).await;
    until(
        "the keeper and its session ended with the stop",
        PATIENCE,
        || !alive(pid) && !alive(keeper),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_background_update_whose_floti_cannot_take_the_keeper_back_its_sessions_are_offered_back_once()
 {
    let mut world = HostWorld::new();
    let marker = world.dirs.data.join(TEST_KEEPER_LINK_VERSION);
    std::fs::write(&marker, (KEEPER_LINK_VERSION + 1).to_string()).unwrap();
    world.start_host();
    let mut client = world.client().await;
    let folder = world.dirs.folder("a project");
    let (session, pid) = world.session(&mut client, &folder).await;
    let keeper = world.keeper_pid();
    // The new floti speaks this build's link version; the keeper the old
    // host started speaks the one it was told.
    std::fs::remove_file(&marker).unwrap();

    world.update_now(&mut client).await;
    drop(client);

    let deadline = Instant::now() + UPDATE_PATIENCE;
    while !world
        .dirs
        .host_log()
        .contains("it is ended with what it holds")
    {
        assert!(
            Instant::now() < deadline,
            "the new host ended the keeper within {UPDATE_PATIENCE:?}:
{}",
            world.dirs.host_log()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut client = world.client().await;
    until("the keeper and its session ended", PATIENCE, || {
        !alive(pid) && !alive(keeper)
    })
    .await;
    let offer = client
        .request::<SessionRestorable>(&())
        .await
        .unwrap()
        .expect("its session is offered back");
    assert_eq!(
        offer.sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
        [session]
    );
    world.stop(client).await;
}
