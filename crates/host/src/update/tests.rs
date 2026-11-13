//! `floti update`'s flow with the network, the installs and the process
//! table faked: each check that stops it, and what it replaces when none
//! does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use floti_protocol::Knowable;
use url::Url;

use super::fakes::{FakeNetwork, Served, Signer};
use super::{
    APP_POLL, FINISHED_LINE, FromDesktop, HostRelaunch, Options, Outcome, STAGING_FOLDER,
    UpdateError, Updater, channel, failure_line,
};
use crate::Result;
use crate::platform::folders::UserEnv;
use crate::platform::installs::{Desktop, Installs};
use crate::platform::process_table::{PortRow, ProcessRow, ProcessTable};
use crate::platform::standalone::{self, Standalone};
use crate::sha256::hex_digest;
use crate::testkit::TestDirs;
use crate::update::run_file::{self, Look, RunLock, RunRecord};

const RUNNING: &str = "2.0.0";
const TARGET: &str = "linux-x64";
const MANIFEST: &str = "https://floti.test/update.json";
const APP: &str = "Floti";

/// Installs that rename as the real ones do and record the desktop's.
struct FakeInstalls {
    desktop: Option<Desktop>,
    /// The installed app's version, when it can be told.
    desktop_version: Option<String>,
    /// Whether the running floti is the app's own.
    bundled: bool,
    standalone: Option<PathBuf>,
    /// The floti the installed app carries, for install-self.
    carried: Option<PathBuf>,
    desktop_installs: Mutex<Vec<Vec<u8>>>,
    binary_replaced_first: Mutex<bool>,
    running_binary: PathBuf,
    /// Whether installing the app fails.
    install_fails: bool,
    /// The variables the running app's process has, as `display_env` reads them.
    display: Vec<(String, String)>,
    /// Where installing puts the app, when that is not where it was: an app
    /// from before the rename, moved to its new place.
    installed_as: Option<Desktop>,
    /// Whether removing the app it replaced fails.
    removal_fails: bool,
}

impl FakeInstalls {
    fn at_version(mut self, version: &str) -> Self {
        self.desktop_version = Some(version.into());
        self
    }

    fn bundling_floti(mut self, standalone: Option<PathBuf>) -> Self {
        self.bundled = true;
        self.standalone = standalone;
        self
    }

    fn carrying_floti(mut self, carried: PathBuf) -> Self {
        self.carried = Some(carried);
        self
    }

    fn failing_to_install(mut self) -> Self {
        self.install_fails = true;
        self
    }

    fn moving_to(mut self, installed: Desktop) -> Self {
        self.installed_as = Some(installed);
        self
    }

    fn with_display(mut self, display: &[(&str, &str)]) -> Self {
        self.display = display
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self
    }
}

/// floti installed on its own at `binary`, put in place as on Windows:
/// renamed aside, which works the same on every OS.
struct FakeStandalone {
    binary: Option<PathBuf>,
}

impl Standalone for FakeStandalone {
    fn folder(&self, _env: &UserEnv) -> Option<PathBuf> {
        self.binary
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    fn binary(&self, _env: &UserEnv) -> Option<PathBuf> {
        self.binary.clone()
    }

    fn replace(&self, target: &Path, new: &Path) -> io::Result<()> {
        standalone::windows::Native.replace(target, new)
    }

    fn remove_asides(&self, target: &Path) -> Vec<PathBuf> {
        standalone::windows::Native.remove_asides(target)
    }

    fn native_cpu(&self) -> &'static str {
        "x64"
    }

    fn own_folders(&self, _env: &UserEnv) -> Vec<PathBuf> {
        Vec::new()
    }

    fn remove_running(&self, _target: &Path) -> io::Result<Option<PathBuf>> {
        unreachable!("an update removes no floti")
    }

    fn delete_after_exit(
        &self,
        _env: &UserEnv,
        _files: &[PathBuf],
        _folders: &[PathBuf],
    ) -> io::Result<crate::platform::process_tree::Outliving> {
        unreachable!("an update leaves nothing to delete after it exits")
    }
}

impl Installs for FakeInstalls {
    fn bundles(&self, _desktop: &Desktop, _running: &Path) -> bool {
        self.bundled
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

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        self.carried.clone()
    }

    fn is_desktop_process(&self, _desktop: &Desktop, row: &ProcessRow) -> bool {
        // The one process the fake table lists for the app.
        row.pid == 2
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (desktop.path.join(format!("{}.exe", desktop.name)), Vec::new())
    }

