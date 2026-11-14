//! How the host starts on this computer's OS. On macOS and Linux it starts at
//! the user's sign-in: the LaunchAgent `dev.floti.host` and the systemd user
//! unit `floti.service`. On Windows nothing starts it at sign-in: a client
//! starts it when it needs one ([`HostStart::WhenNeeded`]), the desktop app
//! when no host answers it, or `floti connect`, run over the user's own SSH.
//! Never a system service.
//!
//! The host generates the definition; registering it with the OS is `floti
//! setup`'s step ([`register`]), so generating one registers nothing. Each
//! generator only composes text, and registering runs each OS command through
//! a [`register::SignInSystem`] a test fakes, so every OS's is compiled on
//! every OS and tested everywhere; the lines below pick which one is
//! `Native`. Windows also names what an earlier floti registered there, the
//! logon task "Floti host" ([`LeftoverTask`]), which setup, `floti update`
//! and `floti uninstall` take out.
//!
//! The same concern covers who may start the host and from where. It refuses
//! to start as root or a Windows service account, and on macOS and Linux
//! from an SSH session, where no sign-in started it
//! ([`HostStartup::launch_refusal`]). `floti connect` asks it whether to
//! start a host when none answers ([`HostStartup::on_connect`]). And it stops
//! on the OS's stop requests ([`StopSignals`]: Ctrl+C, SIGTERM or the console
//! events, and signing out). What those read of the OS (the account,
//! elevation, the user's desktop session: [`Login`]) has one file per OS
//! family, `signin_windows.rs` and `signin_unix.rs`.

pub mod linux;
pub mod macos;
pub mod register;
pub mod windows;

#[cfg(unix)]
mod signin_unix;
#[cfg(windows)]
mod signin_windows;

#[cfg(target_os = "linux")]
pub use linux::Native;
#[cfg(target_os = "macos")]
pub use macos::Native;
#[cfg(target_os = "windows")]
pub use windows::Native;

#[cfg(unix)]
use signin_unix as signin;
#[cfg(windows)]
use signin_windows as signin;

#[cfg(any(test, feature = "testkit"))]
pub use signin::{interrupt, stoppable, terminate};

#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;

use crate::platform::folders::UserEnv;
use crate::{Error, Result};

/// Variables an SSH server sets in the sessions it starts.
pub const SSH_VARS: [&str; 3] = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"];

/// The file in the data folder that says the host starts when a client
/// connects: `floti setup` writes it in a WSL distro whose systemd user
/// manager does not answer, where nothing else would start the host. Only
/// macOS and Linux read it ([`HostStartup::on_connect`]).
pub const START_ON_CONNECT_FILE: &str = "start-on-connect";

/// The account the host is running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account {
    /// An ordinary user.
    User,
    /// An account a host must never run as, by name: root, or a Windows
    /// service account such as SYSTEM.
    Privileged(&'static str),
    /// The OS would not say. The host starts, as it would for a user.
    Unknown,
}

/// The account this process runs as.
pub fn account() -> Account {
    signin::account()
}

/// The build number of the Windows the host runs on, for a client's
/// terminal renderer and its ConPTY quirks; `None` on any other OS, or when
/// Windows would not say.
pub fn windows_build() -> Option<u32> {
    signin::windows_build()
}

/// Why the host must not start here, by this OS's rule
/// ([`HostStartup::launch_refusal`]), as one sentence, or `None` when it
/// may. It is asked before anything is opened.
pub fn launch_refusal(env: &UserEnv, account: Account) -> Option<String> {
    Native.launch_refusal(env, account)
}

/// The refusal on an OS whose host starts at the user's sign-in: a start from
/// an SSH session, as root or as a service account.
fn refused_outside_sign_in(env: &UserEnv, account: Account) -> Option<String> {
    if SSH_VARS.iter().any(|name| env.var(name).is_some()) {
        return Some(
            "The Floti host starts at your sign-in, not from an SSH session, so it was not started."
                .to_owned(),
        );
    }
    match account {
        Account::Privileged(name) => Some(format!(
            "The Floti host starts at your sign-in as you, not as {name}, so it was not started."
        )),
        Account::User | Account::Unknown => None,
    }
}

