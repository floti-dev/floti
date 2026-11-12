//! `floti add desktop`'s flow with the network and the installs faked: what
//! it installs and from where, each check that stops it, and what it says.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use url::Url;

use super::{AddError, Added, Adder, Request, STAGING_FOLDER, failure_line, list, parse};
use crate::Result;
use crate::platform::folders::UserEnv;
use crate::platform::installs::{DISPLAY_TEST_VAR, Desktop, Installs};
use crate::platform::process_table::ProcessRow;
use crate::sha256::hex_digest;
use crate::testkit::TestDirs;
use crate::update::channel;
use crate::update::fakes::{FakeNetwork, Served, Signer};

const RUNNING: &str = "3.0.0";
const TARGET: &str = "linux-x64";
const MANIFEST: &str = "https://floti.test/update.json";

/// Installs that record what they were asked: the app at `target`, installed
/// already when `installed`.
struct FakeInstalls {
    target: Desktop,
    installed: bool,
    version: Option<String>,
    no_display: Option<String>,
    carried: Option<PathBuf>,
    launcher: std::result::Result<Option<PathBuf>, String>,
    install_fails: bool,
    /// The bytes of each file installed, in turn.
    installs: Mutex<Vec<Vec<u8>>>,
    launchers: Mutex<usize>,
}

impl FakeInstalls {
    fn new(apps: &Path) -> Self {
        Self {
            target: Desktop {
                name: "Helm",
                path: apps.join("Helm.AppImage"),
            },
            installed: false,
            version: None,
            no_display: None,
            carried: None,
            launcher: Ok(None),
            install_fails: false,
            installs: Mutex::default(),
            launchers: Mutex::default(),
        }
    }

    fn installed(&self) -> Vec<Vec<u8>> {
        self.installs.lock().unwrap().clone()
    }
}

impl Installs for FakeInstalls {
    fn bundles(&self, _desktop: &Desktop, _running: &Path) -> bool {
        false
    }

    fn desktop(&self, _env: &UserEnv) -> Option<Desktop> {
        self.installed.then(|| self.target.clone())
    }

    fn desktop_target(&self, _env: &UserEnv) -> Option<Desktop> {
        Some(self.target.clone())
    }

    fn desktop_version(&self, _desktop: &Desktop) -> Option<String> {
        self.version.clone()
    }

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        self.carried.clone()
    }

    fn is_desktop_process(&self, _desktop: &Desktop, _row: &ProcessRow) -> bool {
        false
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (desktop.path.clone(), Vec::new())
    }

    fn no_display(&self, _env: &UserEnv) -> Option<String> {
        self.no_display.clone()
    }

    fn add_launcher(&self, _env: &UserEnv, _desktop: &Desktop) -> Result<Option<PathBuf>> {
        *self.launchers.lock().unwrap() += 1;
        self.launcher.clone().map_err(crate::Error::Refused)
    }

    fn install_desktop(&self, _desktop: &Desktop, release: &Path) -> Result<()> {
        if self.install_fails {
            return Err(crate::Error::Refused(
                "The Helm installer ended with exit code: 2.".into(),
            ));
        }
        self.installs
            .lock()
            .unwrap()
            .push(std::fs::read(release).unwrap());
        Ok(())
    }
}

/// A release published at `version`, with a desktop file for [`TARGET`].
struct Published {
    version: String,
    desktop: Vec<u8>,
}

impl Published {
    fn new(version: &str) -> Self {
        Self {
            version: version.into(),
            desktop: format!("Helm AppImage {version}").into_bytes(),
        }
    }

    fn folder(&self) -> String {
        format!("https://floti.test/download/v{}/", self.version)
    }

    fn file(&self) -> String {
        format!("Helm-{}-linux-x64.AppImage", self.version)
    }

    fn url(&self) -> String {
        format!("{}{}", self.folder(), self.file())
    }
}

struct World {
    signer: Signer,
    network: FakeNetwork,
    installs: FakeInstalls,
    install_selfs: RefCell<Vec<PathBuf>>,
    install_self_fails: bool,
    env: UserEnv,
    dirs: TestDirs,
}

