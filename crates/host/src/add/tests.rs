//! `floti add desktop`'s flow with the network and the installs faked: what
//! it installs and from where, each check that stops it, and what it says.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use floti_protocol::Knowable;
use url::Url;

use super::{
    AddError, Added, Adder, Request, STAGING_FOLDER, desktop_file, desktop_sums_name, failure_line,
    list, listed_sha256, parse,
};
use crate::Result;
use crate::platform::folders::UserEnv;
use crate::platform::installs::{DISPLAY_TEST_VAR, Desktop, Installs};
use crate::platform::process_table::ProcessRow;
use crate::sha256::hex_digest;
use crate::testkit::TestDirs;
use crate::update::fakes::{FakeNetwork, Served, Signer};
use crate::update::run_file::{self, Look, RunLock};
use crate::update::versions::Version;

const RUNNING: &str = "3.0.0";
const TARGET: &str = "linux-x64";
const RELEASES: &str = "https://floti.test/releases/download/";

/// Installs that record what they were asked: the app at `target`, installed
/// already when `installed`.
struct FakeInstalls {
    target: Desktop,
    installed: bool,
    version: Option<String>,
    no_display: Option<String>,
    carried: Option<PathBuf>,
    launcher: std::result::Result<Option<PathBuf>, String>,
    /// What repairing the installed app's launcher entry answers.
    repair: std::result::Result<Option<PathBuf>, String>,
    install_fails: bool,
    /// The bytes of each file installed, in turn.
    installs: Mutex<Vec<Vec<u8>>>,
    launchers: Mutex<usize>,
}

