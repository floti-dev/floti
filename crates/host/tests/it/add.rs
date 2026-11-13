//! `floti add`, end to end: a copy of the built `floti`, run from a fake bin
//! folder, installing a fake desktop app from a loopback HTTP server that
//! serves a release of floti's own version, with its desktop SHA-256 list,
//! signed with a key made in the test. No `update.json` is served.
//!
//! The test key and the releases' address reach the binary only through
//! the testkit's marker in the data folder (`floti-test-update.json`); the
//! desktop app is looked for and installed only in the folder
//! `FLOTI_DESKTOP_DIR` names, and `FLOTI_TEST_DISPLAY` says whether there is
//! a display, both read by a build with the testkit alone. Menu entries go
//! under the test's `XDG_DATA_HOME`, floti installed on its own under its
//! home and local app data. On Windows the release's installer is the fake
//! program, which records how it was run. Everything is under the test's
//! `TestDirs`; nothing real is installed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::common::{Served, Server, Signer};
use floti_host::platform::folders::UserEnv;
use floti_host::platform::installs::{
    DESKTOP_DIR_VAR, DISPLAY_TEST_VAR, Desktop, FakeDesktop, Installs, Native,
};
use floti_host::sha256::hex_digest;
use floti_host::testkit::{TestDirs, run_floti};
use floti_host::update::run_file::RunLock;
use floti_host::update::signature::TEST_UPDATE_MARKER;
use floti_host::version::FLOTI_VERSION;

const RECORD_VAR: &str = "FLOTI_FAKE_PROGRAM_RECORD";

