//! `floti install-self`, against folders in the test's own tree: the floti
//! a desktop carries, with its Node beside it, put where the install
//! scripts put floti, over a copy there that is running, and setup run from
//! the installed one.
//!
//! Nothing here installs anything on this computer, changes its `PATH` or
//! runs setup: every file is under the test's temporary tree, the `PATH`
//! and setup are a [`FakeSystem`] that records what it was asked, and the
//! running copy is the test's fake program, ended with the test.

use std::cell::RefCell;
use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use floti_host::install_self::{InstallSystem, install_self};
use floti_host::platform::standalone::{Native, Standalone};
use floti_host::testkit::TestDirs;

/// What the fake computer was asked, and how it answers.
#[derive(Default)]
struct FakeSystem {
    on_path: bool,
    path_refused: Option<String>,
    setup_code: u8,
    added: RefCell<Vec<PathBuf>>,
    setups: RefCell<Vec<PathBuf>>,
}

impl InstallSystem for FakeSystem {
    fn on_path(&self, _floti: &Path) -> Result<bool, String> {
        Ok(self.on_path || !self.added.borrow().is_empty())
    }

    fn add_to_path(&self, folder: &Path) -> Result<bool, String> {
        if let Some(why) = &self.path_refused {
            return Err(why.clone());
        }
        self.added.borrow_mut().push(folder.to_path_buf());
        Ok(true)
    }

    fn setup(&self, floti: &Path) -> Result<u8, String> {
        self.setups.borrow_mut().push(floti.to_path_buf());
        Ok(self.setup_code)
    }
}

/// A desktop's resources: its floti, and the Node runtime beside it.
struct Carried {
    floti: PathBuf,
}

fn floti_name() -> String {
    format!("floti{EXE_SUFFIX}")
}

fn carried(dirs: &TestDirs, version: &[u8]) -> Carried {
    let resources = dirs
        .root()
        .join("Program Files")
        .join("Helm")
        .join("resources");
    let node = resources.join("node");
    std::fs::create_dir_all(node.join("bin")).unwrap();
    std::fs::write(node.join("bin").join("node"), b"node runtime").unwrap();
    std::fs::write(node.join("LICENSE"), b"Node's licence").unwrap();
    let floti = resources.join(floti_name());
    std::fs::write(&floti, version).unwrap();
    Carried { floti }
}

struct Ran {
    code: u8,
    out: String,
    err: String,
}

fn run(source: &Path, folder: &Path, system: &FakeSystem) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = install_self(source, folder, &Native, system, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Every file under `folder`, by its path from there with `/` between parts.
fn files(folder: &Path) -> Vec<String> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let parts: Vec<String> = path
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(parts.join("/"));
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, &mut found);
    found.sort();
    found
}

/// The installed folder's files when it holds floti and its Node only.
fn installed_files() -> Vec<String> {
    let mut wanted = vec![
        floti_name(),
        "node/LICENSE".to_owned(),
        "node/bin/node".to_owned(),
    ];
    wanted.sort();
    wanted
}

#[test]
fn a_first_install_puts_floti_and_its_node_in_the_users_folder_on_path_and_runs_setup_there() {
    let dirs = TestDirs::new();
    let source = carried(&dirs, b"floti 3.0.0");
    let folder = Native.folder(&dirs.user_env()).expect("the user's folder");
    let system = FakeSystem::default();

    let ran = run(&source.floti, &folder, &system);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let floti = folder.join(floti_name());
    assert_eq!(std::fs::read(&floti).unwrap(), b"floti 3.0.0");
    assert_eq!(files(&folder), installed_files());
    assert_eq!(*system.added.borrow(), std::slice::from_ref(&folder));
    assert_eq!(*system.setups.borrow(), std::slice::from_ref(&floti));
    assert!(
        ran.out
            .starts_with(&format!("floti is at {}.", floti.display())),
        "{}",
        ran.out
    );
    assert!(ran.out.contains("on your PATH"), "{}", ran.out);
    assert!(ran.err.is_empty(), "{}", ran.err);
}