impl FakeInstalls {
    fn new(apps: &Path) -> Self {
        Self {
            target: Desktop {
                name: "Floti",
                path: apps.join("Floti.AppImage"),
            },
            installed: false,
            version: None,
            no_display: None,
            carried: None,
            launcher: Ok(None),
            repair: Ok(None),
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

    fn repair_launcher(&self, _env: &UserEnv, _desktop: &Desktop) -> Result<Option<PathBuf>> {
        self.repair.clone().map_err(crate::Error::Refused)
    }

    fn add_launcher(&self, _env: &UserEnv, _desktop: &Desktop) -> Result<Option<PathBuf>> {
        *self.launchers.lock().unwrap() += 1;
        self.launcher.clone().map_err(crate::Error::Refused)
    }

    fn install_desktop(&self, desktop: &Desktop, release: &Path) -> Result<Desktop> {
        if self.install_fails {
            return Err(crate::Error::Refused(
                "The Floti installer ended with exit code: 2.".into(),
            ));
        }
        self.installs
            .lock()
            .unwrap()
            .push(std::fs::read(release).unwrap());
        Ok(desktop.clone())
    }
}

/// A release published at `version`: its desktop file for [`TARGET`], and
/// the signed list of its desktop files' SHA-256s.
struct Published {
    version: String,
    desktop: Vec<u8>,
}

impl Published {
    fn new(version: &str) -> Self {
        Self {
            version: version.into(),
            desktop: format!("Floti AppImage {version}").into_bytes(),
        }
    }

    fn folder(&self) -> String {
        format!("{RELEASES}v{}/", self.version)
    }

    fn file(&self) -> String {
        format!("Helm-{}-linux-x64.AppImage", self.version)
    }

    fn url(&self) -> String {
        format!("{}{}", self.folder(), self.file())
    }

    fn list_file(&self) -> String {
        format!("Helm-{}-SHA256SUMS", self.version)
    }

    fn list_url(&self) -> String {
        format!("{}{}", self.folder(), self.list_file())
    }

    /// The list as the release workflow writes it: every desktop file,
    /// sorted, this one with its real SHA-256.
    fn list(&self) -> Vec<u8> {
        let v = &self.version;
        let mut lines = [
            (format!("Helm-{v}-arm64-mac.zip"), "a".repeat(64)),
            (format!("Helm-{v}-arm64.dmg"), "b".repeat(64)),
            (self.file(), hex_digest(&self.desktop)),
            (format!("Helm-{v}-portable.exe"), "c".repeat(64)),
            (format!("Helm-{v}-setup.exe"), "d".repeat(64)),
        ];
        lines.sort();
        lines
            .iter()
            .map(|(name, sha256)| format!("{sha256}  {name}\n"))
            .collect::<String>()
            .into_bytes()
    }

    fn comment(&self, file: &str) -> String {
        format!("floti {} {file}", self.version)
    }
}

struct World {
    signer: Signer,
    network: FakeNetwork,
    installs: FakeInstalls,
    install_selfs: RefCell<Vec<PathBuf>>,
    install_self_fails: bool,
    running: &'static str,
    target: &'static str,
    /// Whether any other process is alive, for a run file another left.
    other_alive: Knowable<bool>,
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
            running: RUNNING,
            target: TARGET,
            other_alive: Knowable::Known(true),
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

    /// Publishes `release`: its desktop file and its list, each signed.
    fn publish(&self, release: &Published) {
        self.serve_signed(
            &release.url(),
            &release.desktop,
            &release.comment(&release.file()),
        );
        self.serve_signed(
            &release.list_url(),
            &release.list(),
            &release.comment(&release.list_file()),
        );
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
        let is_alive = |_pid: u32| self.other_alive.clone();
        let result = Adder {
            running_version: self.running,
            target: self.target.into(),
            releases: Url::parse(RELEASES).unwrap(),
            key: &key,
            network: &self.network,
            installs: &self.installs,
            install_self: &install_self,
            is_alive: &is_alive,
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

    fn position(&self, url: &str) -> Option<usize> {
        self.network.asked().iter().position(|asked| asked == url)
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
fn installs_the_app_from_its_own_release_after_the_list_s_signature_then_the_file_s_checks() {
    let world = World::new();
    let own = Published::new(RUNNING);
    world.publish(&own);
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
    let order = [
        format!("{}.minisig", own.list_url()),
        own.list_url(),
        format!("{}.minisig", own.url()),
        own.url(),
    ];
    let positions: Vec<Option<usize>> = order.iter().map(|url| world.position(url)).collect();
    assert!(
        positions.iter().all(Option::is_some) && positions.is_sorted(),
        "the list's signature, the list, the file's signature, the file: {:?}",
        world.network.asked()
    );
    assert!(
        world
            .network
            .asked()
            .iter()
            .all(|url| url.starts_with(&own.folder())),
        "nothing but its own release, no update.json: {:?}",
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
            "Floti {RUNNING} is installed at {}.",
            world.installs.target.path.display()
        )),
        "{}",
        run.out
    );
}

#[test]
fn a_preview_floti_adds_its_own_preview_s_app() {
    let mut world = World::new();
    world.running = "3.1.0-preview.2";
    let own = Published::new("3.1.0-preview.2");
    world.publish(&own);
    world.publish(&Published::new(RUNNING));
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
    let list = own.list_file();
    let other = Signer::new();
    let cases: [Spoiling; 8] = [
        (
            "the file signed by another key",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.url()),
                    Served::Body(other.sign(&own.desktop, &own.comment(&file))),
                );
            },
            format!("{file} is not signed with the key built into floti. Nothing was installed."),
        ),
        (
            "a signature of other bytes",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.url()),
                    Served::Body(world.signer.sign(b"other", &own.comment(&file))),
                );
            },
            format!("{file} does not match its signature. Nothing was installed."),
        ),
        (
            "no signature for the file",
            &|world: &World| world.network.forget(&format!("{}.minisig", own.url())),
            format!(
                "{file} has no signature ({file}.minisig is not there). Nothing was installed."
            ),
        ),
        (
            "a file that does not match the list",
            &|world: &World| {
                world.serve_signed(&own.url(), b"tampered", &own.comment(&file));
            },
            format!("{file} does not match its SHA-256 in {list}. Nothing was installed."),
        ),
        (
            "a list whose signature is of other bytes",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.list_url()),
                    Served::Body(world.signer.sign(b"another list", &own.comment(&list))),
                );
            },
            format!("{list} does not match its signature. Nothing was installed."),
        ),
        (
            "a list signed by another key",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.list_url()),
                    Served::Body(other.sign(&own.list(), &own.comment(&list))),
                );
            },
            format!("{list} is not signed with the key built into floti. Nothing was installed."),
        ),
        (
            "a list signed for another version",
            &|world: &World| {
                world.network.serve(
                    &format!("{}.minisig", own.list_url()),
                    Served::Body(
                        world
                            .signer
                            .sign(&own.list(), &format!("floti 2.9.0 {list}")),
                    ),
                );
            },
            format!(
                "the signature of {list} is for \"floti 2.9.0 {list}\", not \"floti {RUNNING} {list}\". Nothing was installed."
            ),
        ),
        (
            "a list that does not name the file",
            &|world: &World| {
                let without: String = String::from_utf8(own.list())
                    .unwrap()
                    .lines()
                    .filter(|line| !line.ends_with(&file))
                    .map(|line| format!("{line}\n"))
                    .collect();
                world.serve_signed(&own.list_url(), without.as_bytes(), &own.comment(&list));
            },
            format!("{list} does not list {file}. Nothing was installed."),
        ),
    ];
    for (case, spoil, line) in cases {
        let world = World::new();
        world.publish(&own);
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
fn a_list_signed_by_another_key_is_refused_before_the_list_is_fetched() {
    let world = World::new();
    let own = Published::new(RUNNING);
    world.publish(&own);
    world.network.serve(
        &format!("{}.minisig", own.list_url()),
        Served::Body(Signer::new().sign(&own.list(), &own.comment(&own.list_file()))),
    );
    world.run();
    assert_eq!(world.position(&own.list_url()), None);
    assert_eq!(world.position(&own.url()), None);
}

#[test]
fn a_published_release_without_the_list_installs_nothing_and_says_so() {
    let world = World::new();
    let own = Published::new(RUNNING);
    world.publish(&own);
    world.network.forget(&own.list_url());
    world.network.forget(&format!("{}.minisig", own.list_url()));
    let run = world.run();
    assert_eq!(
        run.failure(),
        format!(
            "floti {RUNNING}'s release has no {}, the signed list of its desktop files' SHA-256s, so floti cannot check Floti for it. Nothing was installed.",
            own.list_file()
        )
    );
    assert_eq!(
        world.position(&own.url()),
        None,
        "the app is not downloaded"
    );
    assert!(world.installs.installed().is_empty());
}

#[test]
fn a_version_with_no_published_release_is_a_development_build_and_installs_nothing() {
    let world = World::new();
    world.publish(&Published::new("2.9.0"));
    let run = world.run();
    assert_eq!(
        run.failure(),
        "floti 3.0.0 has no published release, so there is no Floti for it (this is a development build). Nothing was installed."
    );
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
            "Floti 2.9.0 is already installed at {}; floti update keeps it up to date. Nothing was changed.",
            world.installs.target.path.display()
        )
    );
    assert!(world.network.asked().is_empty(), "nothing is downloaded");
    assert!(world.installs.installed().is_empty());
    assert_eq!(*world.installs.launchers.lock().unwrap(), 0);

    world.installs.version = None;
    let run = world.run();
    assert!(
        run.out.starts_with("Floti is already installed at "),
        "a version that cannot be told is left out: {}",
        run.out
    );
}

