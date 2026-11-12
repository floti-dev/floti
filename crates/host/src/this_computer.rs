//! What `floti setup` and `floti doctor` check on the computer they run on,
//! besides the host itself, each as a [`Finding`]: nothing wrong, a problem
//! with the line that fixes it, or that it could not be told and why. "Could
//! not tell" is never reported as a problem, nor a problem as "could not
//! tell".
//!
//! - [`claude`]: whether Claude Code is installed and its version is in the
//!   range the Claude Code backend was tested with ([`TESTED_VERSIONS`]),
//!   found the backend's own way (its user program folders, then `PATH`)
//!   and asked with `claude --version`, which prints no credential. Nothing
//!   here opens or reads one. A problem's fix is Anthropic's own installer,
//!   `claude update` for a version older than the range, or `claude
//!   install <first tested>` for one newer; outside the range it still
//!   launches, as it does in the desktop.
//! - [`start_at_sign_in`]: whether the host's start-at-sign-in entry is
//!   registered, looked at by setup's own registration code
//!   ([`is_registered`]), or, in a WSL distro whose systemd user manager does
//!   not answer, whether setup recorded that the host starts on connect.
//! - [`floti_on_path`]: whether this floti's folder is on the `PATH` the
//!   user's next terminal gets ([`UserPath`]), not only this process's.
//!
//! Setup checks only Claude Code, and says what to run when something is
//! wrong; doctor checks all three ([`findings_here`]). Neither starts,
//! registers, writes or changes anything to look.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent::claude_code::{
    ClaudeCodeBackend, ClaudeCodeSettings, TESTED_VERSIONS, install_line,
};
use crate::agent::{ExecutableSetup, TestedVersions, VersionCheck};
use crate::connect::START_ON_CONNECT_FILE;
use crate::platform::command_shell::ShellFamily;
use crate::platform::folders::UserEnv;
use crate::platform::process_tree;
use crate::platform::startup::StartupDefinition;
use crate::platform::startup::StartupKind;
use crate::platform::startup::register::{SignInSystem, is_registered, user_manager_answers};
use crate::platform::user_path::UserPath;

/// What the fix for a missing start-at-sign-in entry runs.
pub const SETUP_FIX: &str = "floti setup";

/// What one check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Nothing wrong.
    Fine,
    /// Something wrong, as a clause, and the one line that fixes it.
    Problem { problem: String, fix: String },
    /// `what` could not be told, and why.
    CouldNotTell { what: String, why: String },
}

