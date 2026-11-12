//! `floti update`'s flow with the network, the installs and the process
//! table faked: each check that stops it, and what it replaces when none
//! does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use floti_protocol::Knowable;
use url::Url;

use super::network::{FetchError, Network};
use super::{APP_POLL, Outcome, STAGING_FOLDER, UpdateError, Updater, channel, failure_line};
use crate::Result;
use crate::platform::folders::UserEnv;
use crate::platform::installs::{Desktop, Installs};
use crate::platform::process_table::{PortRow, ProcessRow, ProcessTable};
use crate::sha256::hex_digest;
use crate::testkit::TestDirs;

const RUNNING: &str = "2.0.0";
const TARGET: &str = "linux-x64";
const MANIFEST: &str = "https://floti.test/update.json";
const APP: &str = "Helm";

/// A minisign key pair made for the test.
struct Signer {
    pair: minisign::KeyPair,
}

impl Signer {
    fn new() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("make a key pair"),
        }
    }

    fn public(&self) -> minisign_verify::PublicKey {
        let text = self.pair.pk.to_box().expect("box the key").into_string();
        minisign_verify::PublicKey::decode(&text).expect("read the key back")
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

/// What the fake network answers for an address.
#[derive(Clone)]
enum Served {
    Body(Vec<u8>),
    /// The first bytes, then the connection drops.
    CutAfter(Vec<u8>, usize),
}

#[derive(Default)]
struct FakeNetwork {
    served: Mutex<HashMap<String, Served>>,
    asked: Mutex<Vec<String>>,
}

impl FakeNetwork {
    fn serve(&self, url: &str, served: Served) {
        self.served.lock().unwrap().insert(url.to_owned(), served);
    }

    fn forget(&self, url: &str) {
        self.served.lock().unwrap().remove(url);
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Network for FakeNetwork {
    fn get(
        &self,
        url: &Url,
        limit: u64,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<(), FetchError> {
        self.asked.lock().unwrap().push(url.to_string());
        let served = self.served.lock().unwrap().get(url.as_str()).cloned();
        match served {
            None => Err(FetchError::NotFound),
            Some(Served::Body(body)) => {
                if body.len() as u64 > limit {
                    return Err(FetchError::TooLarge(limit));
                }
                // In two pieces, as a network hands a body over.
                let (first, second) = body.split_at(body.len() / 2);
                sink(first).map_err(FetchError::Sink)?;
                sink(second).map_err(FetchError::Sink)
            }
            Some(Served::CutAfter(body, kept)) => {
                sink(&body[..kept]).map_err(FetchError::Sink)?;
                Err(FetchError::Cut("the connection closed".into()))
            }
        }
    }
}

/// Installs that rename as the real ones do and record the desktop's.
struct FakeInstalls {
    desktop: Option<Desktop>,
    /// The installed app's version, when it can be told.
    desktop_version: Option<String>,
    /// Whether installing the app stops this computer's host, as on Windows.
    stops_host: bool,
    /// Whether the running floti is the app's own.
    bundled: bool,
    standalone: Option<PathBuf>,
    desktop_installs: Mutex<Vec<Vec<u8>>>,
    binary_replaced_first: Mutex<bool>,
    running_binary: PathBuf,
}

impl FakeInstalls {
    fn at_version(mut self, version: &str) -> Self {
        self.desktop_version = Some(version.into());
        self
    }

    fn stopping_the_host(mut self) -> Self {
        self.stops_host = true;
        self
    }

    fn bundling_floti(mut self, standalone: Option<PathBuf>) -> Self {
        self.bundled = true;
        self.standalone = standalone;
        self
    }
}

impl Installs for FakeInstalls {
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()> {
        std::fs::rename(new, running).map_err(|err| crate::Error::path("replace", running, err))
    }

    fn bundles(&self, _desktop: &Desktop, _running: &Path) -> bool {
        self.bundled
    }

    fn standalone_binary(&self, _env: &UserEnv) -> Option<PathBuf> {
        self.standalone.clone()
    }

    fn desktop(&self, _env: &UserEnv) -> Option<Desktop> {
        self.desktop.clone()
    }

    fn desktop_target(&self, _env: &UserEnv) -> Option<Desktop> {
        self.desktop.clone()
    }

    fn desktop_version(&self, _desktop: &Desktop) -> Option<String> {
        self.desktop_version.clone()
    }

    fn installing_desktop_stops_host(&self) -> bool {
        self.stops_host
    }

    fn is_desktop_process(&self, _desktop: &Desktop, row: &ProcessRow) -> bool {
        row.name == "Helm.exe"
    }

    fn install_desktop(&self, _desktop: &Desktop, release: &Path) -> Result<()> {
        *self.binary_replaced_first.lock().unwrap() =
            std::fs::read(&self.running_binary).unwrap() != b"old floti";
        self.desktop_installs
            .lock()
            .unwrap()
            .push(std::fs::read(release).unwrap());
        Ok(())
    }
}

/// A process table on which the app runs for the first `looks` reads.
struct FakeProcesses {
    looks: AtomicUsize,
}

impl ProcessTable for FakeProcesses {
    fn processes(&self) -> Result<Vec<ProcessRow>> {
        let running = self
            .looks
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        let mut rows = vec![ProcessRow {
            pid: 1,
            parent_pid: 0,
            name: "init".into(),
            command_line: None,
        }];
        if running {
            rows.push(ProcessRow {
                pid: 2,
                parent_pid: 1,
                name: "Helm.exe".into(),
                command_line: None,
            });
        }
        Ok(rows)
    }

    fn listening(&self) -> Result<Vec<PortRow>> {
        Ok(Vec::new())
    }
}

/// A release as the test publishes it.
struct Published {
    version: String,
    floti: Vec<u8>,
    desktop: Option<Vec<u8>>,
}

impl Published {
    fn new(version: &str) -> Self {
        Self {
            version: version.into(),
            floti: format!("new floti {version}").into_bytes(),
            desktop: None,
        }
    }

    fn with_desktop(mut self) -> Self {
        self.desktop = Some(format!("Helm installer {}", self.version).into_bytes());
        self
    }

    fn folder(&self) -> String {
        format!("https://floti.test/download/v{}/", self.version)
    }

    fn floti_file(&self) -> String {
        format!("floti-{}-{TARGET}", self.version)
    }

    fn desktop_file(&self) -> String {
        format!("Helm-{}-setup.exe", self.version)
    }

    fn floti_url(&self) -> String {
        format!("{}{}", self.folder(), self.floti_file())
    }

    fn desktop_url(&self) -> String {
        format!("{}{}", self.folder(), self.desktop_file())
    }
}

struct World {
    signer: Signer,
    network: FakeNetwork,
    running_binary: PathBuf,
    running_version: Cell<&'static str>,
    /// What counting the host's sessions answers.
    sessions: RefCell<std::result::Result<usize, String>>,
    /// What the person answers; `None` is no terminal.
    answer: Cell<Option<bool>>,
    questions: RefCell<Vec<String>>,
    /// Whether each process id is alive; any other is unknown.
    alive: RefCell<HashMap<u32, Knowable<bool>>>,
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let running_binary = dirs.folder("bin").join("floti.exe");
        std::fs::write(&running_binary, b"old floti").unwrap();
        Self {
            signer: Signer::new(),
            network: FakeNetwork::default(),
            running_binary,
            running_version: Cell::new(RUNNING),
            sessions: RefCell::new(Ok(0)),
            answer: Cell::new(None),
            questions: RefCell::default(),
            alive: RefCell::default(),
            dirs,
        }
    }

    fn manifest_for(&self, channels: &[(&str, &Published)]) -> Vec<u8> {
        let channels: serde_json::Map<String, serde_json::Value> = channels
            .iter()
            .map(|(name, release)| {
                let mut entry = serde_json::json!({
                    "version": release.version,
                    "url": release.folder(),
                    "floti": {TARGET: {"file": release.floti_file(), "sha256": hex_digest(&release.floti)}},
                });
                if let Some(desktop) = &release.desktop {
                    entry["desktop"] = serde_json::json!({
                        TARGET: {"file": release.desktop_file(), "sha256": hex_digest(desktop)}
                    });
                }
                ((*name).to_owned(), entry)
            })
            .collect();
        serde_json::json!({"schema": 1, "channels": channels})
            .to_string()
            .into_bytes()
    }

    /// Serves the manifest naming `channels`, and each release's files, all
    /// signed as a release is.
    fn publish(&self, channels: &[(&str, &Published)]) {
        let manifest = self.manifest_for(channels);
        self.network.serve(
            &format!("{MANIFEST}.minisig"),
            Served::Body(self.signer.sign(&manifest, "floti update.json")),
        );
        self.network.serve(MANIFEST, Served::Body(manifest));
        for (_, release) in channels {
            self.serve_signed(
                &release.floti_url(),
                &release.floti,
                &format!("floti {} {}", release.version, release.floti_file()),
            );
            if let Some(desktop) = &release.desktop {
                self.serve_signed(
                    &release.desktop_url(),
                    desktop,
                    &format!("floti {} {}", release.version, release.desktop_file()),
                );
            }
        }
    }

    fn serve_signed(&self, url: &str, bytes: &[u8], comment: &str) {
        self.network.serve(
            &format!("{url}.minisig"),
            Served::Body(self.signer.sign(bytes, comment)),
        );
        self.network.serve(url, Served::Body(bytes.to_vec()));
    }

    fn installs(&self, desktop: bool) -> FakeInstalls {
        FakeInstalls {
            desktop: desktop.then(|| Desktop {
                name: APP,
                path: self.dirs.folder("apps").join(APP),
            }),
            desktop_version: None,
            stops_host: false,
            bundled: false,
            standalone: None,
            desktop_installs: Mutex::new(Vec::new()),
            binary_replaced_first: Mutex::new(false),
            running_binary: self.running_binary.clone(),
        }
    }

    fn update(
        &self,
        installs: &FakeInstalls,
        app_running_for: usize,
        channel: Option<&str>,
    ) -> Run {
        let processes = FakeProcesses {
            looks: AtomicUsize::new(app_running_for),
        };
        let pauses = Mutex::new(Vec::new());
        let pause = |wait: Duration| pauses.lock().unwrap().push(wait);
        let key = self.signer.public();
        let is_alive = |pid: u32| {
            self.alive
                .borrow()
                .get(&pid)
                .cloned()
                .unwrap_or(Knowable::Unknown)
        };
        let sessions_ending = || self.sessions.borrow().clone();
        let confirm = |question: &str| {
            self.questions.borrow_mut().push(question.to_owned());
            self.answer.get()
        };
        let mut out = Vec::new();
        let result = Updater {
            running_version: self.running_version.get(),
            target: TARGET.into(),
            manifest_url: Url::parse(MANIFEST).unwrap(),
            key: &key,
            network: &self.network,
            installs,
            processes: &processes,
            is_alive: &is_alive,
            sessions_ending: &sessions_ending,
            confirm: &confirm,
            pause: &pause,
            env: UserEnv::default(),
            running_binary: self.running_binary.clone(),
            data: self.dirs.data.clone(),
            chosen_channel: channel.map(str::to_owned),
            out: &mut out,
        }
        .run();
        Run {
            result,
            out: String::from_utf8(out).unwrap(),
            pauses: pauses.into_inner().unwrap(),
        }
    }

    fn binary(&self) -> Vec<u8> {
        std::fs::read(&self.running_binary).unwrap()
    }

    /// Everything beside the running binary and in the staging folder:
    /// nothing a failed or finished update should leave.
    fn leftovers(&self) -> Vec<String> {
        let mut left: Vec<String> = std::fs::read_dir(self.running_binary.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "floti.exe")
            .collect();
        if self.dirs.data.join(STAGING_FOLDER).exists() {
            left.push(STAGING_FOLDER.into());
        }
        left
    }
}

struct Run {
    result: Result<Outcome, UpdateError>,
    out: String,
    pauses: Vec<Duration>,
}

impl Run {
    fn error(self) -> UpdateError {
        match self.result {
            Ok(outcome) => panic!("the update went through: {outcome:?}\n{}", self.out),
            Err(err) => err,
        }
    }
}

#[test]
fn a_newer_version_replaces_floti_after_its_checks_pass() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let installs = world.installs(false);
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: None
        }
    );
    assert_eq!(world.binary(), release.floti);
    assert!(
        run.out.contains("floti is updated from 2.0.0 to 2.1.0."),
        "{}",
        run.out
    );
    assert_eq!(world.leftovers(), Vec::<String>::new());
    // Each signature is fetched before the file it signs.
    let asked = world.network.asked();
    assert_eq!(
        asked,
        vec![
            MANIFEST.to_owned(),
            format!("{MANIFEST}.minisig"),
            format!("{}.minisig", release.floti_url()),
            release.floti_url(),
        ]
    );
}

