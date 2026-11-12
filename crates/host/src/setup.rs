//! `floti setup`: run from the floti in the user's own folder, on the
//! computer that is to be a host, usually through ssh by the desktop that
//! installed it there.
//!
//! It makes the data folder and the host's key when they are absent, writes
//! the start-at-sign-in definition for this floti (the logon task "Floti
//! host", the LaunchAgent `dev.floti.host` or the user unit `floti.service`),
//! registers it with the OS's per-user mechanism, replacing its own entry
//! when there is one, and starts the host through that mechanism in the
//! user's signed-in session ([`register`]). It never starts the host as its
//! own child, so a setup run over ssh leaves no host under ssh, which
//! [`launch_refusal`](crate::platform::startup::launch_refusal) refuses
//! anyway. With nobody signed in, it starts nothing.
//!
//! On Linux it first turns lingering on for a server, a computer that does
//! not boot to a desktop, so the user's service manager runs the unit from
//! boot with nobody signed in; a computer that boots to a desktop keeps
//! lingering as it is. `floti setup --linger` turns it on either way and
//! `--no-linger` leaves it as it is ([`Linger`]); the desktop's install
//! passes neither. It is turned on with the user's own `loginctl`, never
//! sudo, and a refusal fails setup before anything is registered.
//!
//! First it checks that Claude Code is installed and its version is in the
//! range the Claude Code backend was tested with, found and asked
//! (`claude --version`) the backend's own way and without reading a
//! credential ([`this_computer::claude`]). When it is not, or that could not
//! be told, setup says so on standard output in one sentence naming what to
//! run, and goes on with everything else: an agent missing or out of range
//! changes neither what setup does nor its exit status.
//!
//! Then, when this floti is the one in the user's own folder, where the
//! install scripts and `floti install-self` put it
//! ([`Standalone::binary`](crate::platform::standalone::Standalone::binary)),
//! and that folder is not on the user's `PATH`,
//! it puts it there ([`user_path::put_on_path`]) and says so in one line: on
//! Windows in the user's `Path` value, and on macOS and Linux by appending
//! floti's line to the startup file the user's shell reads, the one
//! `floti doctor` names, which the line names. So every way floti is
//! installed, `curl | sh`, `irm | iex`, the desktop's installer and the
//! desktop's install over ssh, ends with floti on `PATH`. A shell floti knows
//! no startup file of is given the line to add, and a `PATH` that could not
//! be changed is said in one line with that line; neither changes setup's
//! exit status. A floti anywhere else, such as a build run from its own
//! folder, changes nothing.
//!
//! It ends with one line on standard output and an exit status for each
//! outcome:
//!
//! - [`RUNNING_LINE`], status 0: the host is running and answers on its
//!   socket.
//! - [`KEEPS_RUNNING_LINE`], status 0: the same, on a computer where it
//!   keeps running with nobody signed in (the Linux unit with lingering on).
//! - [`AT_NEXT_SIGN_IN_LINE`], status [`AT_NEXT_SIGN_IN_EXIT`]: registered,
//!   and nobody is signed in, so the host starts at the next sign-in.
//! - [`START_ON_CONNECT_LINE`], status 0: in a WSL distro whose systemd user
//!   manager does not answer (`systemctl --user show-environment`), so no
//!   unit would ever run. Nothing is registered or started; setup records
//!   [`START_ON_CONNECT_FILE`] in the data folder, and `floti connect` starts
//!   the host when a client connects. In a distro where the user manager
//!   answers, the unit is registered as on any Linux computer, and the
//!   record is removed.
//! - A failure: one line on standard error starting `floti setup: `, with
//!   the last line the failing OS command wrote, unedited, and status 1. A
//!   refused lingering also names the fix: an administrator there runs
//!   `sudo loginctl enable-linger <user>`.
//!
//! Then, once the host is set up, and only when setting it up did not fail,
//! the desktop app ([`desktop`]): asked about only where setup can ask (a
//! terminal on standard input and output, in the foreground) and the app is
//! offered (a display, somebody signed in, not WSL), and installed through
//! `floti add desktop`;
//! `--desktop` and `--no-desktop` answer in advance. What that step did
//! never changes the exit status above.
//!
//! [`register`]: crate::platform::startup::register

