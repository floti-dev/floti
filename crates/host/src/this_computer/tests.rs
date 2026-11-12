use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;
use crate::agent::Version;
use crate::platform::startup::register::{OsCommand, Ran};
use crate::platform::startup::{linux, macos, windows};
use crate::testkit::TestDirs;

const RANGE: TestedVersions = TestedVersions {
    min: Version(2, 1, 277),
    below: Version(2, 2, 0),
};

/// A `claude` found at `found`, answering its version with `says`; what it
/// was asked is kept.
struct FakeClaude {
    found: Option<PathBuf>,
    says: Option<&'static str>,
    asked: Mutex<Vec<Option<PathBuf>>>,
}

impl FakeClaude {
    fn new(found: Option<&str>, says: Option<&'static str>) -> Self {
        Self {
            found: found.map(PathBuf::from),
            says,
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl ExecutableSetup for FakeClaude {
    fn discovered(&self) -> Option<PathBuf> {
        self.found.clone()
    }

    fn version(&self, chosen: Option<&Path>) -> Option<String> {
        self.asked
            .lock()
            .unwrap()
            .push(chosen.map(Path::to_path_buf));
        self.says.map(str::to_owned)
    }

    fn version_command(&self) -> &str {
        "claude --version"
    }

    fn use_executable(&self, _chosen: Option<PathBuf>) {}
}

const INSTALLED: &str = "/home/me/.local/bin/claude";

fn check(fake: &FakeClaude) -> Finding {
    claude(
        fake,
        RANGE,
        &[PathBuf::from("/home/me/.local/bin")],
        ShellFamily::Posix,
    )
}

#[test]
fn claude_in_the_tested_range_is_fine_and_is_asked_for_its_version_where_it_was_found() {
    let fake = FakeClaude::new(Some(INSTALLED), Some("2.1.330 (Claude Code)"));
    assert_eq!(check(&fake), Finding::Fine);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [Some(PathBuf::from(INSTALLED))]
    );
    // The first tested version is in the range.
    let first = FakeClaude::new(Some(INSTALLED), Some("2.1.277 (Claude Code)"));
    assert_eq!(check(&first), Finding::Fine);
}

#[test]
fn claude_not_found_is_a_problem_fixed_by_anthropics_installer_in_the_users_shell() {
    let fake = FakeClaude::new(None, None);
    assert_eq!(
        check(&fake),
        Finding::Problem {
            problem:
                "Claude Code is not installed: there is no claude on PATH or in /home/me/.local/bin"
                    .to_owned(),
            fix: "curl -fsSL https://claude.ai/install.sh | bash".to_owned(),
        }
    );
    assert!(fake.asked.lock().unwrap().is_empty(), "nothing to ask");
    let on_windows = claude(&fake, RANGE, &[], ShellFamily::PowerShell);
    assert_eq!(
        on_windows,
        Finding::Problem {
            problem: "Claude Code is not installed: there is no claude on PATH".to_owned(),
            fix: "irm https://claude.ai/install.ps1 | iex".to_owned(),
        }
    );
}

#[test]
fn claude_that_does_not_answer_its_version_is_a_problem_fixed_by_installing_it_again() {
    let fake = FakeClaude::new(Some(INSTALLED), None);
    assert_eq!(
        check(&fake),
        Finding::Problem {
            problem: format!("Claude Code at {INSTALLED} did not answer `claude --version`"),
            fix: "curl -fsSL https://claude.ai/install.sh | bash".to_owned(),
        }
    );
}

#[test]
fn claude_older_than_the_range_is_fixed_by_updating_it_and_newer_by_installing_the_first_tested() {
    let older = FakeClaude::new(Some(INSTALLED), Some("2.0.14 (Claude Code)"));
    assert_eq!(
        check(&older),
        Finding::Problem {
            problem: "Claude Code 2.0.14 is older than Floti was tested with 2.1.277 up to but not including 2.2.0".to_owned(),
            fix: "claude update".to_owned(),
        }
    );
    let newer = FakeClaude::new(Some(INSTALLED), Some("2.2.0 (Claude Code)"));
    assert_eq!(
        check(&newer),
        Finding::Problem {
            problem: "Claude Code 2.2.0 is newer than Floti was tested with 2.1.277 up to but not including 2.2.0, though it still launches".to_owned(),
            fix: "claude install 2.1.277".to_owned(),
        }
    );
}

#[test]
fn claude_whose_answer_names_no_version_is_could_not_tell_never_a_problem() {
    let fake = FakeClaude::new(Some(INSTALLED), Some("Claude Code"));
    let found = check(&fake);
    assert!(
        matches!(&found, Finding::CouldNotTell { what, why }
            if what == "which version of Claude Code is installed" && why.contains("\"Claude Code\"")),
        "{found:?}"
    );
    assert!(!found.is_problem());
}

#[test]
fn claude_here_without_a_home_folder_could_not_tell() {
    let env = UserEnv {
        home: None,
        vars: Default::default(),
    };
    assert!(matches!(
        claude_here(&env, ShellFamily::Posix),
        Finding::CouldNotTell { .. }
    ));
}

#[test]
fn setup_says_a_problem_in_one_sentence_naming_what_to_run_and_nothing_when_fine() {
    let problem = Finding::Problem {
        problem: "claude is old".to_owned(),
        fix: "claude update".to_owned(),
    };
    assert_eq!(
        problem.sentence().as_deref(),
        Some("Claude is old; to fix it, run: claude update")
    );
    assert_eq!(Finding::Fine.sentence(), None);
    let unknown = Finding::CouldNotTell {
        what: "whether it is".to_owned(),
        why: "no home".to_owned(),
    };
    assert_eq!(
        unknown.sentence().as_deref(),
        Some("Could not tell whether it is: no home")
    );
}

// ---------------------------------------------------------------------------
// The start-at-sign-in entry
// ---------------------------------------------------------------------------

/// An OS that answers each command with what `answer` gives, and records
/// them.
struct FakeOs {
    answer: fn(&str) -> io::Result<Ran>,
    wsl: bool,
    runs: RefCell<Vec<String>>,
}

impl FakeOs {
    fn new(answer: fn(&str) -> io::Result<Ran>) -> Self {
        Self {
            answer,
            wsl: false,
            runs: RefCell::new(Vec::new()),
        }
    }
}

impl SignInSystem for FakeOs {
    fn run(&self, command: &OsCommand) -> io::Result<Ran> {
        let line = command.described();
        self.runs.borrow_mut().push(line.clone());
        (self.answer)(&line)
    }

    fn desktop_session(&self) -> io::Result<bool> {
        panic!("looking asks nobody whether they are signed in")
    }

    fn inside_wsl(&self) -> bool {
        self.wsl
    }
}

fn ran(success: bool, stdout: &str, stderr: &str) -> io::Result<Ran> {
    Ok(Ran {
        success,
        stdout: stdout.to_owned(),
        stderr: stderr.to_owned(),
    })
}

fn no_entry() -> Finding {
    Finding::Problem {
        problem: "the host has no start-at-sign-in entry, so it does not start when you sign in"
            .to_owned(),
        fix: "floti setup".to_owned(),
    }
}

#[test]
fn the_logon_task_is_looked_for_with_schtasks_query_only() {
    let dirs = TestDirs::new();
    let task = windows::logon_task(
        Path::new(r"C:\Users\me\AppData\Local\Programs\Floti\bin\floti.exe"),
        r"BOX\me",
    )
    .unwrap();
    let there = FakeOs::new(|_| ran(true, "Floti host  Ready", ""));
    assert_eq!(
        start_at_sign_in(&there, &task, &dirs.home, &dirs.data),
        Finding::Fine
    );
    assert_eq!(*there.runs.borrow(), ["schtasks /Query /TN Floti host"]);

    let absent = FakeOs::new(|_| {
        ran(
            false,
            "",
            "ERROR: The system cannot find the file specified.",
        )
    });
    assert_eq!(
        start_at_sign_in(&absent, &task, &dirs.home, &dirs.data),
        no_entry()
    );

    let no_schtasks = FakeOs::new(|_| Err(io::Error::from(io::ErrorKind::NotFound)));
    assert!(matches!(
        start_at_sign_in(&no_schtasks, &task, &dirs.home, &dirs.data),
        Finding::CouldNotTell { .. }
    ));
}

#[test]
fn the_launch_agent_is_its_property_list_in_launch_agents_and_runs_no_command() {
    let dirs = TestDirs::new();
    let agent = macos::launch_agent(Path::new("/Users/me/.local/bin/floti")).unwrap();
    let os = FakeOs::new(|line| panic!("looking at a LaunchAgent runs nothing, not {line}"));
    assert_eq!(
        start_at_sign_in(&os, &agent, &dirs.home, &dirs.data),
        no_entry()
    );
    let plist = dirs.home.join("Library/LaunchAgents/dev.floti.host.plist");
    std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
    std::fs::write(&plist, agent.file_bytes()).unwrap();
    assert_eq!(
        start_at_sign_in(&os, &agent, &dirs.home, &dirs.data),
        Finding::Fine
    );
}

#[test]
fn the_user_unit_is_its_file_enabled_by_the_users_service_manager() {
    let dirs = TestDirs::new();
    let unit = linux::user_unit(Path::new("/home/me/.local/bin/floti")).unwrap();
    // No unit file: not there, and systemd is not asked.
    let never = FakeOs::new(|line| panic!("no unit file asks nothing, not {line}"));
    assert_eq!(
        start_at_sign_in(&never, &unit, &dirs.home, &dirs.data),
        no_entry()
    );

    let file = dirs.home.join(".config/systemd/user/floti.service");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, unit.file_bytes()).unwrap();
    let enabled = FakeOs::new(|_| ran(true, "enabled\n", ""));
    assert_eq!(
        start_at_sign_in(&enabled, &unit, &dirs.home, &dirs.data),
        Finding::Fine
    );
    assert_eq!(
        *enabled.runs.borrow(),
        ["systemctl --user is-enabled floti.service"]
    );
    let disabled = FakeOs::new(|_| ran(false, "disabled\n", ""));
    assert_eq!(
        start_at_sign_in(&disabled, &unit, &dirs.home, &dirs.data),
        no_entry()
    );
    // A service manager that answers with no state: could not tell.
    let no_bus = FakeOs::new(|_| ran(false, "", "Failed to connect to bus: No medium found"));
    assert_eq!(
        start_at_sign_in(&no_bus, &unit, &dirs.home, &dirs.data),
        Finding::CouldNotTell {
            what: "whether the host starts at sign-in".to_owned(),
            why: "Failed to connect to bus: No medium found".to_owned(),
        }
    );
}

#[test]
fn in_wsl_without_a_user_manager_the_start_on_connect_record_is_what_starts_the_host() {
    let dirs = TestDirs::new();
    let unit = linux::user_unit(Path::new("/home/me/.local/bin/floti")).unwrap();
    let mut os = FakeOs::new(|line| {
        assert_eq!(line, "systemctl --user show-environment");
        ran(
            false,
            "",
            "System has not been booted with systemd as init system (PID 1).",
        )
    });
    os.wsl = true;
    let found = start_at_sign_in(&os, &unit, &dirs.home, &dirs.data);
    assert!(
        matches!(&found, Finding::Problem { fix, .. } if fix == "floti setup"),
        "{found:?}"
    );
    std::fs::create_dir_all(&dirs.data).unwrap();
    std::fs::write(dirs.data.join(START_ON_CONNECT_FILE), "on connect").unwrap();
    assert_eq!(
        start_at_sign_in(&os, &unit, &dirs.home, &dirs.data),
        Finding::Fine
    );
}

// ---------------------------------------------------------------------------
// floti on the user's PATH
// ---------------------------------------------------------------------------

/// A user `PATH` that holds `holds`, or could not be read when `None`.
struct FakePath {
    holds: Option<&'static str>,
}

impl UserPath for FakePath {
    fn has(&self, _env: &UserEnv, folder: &Path) -> Result<bool, String> {
        self.holds
            .map(|holds| Path::new(holds) == folder)
            .ok_or_else(|| "the registry would not say".to_owned())
    }

    fn fix(&self, _env: &UserEnv, folder: &Path) -> String {
        format!("add {}", folder.display())
    }
}

#[test]
fn floti_is_on_path_when_its_own_folder_is_and_the_fix_names_that_folder() {
    let env = UserEnv {
        home: None,
        vars: Default::default(),
    };
    let floti = Path::new("/home/me/.local/bin/floti");
    assert_eq!(
        floti_on_path(
            &FakePath {
                holds: Some("/home/me/.local/bin")
            },
            &env,
            floti
        ),
        Finding::Fine
    );
    assert_eq!(
        floti_on_path(&FakePath { holds: Some("/usr/bin") }, &env, floti),
        Finding::Problem {
            problem: "floti's folder, /home/me/.local/bin, is not on your PATH, so a new terminal does not find floti".to_owned(),
            fix: "add /home/me/.local/bin".to_owned(),
        }
    );
    assert_eq!(
        floti_on_path(&FakePath { holds: None }, &env, floti),
        Finding::CouldNotTell {
            what: "whether floti is on your PATH".to_owned(),
            why: "the registry would not say".to_owned(),
        }
    );
}
