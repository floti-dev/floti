//! Setup's desktop step with the computer and `floti add desktop` faked:
//! when it asks, what each answer and each flag does, and what it says.

use std::cell::Cell;
use std::ffi::OsString;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use super::{
    ANSWER_AGAIN_LINE, DesktopChoice, DesktopOutcome, DesktopStep, NOBODY_SIGNED_IN, Situation,
    Step, answered_yes, can_ask, declined_line, failed_line, no_terminal_line, question, situation,
    step,
};
use crate::Result;
use crate::add::Added;
use crate::platform::folders::UserEnv;
use crate::platform::installs::{DISPLAY_TEST_VAR, Desktop, Installs};
use crate::platform::process_table::ProcessRow;

const APP: &str = "the Helm desktop app";

fn helm() -> Desktop {
    Desktop {
        name: "Helm",
        path: PathBuf::from("/home/me/Applications/Helm.AppImage"),
    }
}

/// A computer's installs: the app installed or not, a display or not, a
/// session the OS leaves the app out of or not, and how many times setup
/// looked.
struct FakeInstalls {
    installed: bool,
    no_display: Option<String>,
    not_offered: Option<String>,
    looked: Cell<usize>,
}

impl FakeInstalls {
    fn new() -> Self {
        Self {
            installed: false,
            no_display: None,
            not_offered: None,
            looked: Cell::new(0),
        }
    }

    fn in_wsl() -> Self {
        Self {
            not_offered: Some("this is a WSL distro, whose desktop is Windows' own".into()),
            ..Self::new()
        }
    }

    fn installed() -> Self {
        Self {
            installed: true,
            ..Self::new()
        }
    }

    fn with_no_display() -> Self {
        Self {
            no_display: Some("this session has no display".into()),
            ..Self::new()
        }
    }
}

impl Installs for FakeInstalls {
    fn bundles(&self, _desktop: &Desktop, _running: &Path) -> bool {
        false
    }

    fn desktop(&self, _env: &UserEnv) -> Option<Desktop> {
        self.looked.set(self.looked.get() + 1);
        self.installed.then(helm)
    }

    fn desktop_target(&self, _env: &UserEnv) -> Option<Desktop> {
        Some(helm())
    }

    fn desktop_version(&self, _desktop: &Desktop) -> Option<String> {
        Some("3.0.0".into())
    }

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        None
    }

    fn is_desktop_process(&self, _desktop: &Desktop, _row: &ProcessRow) -> bool {
        false
    }

    fn install_desktop(&self, _target: &Desktop, _release: &Path) -> Result<()> {
        unreachable!("setup installs only through floti add desktop")
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (desktop.path.clone(), Vec::new())
    }

    fn no_display(&self, _env: &UserEnv) -> Option<String> {
        self.no_display.clone()
    }

    fn not_offered(&self, _env: &UserEnv) -> Option<String> {
        self.not_offered.clone()
    }
}

fn found(installed: bool, display: bool, terminal: bool) -> Situation {
    Situation {
        installed: installed.then(helm),
        not_offered: (!display).then(|| "this session has no display".to_owned()),
        terminal,
        app: APP.into(),
    }
}

fn never_looked() -> Situation {
    panic!("a flag answers in advance; setup looks at nothing")
}

#[test]
fn desktop_and_no_desktop_answer_in_advance_without_looking() {
    assert_eq!(step(DesktopChoice::Install, never_looked), Step::Add);
    assert_eq!(step(DesktopChoice::Skip, never_looked), Step::Skipped);
}

#[test]
fn with_neither_flag_it_asks_only_with_a_terminal_and_a_display() {
    let ask = |installed, display, terminal| {
        step(DesktopChoice::Ask, || found(installed, display, terminal))
    };
    assert_eq!(ask(false, true, true), Step::Ask { app: APP.into() });
    assert_eq!(
        ask(false, true, false),
        Step::NoTerminal { app: APP.into() }
    );
    assert_eq!(ask(false, false, true), Step::NotOffered);
    assert_eq!(ask(false, false, false), Step::NotOffered);
}

#[test]
fn it_can_ask_only_with_both_streams_terminals_and_in_the_foreground() {
    assert!(can_ask(true, true, true));
    for (stdin, stdout, foreground) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
        (false, false, false),
    ] {
        assert!(
            !can_ask(stdin, stdout, foreground),
            "stdin {stdin}, stdout {stdout}, foreground {foreground}"
        );
    }
}

