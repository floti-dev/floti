//! Setup's last step: the desktop app. It runs once the host is set up, so a
//! desktop install that fails leaves a working host, and says so.
//!
//! With `--desktop` it installs the app without asking, through
//! `floti add desktop` itself ([`crate::add::add_desktop_here`]), which
//! refuses a session with no display as it always does and says so of an app
//! already installed. With `--no-desktop` it looks at nothing and says
//! nothing: `floti install-self` passes it, since the app that runs
//! install-self is already there, and `floti update` and `floti add desktop`
//! show what install-self printed.
//!
//! With neither, it looks first ([`step`]):
//!
//! 1. An app already installed: one line saying where, as `floti add desktop`
//!    says it, and no question.
//! 2. A session the app is not offered in: nothing, and nothing is said.
//!    That is one with no display ([`installs::missing_display`]: a server,
//!    an ssh login to Windows, a Mac nobody is signed in to at its screen),
//!    one the OS leaves out ([`Installs::not_offered`]: a WSL distro), and,
//!    on any OS, a computer setup itself found nobody signed in to
//!    ([`super::SetupOutcome::AtNextSignIn`]).
//! 3. Nowhere to ask ([`can_ask`]): standard input or output is no terminal,
//!    such as a script with either redirected or the desktop's own ssh, or
//!    setup runs in a job put in the background. No question, and one line
//!    naming `floti add desktop`.
//! 4. Otherwise it asks once whether to install the app as well, yes on
//!    Enter ([`answered_yes`]). Yes runs `floti add desktop`; no says one
//!    line naming it.
//!
//! What the step did never changes setup's exit status, which is the host's.

use std::io::{BufRead, Write};

use crate::add::{Added, already_installed_line, capitalised};
use crate::platform::folders::UserEnv;
use crate::platform::installs::{self, Desktop, Installs};

/// What `floti setup` was told of the desktop app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopChoice {
    /// Neither flag: ask where it can.
    #[default]
    Ask,
    /// `--desktop`: install it without asking.
    Install,
    /// `--no-desktop`: leave it as it is.
    Skip,
}

/// What setup says to an answer that is neither yes nor no.
pub const ANSWER_AGAIN_LINE: &str = "Answer y or n.";

/// The question, about `app` (`the Helm desktop app`).
pub fn question(app: &str) -> String {
    format!("Install {app} as well? [Y/n] ")
}

/// What setup says with no terminal to ask on.
pub fn no_terminal_line(app: &str) -> String {
    format!("To install {app} as well, run floti add desktop.")
}

/// What setup says once the question was answered no.
pub fn declined_line(app: &str) -> String {
    format!(
        "{} was not installed; floti add desktop installs it.",
        capitalised(app)
    )
}

/// What setup says on standard error when the install failed: `floti add
/// desktop`'s own failure line, then that the host is set up.
pub fn failed_line(line: &str) -> String {
    format!("floti add desktop: {line} The Floti host is set up all the same.")
}

/// What nobody being signed in, as setup itself found it, makes of the
/// session, as a clause.
pub const NOBODY_SIGNED_IN: &str = "nobody is signed in to this computer";

/// Whether setup can ask its question: standard input and standard output
/// are both terminals, and this process is in the foreground of the one it
/// reads, so the question is seen and reading the answer stops nothing.
pub fn can_ask(stdin_terminal: bool, stdout_terminal: bool, foreground: bool) -> bool {
    stdin_terminal && stdout_terminal && foreground
}

/// What setup finds before it decides whether to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situation {
    /// The desktop app installed for this user, if one is.
    pub installed: Option<Desktop>,
    /// Why the app is not offered in this session, or `None` when it is.
    pub not_offered: Option<String>,
    /// Whether setup can ask ([`can_ask`]).
    pub terminal: bool,
    /// What the question calls the app: `the Helm desktop app`.
    pub app: String,
}

/// What setup does about the desktop app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Runs `floti add desktop`.
    Add,
    /// Asks whether to, about `app`.
    Ask { app: String },
    /// Says where the app installed already is.
    AlreadyInstalled(Desktop),
    /// Nothing, and nothing is said: `--no-desktop`.
    Skipped,
    /// Nothing, and nothing is said: the app is not offered here.
    NotOffered,
    /// Says [`no_terminal_line`] about `app`.
    NoTerminal { app: String },
}

/// What setup does about the desktop app for `choice`, looking at the
/// computer (`look`) only when neither flag answered in advance.
pub fn step(choice: DesktopChoice, look: impl FnOnce() -> Situation) -> Step {
    match choice {
        DesktopChoice::Install => Step::Add,
        DesktopChoice::Skip => Step::Skipped,
        DesktopChoice::Ask => {
            let situation = look();
            if let Some(app) = situation.installed {
                Step::AlreadyInstalled(app)
            } else if situation.not_offered.is_some() {
                Step::NotOffered
            } else if !situation.terminal {
                Step::NoTerminal { app: situation.app }
            } else {
                Step::Ask { app: situation.app }
            }
        }
    }
}