pub mod desktop;

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use floti_protocol::message::ClientKind;
use floti_protocol::noise::KeyPair;

use crate::add;
use crate::client;
pub use crate::connect::START_ON_CONNECT_FILE;
use crate::host_log::LOG_FILE;
use crate::keys::load_or_create_key_pair;
use crate::platform::command_shell::{CommandShell, Native as NativeShell};
use crate::platform::folders::{self, Folders, UserEnv};
use crate::platform::installs;
pub use crate::platform::startup::register::Linger;
use crate::platform::startup::register::{
    NativeSystem, SignInSystem, Started, register_and_start, user_manager_answers,
};
use crate::platform::startup::{self, Account, StartAtSignIn, StartupDefinition, StartupKind};
use crate::platform::terminal_size::{Foreground, Native as NativeTerminal};
use crate::platform::user_path::{self, UserPath};
use crate::server::{HOST_KEY_FILE, local_endpoint};
use crate::this_computer;
pub use desktop::DesktopChoice;
use desktop::DesktopStep;

/// What `floti setup` prints once the host is running.
pub const RUNNING_LINE: &str = "The Floti host is running, and starts at each sign-in.";

/// What `floti setup` prints once the host is running where it keeps running
/// with nobody signed in.
pub const KEEPS_RUNNING_LINE: &str =
    "The Floti host is running, and keeps running with nobody signed in.";

/// What `floti setup` prints in a WSL distro whose systemd user manager does
/// not answer, where the host starts when a client connects.
pub const START_ON_CONNECT_LINE: &str = "The Floti host starts when a client connects.";

/// What `floti setup` prints when nobody is signed in to start it in.
pub const AT_NEXT_SIGN_IN_LINE: &str = "The Floti host starts at the next sign-in.";

/// The exit status when the host was registered but nobody is signed in.
pub const AT_NEXT_SIGN_IN_EXIT: u8 = 10;

/// How long setup waits for the host it started to answer.
pub const ANSWER_DEADLINE: Duration = Duration::from_secs(60);

/// How often it looks.
const LOOK_AGAIN: Duration = Duration::from_millis(100);

/// The name the host's log sees setup's one connection as.
const CLIENT_NAME: &str = "floti setup";

/// How setup ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupOutcome {
    /// The host is registered, started, and answers.
    Running,
    /// The host is registered, started, and answers, and keeps running with
    /// nobody signed in.
    KeepsRunning,
    /// The host is registered and starts at the next sign-in.
    AtNextSignIn,
    /// Nothing is registered: in a WSL distro with no systemd user manager,
    /// `floti connect` starts the host when a client connects.
    StartsOnConnect,
    /// Why it failed, as one line.
    Failed(String),
}

impl SetupOutcome {
    /// Whether setup goes on to the desktop app: the host is set up, running
    /// or not.
    pub fn offers_desktop(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }

    /// The process's exit status.
    pub fn code(&self) -> u8 {
        match self {
            Self::Running | Self::KeepsRunning | Self::StartsOnConnect => 0,
            Self::AtNextSignIn => AT_NEXT_SIGN_IN_EXIT,
            Self::Failed(_) => 1,
        }
    }

    /// The one line on standard output, or `None` for a failure, which goes
    /// to standard error.
    pub fn printed(&self) -> Option<&'static str> {
        match self {
            Self::Running => Some(RUNNING_LINE),
            Self::KeepsRunning => Some(KEEPS_RUNNING_LINE),
            Self::AtNextSignIn => Some(AT_NEXT_SIGN_IN_LINE),
            Self::StartsOnConnect => Some(START_ON_CONNECT_LINE),
            Self::Failed(_) => None,
        }
    }
}

/// What `floti setup` was told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SetupOptions {
    pub linger: Linger,
    pub desktop: DesktopChoice,
}