    fn remove_replaced(&self, _replaced: &Desktop, _installed: &Desktop) -> Result<()> {
        if self.removal_fails {
            return Err(crate::Error::Refused("it is in use".into()));
        }
        Ok(())
    }

    fn display_env(&self, pid: u32) -> Vec<(String, String)> {
        assert_eq!(pid, 2, "read from the app's own process");
        self.display.clone()
    }

    fn install_desktop(&self, desktop: &Desktop, release: &Path) -> Result<Desktop> {
        if self.install_fails {
            return Err(crate::Error::Refused(
                "The Floti installer Floti-2.1.0-setup.exe ended with exit code: 2.".into(),
            ));
        }
        *self.binary_replaced_first.lock().unwrap() =
            std::fs::read(&self.running_binary).unwrap() != b"old floti";
        self.desktop_installs
            .lock()
            .unwrap()
            .push(std::fs::read(release).unwrap());
        Ok(self.installed_as.clone().unwrap_or_else(|| desktop.clone()))
    }
}

/// A process table on which the app runs for the first `looks` reads. Each
/// read records what the run file in `data` says then.
struct FakeProcesses {
    looks: AtomicUsize,
    data: PathBuf,
    seen: Mutex<Vec<Look>>,
}

impl ProcessTable for FakeProcesses {
    fn processes(&self) -> Result<Vec<ProcessRow>> {
        self.seen.lock().unwrap().push(run_file::look(&self.data));
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
                name: "Floti.exe".into(),
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
        self.desktop = Some(format!("Floti installer {}", self.version).into_bytes());
        self
    }

    fn folder(&self) -> String {
        format!("https://floti.test/download/v{}/", self.version)
    }

    fn floti_file(&self) -> String {
        format!("floti-{}-{TARGET}", self.version)
    }

