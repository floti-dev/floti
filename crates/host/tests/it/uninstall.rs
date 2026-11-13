//! `floti uninstall`, against folders in the test's own tree: floti and its
//! Node where this OS's install script puts them under the test's home and
//! local app data, a fake desktop install in the folder `FLOTI_DESKTOP_DIR`
//! names, this OS's start-at-sign-in file, and the test's data, logs and
//! config folders.
//!
//! Nothing here uninstalls anything on this computer: the host, the OS's
//! sign-in commands, the desktop app's uninstaller, the `PATH`, the
//! deleting after exit and the secret store are a [`FakeSystem`] that
//! records what it was asked, and a running floti is the test's fake
//! program, ended with the test.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use floti_host::platform::folders::{Folders, UserEnv};
use floti_host::platform::installs::{
    DESKTOP_DIR_VAR, Desktop, DesktopRemoval, FakeDesktop, Installs, Native as NativeInstalls,
};
use floti_host::platform::links::{FolderLinks, Native as NativeLinks};
use floti_host::platform::process_table::ProcessRow;
use floti_host::platform::process_tree::Outliving;
use floti_host::platform::secret_seal::StoreEntry;
use floti_host::platform::standalone::{Native as NativeStandalone, Standalone};
use floti_host::platform::startup::register::{OsCommand, Ran, SignInSystem, definition_file};
use floti_host::platform::startup::{self, StartAtSignIn, StartupDefinition, StartupKind};
use floti_host::testkit::TestDirs;
use floti_host::uninstall::{
    Closed, DESKTOP_SETTINGS_LINE, HostStop, IN_SESSION_LINE, JOB_ENDS_IT_FIRST, LINGER_LINE,
    Uninstall, UninstallSystem, entry_name, uninstall,
};

/// The OS's sign-in commands: answers as each OS's would while the entry
/// is registered, and forgets it once a command takes it out.
struct FakeOs {
    registered: Cell<bool>,
    ran: RefCell<Vec<String>>,
}

impl SignInSystem for FakeOs {
    fn run(&self, command: &OsCommand) -> io::Result<Ran> {
        let line = command.described();
        self.ran.borrow_mut().push(line.clone());
        let registered = self.registered.get();
        let answer = |success: bool, stdout: &str| Ran {
            success,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        };
        Ok(match line.as_str() {
            "schtasks /Query /FO CSV /NH" if registered => {
                answer(true, "\"\\Floti host\",\"N/A\",\"Ready\"\r\n")
            }
            "id -u" => answer(true, "501\n"),
            "launchctl print gui/501/dev.floti.host" => answer(registered, ""),
            "loginctl show-user 501 --property=Linger --value" => answer(true, "yes\n"),
            taken_out
                if taken_out.starts_with("schtasks /Delete")
                    || taken_out.starts_with("launchctl bootout")
                    || taken_out.starts_with("systemctl --user disable") =>
            {
                self.registered.set(false);
                answer(true, "")
            }
            _ => answer(true, ""),
        })
    }

    fn desktop_session(&self) -> io::Result<bool> {
        Ok(true)
    }
}

/// Everything uninstalling asks of the computer, recorded.
struct FakeSystem {
    inside_session: bool,
    desktop_running: Cell<bool>,
    hosts: RefCell<VecDeque<HostStop>>,
    on_path: Cell<bool>,
    path_refused: Option<String>,
    uninstaller_fails: bool,
    /// Why deleting after exit cannot start, when it cannot.
    deferring_refused: Option<String>,
    /// Where what deletes after exit runs once started.
    deferring_where: Outliving,
    secret_entry: Cell<bool>,
    /// Why the secret store refuses, when it does.
    secret_refused: Option<String>,
    /// The processes still running; every other has ended.
    alive: Vec<u32>,
    os: FakeOs,
    closed: RefCell<Vec<PathBuf>>,
    stops: Cell<usize>,
    uninstallers: RefCell<Vec<(PathBuf, String)>>,
    path_asked: RefCell<Vec<PathBuf>>,
    deferred: RefCell<Vec<(Vec<PathBuf>, Vec<PathBuf>)>>,
    secrets_asked: RefCell<Vec<StoreEntry>>,
}