#[test]
fn a_second_add_or_one_during_an_update_refuses_in_one_line_and_downloads_nothing() {
    let world = World::new();
    world.publish(&Published::new(RUNNING));
    let other = RunLock::take(&world.dirs.data, 4242, "Starting floti update.", &|_| {
        Knowable::Known(true)
    })
    .unwrap();
    let run = world.run();
    assert_eq!(
        run.failure(),
        "another floti update or floti add desktop is running (process 4242), so nothing was installed; run floti add desktop again once it has ended."
    );
    assert!(world.network.asked().is_empty());
    assert!(world.installs.installed().is_empty());
    assert!(
        matches!(run_file::look(&world.dirs.data), Look::Running(record) if record.pid == 4242),
        "the other's run file is left as it is"
    );
    drop(other);
}

#[test]
fn add_holds_the_run_file_while_it_runs_and_leaves_no_ended_record() {
    let mut world = World::new();
    world.publish(&Published::new(RUNNING));
    // A run file a process that has ended left is taken over.
    std::mem::forget(
        RunLock::take(&world.dirs.data, 4242, "Starting floti update.", &|_| {
            Knowable::Known(true)
        })
        .unwrap(),
    );
    world.other_alive = Knowable::Known(false);
    let carried = world.dirs.folder("app resources").join("floti");
    std::fs::write(&carried, b"the app's floti").unwrap();
    world.installs.carried = Some(carried);
    let data = world.dirs.data.clone();
    let seen = RefCell::new(None);
    let key = world.signer.public();
    let mut out = Vec::new();
    // install-self runs while add holds the run file: what it says then.
    let install_self = |_floti: &Path| {
        *seen.borrow_mut() = Some(run_file::look(&data));
        Ok(String::new())
    };
    let result = Adder {
        running_version: world.running,
        target: world.target.into(),
        releases: Url::parse(RELEASES).unwrap(),
        key: &key,
        network: &world.network,
        installs: &world.installs,
        install_self: &install_self,
        is_alive: &|_| world.other_alive.clone(),
        env: world.env.clone(),
        data: world.dirs.data.clone(),
        out: &mut out,
    }
    .run();
    assert!(result.is_ok(), "{}", String::from_utf8_lossy(&out));
    assert!(
        matches!(seen.borrow().as_ref(), Some(Look::Running(record)) if record.pid == std::process::id() && record.line == super::STARTING_LINE),
        "{:?}",
        seen.borrow()
    );
    assert_eq!(
        run_file::look(&world.dirs.data),
        Look::Absent,
        "removed once it ends"
    );
    assert_eq!(
        run_file::ended(&world.dirs.data),
        None,
        "an add is no update"
    );
}