    fn desktop_file(&self) -> String {
        format!("Floti-{}-setup.exe", self.version)
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
    /// What asking this computer's host to relaunch answers, and how often
    /// it was asked.
    relaunch: RefCell<HostRelaunch>,
    relaunches: Cell<usize>,
    /// Whether the update is the host's own: floti alone, and no host to ask.
    host_s_own: Cell<bool>,
    /// Whether each process id is alive; any other is unknown.
    alive: RefCell<HashMap<u32, Knowable<bool>>>,
    /// Whether the desktop's Update started the update.
    from_desktop: Cell<bool>,
    /// What the update did to the host and the app, in order: `relaunch`,
    /// and `open <path>` for each time it opened the app.
    done: RefCell<Vec<String>>,
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
            relaunch: RefCell::new(HostRelaunch::Relaunched),
            relaunches: Cell::new(0),
            host_s_own: Cell::new(false),
            alive: RefCell::default(),
            from_desktop: Cell::new(false),
            done: RefCell::default(),
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
            bundled: false,
            standalone: None,
            carried: None,
            desktop_installs: Mutex::new(Vec::new()),
            binary_replaced_first: Mutex::new(false),
            running_binary: self.running_binary.clone(),
            install_fails: false,
            display: Vec::new(),
            installed_as: None,
            removal_fails: false,
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
            data: self.dirs.data.clone(),
            seen: Mutex::new(Vec::new()),
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
        let relaunch = || {
            self.relaunches.set(self.relaunches.get() + 1);
            self.done.borrow_mut().push("relaunch".into());
            self.relaunch.borrow().clone()
        };
        let open = |app: &Desktop, display: &[(String, String)]| {
            let (program, _) = installs.opener(app);
            let display: Vec<String> = display
                .iter()
                .map(|(name, value)| format!(" {name}={value}"))
                .collect();
            self.done
                .borrow_mut()
                .push(format!("open {}{}", program.display(), display.concat()));
            Ok(())
        };
        let standalone = FakeStandalone {
            binary: installs.standalone.clone(),
        };
        let installed_from = RefCell::new(Vec::new());
        let install_self = |floti: &Path| {
            installed_from.borrow_mut().push(floti.to_path_buf());
            Ok(format!(
                "floti is at {}.\nFloti host is running.\n",
                floti.display()
            ))
        };
        let mut out = Vec::new();
        let result = Updater {
            running_version: self.running_version.get(),
            target: TARGET.into(),
            manifest_url: Url::parse(MANIFEST).unwrap(),
            key: &key,
            network: &self.network,
            installs,
            standalone: &standalone,
            install_self: &install_self,
            processes: &processes,
            is_alive: &is_alive,
            relaunch_host: (!self.host_s_own.get())
                .then_some(&relaunch as &dyn Fn() -> HostRelaunch),
            with_desktop: !self.host_s_own.get(),
            pause: &pause,
            env: UserEnv::default(),
            running_binary: self.running_binary.clone(),
            data: self.dirs.data.clone(),
            chosen_channel: channel.map(str::to_owned),
            from_desktop: self
                .from_desktop
                .get()
                .then_some(FromDesktop { open: &open }),
            out: &mut out,
            held: None,
        }
        .run();
        Run {
            result,
            out: String::from_utf8(out).unwrap(),
            pauses: pauses.into_inner().unwrap(),
            installed_from: installed_from.into_inner(),
            seen: processes.seen.into_inner().unwrap(),
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
    /// The floti each `install-self` ran from.
    installed_from: Vec<PathBuf>,
    /// What the run file said at each look for the app.
    seen: Vec<Look>,
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
            .matches("Waiting for Floti to quit; it is updated once it has.")
            .count(),
        1,
        "{}",
        run.out
    );
    assert!(run.out.contains("Floti is updated to 2.1.0."), "{}", run.out);
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
            .contains("floti 2.1.0 has no Floti for linux-x64, so Floti stays as it is."),
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
        "floti 2.1.0 Floti-2.1.0-setup.exe",
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
fn update_takes_nothing_a_channel_or_from_desktop() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let options = |channel: Option<&str>, from_desktop: bool| Options {
        channel: channel.map(str::to_owned),
        from_desktop,
        floti_only: false,
    };
    assert_eq!(super::parse(&args(&[])), Some(options(None, false)));
    assert_eq!(
        super::parse(&args(&["--channel", "beta"])),
        Some(options(Some("beta"), false))
    );
    assert_eq!(
        super::parse(&args(&["--from-desktop"])),
        Some(options(None, true))
    );
    assert_eq!(
        super::parse(&args(&["--from-desktop", "--channel", "beta"])),
        Some(options(Some("beta"), true))
    );
    assert_eq!(
        super::parse(&args(&["--channel", "beta", "--from-desktop"])),
        Some(options(Some("beta"), true))
    );
    assert_eq!(super::parse(&args(&["--channel"])), None);
    assert_eq!(super::parse(&args(&["--channel", "../x"])), None);
    assert_eq!(
        super::parse(&args(&["--channel", "a", "--channel", "b"])),
        None
    );
    assert_eq!(
        super::parse(&args(&["--from-desktop", "--from-desktop"])),
        None
    );
    assert_eq!(super::parse(&args(&["--now"])), None);
}

#[test]
fn update_takes_floti_only_once_and_never_with_from_desktop() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let floti_only = |channel: Option<&str>| Options {
        channel: channel.map(str::to_owned),
        from_desktop: false,
        floti_only: true,
    };
    assert_eq!(
        super::parse(&args(&["--floti-only"])),
        Some(floti_only(None))
    );
    assert_eq!(
        super::parse(&args(&["--floti-only", "--channel", "beta"])),
        Some(floti_only(Some("beta")))
    );
    assert_eq!(super::parse(&args(&["--floti-only", "--floti-only"])), None);
    assert_eq!(
        super::parse(&args(&["--floti-only", "--from-desktop"])),
        None
    );
    assert_eq!(
        super::parse(&args(&["--from-desktop", "--floti-only"])),
        None
    );
}

#[test]
fn floti_only_says_up_to_date_by_its_own_status_and_every_other_ending_as_before() {
    let up_to_date = || {
        Ok(Outcome::UpToDate {
            channel: "stable".to_owned(),
            newest: "2.1.0".to_owned(),
        })
    };
    let updated = || {
        Ok(Outcome::Updated {
            version: "2.2.0".to_owned(),
            floti: true,
            desktop: None,
        })
    };
    assert_eq!(
        super::exit_status(up_to_date(), true),
        super::UP_TO_DATE_EXIT
    );
    assert_eq!(super::exit_status(up_to_date(), false), 0);
    assert_eq!(super::exit_status(updated(), true), 0);
    assert_eq!(super::exit_status(updated(), false), 0);
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
            "This floti is part of Floti ({app}), so it is updated with Floti, never on its own. floti installed on its own is {standalone}; `{standalone} update` updates it."
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
fn installing_the_app_asks_nothing_and_ends_no_session() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 0, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: Some(APP.into())
        }
    );
    assert!(!run.out.contains('?'), "nothing is asked: {}", run.out);
    assert_eq!(installs.desktop_installs.lock().unwrap().len(), 1);
}