impl Default for FakeSystem {
    fn default() -> Self {
        Self {
            inside_session: false,
            desktop_running: Cell::new(false),
            hosts: RefCell::default(),
            on_path: Cell::new(false),
            path_refused: None,
            uninstaller_fails: false,
            deferring_refused: None,
            deferring_where: Outliving::Apart,
            secret_entry: Cell::new(false),
            secret_refused: None,
            alive: Vec::new(),
            os: FakeOs {
                registered: Cell::new(false),
                ran: RefCell::default(),
            },
            closed: RefCell::default(),
            stops: Cell::new(0),
            uninstallers: RefCell::default(),
            path_asked: RefCell::default(),
            deferred: RefCell::default(),
            secrets_asked: RefCell::default(),
        }
    }
}

impl UninstallSystem for FakeSystem {
    fn inside_session(&self) -> bool {
        self.inside_session
    }

    fn close_desktop(&self, desktop: &Desktop) -> Result<Closed, String> {
        self.closed.borrow_mut().push(desktop.path.clone());
        Ok(Closed {
            ended: usize::from(self.desktop_running.replace(false)),
            skipped: 0,
        })
    }

    fn stop_host(&self) -> HostStop {
        self.stops.set(self.stops.get() + 1);
        self.hosts
            .borrow_mut()
            .pop_front()
            .unwrap_or(HostStop::NotRunning)
    }

    fn sign_in(&self) -> &dyn SignInSystem {
        &self.os
    }

    /// Does what the NSIS uninstaller does when run in place: removes
    /// everything in the install folder `_?=` names but itself.
    fn run_uninstaller(&self, program: &Path, line: &str) -> Result<(), String> {
        self.uninstallers
            .borrow_mut()
            .push((program.to_path_buf(), line.to_owned()));
        if self.uninstaller_fails {
            return Err("it ended with exit code: 2".to_owned());
        }
        let folder = line
            .strip_prefix("/S _?=")
            .ok_or_else(|| format!("run with {line}"))?;
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path != program {
                floti_host::platform::links::remove_tree(&NativeLinks, &path).unwrap();
            }
        }
        Ok(())
    }

    fn remove_from_path(&self, folder: &Path) -> Result<bool, String> {
        self.path_asked.borrow_mut().push(folder.to_path_buf());
        if let Some(why) = &self.path_refused {
            return Err(why.clone());
        }
        Ok(self.on_path.replace(false))
    }

    fn delete_after_exit(
        &self,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> Result<Outliving, String> {
        if let Some(why) = &self.deferring_refused {
            return Err(why.clone());
        }
        self.deferred
            .borrow_mut()
            .push((files.to_vec(), folders.to_vec()));
        Ok(self.deferring_where)
    }

    fn remove_secret_entry(&self, entry: &StoreEntry) -> Result<bool, String> {
        self.secrets_asked.borrow_mut().push(entry.clone());
        if let Some(why) = &self.secret_refused {
            return Err(why.clone());
        }
        Ok(self.secret_entry.replace(false))
    }

    fn ended(&self, pid: u32) -> bool {
        !self.alive.contains(&pid)
    }
}

/// A computer Floti was installed on, in the test's tree.
struct World {
    dirs: TestDirs,
    env: UserEnv,
    folders: Folders,
    apps: PathBuf,
    binary: PathBuf,
    definition: StartupDefinition,
}

struct Outcome {
    code: u8,
    out: String,
    err: String,
}

impl Outcome {
    fn lines(&self) -> Vec<&str> {
        self.out.lines().collect()
    }
}

impl World {
    /// A computer with nothing of Floti's on it yet but its test folders.
    fn new() -> Self {
        let dirs = TestDirs::new();
        let apps = dirs.folder("apps dir");
        let mut env = dirs.user_env();
        env.vars.insert(
            "LOCALAPPDATA".into(),
            dirs.folder("Local App Data").into_os_string(),
        );
        env.vars
            .insert(DESKTOP_DIR_VAR.into(), apps.clone().into_os_string());
        let binary = NativeStandalone.binary(&env).expect("floti's place");
        let definition = startup::Native
            .definition(&binary)
            .expect("this OS's definition");
        let folders = dirs.folders();
        Self {
            dirs,
            env,
            folders,
            apps,
            binary,
            definition,
        }
    }

