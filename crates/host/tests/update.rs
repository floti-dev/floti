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

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use floti_host::platform::installs::{DESKTOP_DIR_VAR, FakeDesktop, Native};
use floti_host::sha256::hex_digest;
use floti_host::testkit::{TestDirs, run_floti};
use floti_host::update::signature::TEST_UPDATE_MARKER;
use floti_host::version::{cpu_name, os_name};

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