/// What this computer is for the step: the installed app, whether the app
/// is offered (`signed_in`: whether setup found somebody signed in), whether
/// setup can ask (`terminal`), and the app's name.
pub fn situation(
    installs: &dyn Installs,
    env: &UserEnv,
    signed_in: bool,
    terminal: bool,
) -> Situation {
    let app = match installs.desktop_target(env) {
        Some(target) => format!("the {} desktop app", target.name),
        None => "the desktop app".to_owned(),
    };
    let not_offered = if signed_in {
        installs::missing_display(installs, env).or_else(|| installs::not_offered(installs, env))
    } else {
        Some(NOBODY_SIGNED_IN.to_owned())
    };
    Situation {
        installed: installs.desktop(env),
        not_offered,
        terminal,
        app,
    }
}

/// Asks `question` on `out` and reads the answer from `input`: yes on Enter,
/// `y` or `yes`; no on `n`, `no` or the end of the input. Anything else is
/// asked again.
pub fn answered_yes(question: &str, input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    loop {
        let _ = write!(out, "{question}").and_then(|()| out.flush());
        let mut answer = String::new();
        match input.read_line(&mut answer) {
            Ok(0) | Err(_) => {
                let _ = writeln!(out);
                return false;
            }
            Ok(_) => match answer.trim().to_ascii_lowercase().as_str() {
                "" | "y" | "yes" => return true,
                "n" | "no" => return false,
                _ => {
                    let _ = writeln!(out, "{ANSWER_AGAIN_LINE}");
                }
            },
        }
    }
}

/// How the step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopOutcome {
    /// `floti add desktop` installed the app.
    Installed,
    /// The app was installed already; nothing was changed.
    AlreadyInstalled,
    /// The question was answered no.
    Declined,
    /// `--no-desktop`: nothing was looked at or said.
    Skipped,
    /// Not offered here: nothing was asked or said.
    NotOffered,
    /// No terminal to ask on: the line naming `floti add desktop` was said.
    NoTerminal,
    /// `floti add desktop` failed, with its failure line.
    Failed(String),
}

/// One run of the step: what it was told and every piece of the computer it
/// touches, so a test hands it fakes.
pub struct DesktopStep<'a> {
    pub choice: DesktopChoice,
    pub installs: &'a dyn Installs,
    pub env: &'a UserEnv,
    /// Whether setup found somebody signed in: false when the host starts
    /// at the next sign-in.
    pub signed_in: bool,
    /// Whether setup can ask ([`can_ask`]).
    pub terminal: bool,
    pub input: &'a mut dyn BufRead,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Runs `floti add desktop` with its progress on the writer it is given:
    /// how it ended well, or its failure line.
    pub add: &'a mut dyn FnMut(&mut dyn Write) -> Result<Added, String>,
}

impl DesktopStep<'_> {
    /// Does what [`step`] decides, and says what it did.
    pub fn run(self) -> DesktopOutcome {
        let (installs, env) = (self.installs, self.env);
        let (signed_in, terminal) = (self.signed_in, self.terminal);
        match step(self.choice, || {
            situation(installs, env, signed_in, terminal)
        }) {
            Step::Add => add(self.add, self.out, self.err),
            Step::Ask { app } => {
                if answered_yes(&question(&app), self.input, self.out) {
                    add(self.add, self.out, self.err)
                } else {
                    say(self.out, &declined_line(&app));
                    DesktopOutcome::Declined
                }
            }
            Step::AlreadyInstalled(app) => {
                say(self.out, &already_installed_line(installs, &app));
                DesktopOutcome::AlreadyInstalled
            }
            Step::Skipped => DesktopOutcome::Skipped,
            Step::NotOffered => DesktopOutcome::NotOffered,
            Step::NoTerminal { app } => {
                say(self.out, &no_terminal_line(&app));
                DesktopOutcome::NoTerminal
            }
        }
    }
}

fn add(
    run: &mut dyn FnMut(&mut dyn Write) -> Result<Added, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> DesktopOutcome {
    match run(out) {
        Ok(Added::Installed { .. }) => DesktopOutcome::Installed,
        Ok(Added::AlreadyInstalled(_)) => DesktopOutcome::AlreadyInstalled,
        Err(line) => {
            say(err, &failed_line(&line));
            DesktopOutcome::Failed(line)
        }
    }
}

fn say(out: &mut dyn Write, line: &str) {
    let _ = writeln!(out, "{line}").and_then(|()| out.flush());
}

#[cfg(test)]
mod tests;