    /// What the install script, setup and `floti add desktop` put there,
    /// with the fake system saying the host runs two sessions and floti's
    /// folder is on the user's PATH.
    fn installed() -> (Self, FakeSystem) {
        let world = Self::new();
        let folder = world.folder();
        std::fs::create_dir_all(folder.join("node").join("bin")).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_floti-fake-program"), &world.binary).unwrap();
        std::fs::write(folder.join("node").join("bin").join("node"), b"node").unwrap();
        std::fs::write(folder.join("node").join("LICENSE"), b"Node's licence").unwrap();
        let name = world.binary.file_name().unwrap().to_string_lossy();
        std::fs::write(folder.join(format!(".{name}.floti-install-99")), b"half").unwrap();
        // What an install script left when it stopped part way: its
        // download, whose process has ended, and install.ps1's copy aside.
        let extension = world.extension();
        std::fs::write(
            folder.join(format!(".floti-3.0.0.4120{extension}")),
            b"half",
        )
        .unwrap();
        std::fs::write(folder.join(format!("floti.old{extension}")), b"old").unwrap();

        NativeInstalls.fake_install(&world.apps, "3.0.0");
        let desktop = NativeInstalls
            .desktop(&world.env)
            .expect("the fake install");
        NativeInstalls.add_launcher(&world.env, &desktop).unwrap();

        let entry = world.entry_file();
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, world.definition.file_bytes()).unwrap();

        std::fs::write(world.folders.data.join("floti.db"), b"history").unwrap();
        std::fs::write(world.folders.logs.join("host.log"), b"log").unwrap();
        std::fs::write(world.folders.config.join("settings.json"), b"{}").unwrap();

        let system = FakeSystem::default();
        system.desktop_running.set(true);
        system
            .hosts
            .borrow_mut()
            .push_back(HostStop::Stopped { sessions: Some(2) });
        system.on_path.set(true);
        system.secret_entry.set(true);
        system.os.registered.set(true);
        (world, system)
    }

    fn folder(&self) -> PathBuf {
        self.binary.parent().unwrap().to_path_buf()
    }

    /// floti's extension with its dot (`.exe`), or nothing.
    fn extension(&self) -> String {
        self.binary
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default()
    }

    fn entry_file(&self) -> PathBuf {
        definition_file(&self.definition, &self.dirs.home, &self.folders.data)
    }

    fn desktop(&self) -> Option<Desktop> {
        NativeInstalls.desktop(&self.env)
    }

    fn run(&self, system: &FakeSystem, purge: bool) -> Outcome {
        self.run_as(&self.binary, &NativeInstalls, system, purge)
    }

    fn run_as(
        &self,
        running: &Path,
        installs: &dyn Installs,
        system: &FakeSystem,
        purge: bool,
    ) -> Outcome {
        self.run_with(
            running,
            &NativeStandalone,
            installs,
            system,
            purge,
            Some(self.folders.clone()),
        )
    }

    /// Runs with `layout` as the folders floti uses with no override.
    fn run_with(
        &self,
        running: &Path,
        place: &dyn Standalone,
        installs: &dyn Installs,
        system: &FakeSystem,
        purge: bool,
        layout: Option<Folders>,
    ) -> Outcome {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let what = Uninstall {
            env: &self.env,
            folders: &self.folders,
            layout,
            running,
            definition: &self.definition,
            secret_entry: StoreEntry::for_moved_data_folder(&self.folders.data),
            purge,
        };
        let code = uninstall(&what, place, installs, system, &mut out, &mut err);
        Outcome {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// The lines a full run prints from the sign-in entry to the PATH entry,
    /// after the host's: what this OS's install put there, removed.
    fn removed_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "sign-in entry: removed ({})",
            entry_name(&self.definition)
        )];
        if self.definition.kind == StartupKind::SystemdUserUnit {
            lines.push(LINGER_LINE.to_owned());
        }
        lines.push(format!(
            "desktop app: removed ({})",
            NativeInstalls
                .desktop_target(&self.env)
                .unwrap()
                .path
                .display()
        ));
        lines.push(DESKTOP_SETTINGS_LINE.to_owned());
        lines.push(format!("PATH entry: removed ({})", self.folder().display()));
        lines
    }

    fn floti_removed_line(&self) -> String {
        format!("floti: removed ({})", self.binary.display())
    }

    fn kept_lines(&self) -> Vec<String> {
        [
            ("data", &self.folders.data),
            ("logs", &self.folders.logs),
            ("config", &self.folders.config),
        ]
        .iter()
        .map(|(item, folder)| {
            format!(
                "{item}: kept ({}); floti uninstall --purge removes it",
                folder.display()
            )
        })
        .collect()
    }
}