#[test]
fn an_app_already_installed_is_said_and_never_asked_about() {
    for (display, terminal) in [(true, true), (true, false), (false, true), (false, false)] {
        assert_eq!(
            step(DesktopChoice::Ask, || found(true, display, terminal)),
            Step::AlreadyInstalled(helm()),
            "display {display}, terminal {terminal}"
        );
    }
}

#[test]
fn the_situation_names_the_app_and_takes_the_display_from_the_session() {
    let installs = FakeInstalls::new();
    let user = UserEnv::default();
    assert_eq!(
        situation(&installs, &user, true, true),
        found(false, true, true)
    );

    let none = UserEnv {
        home: None,
        vars: [(OsString::from(DISPLAY_TEST_VAR), OsString::from("none"))].into(),
    };
    let seen = situation(&FakeInstalls::installed(), &none, true, false);
    assert_eq!(seen.installed, Some(helm()));
    assert!(seen.not_offered.is_some());
    assert!(!seen.terminal);
}

#[test]
fn nobody_signed_in_and_a_session_the_os_leaves_out_are_not_offered_the_app() {
    let user = UserEnv::default();
    let seen = situation(&FakeInstalls::new(), &user, false, true);
    assert_eq!(seen.not_offered.as_deref(), Some(NOBODY_SIGNED_IN));
    assert_eq!(step(DesktopChoice::Ask, || seen.clone()), Step::NotOffered);

    let seen = situation(&FakeInstalls::in_wsl(), &user, true, true);
    assert_eq!(
        seen.not_offered.as_deref(),
        Some("this is a WSL distro, whose desktop is Windows' own")
    );
    assert_eq!(step(DesktopChoice::Ask, || seen), Step::NotOffered);
}

fn answer(typed: &str) -> (bool, String) {
    let mut out = Vec::new();
    let yes = answered_yes(&question(APP), &mut Cursor::new(typed), &mut out);
    (yes, String::from_utf8(out).unwrap())
}

#[test]
fn the_question_is_yes_on_enter_and_takes_y_or_n_in_any_case() {
    let asked = "Install the Helm desktop app as well? [Y/n] ";
    for typed in ["\n", "\r\n", "y\n", "Yes\n", "  YES  \n"] {
        assert_eq!(answer(typed), (true, asked.to_owned()), "{typed:?}");
    }
    for typed in ["n\n", "No\n", "N\r\n"] {
        assert_eq!(answer(typed), (false, asked.to_owned()), "{typed:?}");
    }
}

#[test]
fn the_end_of_the_input_is_no_and_anything_else_is_asked_again() {
    let asked = question(APP);
    assert_eq!(answer(""), (false, format!("{asked}\n")));
    assert_eq!(
        answer("maybe\ny\n"),
        (true, format!("{asked}{ANSWER_AGAIN_LINE}\n{asked}"))
    );
}

/// What one run of the step did: how it ended, what it said on each
/// stream, and how many times `floti add desktop` ran.
struct Ran {
    outcome: DesktopOutcome,
    out: String,
    err: String,
    adds: usize,
}

fn run(
    choice: DesktopChoice,
    installs: &FakeInstalls,
    terminal: bool,
    typed: &str,
    added: std::result::Result<Added, String>,
) -> Ran {
    run_signed_in(choice, installs, true, terminal, typed, added)
}

fn run_signed_in(
    choice: DesktopChoice,
    installs: &FakeInstalls,
    signed_in: bool,
    terminal: bool,
    typed: &str,
    added: std::result::Result<Added, String>,
) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut adds = 0;
    let mut add = |out: &mut dyn Write| {
        adds += 1;
        writeln!(out, "Downloading Helm-3.0.0-linux-x64.AppImage.").unwrap();
        added.clone()
    };
    let outcome = DesktopStep {
        choice,
        installs,
        env: &UserEnv::default(),
        signed_in,
        terminal,
        input: &mut Cursor::new(typed),
        out: &mut out,
        err: &mut err,
        add: &mut add,
    }
    .run();
    Ran {
        outcome,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        adds,
    }
}

fn installed() -> std::result::Result<Added, String> {
    Ok(Added::Installed {
        desktop: helm(),
        version: "3.0.0".into(),
    })
}

#[test]
fn yes_runs_floti_add_desktop_with_its_progress_after_the_question() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::new(),
        true,
        "\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::Installed);
    assert_eq!(ran.adds, 1);
    assert_eq!(
        ran.out,
        format!(
            "{}Downloading Helm-3.0.0-linux-x64.AppImage.\n",
            question(APP)
        )
    );
    assert_eq!(ran.err, "");
}