#[test]
fn once_floti_is_replaced_this_computer_s_host_is_asked_to_start_it_in_its_place() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 0, None);
    assert_eq!(world.relaunches.get(), 1);
    let replaced = run
        .out
        .find("floti is updated from 2.0.0 to 2.1.0.")
        .unwrap();
    let relaunched = run
        .out
        .find(
            "This computer's host is starting floti 2.1.0 in its place; its sessions keep running.",
        )
        .unwrap_or_else(|| panic!("{}", run.out));
    let app = run.out.find("Floti is updated to 2.1.0.").unwrap();
    assert!(replaced < relaunched && relaunched < app, "{}", run.out);
}

#[test]
fn with_no_host_running_nothing_is_said_and_a_host_that_cannot_be_asked_is_named() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    *world.relaunch.borrow_mut() = HostRelaunch::NoHost;
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok());
    assert!(!run.out.contains("host"), "{}", run.out);

    let world = World::new();
    world.publish(&[("stable", &release)]);
    *world.relaunch.borrow_mut() = HostRelaunch::Failed("the host closed the connection.".into());
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok(), "floti is updated all the same");
    assert!(
        run.out.contains(
            "This computer's host was not restarted (the host closed the connection.); it runs floti 2.0.0 until it next starts."
        ),
        "{}",
        run.out
    );
    assert_eq!(world.binary(), release.floti);
}

#[test]
fn the_host_is_not_asked_when_floti_was_not_replaced() {
    let world = World::new();
    world.running_version.set("2.1.0");
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true).at_version("2.0.0");
    let run = world.update(&installs, 0, None);
    assert!(matches!(
        run.result,
        Ok(Outcome::Updated {
            floti: false,
            desktop: Some(_),
            ..
        })
    ));
    assert_eq!(world.relaunches.get(), 0);
}

#[test]
fn the_host_s_own_update_replaces_floti_alone_and_asks_no_host() {
    let world = World::new();
    world.host_s_own.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
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
    assert_eq!(world.relaunches.get(), 0);
    assert!(installs.desktop_installs.lock().unwrap().is_empty());
    assert!(
        !world.network.asked().contains(&release.desktop_url()),
        "the app is not downloaded"
    );
}

#[test]
fn after_the_app_is_updated_its_own_floti_puts_itself_and_its_node_in_place() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let carried = world.dirs.folder("new app resources").join("floti");
    std::fs::write(&carried, b"the new app's floti").unwrap();
    let installs = world
        .installs(true)
        .at_version("2.0.0")
        .carrying_floti(carried.clone());
    let run = world.update(&installs, 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(run.installed_from, std::slice::from_ref(&carried));
    let updated = run
        .out
        .find("Floti is updated to 2.1.0.")
        .expect("the app updated");
    let placed = run
        .out
        .find(&format!("floti is at {}.", carried.display()))
        .expect("install-self's lines");
    assert!(updated < placed, "{}", run.out);

    // An app that carries none where it is looked for (the AppImage's is
    // inside its image) runs nothing.
    let world = World::new();
    world.publish(&[("stable", &release)]);
    let missing = world.dirs.root().join("nowhere").join("floti");
    let installs = world
        .installs(true)
        .at_version("2.0.0")
        .carrying_floti(missing);
    let run = world.update(&installs, 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert!(run.installed_from.is_empty());
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
            "floti.old.exe",
            "notes.txt",
        ],
        "downloads only where the process that left them ended, and copies put aside where nothing runs them"
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
        reason: "The Floti installer ended with 2.".into(),
    };
    assert_eq!(
        failure_line(&alone),
        "Floti could not be updated to 2.1.0: The Floti installer ended with 2. Nothing was updated."
    );
    let after_floti = UpdateError::Desktop {
        version: "2.1.0".into(),
        app: APP.into(),
        reason: "The Floti installer ended with 2.".into(),
    };
    assert_eq!(
        failure_line(&after_floti),
        "floti is updated to 2.1.0, but Floti is not: The Floti installer ended with 2."
    );
}