/// Every file under `folder`, by its path from there.
fn files(folder: &Path) -> Vec<PathBuf> {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, &mut found);
    found.sort();
    found
}

#[test]
fn a_first_run_removes_what_install_put_there_in_order_and_keeps_the_data() {
    let (world, system) = World::installed();

    let ran = world.run(&system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.err.is_empty(), "{}", ran.err);
    let mut wanted = vec![
        "desktop app: closed, so it starts no host".to_owned(),
        "host: stopped, ending 2 sessions".to_owned(),
    ];
    wanted.extend(world.removed_lines());
    wanted.extend(world.kept_lines());
    wanted.push(world.floti_removed_line());
    assert_eq!(ran.lines(), wanted);

    // What install put there is gone.
    assert!(!world.binary.exists());
    assert!(
        files(&world.folder()).is_empty(),
        "{:?}",
        files(&world.folder())
    );
    assert!(world.desktop().is_none());
    assert!(
        NativeInstalls.removal(&world.env).is_none(),
        "nor its menu entry"
    );
    assert!(!world.entry_file().exists());
    assert!(!system.on_path.get());
    assert_eq!(*system.path_asked.borrow(), [world.folder()]);
    assert!(system.deferred.borrow().is_empty(), "nothing ran from it");
    // The data stays, and the secret store is not asked.
    assert!(world.folders.data.join("floti.db").exists());
    assert!(world.folders.config.join("settings.json").exists());
    assert!(system.secrets_asked.borrow().is_empty());
}

#[test]
fn a_second_run_finds_nothing_and_exits_0() {
    let (world, system) = World::installed();
    assert_eq!(world.run(&system, false).code, 0);
    let commands = system.os.ran.borrow().len();
    let uninstalls = system.uninstallers.borrow().len();

    let again = world.run(&system, false);
    assert_eq!(again.code, 0, "{}", again.err);
    let mut wanted = vec![
        "host: not running".to_owned(),
        "sign-in entry: not found".to_owned(),
    ];
    if world.definition.kind == StartupKind::SystemdUserUnit {
        wanted.push(LINGER_LINE.to_owned());
    }
    wanted.extend(["desktop app: not found", "PATH entry: not found"].map(str::to_owned));
    wanted.extend(world.kept_lines());
    wanted.push("floti: not found".to_owned());
    assert_eq!(again.lines(), wanted);
    // Nothing is taken out that is not there: no delete, bootout or disable.
    let taken_out = system.os.ran.borrow()[commands..]
        .iter()
        .filter(|line| {
            line.contains("/Delete") || line.contains("bootout") || line.contains("disable")
        })
        .count();
    assert_eq!(taken_out, 0, "{:?}", system.os.ran.borrow());
    assert_eq!(
        system.uninstallers.borrow().len(),
        uninstalls,
        "no uninstaller ran"
    );
    assert!(system.deferred.borrow().is_empty());
}

#[test]
fn purge_removes_the_data_config_and_secret_entry_and_never_walks_into_a_link() {
    let (world, system) = World::installed();
    // A launch folder's link into a repository, as an overlay makes.
    let repository = world.dirs.folder("a repository");
    std::fs::write(repository.join("README.md"), b"the user's work").unwrap();
    let launch = world.folders.data.join("launch").join("session 1");
    std::fs::create_dir_all(&launch).unwrap();
    NativeLinks
        .link_folder(&repository, &launch.join("skills"))
        .unwrap();

    let ran = world.run(&system, true);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let lines = ran.lines();
    // The test's logs and socket folders are apart from its data, as on
    // Linux; elsewhere they are inside it and go with it.
    let purged: Vec<String> = [
        ("data", &world.folders.data),
        ("logs", &world.folders.logs),
        ("runtime", &world.folders.runtime),
        ("config", &world.folders.config),
    ]
    .iter()
    .map(|(item, folder)| format!("{item}: removed ({})", folder.display()))
    .chain(["secret store: removed Floti's key".to_owned()])
    // The data goes before floti, so a purge that fails leaves floti to
    // try again with.
    .chain([world.floti_removed_line()])
    .collect();
    assert_eq!(lines[lines.len() - 6..], purged);
    assert_eq!(
        lines
            .iter()
            .filter(|line| **line == DESKTOP_SETTINGS_LINE)
            .count(),
        1,
        "the app's settings are said to stay once: {lines:?}"
    );
    assert!(!world.folders.data.exists());
    assert!(!world.folders.logs.exists());
    assert!(!world.folders.runtime.exists());
    assert!(!world.folders.config.exists());
    assert_eq!(
        std::fs::read(repository.join("README.md")).unwrap(),
        b"the user's work",
        "the repository behind the link is untouched"
    );
    assert_eq!(
        *system.secrets_asked.borrow(),
        [StoreEntry::for_moved_data_folder(&world.folders.data)]
    );

    let again = world.run(&system, true);
    assert_eq!(again.code, 0, "{}", again.err);
    let lines = again.lines();
    assert_eq!(
        lines[lines.len() - 6..],
        [
            "data: not found",
            "logs: not found",
            "runtime: not found",
            "config: not found",
            "secret store: not found",
            "floti: not found"
        ]
    );
    // The app is gone already; its settings are still said to stay.
    assert!(lines.contains(&DESKTOP_SETTINGS_LINE), "{lines:?}");
}