#[test]
fn an_older_or_equal_version_is_never_installed() {
    for version in ["1.9.9", RUNNING, "2.0.0-preview.3"] {
        let world = World::new();
        let release = Published::new(version);
        world.publish(&[("stable", &release)]);
        let run = world.update(&world.installs(true).at_version(version), 0, None);
        assert_eq!(
            run.result.unwrap(),
            Outcome::UpToDate {
                channel: "stable".into(),
                newest: version.into()
            },
            "{version}"
        );
        assert!(
            run.out.contains("floti 2.0.0 is up to date on stable"),
            "{}",
            run.out
        );
        assert_eq!(world.binary(), b"old floti", "{version}");
        assert_eq!(
            world.network.asked().len(),
            2,
            "only the manifest and its signature"
        );
    }
}

#[test]
fn a_bad_signature_stops_the_update_and_leaves_floti_as_it_was() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    // A file other than the one signed, with the manifest's SHA-256 its own.
    let mut tampered = release.floti.clone();
    tampered.push(b'!');
    let signature = world
        .signer
        .sign(&tampered, &format!("floti 2.1.0 {}", release.floti_file()));
    world.network.serve(
        &format!("{}.minisig", release.floti_url()),
        Served::Body(signature),
    );
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(
        matches!(&err, UpdateError::BadSignature { file } if file == &release.floti_file()),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        format!("{} does not match its signature.", release.floti_file())
    );
    assert_eq!(
        failure_line(&err),
        format!(
            "{} does not match its signature. Nothing was updated.",
            release.floti_file()
        )
    );
    assert_eq!(world.binary(), b"old floti");
    assert_eq!(world.leftovers(), Vec::<String>::new());
}

