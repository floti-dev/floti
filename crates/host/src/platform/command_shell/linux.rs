//! Linux: the login shell, `$SHELL`, else `/bin/sh`.

use super::{CommandShell, FindShell, ShellCommand, ShellFamily, posix};
use crate::platform::folders::UserEnv;

/// The Linux shell.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl CommandShell for Native {
    fn family(&self) -> ShellFamily {
        ShellFamily::Posix
    }

    fn run_line(&self, env: &UserEnv, _find: FindShell<'_>, line: &str) -> ShellCommand {
        posix::run_line(env, line)
    }

    fn command_line(&self, words: &[String]) -> String {
        posix::command_line(words)
    }
}