#[test]
fn a_purge_of_an_overrides_folder_with_no_mark_of_flotis_changes_nothing() {
    let (world, system) = World::installed();
    // The data folder an override named holds only the user's own files.
    std::fs::remove_file(world.folders.data.join("floti.db")).unwrap();
    std::fs::write(world.folders.data.join("notes.txt"), b"the user's").unwrap();

    let ran = world.run_with(
        &world.binary,
        &NativeStandalone,
        &NativeInstalls,
        &system,
        true,
        None,
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.starts_with(&format!(
            "floti uninstall: data: {} is not the folder floti uses",
            world.folders.data.display()
        )),
        "{}",
        ran.err
    );
    assert!(ran.err.contains("Nothing was changed"), "{}", ran.err);
    assert!(ran.out.is_empty(), "{}", ran.out);
    assert_eq!(system.stops.get(), 0, "the host was not stopped");
    assert!(world.folders.data.join("notes.txt").exists());
    assert!(world.binary.exists() && world.entry_file().exists());
    assert!(world.desktop().is_some());
}

#[test]
fn a_purge_that_fails_leaves_floti_to_run_it_again() {
    let (world, mut system) = World::installed();
    // The secret store refuses, after the folders went.
    system.secret_refused = Some("the Keychain is locked".to_owned());

    let ran = world.run(&system, true);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        "floti uninstall: secret store: the Keychain is locked\n"
    );
    assert!(world.binary.exists(), "floti is there to try again with");
    assert!(!ran.out.contains("floti: "), "{}", ran.out);
}

/// The fake program, running from `path` until the test ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_running_floti_frees_its_name_at_once_and_what_is_left_of_it_goes_after_it_exits() {
    let (world, system) = World::installed();
    // floti uninstall running from the installed floti itself.
    let mut running = Running(
        Command::new(&world.binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let ran = world.run(&system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(!world.binary.exists(), "its name is free");
    assert!(running.0.try_wait().unwrap().is_none(), "it still runs");
    let floti_line = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with("floti: "))
        .unwrap()
        .to_owned();
    assert!(
        floti_line.starts_with(&format!("floti: removed ({})", world.binary.display())),
        "{floti_line}"
    );

    // Whatever is left in its folder is what is deleted once it has exited
    // (on Windows the copy it runs from; on macOS and Linux nothing).
    let left = files(&world.folder());
    let deferred = system.deferred.borrow().clone();
    if left.is_empty() {
        assert!(deferred.is_empty(), "{deferred:?}");
        assert!(!floti_line.contains("once it exits"), "{floti_line}");
    } else {
        assert_eq!(deferred.len(), 1, "{deferred:?}");
        let (files_after, folders_after) = &deferred[0];
        assert_eq!(*files_after, left);
        assert_eq!(*folders_after, NativeStandalone.own_folders(&world.env));
        assert!(floti_line.ends_with("once it exits"), "{floti_line}");
    }

    // It exits; what was deferred goes, and nothing of floti is left.
    drop(running.0.stdin.take());
    running.0.wait().unwrap();
    for (files_after, _) in &deferred {
        for file in files_after {
            std::fs::remove_file(file).unwrap();
        }
    }
    assert!(files(&world.folder()).is_empty());
}

#[test]
fn a_step_that_fails_stops_the_run_there_and_the_next_run_carries_on() {
    let (world, mut system) = World::installed();
    system.path_refused = Some("the registry refused".to_owned());

    let ran = world.run(&system, false);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        format!(
            "floti uninstall: PATH entry: could not take {} off your PATH: the registry refused\n",
            world.folder().display()
        )
    );
    // What comes before it is gone; floti, after it, still works.
    assert!(!world.entry_file().exists());
    assert!(world.desktop().is_none());
    assert!(world.binary.exists());
    assert!(world.folder().join("node").join("LICENSE").exists());
    assert!(!ran.out.contains("floti: "), "{}", ran.out);
    assert!(!ran.out.contains("data: "), "{}", ran.out);

    system.path_refused = None;
    let again = world.run(&system, false);
    assert_eq!(again.code, 0, "{}", again.err);
    let lines = again.lines();
    assert!(lines.contains(&"sign-in entry: not found"), "{lines:?}");
    assert!(lines.contains(&"desktop app: not found"), "{lines:?}");
    assert!(
        lines.contains(&format!("PATH entry: removed ({})", world.folder().display()).as_str()),
        "{lines:?}"
    );
    assert!(!world.binary.exists());
}