#[test]
fn a_tampered_manifest_is_refused_before_anything_is_downloaded() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let mut manifest = world.manifest_for(&[("stable", &release)]);
    manifest.extend_from_slice(b" ");
    world.network.serve(MANIFEST, Served::Body(manifest));
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(
        matches!(&err, UpdateError::BadSignature { file } if file == "update.json"),
        "{err}"
    );
    assert_eq!(world.network.asked().len(), 2);
    assert_eq!(world.binary(), b"old floti");
}

#[test]
fn a_missing_signature_stops_the_update_and_says_so() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    world
        .network
        .forget(&format!("{}.minisig", release.floti_url()));
    let err = world.update(&world.installs(false), 0, None).error();
    assert_eq!(
        err.to_string(),
        format!(
            "{0} has no signature ({0}.minisig is not there).",
            release.floti_file()
        )
    );
    assert!(
        !world.network.asked().contains(&release.floti_url()),
        "the file is not fetched"
    );
    assert_eq!(world.binary(), b"old floti");

    world.network.forget(&format!("{MANIFEST}.minisig"));
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(
        matches!(&err, UpdateError::MissingSignature { file } if file == "update.json"),
        "{err}"
    );
}

#[test]
fn a_signature_moved_from_another_file_or_version_is_refused_by_its_trusted_comment() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    for comment in [
        "floti 2.0.9 floti-2.1.0-linux-x64".to_owned(),
        "floti 2.1.0 floti-2.1.0-linux-arm64".to_owned(),
        "floti 2.1.0 floti-2.1.0-linux-x64 ".to_owned(),
    ] {
        world.serve_signed(&release.floti_url(), &release.floti, &comment);
        let err = world.update(&world.installs(false), 0, None).error();
        assert!(
            matches!(&err, UpdateError::WrongComment { found, wanted, .. }
                if found == &comment && wanted == "floti 2.1.0 floti-2.1.0-linux-x64"),
            "{comment}: {err}"
        );
        assert_eq!(world.binary(), b"old floti");
    }

    let manifest = world.manifest_for(&[("stable", &release)]);
    world.network.serve(
        &format!("{MANIFEST}.minisig"),
        Served::Body(world.signer.sign(&manifest, "floti 2.1.0 update.json")),
    );
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(
        matches!(&err, UpdateError::WrongComment { file, .. } if file == "update.json"),
        "{err}"
    );
}