/// What follows `setup` on the command line: at most one of `--linger` and
/// `--no-linger`, and at most one of `--desktop` and `--no-desktop`, in any
/// order. `None` for anything else, both of a pair or one given twice
/// included.
pub fn parse(args: &[&str]) -> Option<SetupOptions> {
    fn once<T>(slot: &mut Option<T>, value: T) -> Option<()> {
        match slot {
            Some(_) => None,
            None => {
                *slot = Some(value);
                Some(())
            }
        }
    }
    let (mut linger, mut desktop) = (None, None);
    for arg in args {
        match *arg {
            "--linger" => once(&mut linger, Linger::On)?,
            "--no-linger" => once(&mut linger, Linger::AsItIs)?,
            "--desktop" => once(&mut desktop, DesktopChoice::Install)?,
            "--no-desktop" => once(&mut desktop, DesktopChoice::Skip)?,
            _ => return None,
        }
    }
    Some(SetupOptions {
        linger: linger.unwrap_or_default(),
        desktop: desktop.unwrap_or_default(),
    })
}

/// Runs `floti setup` for the floti this process is, as `options` say, says
/// how it ended, then settles the desktop app when the host is set up, and
/// answers the host's exit status.
pub fn run_setup(options: SetupOptions) -> u8 {
    let linger = options.linger;
    let env = UserEnv::from_process();
    for sentence in [claude_sentence(&env), native_path_sentence(&env)]
        .into_iter()
        .flatten()
    {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{sentence}").and_then(|()| out.flush());
    }
    let outcome = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(native_setup(linger)),
        Err(err) => SetupOutcome::Failed(format!("could not start: {err}")),
    };
    match &outcome {
        SetupOutcome::Failed(why) => {
            let line = why.replace(['\r', '\n'], " ");
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "floti setup: {line}").and_then(|()| err.flush());
        }
        done => {
            let mut out = std::io::stdout().lock();
            let line = done.printed().unwrap_or_default();
            let _ = writeln!(out, "{line}").and_then(|()| out.flush());
        }
    }
    if outcome.offers_desktop() {
        native_desktop(options.desktop, outcome != SetupOutcome::AtNextSignIn);
    }
    outcome.code()
}

/// Setup's desktop step on this computer, for this process's user, asking on
/// its own standard input and output; `signed_in` is whether setup found
/// somebody signed in.
fn native_desktop(choice: DesktopChoice, signed_in: bool) {
    let env = UserEnv::from_process();
    let stdin = std::io::stdin();
    let terminal = desktop::can_ask(
        stdin.is_terminal(),
        std::io::stdout().is_terminal(),
        NativeTerminal.owns_standard_input(),
    );
    DesktopStep {
        choice,
        installs: &installs::Native,
        env: &env,
        signed_in,
        terminal,
        input: &mut stdin.lock(),
        out: &mut std::io::stdout(),
        err: &mut std::io::stderr(),
        add: &mut add::add_desktop_here,
    }
    .run();
}

/// What setup says of Claude Code for the user `env` describes, as one
/// sentence naming what to run, or `None` when it is installed and in the
/// tested range.
pub fn claude_sentence(env: &UserEnv) -> Option<String> {
    this_computer::claude_here(env, NativeShell.family())
        .sentence()
        .map(|sentence| sentence.replace(['\r', '\n'], " "))
}

/// What setup says of putting `floti`, this program's own path, on the
/// `PATH` of the user `env` describes through `path`, when it is `own`, the
/// floti in the user's own folder: the line [`user_path::Added::sentence`]
/// says, or why the `PATH` could not be changed and the line that changes
/// it. `None` when nothing was changed, or `floti` is not `own`.
pub fn path_sentence(
    path: &dyn UserPath,
    env: &UserEnv,
    floti: &Path,
    own: Option<&Path>,
) -> Option<String> {
    if !own.is_some_and(|own| crate::install_self::same_file(own, floti)) {
        return None;
    }
    let folder = floti.parent()?;
    match user_path::put_on_path(path, env, floti) {
        Ok(added) => added.sentence(folder),
        Err(why) => Some(format!(
            "Floti could not put {} on your PATH: {}; this puts it there: {}",
            folder.display(),
            why.replace(['\r', '\n'], " "),
            path.fix(env, folder)
        )),
    }
}