#[test]
fn no_installs_nothing_and_names_floti_add_desktop() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::new(),
        true,
        "n\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::Declined);
    assert_eq!(ran.adds, 0);
    assert_eq!(
        ran.out,
        format!(
            "{}The Helm desktop app was not installed; floti add desktop installs it.\n",
            question(APP)
        )
    );
    assert_eq!(declined_line(APP).lines().count(), 1);
}

#[test]
fn with_no_terminal_nothing_is_asked_or_installed_and_one_line_names_floti_add_desktop() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::new(),
        false,
        "y\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::NoTerminal);
    assert_eq!(ran.adds, 0);
    assert_eq!(
        ran.out,
        "To install the Helm desktop app as well, run floti add desktop.\n"
    );
    assert_eq!(ran.out, format!("{}\n", no_terminal_line(APP)));
}

#[test]
fn with_no_display_nothing_is_asked_said_or_installed() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::with_no_display(),
        true,
        "y\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::NotOffered);
    assert_eq!((ran.out.as_str(), ran.err.as_str(), ran.adds), ("", "", 0));
}

#[test]
fn an_installed_app_gets_floti_add_desktop_s_one_line_and_no_question() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::installed(),
        true,
        "n\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::AlreadyInstalled);
    assert_eq!(ran.adds, 0);
    assert_eq!(
        ran.out,
        "Helm 3.0.0 is already installed at /home/me/Applications/Helm.AppImage; floti update keeps it up to date. Nothing was changed.\n"
    );
}

#[test]
fn desktop_runs_floti_add_desktop_without_asking_even_with_no_terminal_or_display() {
    for installs in [FakeInstalls::new(), FakeInstalls::with_no_display()] {
        let ran = run(DesktopChoice::Install, &installs, false, "", installed());
        assert_eq!(ran.outcome, DesktopOutcome::Installed);
        assert_eq!(ran.adds, 1);
        assert!(!ran.out.contains("[Y/n]"), "{}", ran.out);
        assert_eq!(installs.looked.get(), 0, "it never looks itself");
    }
}

#[test]
fn a_failed_install_says_add_s_line_and_that_the_host_is_set_up() {
    let refused = "This session has no display, so Helm is not installed here.";
    let ran = run(
        DesktopChoice::Install,
        &FakeInstalls::with_no_display(),
        true,
        "",
        Err(refused.into()),
    );
    assert_eq!(ran.outcome, DesktopOutcome::Failed(refused.into()));
    assert_eq!(
        ran.err,
        "floti add desktop: This session has no display, so Helm is not installed here. The Floti host is set up all the same.\n"
    );
    assert_eq!(ran.err, format!("{}\n", failed_line(refused)));
}

#[test]
fn an_installed_app_found_by_floti_add_desktop_ends_as_already_installed() {
    let ran = run(
        DesktopChoice::Install,
        &FakeInstalls::new(),
        true,
        "",
        Ok(Added::AlreadyInstalled(helm())),
    );
    assert_eq!(ran.outcome, DesktopOutcome::AlreadyInstalled);
}

#[test]
fn no_desktop_says_one_line_and_looks_at_nothing() {
    let installs = FakeInstalls::installed();
    let ran = run(DesktopChoice::Skip, &installs, true, "y\n", installed());
    assert_eq!(ran.outcome, DesktopOutcome::Skipped);
    assert_eq!(ran.out, "", "it says nothing");
    assert_eq!((ran.adds, installs.looked.get()), (0, 0));
}

#[test]
fn after_setup_found_nobody_signed_in_nothing_is_asked_said_or_installed() {
    let ran = run_signed_in(
        DesktopChoice::Ask,
        &FakeInstalls::new(),
        false,
        true,
        "y\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::NotOffered);
    assert_eq!((ran.out.as_str(), ran.err.as_str(), ran.adds), ("", "", 0));
}

#[test]
fn in_wsl_nothing_is_asked_but_desktop_still_installs() {
    let ran = run(
        DesktopChoice::Ask,
        &FakeInstalls::in_wsl(),
        true,
        "y\n",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::NotOffered);
    assert_eq!((ran.out.as_str(), ran.adds), ("", 0));

    let ran = run(
        DesktopChoice::Install,
        &FakeInstalls::in_wsl(),
        true,
        "",
        installed(),
    );
    assert_eq!(ran.outcome, DesktopOutcome::Installed);
    assert_eq!(ran.adds, 1);
}