#[test]
fn a_signature_by_another_key_is_refused() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let other = Signer::new();
    world.network.serve(
        &format!("{}.minisig", release.floti_url()),
        Served::Body(other.sign(
            &release.floti,
            &format!("floti 2.1.0 {}", release.floti_file()),
        )),
    );
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(matches!(err, UpdateError::WrongKey { .. }), "{err}");
    assert_eq!(world.binary(), b"old floti");
}

#[test]
fn a_file_whose_sha256_differs_from_the_manifest_s_is_refused() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    // Genuinely signed, but not the file the manifest lists.
    let other = b"another floti".to_vec();
    world.serve_signed(
        &release.floti_url(),
        &other,
        &format!("floti 2.1.0 {}", release.floti_file()),
    );
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(matches!(err, UpdateError::WrongSha256 { .. }), "{err}");
    assert_eq!(world.binary(), b"old floti");
    assert_eq!(world.leftovers(), Vec::<String>::new());
}

#[test]
fn a_cut_off_download_stops_the_update_and_leaves_nothing_behind() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    world.network.serve(
        &release.desktop_url(),
        Served::CutAfter(release.desktop.clone().unwrap(), 5),
    );
    let installs = world.installs(true);
    let err = world.update(&installs, 0, None).error();
    assert!(
        matches!(&err, UpdateError::CutOff { file, .. } if file == &release.desktop_file()),
        "{err}"
    );
    // The desktop failed its download, so floti was not replaced either.
    assert_eq!(world.binary(), b"old floti");
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
    assert_eq!(world.leftovers(), Vec::<String>::new());
}