impl Finding {
    fn could_not_tell(what: &str, why: impl Into<String>) -> Self {
        Self::CouldNotTell {
            what: what.to_owned(),
            why: why.into(),
        }
    }

    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Problem { .. })
    }

    /// The finding as the one sentence setup says, or `None` when nothing is
    /// wrong.
    pub fn sentence(&self) -> Option<String> {
        match self {
            Self::Fine => None,
            Self::Problem { problem, fix } => {
                Some(capitalized(&format!("{problem}; to fix it, run: {fix}")))
            }
            Self::CouldNotTell { what, why } => Some(format!("Could not tell {what}: {why}")),
        }
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Whether the `claude` `executable` finds is installed and in `range`,
/// with `looked_in` the folders it looks in before `PATH` and `shell` the
/// kind of shell the fix is typed in.
pub fn claude(
    executable: &dyn ExecutableSetup,
    range: TestedVersions,
    looked_in: &[PathBuf],
    shell: ShellFamily,
) -> Finding {
    let Some(path) = executable.discovered() else {
        let folders: Vec<String> = looked_in
            .iter()
            .map(|folder| folder.display().to_string())
            .collect();
        let where_ = if folders.is_empty() {
            "on PATH".to_owned()
        } else {
            format!("on PATH or in {}", folders.join(" or "))
        };
        return Finding::Problem {
            problem: format!("Claude Code is not installed: there is no claude {where_}"),
            fix: install_line(shell).to_owned(),
        };
    };
    let Some(said) = executable.version(Some(&path)) else {
        return Finding::Problem {
            problem: format!(
                "Claude Code at {} did not answer `{}`",
                path.display(),
                executable.version_command()
            ),
            fix: install_line(shell).to_owned(),
        };
    };
    let tested = format!(
        "Floti was tested with {} up to but not including {}",
        range.min, range.below
    );
    match range.check(&said) {
        VersionCheck::Tested(_) => Finding::Fine,
        VersionCheck::Untested { version, .. } if version < range.min => Finding::Problem {
            problem: format!("Claude Code {version} is older than {tested}"),
            fix: "claude update".to_owned(),
        },
        VersionCheck::Untested { version, .. } => Finding::Problem {
            problem: format!(
                "Claude Code {version} is newer than {tested}, though it still launches"
            ),
            fix: format!("claude install {}", range.min),
        },
        VersionCheck::Unknown => Finding::could_not_tell(
            "which version of Claude Code is installed",
            format!(
                "{} answered `{}` with {said:?}, which names no version",
                path.display(),
                executable.version_command()
            ),
        ),
    }
}

/// [`claude`] for the user `env` describes, found as the host's Claude Code
/// backend finds it.
pub fn claude_here(env: &UserEnv, shell: ShellFamily) -> Finding {
    let Some(settings) = ClaudeCodeSettings::from_env(env) else {
        return Finding::could_not_tell(
            "whether Claude Code is installed",
            "there is no home folder to look for it from",
        );
    };
    let looked_in = settings.search_first.clone();
    let backend = ClaudeCodeBackend::new(settings, Arc::new(process_tree::Native));
    claude(&backend, TESTED_VERSIONS, &looked_in, shell)
}

/// Whether the host starts on its own: `definition` is registered through
/// `system`, for the user whose home and Floti data folder are `home` and
/// `data`; or, in a WSL distro whose systemd user manager does not answer,
/// setup recorded that the host starts when a client connects.
pub fn start_at_sign_in(
    system: &dyn SignInSystem,
    definition: &StartupDefinition,
    home: &Path,
    data: &Path,
) -> Finding {
    if definition.kind == StartupKind::SystemdUserUnit
        && system.inside_wsl()
        && !user_manager_answers(system)
    {
        let record = data.join(START_ON_CONNECT_FILE);
        return match std::fs::metadata(&record) {
            Ok(_) => Finding::Fine,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Finding::Problem {
                problem: "the host is not set to start when a client connects, and this distro runs no systemd user manager to start it".to_owned(),
                fix: SETUP_FIX.to_owned(),
            },
            Err(err) => Finding::could_not_tell(
                "whether the host starts when a client connects",
                format!("could not look at {}: {err}", record.display()),
            ),
        };
    }
    match is_registered(system, definition, home, data) {
        Ok(true) => Finding::Fine,
        Ok(false) => Finding::Problem {
            problem:
                "the host has no start-at-sign-in entry, so it does not start when you sign in"
                    .to_owned(),
            fix: SETUP_FIX.to_owned(),
        },
        Err(why) => Finding::could_not_tell("whether the host starts at sign-in", why),
    }
}

/// Whether `floti`'s folder is on the `PATH` the next terminal of the user
/// `env` describes gets.
pub fn floti_on_path(path: &dyn UserPath, env: &UserEnv, floti: &Path) -> Finding {
    let Some(folder) = floti
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    else {
        return Finding::could_not_tell(
            "whether floti is on your PATH",
            format!("{} is not in a folder", floti.display()),
        );
    };
    match path.has(env, folder) {
        Ok(true) => Finding::Fine,
        Ok(false) => Finding::Problem {
            problem: format!(
                "floti's folder, {}, is not on your PATH, so a new terminal does not find floti",
                folder.display()
            ),
            fix: path.fix(env, folder),
        },
        Err(why) => Finding::could_not_tell("whether floti is on your PATH", why),
    }
}

/// What doctor reports of this computer, for the user `env` describes with
/// Floti's data folder `data`, in the order it prints them: the
/// start-at-sign-in entry, floti on `PATH`, then Claude Code.
pub fn findings_here(env: &UserEnv, data: &Path, shell: ShellFamily) -> Vec<Finding> {
    let [entry, path] = os_findings(env, data);
    vec![entry, path, claude_here(env, shell)]
}

/// This computer's start-at-sign-in entry and `PATH`, asked of the OS.
#[cfg(not(feature = "testkit"))]
fn os_findings(env: &UserEnv, data: &Path) -> [Finding; 2] {
    use crate::platform::startup::register::NativeSystem;
    use crate::platform::startup::{self, StartAtSignIn};
    use crate::platform::user_path;

    let floti = match std::env::current_exe() {
        Ok(floti) => floti,
        Err(err) => {
            let why = format!("could not tell where this floti is: {err}");
            return [
                Finding::could_not_tell("whether the host starts at sign-in", why.clone()),
                Finding::could_not_tell("whether floti is on your PATH", why),
            ];
        }
    };
    let entry = match (startup::Native.definition(&floti), env.require_home()) {
        (Ok(definition), Ok(home)) => start_at_sign_in(&NativeSystem, &definition, home, data),
        (Err(err), _) | (_, Err(err)) => {
            Finding::could_not_tell("whether the host starts at sign-in", err.to_string())
        }
    };
    [entry, floti_on_path(&user_path::Native, env, &floti)]
}

/// In a test build of floti, which the crate's tests run as a program,
/// neither is asked: a test never reads this computer's sign-in entries or
/// the user's `PATH`. The checks themselves are tested against a fake OS.
#[cfg(feature = "testkit")]
fn os_findings(_env: &UserEnv, _data: &Path) -> [Finding; 2] {
    let why = TEST_BUILD_LOOKS_AT_NOTHING;
    [
        Finding::could_not_tell("whether the host starts at sign-in", why),
        Finding::could_not_tell("whether floti is on your PATH", why),
    ]
}

/// Why a test build of floti says it could not tell either.
#[cfg(feature = "testkit")]
pub const TEST_BUILD_LOOKS_AT_NOTHING: &str =
    "a test build of floti does not look at this computer's sign-in entries or PATH";

#[cfg(test)]
mod tests;