/// A fake bin folder holding a copy of the built floti, an empty
/// Applications folder, and a server with floti's own release on it.
struct World {
    server: Server,
    signer: Signer,
    floti: PathBuf,
    apps: PathBuf,
    record: PathBuf,
    data_home: PathBuf,
    local_app_data: PathBuf,
    desktop_file: String,
    list_file: String,
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let bin = dirs.folder("bin dir");
        let floti = bin.join(format!("floti{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(env!("CARGO_BIN_EXE_floti"), &floti).expect("copy the built floti");
        let apps = dirs.folder("apps dir");
        let record = dirs.root().join("installer runs.jsonl");
        let data_home = dirs.folder("data home");
        let local_app_data = dirs.folder("local app data");

        let server = Server::start();
        let signer = Signer::new();
        let (desktop_file, desktop) = Native.fake_release(
            &dirs.folder("release work"),
            FLOTI_VERSION,
            Path::new(env!("CARGO_BIN_EXE_floti-fake-program")),
        );
        let list_file = format!("Helm-{FLOTI_VERSION}-SHA256SUMS");
        let list = format!("{}  {desktop_file}\n", hex_digest(&desktop));

        let world = Self {
            server,
            signer,
            floti,
            apps,
            record,
            data_home,
            local_app_data,
            desktop_file,
            list_file,
            dirs,
        };
        world.serve_signed(
            &world.file_path(&world.list_file),
            list.as_bytes(),
            &format!("floti {FLOTI_VERSION} {}", world.list_file),
        );
        world.serve_signed(
            &world.file_path(&world.desktop_file),
            &desktop,
            &format!("floti {FLOTI_VERSION} {}", world.desktop_file),
        );
        let marker = serde_json::json!({
            "releases": world.server.url("/download/"),
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

    /// Where `file` of floti's own release is on the server.
    fn file_path(&self, file: &str) -> String {
        format!("/download/v{FLOTI_VERSION}/{file}")
    }

    async fn add(&self, args: &[&str], display: &str) -> std::process::Output {
        let mut command = self.dirs.floti_command(&self.floti);
        command
            .arg("add")
            .args(args)
            .env(DESKTOP_DIR_VAR, &self.apps)
            .env(DISPLAY_TEST_VAR, display)
            .env(RECORD_VAR, &self.record)
            .env("XDG_DATA_HOME", &self.data_home)
            .env("LOCALAPPDATA", &self.local_app_data);
        run_floti(command).await
    }

    /// Whether `install-self` was run from the floti the installed app
    /// carries, as the record of the fake program's runs says.
    fn carried_install_self_ran(&self) -> bool {
        std::fs::read_to_string(&self.record)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|run| run["args"] == serde_json::json!(["install-self"]))
    }

    /// The installed app, as this OS finds it in the test's folder.
    fn app(&self) -> Desktop {
        let user = UserEnv {
            home: Some(self.dirs.home.clone()),
            vars: [(
                OsString::from(DESKTOP_DIR_VAR),
                self.apps.clone().into_os_string(),
            )]
            .into(),
        };
        Native.desktop_target(&user).unwrap()
    }

    /// The user as the add's own session sees it for menu entries: the
    /// test's home, with `data_home` as its `XDG_DATA_HOME`.
    fn launcher_user(&self, data_home: &Path) -> UserEnv {
        UserEnv {
            home: Some(self.dirs.home.clone()),
            vars: [(
                OsString::from("XDG_DATA_HOME"),
                data_home.as_os_str().to_owned(),
            )]
            .into(),
        }
    }

    /// The installed app as add names it: with its version where this OS
    /// can tell it.
    fn named(&self) -> String {
        match Native.desktop_version(&self.app()) {
            Some(version) => format!("Floti {version}"),
            None => "Helm".to_owned(),
        }
    }

    fn installed(&self) -> bool {
        Native.was_updated(&self.apps, FLOTI_VERSION, &self.record)
    }

    /// Whether anything was written where floti installed on its own goes,
    /// or beside the floti that ran.
    fn second_floti(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for folder in [
            self.dirs.home.join(".local").join("bin"),
            self.local_app_data.join("Programs").join("Floti"),
        ] {
            if folder.exists() {
                found.push(folder);
            }
        }
        for entry in std::fs::read_dir(self.floti.parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path != self.floti {
                found.push(path);
            }
        }
        found
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn floti_add_desktop_installs_the_app_of_its_own_release_and_no_second_floti() {
    let world = World::new();
    let before = std::fs::read(&world.floti).unwrap();
    let output = world.add(&["desktop"], "present").await;
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(world.installed(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains(&format!("Downloading {}.", world.desktop_file)),
        "{stdout}"
    );
    assert!(stdout.contains(" is installed at "), "{stdout}");
    assert!(
        !world.dirs.data.join("add").exists(),
        "the download is removed once installed"
    );
    assert_eq!(std::fs::read(&world.floti).unwrap(), before);
    assert_eq!(world.second_floti(), Vec::<PathBuf>::new());
    // floti runs install-self from the app's own floti where the app's
    // installer does not (macOS); where it does (Windows) or the floti is
    // inside an image (Linux), floti runs none.
    assert_eq!(
        world.carried_install_self_ran(),
        Native.carried_floti(&world.app()).is_some(),
        "{stdout}"
    );
    // floti writes a menu entry where this OS's install makes none (Linux),
    // under the session's XDG_DATA_HOME, and says where.
    let probe = world.dirs.folder("probe data home");
    let user = UserEnv {
        home: Some(world.dirs.home.clone()),
        vars: [(OsString::from("XDG_DATA_HOME"), probe.into_os_string())].into(),
    };
    let app = Desktop {
        name: "Helm",
        path: world.apps.join("probe"),
    };
    let writes_one = Native.add_launcher(&user, &app).unwrap().is_some();
    let entry = world.data_home.join("applications").join("helm.desktop");
    assert_eq!(entry.is_file(), writes_one, "{stdout}");
    assert_eq!(
        stdout.contains(&format!("Its menu entry is {}.", entry.display())),
        writes_one,
        "{stdout}"
    );
}

#[tokio::test]
async fn a_download_that_fails_its_signature_installs_nothing_and_says_so_in_one_line() {
    let world = World::new();
    world.server.serve(
        &format!("{}.minisig", world.file_path(&world.desktop_file)),
        Served::Body(world.signer.sign(
            b"another file",
            &format!("floti {FLOTI_VERSION} {}", world.desktop_file),
        )),
    );
    let output = world.add(&["desktop"], "present").await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim_end(),
        format!(
            "floti add desktop: {} does not match its signature. Nothing was installed.",
            world.desktop_file
        )
    );
    assert!(!world.installed());
    assert!(!world.dirs.data.join("add").exists());
}

#[tokio::test]
async fn an_installed_app_and_a_session_with_no_display_each_change_nothing() {
    let world = World::new();
    let output = world.add(&["desktop"], "none").await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim_end(),
        "floti add desktop: This test's session has no display, so Floti is not installed here."
    );
    assert!(!world.installed());

    Native.fake_install(&world.apps, "1.0.0");
    // Its menu entry is there wherever floti makes one (Linux), so there is
    // nothing to repair either.
    let seeded = Native
        .add_launcher(&world.launcher_user(&world.data_home), &world.app())
        .unwrap();
    let entry_before = seeded.as_ref().map(|entry| std::fs::read(entry).unwrap());
    let output = world.add(&["desktop"], "present").await;
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert_eq!(
        stdout,
        format!(
            "{} is already installed at {}; floti update keeps it up to date. Nothing was changed.\n",
            world.named(),
            world.app().path.display()
        )
    );
    assert_eq!(
        seeded.as_ref().map(|entry| std::fs::read(entry).unwrap()),
        entry_before
    );
    assert!(!world.installed(), "nothing was run");
}

#[tokio::test]
async fn an_installed_app_missing_its_menu_entry_gets_it_in_one_line_that_says_so() {
    let world = World::new();
    Native.fake_install(&world.apps, "1.0.0");
    // Whether this OS's install leaves floti a menu entry to make (Linux),
    // asked of the OS's own code in a folder of the probe's own.
    let probe = world.dirs.folder("probe data home");
    let entry = world.data_home.join("applications").join("helm.desktop");
    let makes_one = Native
        .add_launcher(&world.launcher_user(&probe), &world.app())
        .unwrap()
        .is_some();
    let output = world.add(&["desktop"], "present").await;
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let said = format!(
        "{} is already installed at {}; floti update keeps it up to date.",
        world.named(),
        world.app().path.display()
    );
    let expected = if makes_one {
        format!(
            "{said} Its menu entry was missing, so floti added it at {}.\n",
            entry.display()
        )
    } else {
        format!("{said} Nothing was changed.\n")
    };
    assert_eq!(stdout, expected);
    assert_eq!(entry.is_file(), makes_one);
    assert!(!world.installed(), "nothing was run");
}

#[tokio::test]
async fn floti_add_with_no_name_or_an_unknown_one_lists_what_it_adds() {
    let world = World::new();
    let output = world.add(&[], "present").await;
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.starts_with("floti add <name> installs one of these"),
        "{stdout}"
    );
    assert!(stdout.contains("  desktop  "), "{stdout}");

    let output = world.add(&["tmux"], "present").await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.starts_with("floti add: floti cannot add \"tmux\".\n"),
        "{stderr}"
    );
    assert!(stderr.contains("  desktop  "), "{stderr}");
}

#[tokio::test]
async fn a_release_without_the_desktop_list_installs_nothing_and_says_so_in_one_line() {
    let world = World::new();
    world.server.forget(&world.file_path(&world.list_file));
    world
        .server
        .forget(&format!("{}.minisig", world.file_path(&world.list_file)));
    let output = world.add(&["desktop"], "present").await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim_end(),
        format!(
            "floti add desktop: floti {FLOTI_VERSION}'s release has no {}, the signed list of its desktop files' SHA-256s, so floti cannot check Floti for it. Nothing was installed.",
            world.list_file
        )
    );
    assert!(!world.installed());
}

#[tokio::test]
async fn an_add_while_another_holds_the_run_file_refuses_in_one_line() {
    let world = World::new();
    // This test's own process, which is alive, holds it.
    let held = RunLock::take(
        &world.dirs.data,
        std::process::id(),
        "Starting floti update.",
        &|_| floti_protocol::Knowable::Known(true),
    )
    .unwrap();
    let output = world.add(&["desktop"], "present").await;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim_end(),
        format!(
            "floti add desktop: another floti update or floti add desktop is running (process {}), so nothing was installed; run floti add desktop again once it has ended.",
            std::process::id()
        )
    );
    assert!(!world.installed());
    drop(held);
}