#[test]
fn a_running_app_is_waited_for_and_replaced_once_it_has_quit() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 3, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: Some(APP.into())
        }
    );
    assert_eq!(run.pauses, vec![APP_POLL; 3]);
    assert_eq!(
        run.out
            .matches("Waiting for Helm to quit; it is updated once it has.")
            .count(),
        1,
        "{}",
        run.out
    );
    assert!(run.out.contains("Helm is updated to 2.1.0."), "{}", run.out);
    assert_eq!(
        *installs.desktop_installs.lock().unwrap(),
        vec![release.desktop.clone().unwrap()]
    );
    assert!(
        *installs.binary_replaced_first.lock().unwrap(),
        "floti is replaced first"
    );
    assert_eq!(world.leftovers(), Vec::<String>::new());
}

#[test]
fn an_app_that_is_not_running_is_replaced_without_waiting() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 0, None);
    assert!(run.pauses.is_empty());
    assert!(!run.out.contains("Waiting"), "{}", run.out);
    assert_eq!(installs.desktop_installs.lock().unwrap().len(), 1);
}

#[test]
fn a_release_with_no_desktop_for_this_computer_still_updates_floti() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 0, None);
    assert!(
        run.out
            .contains("floti 2.1.0 has no Helm for linux-x64, so Helm stays as it is."),
        "{}",
        run.out
    );
    assert_eq!(world.binary(), release.floti);
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
}

#[test]
fn a_bad_desktop_signature_replaces_nothing() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    world.serve_signed(
        &release.desktop_url(),
        b"something else",
        "floti 2.1.0 Helm-2.1.0-setup.exe",
    );
    world.network.serve(
        &release.desktop_url(),
        Served::Body(release.desktop.clone().unwrap()),
    );
    let installs = world.installs(true);
    let err = world.update(&installs, 0, None).error();
    assert!(matches!(err, UpdateError::BadSignature { .. }), "{err}");
    assert_eq!(world.binary(), b"old floti");
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
}