#[test]
fn a_host_that_cannot_be_stopped_leaves_everything_as_it_was() {
    let (world, system) = World::installed();
    system.hosts.borrow_mut().clear();
    system
        .hosts
        .borrow_mut()
        .push_back(HostStop::Failed("the host answered nothing".to_owned()));

    let ran = world.run(&system, true);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        "floti uninstall: host: could not stop it: the host answered nothing. Nothing was removed.\n"
    );
    assert!(world.entry_file().exists());
    assert!(world.desktop().is_some());
    assert!(world.binary.exists());
    assert!(system.on_path.get());
    assert!(world.folders.data.join("floti.db").exists());
}

#[test]
fn inside_one_of_floti_s_own_sessions_it_changes_nothing() {
    let (world, mut system) = World::installed();
    system.inside_session = true;

    let ran = world.run(&system, true);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.err, format!("floti uninstall: {IN_SESSION_LINE}\n"));
    assert!(ran.out.is_empty(), "{}", ran.out);
    assert_eq!(system.stops.get(), 0, "the host was not stopped");
    assert!(system.closed.borrow().is_empty(), "the app was not closed");
    assert!(world.entry_file().exists() && world.binary.exists());
}

#[test]
fn a_floti_from_elsewhere_removes_what_it_installed_and_leaves_floti_and_the_path() {
    let (world, system) = World::installed();
    let elsewhere = world.dirs.folder("Cellar").join("floti");
    std::fs::write(&elsewhere, b"a package's floti").unwrap();

    let ran = world.run_as(&elsewhere, &NativeInstalls, &system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let floti_line = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with("floti: "))
        .unwrap()
        .to_owned();
    assert!(
        floti_line.starts_with(&format!(
            "floti: left as it is (installed by {})",
            elsewhere.display()
        )),
        "{floti_line}"
    );
    assert!(
        !ran.lines()
            .iter()
            .any(|line| line.starts_with("PATH entry:")),
        "{}",
        ran.out
    );
    assert!(system.path_asked.borrow().is_empty());
    assert!(world.binary.exists() && elsewhere.exists());
    // What it did install goes.
    assert!(!world.entry_file().exists());
    assert!(world.desktop().is_none());
}