/// What `floti connect` does when no host answers it
/// ([`HostStartup::on_connect`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnConnect {
    /// Start `floti host`, with these variables taken out of its
    /// environment.
    Start { env_remove: &'static [&'static str] },
    /// Start nothing: no host runs here, and none starts on connect.
    NoHost,
    /// Start nothing, for this reason: one sentence, which says what to do
    /// instead.
    Refuse(&'static str),
}

/// What `floti connect` does when no host answers it, in the login `env`
/// describes, with the data folder `data`, by this OS's rule.
pub fn on_connect(env: &UserEnv, data: &Path) -> OnConnect {
    Native.on_connect(data, this_login(env).as_ref())
}

/// The rule on an OS whose host starts at sign-in: `floti connect` starts it
/// only where `floti setup` recorded that nothing else would
/// ([`START_ON_CONNECT_FILE`]), with the environment it has.
fn on_connect_where_recorded(data: &Path) -> OnConnect {
    if data.join(START_ON_CONNECT_FILE).is_file() {
        OnConnect::Start { env_remove: &[] }
    } else {
        OnConnect::NoHost
    }
}

/// What the OS says of the login a process runs in, which
/// [`HostStartup::on_connect`] decides by.
pub trait Login {
    /// Whether this process runs with an elevated token (Windows'
    /// `TokenElevation`), or `None` where the OS has no such thing or would
    /// not say.
    fn elevated(&self) -> Option<bool>;

    /// Whether this user has a desktop session on this computer, at the
    /// console or over remote desktop, connected or not. An SSH login is
    /// not one.
    fn desktop_session(&self) -> io::Result<bool>;
}

/// This process's login, as the OS says it is.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeLogin;

impl Login for NativeLogin {
    fn elevated(&self) -> Option<bool> {
        signin::elevated()
    }

    fn desktop_session(&self) -> io::Result<bool> {
        signin::desktop_session()
    }
}

/// A login as it is said to be, for a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaidLogin {
    pub elevated: Option<bool>,
    /// What [`Login::desktop_session`] answers; `None` is an error.
    pub desktop_session: Option<bool>,
}

impl Login for SaidLogin {
    fn elevated(&self) -> Option<bool> {
        self.elevated
    }

    fn desktop_session(&self) -> io::Result<bool> {
        self.desktop_session
            .ok_or_else(|| io::Error::other("the sessions could not be listed"))
    }
}

/// Says what the login of a `floti` run is, in a build with the testkit
/// only, whatever computer runs the test: `plain` (not elevated, no desktop
/// session), `elevated` (elevated, and nobody signed in at the desktop) or
/// `elevated-at-desktop` (elevated, and the user signed in at the desktop).
/// A CI runner's tests run elevated, with a desktop session, so a test that
/// has `floti connect` start a host says which login it runs in. A release
/// build has no such switch.
#[cfg(any(test, feature = "testkit"))]
pub const LOGIN_TEST_VAR: &str = "FLOTI_TEST_LOGIN";

/// The login `env` describes: this process's, or in a build with the
/// testkit, the one [`LOGIN_TEST_VAR`] names.
fn this_login(env: &UserEnv) -> Box<dyn Login> {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(said) = env.var(LOGIN_TEST_VAR) {
        let (elevated, desktop_session) = match said.to_string_lossy().as_ref() {
            "plain" => (false, false),
            "elevated" => (true, false),
            "elevated-at-desktop" => (true, true),
            other => panic!("{LOGIN_TEST_VAR}={other} names no login"),
        };
        return Box::new(SaidLogin {
            elevated: Some(elevated),
            desktop_session: Some(desktop_session),
        });
    }
    #[cfg(not(any(test, feature = "testkit")))]
    let _ = env;
    Box::new(NativeLogin)
}