#[test]
fn a_chosen_channel_is_followed_and_remembered_and_a_missing_one_is_named() {
    let world = World::new();
    let stable = Published::new("2.0.0");
    let beta = Published::new("2.1.0-beta.1");
    world.publish(&[("stable", &stable), ("beta", &beta)]);

    let err = world
        .update(&world.installs(false), 0, Some("nightly"))
        .error();
    assert_eq!(
        err.to_string(),
        "update.json has no channel \"nightly\"; it has beta, stable."
    );
    assert_eq!(
        channel::remembered(&world.dirs.data),
        "stable",
        "a missing channel is not remembered"
    );

    let run = world.update(&world.installs(false), 0, Some("beta"));
    assert!(
        matches!(run.result, Ok(Outcome::Updated { ref version, .. }) if version == "2.1.0-beta.1")
    );
    assert_eq!(world.binary(), beta.floti);
    assert_eq!(channel::remembered(&world.dirs.data), "beta");

    // A later run with no --channel follows beta.
    let run = world.update(&world.installs(false), 0, None);
    assert!(
        matches!(run.result, Ok(Outcome::Updated { ref version, .. }) if version == "2.1.0-beta.1")
    );
}

#[test]
fn a_release_with_no_floti_for_this_computer_changes_nothing() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let manifest = serde_json::json!({"schema": 1, "channels": {"stable": {
        "version": "2.1.0", "url": release.folder(), "floti": {}
    }}})
    .to_string()
    .into_bytes();
    world.network.serve(
        &format!("{MANIFEST}.minisig"),
        Served::Body(world.signer.sign(&manifest, "floti update.json")),
    );
    world.network.serve(MANIFEST, Served::Body(manifest));
    let err = world.update(&world.installs(false), 0, None).error();
    assert_eq!(
        err.to_string(),
        "floti 2.1.0 on stable has no build for linux-x64."
    );
}

#[test]
fn update_takes_nothing_or_a_channel() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    assert_eq!(super::parse(&args(&[])), Some(None));
    assert_eq!(
        super::parse(&args(&["--channel", "beta"])),
        Some(Some("beta".into()))
    );
    assert_eq!(super::parse(&args(&["--channel"])), None);
    assert_eq!(super::parse(&args(&["--channel", "../x"])), None);
    assert_eq!(super::parse(&args(&["--now"])), None);
}

#[test]
fn a_floti_whose_own_version_cannot_be_read_updates_nothing() {
    let world = World::new();
    world.publish(&[("stable", &Published::new("2.1.0"))]);
    world.running_version.set("dev");
    let err = world.update(&world.installs(false), 0, None).error();
    assert!(
        matches!(&err, UpdateError::RunningVersion { version } if version == "dev"),
        "{err}"
    );
    assert!(world.network.asked().is_empty(), "nothing is fetched");
    assert_eq!(world.binary(), b"old floti");
}

#[test]
fn an_app_behind_an_up_to_date_floti_is_updated_alone() {
    let world = World::new();
    world.running_version.set("2.1.0");
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true).at_version("2.0.0");
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: false,
            desktop: Some(APP.into())
        }
    );
    assert_eq!(
        world.binary(),
        b"old floti",
        "floti is not downloaded again"
    );
    assert!(
        !world.network.asked().contains(&release.floti_url()),
        "{:?}",
        world.network.asked()
    );
    assert_eq!(
        *installs.desktop_installs.lock().unwrap(),
        vec![release.desktop.clone().unwrap()]
    );
    assert_eq!(world.leftovers(), Vec::<String>::new());
}

#[test]
fn an_app_at_the_newest_version_is_left_and_one_of_unknown_version_is_updated() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let current = world.installs(true).at_version("2.1.0");
    let run = world.update(&current, 0, None);
    assert!(
        matches!(
            run.result,
            Ok(Outcome::Updated {
                floti: true,
                desktop: None,
                ..
            })
        ),
        "{}",
        run.out
    );
    assert!(current.desktop_installs.lock().unwrap().is_empty());

    let world = World::new();
    world.running_version.set("2.1.0");
    world.publish(&[("stable", &release)]);
    let unknown = world.installs(true);
    let run = world.update(&unknown, 0, None);
    assert!(
        matches!(
            run.result,
            Ok(Outcome::Updated {
                floti: false,
                desktop: Some(_),
                ..
            })
        ),
        "{}",
        run.out
    );
    assert_eq!(unknown.desktop_installs.lock().unwrap().len(), 1);
}