/// Setup's `PATH` step on this computer, for the floti this process is.
#[cfg(not(feature = "testkit"))]
fn native_path_sentence(env: &UserEnv) -> Option<String> {
    use crate::platform::standalone::{Native as NativeStandalone, Standalone};
    let floti = std::env::current_exe().ok()?;
    let own = NativeStandalone.binary(env);
    path_sentence(&user_path::Native, env, &floti, own.as_deref())
}

/// In a test build of floti, which the crate's tests run as a program, the
/// user's `PATH` is never changed: the step itself is tested against a fake
/// `PATH` ([`path_sentence`]).
#[cfg(feature = "testkit")]
fn native_path_sentence(_env: &UserEnv) -> Option<String> {
    None
}

async fn native_setup(linger: Linger) -> SetupOutcome {
    if let Account::Privileged(name) = startup::account() {
        return SetupOutcome::Failed(format!(
            "The Floti host starts at your sign-in as you, not as {name}; run floti setup as yourself."
        ));
    }
    let binary = match std::env::current_exe() {
        Ok(binary) => binary,
        Err(err) => {
            return SetupOutcome::Failed(format!("could not tell where this floti is: {err}"));
        }
    };
    let definition = match startup::Native.definition(&binary) {
        Ok(definition) => definition,
        Err(err) => return SetupOutcome::Failed(err.to_string()),
    };
    setup(
        &UserEnv::from_process(),
        &NativeSystem,
        &definition,
        linger,
        ANSWER_DEADLINE,
    )
    .await
}

/// Sets up the host for the user `env` describes, registering `definition`
/// through `system` with lingering settled as `linger` says, and, once it is
/// started, waits up to `answer_within` for it to answer.
pub async fn setup(
    env: &UserEnv,
    system: &dyn SignInSystem,
    definition: &StartupDefinition,
    linger: Linger,
    answer_within: Duration,
) -> SetupOutcome {
    let folders = match folders::resolve(env) {
        Ok(folders) => folders,
        Err(err) => return SetupOutcome::Failed(err.to_string()),
    };
    let home = match env.require_home() {
        Ok(home) => home.to_path_buf(),
        Err(err) => return SetupOutcome::Failed(err.to_string()),
    };
    let key = match data_folder_and_key(&folders.data) {
        Ok(key) => key,
        Err(why) => return SetupOutcome::Failed(why),
    };
    let on_connect = folders.data.join(START_ON_CONNECT_FILE);
    if definition.kind == StartupKind::SystemdUserUnit
        && system.inside_wsl()
        && !user_manager_answers(system)
    {
        // Nothing would start a unit here: `floti connect` starts the host.
        return match std::fs::write(&on_connect, START_ON_CONNECT_LINE) {
            Ok(()) => SetupOutcome::StartsOnConnect,
            Err(err) => {
                SetupOutcome::Failed(format!("could not write {}: {err}", on_connect.display()))
            }
        };
    }
    if let Err(err) = std::fs::remove_file(&on_connect)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return SetupOutcome::Failed(format!("could not remove {}: {err}", on_connect.display()));
    }
    match register_and_start(system, definition, &home, &folders.data, linger) {
        Ok(Started::AtNextSignIn) => SetupOutcome::AtNextSignIn,
        Ok(started @ (Started::Running | Started::KeepsRunning)) => {
            if !answers(&folders, &key, answer_within).await {
                SetupOutcome::Failed(format!(
                    "the host was started but did not answer within {answer_within:?}; its log is {}",
                    folders.logs.join(LOG_FILE).display()
                ))
            } else if started == Started::KeepsRunning {
                SetupOutcome::KeepsRunning
            } else {
                SetupOutcome::Running
            }
        }
        Err(why) => SetupOutcome::Failed(why),
    }
}

/// Makes the data folder and the host's key when they are absent, and
/// answers the key, which the host proves when it answers.
fn data_folder_and_key(data: &Path) -> Result<KeyPair, String> {
    std::fs::create_dir_all(data)
        .map_err(|err| format!("could not make {}: {err}", data.display()))?;
    load_or_create_key_pair(&data.join(HOST_KEY_FILE)).map_err(|err| err.to_string())
}