struct Run {
    result: std::result::Result<Added, AddError>,
    out: String,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let installs = FakeInstalls::new(&dirs.folder("apps"));
        Self {
            signer: Signer::new(),
            network: FakeNetwork::default(),
            installs,
            install_selfs: RefCell::default(),
            install_self_fails: false,
            env: UserEnv {
                home: Some(dirs.home.clone()),
                vars: Default::default(),
            },
            dirs,
        }
    }

    fn serve_signed(&self, url: &str, bytes: &[u8], comment: &str) {
        self.network.serve(
            &format!("{url}.minisig"),
            Served::Body(self.signer.sign(bytes, comment)),
        );
        self.network.serve(url, Served::Body(bytes.to_vec()));
    }

    /// Publishes a manifest whose channels are `channels`, each naming a
    /// release; a release in `files` has its desktop file served too.
    fn publish(&self, channels: &[(&str, &Published)], with_desktop: bool) {
        let mut listed = serde_json::Map::new();
        for (name, release) in channels {
            let mut entry = serde_json::json!({
                "version": release.version,
                "url": release.folder(),
                "floti": {},
                "desktop": {},
            });
            if with_desktop {
                entry["desktop"] = serde_json::json!({TARGET: {
                    "file": release.file(),
                    "sha256": hex_digest(&release.desktop),
                }});
            }
            listed.insert((*name).to_owned(), entry);
            self.serve_signed(
                &release.url(),
                &release.desktop,
                &format!("floti {} {}", release.version, release.file()),
            );
        }
        let manifest = serde_json::json!({"schema": 1, "channels": listed}).to_string();
        self.serve_signed(MANIFEST, manifest.as_bytes(), "floti update.json");
    }

    fn run(&self) -> Run {
        let key = self.signer.public();
        let mut out = Vec::new();
        let install_self = |floti: &Path| {
            self.install_selfs.borrow_mut().push(floti.to_path_buf());
            if self.install_self_fails {
                Err("floti install-self: no room".to_owned())
            } else {
                Ok("floti is at /home/me/.local/bin/floti.\nIt is up to date.\n".to_owned())
            }
        };
        let result = Adder {
            running_version: RUNNING,
            target: TARGET.into(),
            manifest_url: Url::parse(MANIFEST).unwrap(),
            key: &key,
            network: &self.network,
            installs: &self.installs,
            install_self: &install_self,
            env: self.env.clone(),
            data: self.dirs.data.clone(),
            out: &mut out,
        }
        .run();
        Run {
            result,
            out: String::from_utf8(out).unwrap(),
        }
    }

    fn staging_is_gone(&self) -> bool {
        !self.dirs.data.join(STAGING_FOLDER).exists()
    }
}

impl Run {
    fn failure(&self) -> String {
        match &self.result {
            Err(err) => failure_line(err),
            Ok(added) => panic!("installed: {added:?}\n{}", self.out),
        }
    }
}

#[test]
fn installs_the_app_from_the_release_of_floti_s_own_version_once_its_checks_pass() {
    let world = World::new();
    let own = Published::new(RUNNING);
    let newer = Published::new("3.1.0-preview.1");
    world.publish(&[("stable", &own), ("preview", &newer)], true);
    let run = world.run();
    assert_eq!(
        run.result.as_ref().unwrap(),
        &Added::Installed {
            desktop: world.installs.target.clone(),
            version: RUNNING.into(),
        },
        "{}",
        run.out
    );
    assert_eq!(world.installs.installed(), vec![own.desktop.clone()]);
    assert!(
        !world.network.asked().contains(&newer.url()),
        "the newest preview is not this floti's release"
    );
    assert!(
        world
            .network
            .asked()
            .iter()
            .position(|url| *url == format!("{}.minisig", own.url()))
            < world
                .network
                .asked()
                .iter()
                .position(|url| *url == own.url()),
        "the signature is fetched before the file: {:?}",
        world.network.asked()
    );
    assert_eq!(*world.installs.launchers.lock().unwrap(), 1);
    assert!(
        world.staging_is_gone(),
        "the download is removed once installed"
    );
    assert!(
        run.out.contains(&format!("Downloading {}.", own.file())),
        "{}",
        run.out
    );
    assert!(
        run.out.contains(&format!(
            "Helm {RUNNING} is installed at {}.",
            world.installs.target.path.display()
        )),
        "{}",
        run.out
    );
}

#[test]
fn the_remembered_channel_s_release_is_taken_when_two_name_floti_s_version() {
    let world = World::new();
    channel::remember(&world.dirs.data, "preview").unwrap();
    let own = Published::new(RUNNING);
    world.publish(&[("stable", &own), ("preview", &own)], true);
    let run = world.run();
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(world.installs.installed(), vec![own.desktop]);
}

/// One way to spoil a published release: what it is, what it does to the
/// world, and the line floti then prints.
type Spoiling<'a> = (&'a str, &'a dyn Fn(&World), String);