#[test]
fn a_download_whose_process_still_runs_is_left_to_it() {
    let (world, mut system) = World::installed();
    let download = world
        .folder()
        .join(format!(".floti-3.1.0.77{}", world.extension()));
    std::fs::write(&download, b"being written").unwrap();
    system.alive = vec![77];

    let ran = world.run(&system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(download.exists(), "another floti is still writing it");
    assert_eq!(files(&world.folder()), [download]);
    assert!(!world.binary.exists());
}

#[test]
fn a_package_managers_floti_with_none_installed_on_its_own_leaves_the_folder_as_it_is() {
    let (world, system) = World::installed();
    std::fs::remove_file(&world.binary).unwrap();
    // A `node` folder of the user's own where floti's would be.
    assert!(world.folder().join("node").join("LICENSE").exists());
    let elsewhere = world.dirs.folder("Cellar").join("floti");
    std::fs::write(&elsewhere, b"a package's floti").unwrap();

    let ran = world.run_as(&elsewhere, &NativeInstalls, &system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let floti_line = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with("floti: "))
        .unwrap()
        .to_owned();
    assert!(
        floti_line.starts_with(&format!(
            "floti: left as it is (installed by {})",
            elsewhere.display()
        )),
        "{floti_line}"
    );
    assert!(world.folder().join("node").join("LICENSE").exists());
    assert!(system.path_asked.borrow().is_empty());
}

/// floti's place as this OS has it, but a floti there is always running:
/// it is renamed aside, as on Windows, on every OS.
struct HeldPlace;

impl Standalone for HeldPlace {
    fn folder(&self, env: &UserEnv) -> Option<PathBuf> {
        NativeStandalone.folder(env)
    }

    fn binary(&self, env: &UserEnv) -> Option<PathBuf> {
        NativeStandalone.binary(env)
    }

    fn replace(&self, target: &Path, new: &Path) -> io::Result<()> {
        NativeStandalone.replace(target, new)
    }

    fn remove_asides(&self, target: &Path) -> Vec<PathBuf> {
        NativeStandalone.remove_asides(target)
    }

    fn native_cpu(&self) -> &'static str {
        NativeStandalone.native_cpu()
    }

    fn own_folders(&self, env: &UserEnv) -> Vec<PathBuf> {
        NativeStandalone.own_folders(env)
    }

    fn remove_running(&self, target: &Path) -> io::Result<Option<PathBuf>> {
        let aside = target.with_file_name("floti.old-1-0");
        std::fs::rename(target, &aside)?;
        Ok(Some(aside))
    }

    fn delete_after_exit(
        &self,
        env: &UserEnv,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> io::Result<Outliving> {
        NativeStandalone.delete_after_exit(env, files, folders)
    }
}

#[test]
fn deleting_after_exit_that_cannot_start_leaves_the_copy_for_the_next_sweep_and_succeeds() {
    let (world, mut system) = World::installed();
    system.deferring_refused = Some("the job refused to let it go".to_owned());

    let ran = world.run_with(
        &world.binary,
        &HeldPlace,
        &NativeInstalls,
        &system,
        false,
        Some(world.folders.clone()),
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let aside = world.folder().join("floti.old-1-0");
    let floti_line = ran.lines().last().unwrap().to_owned();
    assert_eq!(
        floti_line,
        format!(
            "{}; {} is left for the next floti install or update to remove, since what would delete it once this floti exits could not start (the job refused to let it go)",
            world.floti_removed_line(),
            aside.display()
        )
    );
    assert!(!world.binary.exists());
    assert!(aside.exists());
}

#[test]
fn deleting_after_exit_inside_a_job_that_ends_what_it_holds_says_the_job_may_end_it_first() {
    for (ends_with_job, unless) in [(false, ""), (true, JOB_ENDS_IT_FIRST)] {
        let (world, mut system) = World::installed();
        system.deferring_where = Outliving::InJob { ends_with_job };

        let ran = world.run_with(
            &world.binary,
            &HeldPlace,
            &NativeInstalls,
            &system,
            false,
            Some(world.folders.clone()),
        );
        assert_eq!(ran.code, 0, "{}", ran.err);
        assert_eq!(
            ran.lines().last().unwrap().to_owned(),
            format!(
                "{}; the copy this process runs from goes once it exits{unless}",
                world.floti_removed_line()
            ),
            "ends_with_job {ends_with_job}"
        );
        assert_eq!(system.deferred.borrow().len(), 1);
    }
}

#[test]
fn anything_but_purge_is_a_usage_error_before_anything_is_looked_at() {
    // Safe to run: the arguments are refused before uninstalling begins.
    let output = Command::new(env!("CARGO_BIN_EXE_floti"))
        .args(["uninstall", "--yes"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("floti uninstall [--purge]"), "{said}");
}

/// A desktop install with an uninstaller, as on Windows, on every OS: the
/// app's program and its uninstaller in `folder`.
struct WithUninstaller {
    folder: PathBuf,
}

impl WithUninstaller {
    fn program(&self) -> PathBuf {
        self.folder.join("App.exe")
    }

    fn uninstaller(&self) -> PathBuf {
        self.folder.join("Uninstall App.exe")
    }
}

impl Installs for WithUninstaller {
    fn bundles(&self, _desktop: &Desktop, _running: &Path) -> bool {
        false
    }

    fn desktop(&self, _env: &UserEnv) -> Option<Desktop> {
        self.program().exists().then(|| Desktop {
            name: "App",
            path: self.folder.clone(),
        })
    }

    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop> {
        self.desktop(env)
    }

    fn desktop_version(&self, _desktop: &Desktop) -> Option<String> {
        None
    }

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        None
    }

    fn is_desktop_process(&self, _desktop: &Desktop, _row: &ProcessRow) -> bool {
        false
    }

    fn install_desktop(&self, _target: &Desktop, _release: &Path) -> floti_host::Result<Desktop> {
        unreachable!("uninstalling installs nothing")
    }

    fn opener(&self, _desktop: &Desktop) -> (PathBuf, Vec<String>) {
        unreachable!("uninstalling opens nothing")
    }

    fn removal(&self, env: &UserEnv) -> Option<DesktopRemoval> {
        self.desktop(env).map(|desktop| DesktopRemoval {
            shown: desktop.path.clone(),
            uninstaller: Some((
                self.uninstaller(),
                format!("/S _?={}", desktop.path.display()),
            )),
            gone_after: Some(self.program()),
            remove: vec![desktop.path],
        })
    }
}

fn with_uninstaller(world: &World) -> WithUninstaller {
    let app = WithUninstaller {
        folder: world.dirs.folder("Programs").join("App"),
    };
    std::fs::create_dir_all(&app.folder).unwrap();
    std::fs::write(app.program(), b"app").unwrap();
    std::fs::write(app.uninstaller(), b"uninstaller").unwrap();
    std::fs::write(app.folder.join("resources.pak"), b"resources").unwrap();
    app
}

#[test]
fn the_apps_own_uninstaller_runs_in_place_then_what_it_leaves_of_its_folder_goes() {
    let (world, system) = World::installed();
    let app = with_uninstaller(&world);

    let ran = world.run_as(&world.binary, &app, &system, false);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        *system.uninstallers.borrow(),
        [(app.uninstaller(), format!("/S _?={}", app.folder.display()))]
    );
    assert!(
        !app.folder.exists(),
        "the uninstaller and its folder went too"
    );
    assert!(
        ran.lines()
            .contains(&format!("desktop app: removed ({})", app.folder.display()).as_str()),
        "{}",
        ran.out
    );
}

#[test]
fn an_uninstaller_that_fails_stops_the_run_before_the_path_and_floti() {
    let (world, mut system) = World::installed();
    system.uninstaller_fails = true;
    let app = with_uninstaller(&world);

    let ran = world.run_as(&world.binary, &app, &system, false);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        format!(
            "floti uninstall: desktop app: its uninstaller {} failed: it ended with exit code: 2\n",
            app.uninstaller().display()
        )
    );
    assert!(app.program().exists());
    assert!(world.binary.exists());
    assert!(system.path_asked.borrow().is_empty());
}