#[test]
fn installing_the_same_floti_again_changes_no_file_and_still_runs_setup() {
    let dirs = TestDirs::new();
    let source = carried(&dirs, b"floti 3.0.0");
    let folder = dirs.folder("bin");
    let system = FakeSystem {
        on_path: true,
        ..FakeSystem::default()
    };
    assert_eq!(run(&source.floti, &folder, &system).code, 0);
    let floti = folder.join(floti_name());
    let before = std::fs::metadata(&floti).unwrap().modified().unwrap();

    let ran = run(&source.floti, &folder, &system);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        std::fs::metadata(&floti).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(files(&folder), installed_files());
    assert!(system.added.borrow().is_empty(), "already on PATH");
    assert_eq!(system.setups.borrow().len(), 2);
    // Run from the installed copy itself, it has nothing to copy.
    assert_eq!(run(&floti, &folder, &system).code, 0);
    assert_eq!(files(&folder), installed_files());
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
fn a_new_floti_takes_the_place_of_a_running_one_which_keeps_running_and_goes_once_it_has_ended() {
    let dirs = TestDirs::new();
    let folder = dirs.folder("bin");
    let installed = folder.join(floti_name());
    // A program that runs until its standard input closes stands in for the
    // host running from the installed floti.
    std::fs::copy(env!("CARGO_BIN_EXE_floti-fake-program"), &installed).unwrap();
    let mut running = Running(
        Command::new(&installed)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let system = FakeSystem {
        on_path: true,
        ..FakeSystem::default()
    };

    let source = carried(&dirs, b"floti 3.1.0");
    let ran = run(&source.floti, &folder, &system);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(std::fs::read(&installed).unwrap(), b"floti 3.1.0");
    assert!(
        running.0.try_wait().unwrap().is_none(),
        "the running copy was not ended"
    );

    // It ends; the next install leaves nothing of it.
    drop(running.0.stdin.take());
    running.0.wait().unwrap();
    assert_eq!(run(&source.floti, &folder, &system).code, 0);
    assert_eq!(files(&folder), installed_files());
}

#[test]
fn a_node_there_that_is_not_a_folder_is_left_as_it_is_and_said() {
    let dirs = TestDirs::new();
    let source = carried(&dirs, b"floti 3.0.0");
    let folder = dirs.folder("bin");
    std::fs::write(folder.join("node"), b"the user's own node").unwrap();
    let system = FakeSystem {
        on_path: true,
        ..FakeSystem::default()
    };

    let ran = run(&source.floti, &folder, &system);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        std::fs::read(folder.join("node")).unwrap(),
        b"the user's own node"
    );
    assert!(ran.out.contains("Left "), "{}", ran.out);
    assert_eq!(system.setups.borrow().len(), 1);
}

#[test]
fn setups_status_is_the_status_and_a_refused_path_is_a_failure_after_setup_ran() {
    let dirs = TestDirs::new();
    let source = carried(&dirs, b"floti 3.0.0");
    let folder = dirs.folder("bin");

    let at_next_sign_in = FakeSystem {
        on_path: true,
        setup_code: 4,
        ..FakeSystem::default()
    };
    assert_eq!(run(&source.floti, &folder, &at_next_sign_in).code, 4);

    let refused = FakeSystem {
        path_refused: Some("access is denied".to_owned()),
        ..FakeSystem::default()
    };
    let ran = run(&source.floti, &folder, &refused);
    assert_eq!(ran.code, 1);
    assert_eq!(refused.setups.borrow().len(), 1, "setup still ran");
    assert!(
        ran.err.starts_with("floti install-self: could not put ")
            && ran.err.contains("access is denied"),
        "{}",
        ran.err
    );
}

#[test]
fn a_floti_that_cannot_be_placed_stops_before_setup() {
    let dirs = TestDirs::new();
    let source = carried(&dirs, b"floti 3.0.0");
    // A file where the folder should be.
    let folder = dirs.root().join("not a folder");
    std::fs::write(&folder, b"in the way").unwrap();
    let system = FakeSystem::default();

    let ran = run(&source.floti, &folder, &system);
    assert_eq!(ran.code, 1);
    assert!(ran.err.starts_with("floti install-self: "), "{}", ran.err);
    assert!(system.setups.borrow().is_empty());
    assert!(system.added.borrow().is_empty());
}