#[test]
fn a_download_that_fails_a_check_installs_nothing_and_says_which_check() {
    let own = Published::new(RUNNING);
    let file = own.file();
    let cases: [Spoiling; 4] = [
        (
            "signed by another key",
            &|world: &World| {
                let other = Signer::new();
                world.network.serve(
                    &format!("{}.minisig", own.url()),
                    Served::Body(other.sign(&own.desktop, &format!("floti {RUNNING} {file}"))),
                );
            },
            format!("{file} is not signed with the key built into floti. Nothing was installed."),
        ),
        (
            "a signature of other bytes",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.url()),
                    Served::Body(
                        world
                            .signer
                            .sign(b"other", &format!("floti {RUNNING} {file}")),
                    ),
                );
            },
            format!("{file} does not match its signature. Nothing was installed."),
        ),
        (
            "no signature",
            &|world: &World| world.network.forget(&format!("{}.minisig", own.url())),
            format!(
                "{file} has no signature ({file}.minisig is not there). Nothing was installed."
            ),
        ),
        (
            "another SHA-256",
            &|world: &World| {
                let tampered = b"tampered".to_vec();
                world.serve_signed(&own.url(), &tampered, &format!("floti {RUNNING} {file}"));
            },
            format!("{file} does not match its SHA-256 in update.json. Nothing was installed."),
        ),
    ];
    for (case, spoil, line) in cases {
        let world = World::new();
        world.publish(&[("stable", &own)], true);
        spoil(&world);
        let run = world.run();
        assert_eq!(run.failure(), line, "{case}");
        assert!(
            world.installs.installed().is_empty(),
            "{case}: nothing runs"
        );
        assert_eq!(*world.installs.launchers.lock().unwrap(), 0, "{case}");
        assert!(world.staging_is_gone(), "{case}: the download is removed");
    }
}

#[test]
fn a_manifest_that_fails_its_signature_installs_nothing() {
    let world = World::new();
    let own = Published::new(RUNNING);
    world.publish(&[("stable", &own)], true);
    world.network.serve(
        &format!("{MANIFEST}.minisig"),
        Served::Body(world.signer.sign(b"{}", "floti update.json")),
    );
    let run = world.run();
    assert_eq!(
        run.failure(),
        "update.json does not match its signature. Nothing was installed."
    );
    assert!(!world.network.asked().contains(&own.url()));
    assert!(world.installs.installed().is_empty());
}

#[test]
fn an_app_already_installed_is_named_and_nothing_is_changed() {
    let mut world = World::new();
    world.installs.installed = true;
    world.installs.version = Some("2.9.0".into());
    let run = world.run();
    assert_eq!(
        run.result.as_ref().unwrap(),
        &Added::AlreadyInstalled(world.installs.target.clone())
    );
    assert_eq!(
        run.out.trim_end(),
        format!(
            "Helm 2.9.0 is already installed at {}; floti update keeps it up to date. Nothing was changed.",
            world.installs.target.path.display()
        )
    );
    assert!(world.network.asked().is_empty(), "nothing is downloaded");
    assert!(world.installs.installed().is_empty());
    assert_eq!(*world.installs.launchers.lock().unwrap(), 0);

    world.installs.version = None;
    let run = world.run();
    assert!(
        run.out.starts_with("Helm is already installed at "),
        "a version that cannot be told is left out: {}",
        run.out
    );
}

#[test]
fn a_session_with_no_display_is_refused_in_one_sentence_before_anything_is_downloaded() {
    let mut world = World::new();
    world.installs.no_display =
        Some("this session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set)".into());
    let run = world.run();
    assert_eq!(
        run.failure(),
        "This session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set), so Helm is not installed here. Nothing was installed."
    );
    assert!(world.network.asked().is_empty());
}

#[test]
fn a_test_build_s_display_switch_overrides_the_os_s_answer() {
    let mut world = World::new();
    world
        .env
        .vars
        .insert(OsString::from(DISPLAY_TEST_VAR), OsString::from("none"));
    assert!(matches!(
        world.run().result,
        Err(AddError::NoDisplay { .. })
    ));

    world.installs.no_display = Some("this session has no display".into());
    world
        .env
        .vars
        .insert(OsString::from(DISPLAY_TEST_VAR), OsString::from("present"));
    let own = Published::new(RUNNING);
    world.publish(&[("stable", &own)], true);
    let run = world.run();
    assert!(run.result.is_ok(), "{}", run.out);
}