#[test]
fn a_floti_inside_the_app_is_never_replaced_and_the_app_brings_its_own() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let standalone = world.dirs.folder("standalone bin").join("floti.exe");
    std::fs::write(&standalone, b"standalone floti").unwrap();
    let installs = world
        .installs(true)
        .at_version("2.0.0")
        .bundling_floti(Some(standalone.clone()));
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: false,
            desktop: Some(APP.into())
        }
    );
    assert_eq!(world.binary(), b"old floti");
    let app = installs
        .desktop
        .as_ref()
        .unwrap()
        .path
        .display()
        .to_string();
    let standalone = standalone.display().to_string();
    assert!(
        run.out.contains(&format!(
            "This floti is part of Helm ({app}), so it is updated with Helm, never on its own. floti installed on its own is {standalone}; `{standalone} update` updates it."
        )),
        "{}",
        run.out
    );
    assert_eq!(installs.desktop_installs.lock().unwrap().len(), 1);

    // With no floti installed on its own, only the first sentence.
    let world = World::new();
    world.publish(&[("stable", &release)]);
    let missing = world.dirs.root().join("nowhere").join("floti.exe");
    let installs = world.installs(true).bundling_floti(Some(missing));
    let run = world.update(&installs, 0, None);
    assert!(run.out.contains("never on its own.\n"), "{}", run.out);
}

#[test]
fn installing_an_app_that_stops_the_host_asks_first_saying_how_many_sessions_end() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    *world.sessions.borrow_mut() = Ok(2);
    world.answer.set(Some(true));
    let installs = world.installs(true).stopping_the_host();
    let run = world.update(&installs, 0, None);
    assert!(matches!(
        run.result,
        Ok(Outcome::Updated {
            desktop: Some(_),
            ..
        })
    ));
    assert_eq!(
        *world.questions.borrow(),
        vec![
            "Updating Helm stops this computer's host, ending 2 sessions; they come back when Helm next starts. Update Helm now?"
                .to_owned()
        ]
    );
    assert_eq!(installs.desktop_installs.lock().unwrap().len(), 1);
}

#[test]
fn no_to_the_question_updates_floti_alone_and_leaves_the_app() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    *world.sessions.borrow_mut() = Ok(1);
    world.answer.set(Some(false));
    let installs = world.installs(true).stopping_the_host();
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: None
        }
    );
    assert!(world.questions.borrow()[0].contains("ending 1 session; it comes back"));
    assert!(
        run.out
            .contains("Helm stays as it is; floti update updates it when run again."),
        "{}",
        run.out
    );
    assert_eq!(world.binary(), release.floti);
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
    assert!(
        !world.network.asked().contains(&release.desktop_url()),
        "the app is not downloaded"
    );
}

#[test]
fn with_no_terminal_to_ask_on_the_app_is_left_and_the_line_says_why() {
    let world = World::new();
    world.running_version.set("2.1.0");
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    *world.sessions.borrow_mut() = Ok(3);
    world.answer.set(None);
    let installs = world.installs(true).at_version("2.0.0").stopping_the_host();
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::DesktopLeft {
            version: "2.1.0".into(),
            app: APP.into()
        }
    );
    assert_eq!(
        run.out.trim_end(),
        "Helm stays as it is: updating it ends sessions, and there is no terminal to ask on first. Run floti update in a terminal to update Helm."
    );
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
}

#[test]
fn nothing_is_asked_when_the_host_runs_no_session_or_installing_stops_no_host() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    world.answer.set(Some(false));
    let installs = world.installs(true).stopping_the_host();
    let run = world.update(&installs, 0, None);
    assert!(matches!(
        run.result,
        Ok(Outcome::Updated {
            desktop: Some(_),
            ..
        })
    ));

    let world = World::new();
    world.publish(&[("stable", &release)]);
    *world.sessions.borrow_mut() = Ok(4);
    world.answer.set(Some(false));
    let installs = world.installs(true);
    let run = world.update(&installs, 0, None);
    assert!(matches!(
        run.result,
        Ok(Outcome::Updated {
            desktop: Some(_),
            ..
        })
    ));
    assert!(world.questions.borrow().is_empty());
}