#[test]
fn an_uninstaller_that_ends_leaving_the_app_there_is_a_failure() {
    let (world, system) = World::installed();
    let app = with_uninstaller(&world);
    let program = app.program();
    // The uninstaller is told a folder that holds nothing of the app's.
    let lying = LyingUninstaller {
        app,
        told: world.dirs.folder("empty"),
    };

    let ran = world.run_as(&world.binary, &lying, &system, false);
    assert_eq!(ran.code, 1);
    assert_eq!(
        ran.err,
        format!(
            "floti uninstall: desktop app: its uninstaller ended, and {} is still there\n",
            program.display()
        )
    );
    assert!(program.exists());
    assert!(world.binary.exists());
}

/// An app whose uninstaller is told another folder, so it removes nothing
/// of the app's.
struct LyingUninstaller {
    app: WithUninstaller,
    told: PathBuf,
}

impl Installs for LyingUninstaller {
    fn bundles(&self, desktop: &Desktop, running: &Path) -> bool {
        self.app.bundles(desktop, running)
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        self.app.desktop(env)
    }

    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop> {
        self.app.desktop_target(env)
    }

    fn desktop_version(&self, desktop: &Desktop) -> Option<String> {
        self.app.desktop_version(desktop)
    }

    fn carried_floti(&self, desktop: &Desktop) -> Option<PathBuf> {
        self.app.carried_floti(desktop)
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        self.app.is_desktop_process(desktop, row)
    }

    fn install_desktop(&self, target: &Desktop, release: &Path) -> floti_host::Result<Desktop> {
        self.app.install_desktop(target, release)
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        self.app.opener(desktop)
    }

    fn removal(&self, env: &UserEnv) -> Option<DesktopRemoval> {
        self.app.removal(env).map(|removal| DesktopRemoval {
            uninstaller: Some((
                self.app.uninstaller(),
                format!("/S _?={}", self.told.display()),
            )),
            ..removal
        })
    }
}
