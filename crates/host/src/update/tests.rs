//! `floti update`'s flow with the network, the installs and the process
//! table faked: each check that stops it, and what it replaces when none
//! does.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use url::Url;

use super::network::{FetchError, Network};
use super::{APP_POLL, Outcome, STAGING_FOLDER, UpdateError, Updater, channel};
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
    desktop_installs: Mutex<Vec<Vec<u8>>>,
    binary_replaced_first: Mutex<bool>,
    running_binary: PathBuf,
}

impl Installs for FakeInstalls {
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()> {
        std::fs::rename(new, running).map_err(|err| crate::Error::path("replace", running, err))
    }

    fn desktop(&self, _env: &UserEnv) -> Option<Desktop> {
        self.desktop.clone()
    }

    fn is_desktop_process(&self, _desktop: &Desktop, row: &ProcessRow) -> bool {
        row.name == "Helm.exe"
    }

    fn replace_desktop(&self, _desktop: &Desktop, release: &Path) -> Result<()> {
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
        let mut out = Vec::new();
        let result = Updater {
            running_version: RUNNING,
            target: TARGET.into(),
            manifest_url: Url::parse(MANIFEST).unwrap(),
            key: &key,
            network: &self.network,
            installs,
            processes: &processes,
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
        let run = world.update(&world.installs(true), 0, None);
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
        format!(
            "{} does not match its signature, so nothing was updated.",
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
            "{0} has no signature ({0}.minisig is not there), so nothing was updated.",
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
        "floti 2.1.0 on stable has no build for linux-x64, so nothing was updated."
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