#[test]
fn what_an_earlier_install_left_beside_the_app_is_removed_once_its_process_has_ended() {
    let world = World::new();
    let installs = world.installs(true).at_version(RUNNING);
    let app = installs.desktop.clone().unwrap().path;
    let apps = app.parent().unwrap().to_path_buf();
    // An unpacked app is a folder; a written AppImage is a file.
    let old = apps.join(".Floti.floti-old-21");
    std::fs::create_dir_all(old.join("Contents")).unwrap();
    std::fs::write(old.join("Contents").join("Info.plist"), b"old").unwrap();
    std::fs::write(apps.join(".Floti.floti-update-22"), b"new").unwrap();
    std::fs::create_dir_all(apps.join(".Floti.floti-old-23")).unwrap();
    std::fs::write(apps.join(".Floti.floti-update-24"), b"new").unwrap();
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
        vec![".Floti.floti-old-23", ".Floti.floti-update-24"],
        "only what a process known to have ended left"
    );
}

const WAITING: &str = "Waiting for Floti to quit; it is updated once it has.";

#[test]
fn the_run_file_says_the_update_waits_for_the_app_while_it_does() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let run = world.update(&world.installs(true), 2, None);
    assert!(run.result.is_ok(), "{}", run.out);
    let pid = std::process::id();
    // The first look finds the app running: everything is downloaded and
    // checked and floti replaced, and the wait is not said yet; each look
    // after says it.
    assert_eq!(
        run.seen,
        vec![
            Look::Running(RunRecord {
                pid,
                line: "floti is updated from 2.0.0 to 2.1.0.".into(),
                waiting_for_app: false
            }),
            Look::Running(RunRecord {
                pid,
                line: WAITING.into(),
                waiting_for_app: true
            }),
            Look::Running(RunRecord {
                pid,
                line: WAITING.into(),
                waiting_for_app: true
            }),
        ]
    );
    assert!(run.out.contains(WAITING), "{}", run.out);
}

#[test]
fn an_update_keeps_how_it_ended_and_leaves_no_run_file() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(run_file::look(&world.dirs.data), Look::Absent);
    let ended = run_file::ended(&world.dirs.data).unwrap();
    assert!(ended.ok);
    assert_eq!(ended.line, FINISHED_LINE);
    assert_eq!(ended.pid, std::process::id());

    let world = World::new();
    world.publish(&[("stable", &release)]);
    world.network.forget(&release.floti_url());
    let run = world.update(&world.installs(false), 0, None);
    let err = run.error();
    let ended = run_file::ended(&world.dirs.data).unwrap();
    assert!(!ended.ok);
    assert_eq!(ended.line, failure_line(&err));
    assert_eq!(run_file::look(&world.dirs.data), Look::Absent);
}

#[test]
fn a_second_update_refuses_while_one_runs_and_touches_nothing() {
    let world = World::new();
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    // The first run's installer, downloaded and waiting for the app.
    let staged = world.dirs.data.join(STAGING_FOLDER);
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(
        staged.join(release.desktop_file()),
        b"first run's installer",
    )
    .unwrap();
    let first = RunLock::take(&world.dirs.data, 31_337, "Waiting.", &|_| {
        Knowable::Known(true)
    })
    .unwrap();
    world
        .alive
        .borrow_mut()
        .insert(31_337, Knowable::Known(true));

    let run = world.update(&world.installs(true), 0, None);
    let err = run.error();
    assert!(
        matches!(err, UpdateError::AlreadyRunning { pid: 31_337 }),
        "{err:?}"
    );
    assert_eq!(
        failure_line(&err),
        "another floti update is running (process 31337). Nothing was updated."
    );
    assert_eq!(world.binary(), b"old floti");
    assert_eq!(
        std::fs::read(staged.join(release.desktop_file())).unwrap(),
        b"first run's installer",
        "the first run's download is left alone"
    );
    assert!(world.network.asked().is_empty(), "nothing was downloaded");
    // The first run's file is its own.
    assert!(matches!(
        run_file::look(&world.dirs.data),
        Look::Running(RunRecord { pid: 31_337, .. })
    ));
    drop(first);
}

#[test]
fn a_run_file_left_by_an_update_that_ended_is_taken_over() {
    let world = World::new();
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    std::mem::forget(
        RunLock::take(&world.dirs.data, 31_338, "Downloading.", &|_| {
            Knowable::Known(true)
        })
        .unwrap(),
    );
    world
        .alive
        .borrow_mut()
        .insert(31_338, Knowable::Known(false));
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(world.binary(), release.floti);
}