#[test]
fn something_at_the_install_s_place_that_is_not_an_install_is_left_and_refused() {
    let world = World::new();
    world.publish(&Published::new(RUNNING));
    std::fs::create_dir_all(&world.installs.target.path).unwrap();
    std::fs::write(world.installs.target.path.join("mine.txt"), b"keep").unwrap();
    let run = world.run();
    assert_eq!(
        run.failure(),
        format!(
            "{0} is there but is not an install of Floti, so nothing was installed; move it away to install Floti there.",
            world.installs.target.path.display()
        )
    );
    assert!(world.network.asked().is_empty());
    assert!(world.installs.installed().is_empty());
    assert_eq!(
        std::fs::read(world.installs.target.path.join("mine.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn an_installed_app_missing_its_menu_entry_gets_it_and_is_still_already_installed() {
    let mut world = World::new();
    world.installs.installed = true;
    world.installs.repair = Ok(Some(PathBuf::from(
        "/home/me/.local/share/applications/helm.desktop",
    )));
    let run = world.run();
    assert_eq!(
        run.result.as_ref().unwrap(),
        &Added::AlreadyInstalled(world.installs.target.clone())
    );
    // One line, which says what changed and never that nothing did.
    assert_eq!(
        run.out,
        format!(
            "Floti is already installed at {}; floti update keeps it up to date. Its menu entry was missing, so floti added it at {}.\n",
            world.installs.target.path.display(),
            Path::new("/home/me/.local/share/applications/helm.desktop").display()
        )
    );
    assert!(world.network.asked().is_empty());

    world.installs.repair = Err("No HOME or XDG_DATA_HOME names where menu entries go.".into());
    let run = world.run();
    assert_eq!(
        run.failure(),
        format!(
            "Floti is installed at {}, but its menu entry could not be written: No HOME or XDG_DATA_HOME names where menu entries go.",
            world.installs.target.path.display()
        )
    );
    assert_eq!(run.out, "", "the failure line alone says the app is there");
}

#[test]
fn a_session_with_no_display_is_refused_in_one_sentence_before_anything_is_downloaded() {
    let mut world = World::new();
    world.installs.no_display =
        Some("this session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set)".into());
    let run = world.run();
    assert_eq!(
        run.failure(),
        "This session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set), so Floti is not installed here."
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
    world.publish(&Published::new(RUNNING));
    let run = world.run();
    assert!(run.result.is_ok(), "{}", run.out);
}

#[test]
fn a_computer_the_release_has_no_app_for_installs_nothing() {
    let mut world = World::new();
    world.target = "windows-arm64";
    world.publish(&Published::new(RUNNING));
    let run = world.run();
    assert_eq!(
        run.failure(),
        "floti 3.0.0 has no Floti for windows-arm64. Nothing was installed."
    );
    assert!(world.network.asked().is_empty());
}

#[test]
fn an_installer_that_fails_installs_nothing_and_says_so() {
    let mut world = World::new();
    world.installs.install_fails = true;
    world.publish(&Published::new(RUNNING));
    let run = world.run();
    assert_eq!(
        run.failure(),
        "Floti could not be installed: The Floti installer ended with exit code: 2. Nothing was installed."
    );
    assert!(world.staging_is_gone());
}

#[test]
fn the_menu_entry_is_named_and_a_failure_to_write_it_says_the_app_is_installed() {
    let mut world = World::new();
    world.installs.launcher = Ok(Some(PathBuf::from(
        "/home/me/.local/share/applications/helm.desktop",
    )));
    world.publish(&Published::new(RUNNING));
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
            "Floti is installed at {}, but its menu entry could not be written: No HOME or XDG_DATA_HOME names where menu entries go.",
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
    world.publish(&Published::new(RUNNING));
    let run = world.run();
    assert!(run.result.is_ok(), "{}", run.out);
    assert_eq!(*world.install_selfs.borrow(), vec![carried.clone()]);
    assert!(run.out.ends_with("It is up to date.\n"), "{}", run.out);

    world.install_self_fails = true;
    let run = world.run();
    assert!(run.result.is_ok(), "a failed install-self stops nothing");
    assert!(
        run.out.contains("Floti could not put its floti and Node in place now (floti install-self: no room); it does when it next starts."),
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
fn a_sha256_list_line_gives_its_file_s_digest_in_either_of_sha256sum_s_modes() {
    let sha = "AB".repeat(32);
    let list = format!(
        "{}  other\r\n{sha} *Helm-3.0.0-setup.exe\n{}  Helm-3.0.0-setup.exe.minisig\n",
        "c".repeat(64),
        "d".repeat(64)
    );
    assert_eq!(
        listed_sha256(&list, "Helm-3.0.0-setup.exe"),
        Some(sha.to_ascii_lowercase())
    );
    assert_eq!(
        listed_sha256(&list, "other"),
        Some("c".repeat(64)),
        "a CRLF line"
    );
    assert_eq!(listed_sha256(&list, "Helm-3.0.0"), None);
    assert_eq!(
        listed_sha256("zz  Helm-3.0.0-setup.exe\n", "Helm-3.0.0-setup.exe"),
        None
    );
    let not_hex = format!("{}  f\n", "g".repeat(64));
    assert_eq!(listed_sha256(&not_hex, "f"), None);
}

#[test]
fn the_desktop_files_are_named_as_the_release_workflow_names_them() {
    let version = Version::parse("3.0.0").unwrap();
    let workflow = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/desktop/scripts/release.mjs"),
    )
    .unwrap();
    for (target, file) in [
        ("windows-x64", "Helm-${version}-setup.exe"),
        ("macos-arm64", "Helm-${version}-arm64-mac.zip"),
        ("linux-x64", "Helm-${version}-linux-x64.AppImage"),
    ] {
        assert!(
            workflow.contains(&format!("`{file}`")),
            "{file} in release.mjs"
        );
        assert_eq!(
            desktop_file(&version, target).as_deref(),
            Some(file.replace("${version}", "3.0.0").as_str())
        );
    }
    assert!(workflow.contains("`Helm-${version}-SHA256SUMS`"));
    assert_eq!(desktop_sums_name(&version), "Helm-3.0.0-SHA256SUMS");
    for target in ["windows-arm64", "linux-arm64", "freebsd-x64"] {
        assert_eq!(desktop_file(&version, target), None, "{target}");
    }
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