/// Whether the host on `folders` answers within `within`: it takes a
/// connection proving its own `key`, as `floti devices` makes, and welcomes
/// it.
async fn answers(folders: &Folders, key: &KeyPair, within: Duration) -> bool {
    let endpoint = local_endpoint(folders);
    let deadline = Instant::now() + within;
    loop {
        if let Ok(mut connection) = client::connect(&endpoint, key, &key.public()).await {
            let welcomed = connection.say_hello(CLIENT_NAME, ClientKind::Other).await;
            connection.close().await;
            if welcomed.is_ok() {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(LOOK_AGAIN).await;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use super::*;
    use crate::platform::user_path::Added;
    use crate::testkit::TestDirs;

    /// A user `PATH` that holds floti's folder when `on`, and records what
    /// it was asked to add.
    struct FakePath {
        on: bool,
        refused: Option<&'static str>,
        added: RefCell<Vec<PathBuf>>,
    }

    impl FakePath {
        fn new(on: bool) -> Self {
            Self {
                on,
                refused: None,
                added: RefCell::new(Vec::new()),
            }
        }
    }

    impl UserPath for FakePath {
        fn has(&self, _env: &UserEnv, _floti: &Path) -> Result<bool, String> {
            Ok(self.on)
        }

        fn fix(&self, _env: &UserEnv, folder: &Path) -> String {
            format!("add {}", folder.display())
        }

        fn add(&self, _env: &UserEnv, folder: &Path) -> Result<Added, String> {
            if let Some(why) = self.refused {
                return Err(why.to_owned());
            }
            self.added.borrow_mut().push(folder.to_path_buf());
            Ok(Added::Put {
                file: Some(folder.join("startup file")),
            })
        }

        fn remove(&self, _env: &UserEnv, _folder: &Path) -> Result<bool, String> {
            unreachable!("setup never takes floti off PATH")
        }
    }

    /// floti in the user's own folder of the test's tree.
    fn own_floti(dirs: &TestDirs) -> PathBuf {
        let folder = dirs.home.join(".local").join("bin");
        std::fs::create_dir_all(&folder).unwrap();
        let floti = folder.join("floti");
        std::fs::write(&floti, b"floti").unwrap();
        floti
    }

    #[test]
    fn setup_puts_the_users_own_floti_on_path_and_says_which_file() {
        let dirs = TestDirs::new();
        let floti = own_floti(&dirs);
        let folder = floti.parent().unwrap();
        let path = FakePath::new(false);
        let said = path_sentence(&path, &dirs.user_env(), &floti, Some(&floti));
        assert_eq!(*path.added.borrow(), [folder.to_path_buf()]);
        assert_eq!(
            said,
            Some(format!(
                "Put {} on your PATH in {}, for terminals and logins started from now on.",
                folder.display(),
                folder.join("startup file").display()
            ))
        );
    }

    #[test]
    fn setup_adds_nothing_where_floti_is_on_path_already_or_is_not_the_users_own() {
        let dirs = TestDirs::new();
        let floti = own_floti(&dirs);
        let on = FakePath::new(true);
        assert_eq!(
            path_sentence(&on, &dirs.user_env(), &floti, Some(&floti)),
            None
        );
        assert!(on.added.borrow().is_empty());

        // A floti elsewhere, such as a build run from its own folder.
        let elsewhere = dirs.folder("a build").join("floti");
        std::fs::write(&elsewhere, b"floti").unwrap();
        let off = FakePath::new(false);
        for own in [Some(floti.as_path()), None] {
            assert_eq!(path_sentence(&off, &dirs.user_env(), &elsewhere, own), None);
        }
        assert!(off.added.borrow().is_empty());
    }

    #[test]
    fn a_path_that_could_not_be_changed_is_one_line_with_the_fix() {
        let dirs = TestDirs::new();
        let floti = own_floti(&dirs);
        let folder = floti.parent().unwrap();
        let path = FakePath {
            refused: Some("could not add to ~/.profile:\nread-only"),
            ..FakePath::new(false)
        };
        assert_eq!(
            path_sentence(&path, &dirs.user_env(), &floti, Some(&floti)),
            Some(format!(
                "Floti could not put {} on your PATH: could not add to ~/.profile: read-only; this puts it there: add {}",
                folder.display(),
                folder.display()
            ))
        );
    }
}