#[test]
fn a_floti_older_than_a_channel_s_newest_is_told_to_update_first() {
    let world = World::new();
    world.publish(&[("stable", &Published::new("3.0.1"))], true);
    let run = world.run();
    assert_eq!(
        run.failure(),
        "floti 3.0.0 is older than stable's newest, 3.0.1, and update.json lists the files of each channel's newest alone; run floti update, then floti add desktop. Nothing was installed."
    );
    assert!(world.installs.installed().is_empty());
}

#[test]
fn a_version_no_channel_has_reached_is_a_development_build_and_installs_nothing() {
    let world = World::new();
    world.publish(
        &[
            ("stable", &Published::new("2.9.0")),
            ("preview", &Published::new("3.0.0-preview.4")),
        ],
        true,
    );
    let run = world.run();
    assert_eq!(
        run.failure(),
        "floti 3.0.0 has no published release, so there is no Helm for it (this is a development build). Nothing was installed."
    );
    assert_eq!(
        world.network.asked().len(),
        2,
        "update.json and its signature alone"
    );
}

#[test]
fn a_release_with_no_app_for_this_computer_installs_nothing() {
    let world = World::new();
    world.publish(&[("stable", &Published::new(RUNNING))], false);
    let run = world.run();
    assert_eq!(
        run.failure(),
        "floti 3.0.0 has no Helm for linux-x64. Nothing was installed."
    );
}

#[test]
fn an_installer_that_fails_installs_nothing_and_says_so() {
    let mut world = World::new();
    world.installs.install_fails = true;
    world.publish(&[("stable", &Published::new(RUNNING))], true);
    let run = world.run();
    assert_eq!(
        run.failure(),
        "Helm could not be installed: The Helm installer ended with exit code: 2. Nothing was installed."
    );
    assert!(world.staging_is_gone());
}

#[test]
fn the_menu_entry_is_named_and_a_failure_to_write_it_says_the_app_is_installed() {
    let mut world = World::new();
    world.installs.launcher = Ok(Some(PathBuf::from(
        "/home/me/.local/share/applications/helm.desktop",
    )));
    world.publish(&[("stable", &Published::new(RUNNING))], true);
    let run = world.run();
    assert!(
        run.out
            .contains("Its menu entry is /home/me/.local/share/applications/helm.desktop."),
        "{}",
        run.out
    );

    world.installs.launcher = Err("No HOME or XDG_DATA_HOME names where menu entries go.".into());
    let run = world.run();
    assert_eq!(
        run.failure(),
        format!(
            "Helm is installed at {}, but its menu entry could not be written: No HOME or XDG_DATA_HOME names where menu entries go.",
            world.installs.target.path.display()
        )
    );
}

#[test]
fn the_new_app_s_own_floti_runs_install_self_where_its_installer_does_not() {
    let mut world = World::new();
    let carried = world.dirs.folder("app resources").join("floti");
    std::fs::write(&carried, b"the app's floti").unwrap();
    world.installs.carried = Some(carried.clone());
    world.publish(&[("stable", &Published::new(RUNNING))], true);
    let run = world.run();
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(*world.install_selfs.borrow(), vec![carried.clone()]);
    assert!(run.out.ends_with("It is up to date.\n"), "{}", run.out);

    world.install_self_fails = true;
    let run = world.run();
    assert!(run.result.is_ok(), "a failed install-self stops nothing");
    assert!(
        run.out.contains("Helm could not put its floti and Node in place now (floti install-self: no room); it does when it next starts."),
        "{}",
        run.out
    );

    world.installs.carried = Some(world.dirs.root().join("no floti here"));
    world.install_selfs.borrow_mut().clear();
    world.run();
    assert!(
        world.install_selfs.borrow().is_empty(),
        "no carried floti, no install-self"
    );
}

#[test]
fn add_takes_one_known_name_or_lists_what_it_adds() {
    let args = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(parse(&args(&[])), Some(Request::List));
    assert_eq!(parse(&args(&["desktop"])), Some(Request::Desktop));
    assert_eq!(
        parse(&args(&["tmux"])),
        Some(Request::Unknown("tmux".into()))
    );
    assert_eq!(parse(&args(&["desktop", "tmux"])), None);
    assert_eq!(parse(&args(&["--force"])), None);
    let listed = list();
    assert_eq!(listed.lines().count(), 2, "{listed}");
    assert!(
        listed
            .lines()
            .nth(1)
            .unwrap()
            .trim_start()
            .starts_with("desktop "),
        "{listed}"
    );
}