/// The OS's requests to stop the host, listened for from [`register`]
/// on. Registering replaces the default action, which would end the host
/// without recording its sessions.
///
/// [`register`]: StopSignals::register
pub struct StopSignals(signin::Signals);

impl StopSignals {
    /// Starts listening. Call it from inside a Tokio runtime.
    pub fn register() -> Result<Self> {
        signin::Signals::register().map(Self)
    }

    /// Waits for the next stop request, and says what it was.
    pub async fn recv(&mut self) -> &'static str {
        self.0.recv().await
    }
}

/// How the host starts on an OS: what `floti setup` registers, `floti
/// doctor` looks for and `floti uninstall` takes out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStart {
    /// At the user's sign-in, through this definition.
    AtSignIn(StartupDefinition),
    /// When a client needs one: the desktop app when no host answers it, or
    /// `floti connect`. Nothing is registered and nothing is started at
    /// sign-in or by setup; `leftover` is what an earlier floti registered,
    /// which is taken out.
    WhenNeeded { leftover: LeftoverTask },
}

/// A Task Scheduler task an earlier floti registered, described well enough
/// to find and delete it, and the XML it was imported from, without
/// generating it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeftoverTask {
    /// The task's name in Task Scheduler's root folder.
    pub name: &'static str,
    /// The file name of its XML in [`register::TASK_FILE_FOLDER`] under the
    /// data folder.
    pub file_name: &'static str,
}

/// The OS mechanism a definition is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKind {
    /// A launchd agent in the user's `~/Library/LaunchAgents`.
    LaunchAgent,
    /// A systemd user unit.
    SystemdUserUnit,
}

/// A generated start-at-sign-in definition, ready for the installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupDefinition {
    pub kind: StartupKind,
    /// What the OS knows it by: the launchd label or the unit name.
    pub name: String,
    /// The file name the definition is written as.
    pub file_name: String,
    /// The folder, relative to the user's home, the file belongs in:
    /// `Library/LaunchAgents` or `.config/systemd/user`.
    pub user_folder: &'static str,
    /// The definition itself: a property list or a unit file.
    pub contents: String,
}

impl StartupDefinition {
    /// The file's bytes, UTF-8 for the property list and the unit file.
    pub fn file_bytes(&self) -> Vec<u8> {
        self.contents.clone().into_bytes()
    }
}

/// The OS's way to start the host.
pub trait HostStartup {
    /// How the host whose binary is `binary` starts for this user.
    fn host_start(&self, binary: &Path) -> Result<HostStart>;

    /// Why the host must not start in the login `env` describes, running as
    /// `account`, as one sentence, or `None` when it may. Root and the
    /// Windows service accounts are refused on every OS.
    fn launch_refusal(&self, env: &UserEnv, account: Account) -> Option<String>;

    /// What `floti connect` does when no host answers it, in `login`, with
    /// the data folder `data`.
    fn on_connect(&self, data: &Path, login: &dyn Login) -> OnConnect;
}

/// The binary's path as text, refused unless it is valid Unicode, has no
/// control characters (which no definition format can carry) and is absolute
/// by the target OS's rule, which `is_absolute` gives. The target's rule, not
/// the build's, so each generator behaves the same on every OS.
fn binary_text(binary: &Path, is_absolute: fn(&str) -> bool) -> Result<&str> {
    let text = binary.to_str().ok_or_else(|| {
        Error::Refused(format!(
            "The path to floti, {}, is not valid Unicode, so it cannot be written into a start-at-sign-in definition.",
            binary.display()
        ))
    })?;
    if text.chars().any(char::is_control) {
        return Err(Error::Refused(format!(
            "The path to floti, {text:?}, contains a control character, so it cannot be written into a start-at-sign-in definition."
        )));
    }
    if !is_absolute(text) {
        return Err(Error::Refused(format!(
            "The path to floti, {text}, is not absolute; a start-at-sign-in definition needs the full path."
        )));
    }
    Ok(text)
}

/// `text` escaped for XML character data and attribute values.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}