#[test]
fn from_the_desktop_the_host_relaunches_once_the_app_is_replaced_and_before_it_opens_again() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world.installs(true);
    let run = world.update(&installs, 1, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: Some(APP.into())
        }
    );
    let app = world.dirs.folder("apps").join(APP).join("Floti.exe");
    assert_eq!(
        *world.done.borrow(),
        vec!["relaunch".to_owned(), format!("open {}", app.display())]
    );
    let updated = run.out.find("Floti is updated to 2.1.0.").unwrap();
    let opening = run.out.find("Opening Floti again.").unwrap();
    assert!(updated < opening, "{}", run.out);
}

#[test]
fn an_app_from_before_the_rename_is_updated_into_its_new_place_and_opens_again_there() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let moved = world.dirs.folder("apps").join("Floti").join("desktop");
    let mut installs = world.installs(true).moving_to(Desktop {
        name: "Floti",
        path: moved.clone(),
    });
    installs.desktop = Some(Desktop {
        name: "Helm",
        path: world.dirs.folder("apps").join("Helm"),
    });
    let run = world.update(&installs, 1, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: Some("Floti".into())
        }
    );
    assert!(
        run.out.contains(&format!(
            "Helm is updated to Floti 2.1.0, at {}.",
            moved.display()
        )),
        "{}",
        run.out
    );
    assert_eq!(
        world.done.borrow().last().cloned(),
        Some(format!("open {}", moved.join("Floti.exe").display()))
    );
    assert!(run.out.contains("Opening Floti again."), "{}", run.out);
}

#[test]
fn an_old_app_that_cannot_be_removed_leaves_the_update_done_with_a_warning() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let moved = world.dirs.folder("apps").join("Floti").join("desktop");
    let mut installs = world.installs(true).moving_to(Desktop {
        name: "Floti",
        path: moved.clone(),
    });
    installs.desktop = Some(Desktop {
        name: "Helm",
        path: world.dirs.folder("apps").join("Helm"),
    });
    installs.removal_fails = true;
    let run = world.update(&installs, 1, None);
    assert_eq!(
        run.result.unwrap(),
        Outcome::Updated {
            version: "2.1.0".into(),
            floti: true,
            desktop: Some("Floti".into())
        }
    );
    assert!(
        run.out.contains("Warning: Helm at ") && run.out.contains("was not removed (it is in use)"),
        "{}",
        run.out
    );
    // The new app opens, not the old one.
    assert_eq!(
        world.done.borrow().last().cloned(),
        Some(format!("open {}", moved.join("Floti.exe").display()))
    );
}

#[test]
fn from_the_desktop_the_app_opens_again_with_the_display_variables_it_ran_with() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world
        .installs(true)
        .with_display(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
    let run = world.update(&installs, 1, None);
    assert!(run.result.is_ok(), "{}", run.out);
    let app = world.dirs.folder("apps").join(APP).join("Floti.exe");
    assert_eq!(
        world.done.borrow().last().cloned(),
        Some(format!(
            "open {} WAYLAND_DISPLAY=wayland-0 DISPLAY=:0",
            app.display()
        ))
    );
}

#[test]
fn from_the_desktop_an_app_that_failed_to_install_opens_again_as_it_was() {
    let world = World::new();
    world.from_desktop.set(true);
    world.running_version.set("2.1.0");
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let installs = world
        .installs(true)
        .at_version("2.0.0")
        .failing_to_install();
    let run = world.update(&installs, 1, None);
    assert!(
        matches!(run.result, Err(UpdateError::DesktopAlone { .. })),
        "{}",
        run.out
    );
    let app = world.dirs.folder("apps").join(APP).join("Floti.exe");
    // floti was not replaced, so the host is not asked to relaunch.
    assert_eq!(
        *world.done.borrow(),
        vec![format!("open {}", app.display())]
    );
}

#[test]
fn from_the_desktop_an_app_that_was_not_running_is_not_opened() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0").with_desktop();
    world.publish(&[("stable", &release)]);
    let run = world.update(&world.installs(true), 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(*world.done.borrow(), vec!["relaunch".to_owned()]);
    assert!(!run.out.contains("Waiting"), "{}", run.out);
}

#[test]
fn from_the_desktop_with_no_app_installed_the_host_still_relaunches_on_the_new_floti() {
    let world = World::new();
    world.from_desktop.set(true);
    let release = Published::new("2.1.0");
    world.publish(&[("stable", &release)]);
    let run = world.update(&world.installs(false), 0, None);
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(*world.done.borrow(), vec!["relaunch".to_owned()]);
    assert_eq!(world.binary(), release.floti);
}