#[test]
fn a_host_that_cannot_be_asked_still_gets_the_question() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    *world.sessions.borrow_mut() = Err("the host closed the connection.".into());
    world.answer.set(Some(false));
    let installs = world.installs(true).stopping_the_host();
    let run = world.update(&installs, 0, None);
    assert!(run.result.is_ok());
    assert!(
        run.out.contains(
            "floti could not ask this computer's host which sessions updating Helm ends: the host closed the connection."
        ),
        "{}",
        run.out
    );
    assert!(world.questions.borrow()[0].contains("ending the sessions it runs"));
}

#[test]
fn what_an_earlier_update_left_is_removed_once_its_process_has_ended() {
    let world = World::new();
    let bin = world.running_binary.parent().unwrap().to_path_buf();
    for name in [
        "floti.old-11.exe",
        "floti.old-12.exe",
        "floti.old-13.exe",
        ".floti-2.1.0.11.exe",
        ".floti-2.1.0-beta.1.12.exe",
        ".floti-not-a-version.11.exe",
        "floti.old.exe",
        "notes.txt",
    ] {
        std::fs::write(bin.join(name), b"left").unwrap();
    }
    world
        .alive
        .borrow_mut()
        .extend([(11, Knowable::Known(false)), (12, Knowable::Known(true))]);
    world.publish(&[("stable", &Published::new("2.0.0"))]);
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok());
    let mut left = world.leftovers();
    left.sort();
    assert_eq!(
        left,
        vec![
            ".floti-2.1.0-beta.1.12.exe",
            ".floti-not-a-version.11.exe",
            "floti.old-12.exe",
            "floti.old-13.exe",
            "floti.old.exe",
            "notes.txt",
        ],
        "only what a process known to have ended left"
    );
}

#[test]
fn a_failure_line_says_nothing_was_updated_unless_floti_was() {
    let refused = UpdateError::Replace {
        reason: "access denied".into(),
    };
    assert_eq!(
        failure_line(&refused),
        "floti could not be replaced: access denied. Nothing was updated."
    );
    let alone = UpdateError::DesktopAlone {
        version: "2.1.0".into(),
        app: APP.into(),
        reason: "The Helm installer ended with 2.".into(),
    };
    assert_eq!(
        failure_line(&alone),
        "Helm could not be updated to 2.1.0: The Helm installer ended with 2. Nothing was updated."
    );
    let after_floti = UpdateError::Desktop {
        version: "2.1.0".into(),
        app: APP.into(),
        reason: "The Helm installer ended with 2.".into(),
    };
    assert_eq!(
        failure_line(&after_floti),
        "floti is updated to 2.1.0, but Helm is not: The Helm installer ended with 2."
    );
}

#[test]
fn what_an_earlier_install_left_beside_the_app_is_removed_once_its_process_has_ended() {
    let world = World::new();
    let installs = world.installs(true).at_version(RUNNING);
    let app = installs.desktop.clone().unwrap().path;
    let apps = app.parent().unwrap().to_path_buf();
    // An unpacked app is a folder; a written AppImage is a file.
    let old = apps.join(".Helm.floti-old-21");
    std::fs::create_dir_all(old.join("Contents")).unwrap();
    std::fs::write(old.join("Contents").join("Info.plist"), b"old").unwrap();
    std::fs::write(apps.join(".Helm.floti-update-22"), b"new").unwrap();
    std::fs::create_dir_all(apps.join(".Helm.floti-old-23")).unwrap();
    std::fs::write(apps.join(".Helm.floti-update-24"), b"new").unwrap();
    world.alive.borrow_mut().extend([
        (21, Knowable::Known(false)),
        (22, Knowable::Known(false)),
        (23, Knowable::Known(true)),
    ]);
    world.publish(&[("stable", &Published::new(RUNNING))]);
    let run = world.update(&installs, 0, None);
    assert!(
        matches!(run.result, Ok(Outcome::UpToDate { .. })),
        "{}",
        run.out
    );
    let mut left: Vec<String> = std::fs::read_dir(&apps)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec![".Helm.floti-old-23", ".Helm.floti-update-24"],
        "only what a process known to have ended left"
    );
}
