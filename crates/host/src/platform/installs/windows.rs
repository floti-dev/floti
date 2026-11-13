//! Windows: the desktop app is the per-user NSIS install, installed or
//! updated by the release's own installer run silently. floti itself is put
//! in place by `platform::standalone`.
//!
//! The installer is started out of any job the caller is in
//! (`CREATE_BREAKAWAY_FROM_JOB`), so it finishes even when what ran
//! `floti update` ends first: a session's shell is in its host's job, which
//! ends with that host however the host comes to stop. A job that refuses
//! to let go is no reason not to install; the installer then starts inside
//! it.
//!
//! A session has no display in session 0, where services and ssh logins
//! run: nothing started there reaches the signed-in user's desktop. The
//! installer makes the Start menu entry.
//!
//! The app is installed in `%LOCALAPPDATA%\Programs\Floti\desktop`, beside
//! floti's own `Programs\Floti\bin`. While it was called Helm it was
//! installed in `Programs\Helm`, as `Helm.exe` with `Uninstall Helm.exe`; such
//! an install is still found, so `floti update` updates it, and the release's
//! installer moves it to the new folder (`build/installer.nsh` runs the old
//! uninstaller, which takes its uninstall entry and Start menu shortcut with
//! it, and installs in the new folder).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Desktop, DesktopRemoval, Installs, desktop_dir_override, file_name};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::platform::windows_path::{self, comparable, inside};
use crate::{Error, Result};

/// What the desktop app is called, and so its program and its uninstaller:
/// `productName` in `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Floti";

/// Where the app is installed, under the per-user Programs folder: beside
/// floti's own `Floti\bin` (`INSTALL_FOLDER` in `build/installer.nsh`).
pub const INSTALL_FOLDER: [&str; 2] = ["Floti", "desktop"];

/// What the app was called before it was Floti, and so the folder under
/// Programs it was installed in, its program and its uninstaller.
pub const HELM_NAME: &str = "Helm";

pub struct Native;

/// One per-user uninstall entry: what it runs to uninstall, and the version
/// it says is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallEntry {
    pub uninstall_string: String,
    pub display_version: String,
}

impl Native {
    fn program(desktop: &Desktop) -> PathBuf {
        windows_path::join(&desktop.path, &[&Self::program_name(desktop)])
    }

    /// The app's program's file name: `Floti.exe`, or `Helm.exe` for an
    /// install from before the rename.
    fn program_name(desktop: &Desktop) -> String {
        format!("{}.exe", desktop.name)
    }

    fn uninstaller(desktop: &Desktop) -> PathBuf {
        windows_path::join(&desktop.path, &[&format!("Uninstall {}.exe", desktop.name)])
    }

    /// Where the app is installed in `programs`, whether it is there or not.
    fn floti_install(programs: &Path) -> Desktop {
        Desktop {
            name: DESKTOP_NAME,
            path: windows_path::join(programs, &INSTALL_FOLDER),
        }
    }

    /// Where an install from before the rename is in `programs`.
    fn helm_install(programs: &Path) -> Desktop {
        Desktop {
            name: HELM_NAME,
            path: windows_path::join(programs, &[HELM_NAME]),
        }
    }

    /// `desktop` when it is an install: its program and its uninstaller are
    /// there. A portable copy has no uninstaller and is not one.
    fn installed(desktop: Desktop) -> Option<Desktop> {
        (Self::program(&desktop).is_file() && Self::uninstaller(&desktop).is_file())
            .then_some(desktop)
    }

    /// What the release's installer leaves in place of `replaced`: the app
    /// in the new folder of the Programs folder `replaced` is in.
    pub fn installed_over(replaced: &Desktop) -> Desktop {
        if replaced.name == DESKTOP_NAME {
            return replaced.clone();
        }
        let programs = replaced
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        Self::floti_install(&programs)
    }

    /// The folder per-user programs are installed in for `env`.
    fn programs(env: &UserEnv) -> Option<PathBuf> {
        // Only a full path: a relative one would be read against whatever
        // folder floti happened to start in.
        desktop_dir_override(env).or_else(|| {
            env.var_any_case("LOCALAPPDATA")
                .map(PathBuf::from)
                .filter(|local| windows_path::is_absolute(local))
                .map(|local| windows_path::join(&local, &["Programs"]))
        })
    }

    /// The version the uninstall entry of the install `desktop` gives,
    /// among `entries`: the one whose uninstaller is that install's.
    pub fn uninstall_version(entries: &[UninstallEntry], desktop: &Desktop) -> Option<String> {
        let uninstaller = comparable(&Self::uninstaller(desktop).to_string_lossy());
        entries
            .iter()
            .find(|entry| {
                let program = entry.uninstall_string.trim_start();
                let program = match program.strip_prefix('"') {
                    Some(quoted) => quoted.split('"').next().unwrap_or_default(),
                    None => program.split(" /").next().unwrap_or_default().trim_end(),
                };
                comparable(program) == uninstaller
            })
            .map(|entry| entry.display_version.trim().to_owned())
            .filter(|version| !version.is_empty())
    }

    /// Why a process in the session `session` has no display: session 0
    /// has none. A session that cannot be told counts as having one.
    pub fn no_display_in(session: Option<u32>) -> Option<String> {
        (session == Some(0)).then(|| {
            "this session has no desktop (it is session 0, where ssh logins and services run)"
                .to_owned()
        })
    }
}

impl Installs for Native {
    fn bundles(&self, desktop: &Desktop, running: &Path) -> bool {
        inside(running, &desktop.path)
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        let programs = Self::programs(env)?;
        Self::installed(Self::floti_install(&programs))
            .or_else(|| Self::installed(Self::helm_install(&programs)))
    }

    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop> {
        // The installed app, which the installer updates; with none, the
        // folder a new install goes in.
        self.desktop(env)
            .or_else(|| Self::programs(env).map(|programs| Self::floti_install(&programs)))
    }

    fn desktop_version(&self, desktop: &Desktop) -> Option<String> {
        Self::uninstall_version(&system::uninstall_entries(), desktop)
    }

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        // installer.nsh runs `floti install-self` itself.
        None
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        if !row.name.eq_ignore_ascii_case(&Self::program_name(desktop)) {
            return false;
        }
        let Some(command_line) = &row.command_line else {
            return true;
        };
        let wanted = comparable(&Self::program(desktop).to_string_lossy());
        comparable(command_line).contains(&wanted)
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (Self::program(desktop), Vec::new())
    }

    fn no_display(&self, _env: &UserEnv) -> Option<String> {
        Self::no_display_in(system::session_id())
    }

    fn removal(&self, env: &UserEnv) -> Option<DesktopRemoval> {
        let desktop = self.desktop(env)?;
        Some(DesktopRemoval {
            uninstaller: Some((
                Self::uninstaller(&desktop),
                format!("/S _?={}", desktop.path.display()),
            )),
            gone_after: Some(Self::program(&desktop)),
            remove: vec![desktop.path.clone()],
            shown: desktop.path,
        })
    }

    fn install_desktop(&self, desktop: &Desktop, release: &Path) -> Result<Desktop> {
        let status = system::run_out_of_job(
            Command::new(release)
                .arg("/S")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .map_err(|source| Error::path("run", release, source))?;
        if status.success() {
            // The installer installs in the new folder, moving an install
            // from before the rename there.
            Ok(Self::installed_over(desktop))
        } else {
            Err(Error::Refused(format!(
                "The {} installer {} ended with {status}.",
                desktop.name,
                file_name(release)
            )))
        }
    }
}

/// What only Windows itself can answer: the uninstall entries in the
/// registry, and starting a program out of a job. Elsewhere there are no
/// entries, and a program starts as it is.
#[cfg(windows)]
mod system {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, ExitStatus};

    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
        RegOpenKeyExW,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, GetCurrentProcessId};

    use super::UninstallEntry;

    /// The Remote Desktop Services session this process runs in.
    pub fn session_id() -> Option<u32> {
        let mut session = 0u32;
        // SAFETY: `session` receives the id; the process id is this one's.
        let ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
        (ok != 0).then_some(session)
    }

    /// Where every per-user uninstall entry is, under `HKEY_CURRENT_USER`.
    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Every per-user uninstall entry that has both values; none when the
    /// key cannot be read.
    pub fn uninstall_entries() -> Vec<UninstallEntry> {
        let path = wide(UNINSTALL_KEY);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is nul-terminated and `key` receives the handle.
        let opened =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) };
        if opened != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut entries = Vec::new();
        for index in 0.. {
            // Registry key names are at most 255 characters.
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            // SAFETY: `name` holds `length` characters; the other outputs
            // are not asked for.
            let status = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                continue;
            }
            let mut subkey = name[..length as usize].to_vec();
            subkey.push(0);
            if let (Some(uninstall_string), Some(display_version)) = (
                string_value(key, &subkey, "UninstallString"),
                string_value(key, &subkey, "DisplayVersion"),
            ) {
                entries.push(UninstallEntry {
                    uninstall_string,
                    display_version,
                });
            }
        }
        // SAFETY: `key` was opened above and is closed once.
        unsafe { RegCloseKey(key) };
        entries
    }

    /// The text value `value` of `key`'s subkey `subkey` (nul-terminated).
    fn string_value(key: HKEY, subkey: &[u16], value: &str) -> Option<String> {
        let value = wide(value);
        let mut size: u32 = 0;
        // SAFETY: both names are nul-terminated; with no buffer the call
        // only says how many bytes the value needs.
        let sized = unsafe {
            RegGetValueW(
                key,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if sized != ERROR_SUCCESS || size == 0 {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `size` bytes.
        let read = unsafe {
            RegGetValueW(
                key,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if read != ERROR_SUCCESS {
            return None;
        }
        let text = &buffer[..(size as usize / 2).min(buffer.len())];
        let text = text.split(|&unit| unit == 0).next().unwrap_or_default();
        Some(String::from_utf16_lossy(text))
    }

    /// Runs `command` out of the caller's job and waits for it; inside the
    /// job when the job refuses to let it go.
    pub fn run_out_of_job(command: &mut Command) -> std::io::Result<ExitStatus> {
        command.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
        match command.status() {
            Err(err) if err.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                command.creation_flags(0);
                command.status()
            }
            status => status,
        }
    }
}

#[cfg(not(windows))]
mod system {
    use std::process::{Command, ExitStatus};

    use super::UninstallEntry;

    pub fn uninstall_entries() -> Vec<UninstallEntry> {
        Vec::new()
    }

    pub fn session_id() -> Option<u32> {
        None
    }

    pub fn run_out_of_job(command: &mut Command) -> std::io::Result<ExitStatus> {
        command.status()
    }
}

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path, _version: &str) {
        let install = Self::floti_install(apps);
        std::fs::create_dir_all(&install.path).expect("make the fake install folder");
        std::fs::write(Self::program(&install), b"fake app").expect("write the fake app");
        std::fs::write(Self::uninstaller(&install), b"fake uninstaller")
            .expect("write the fake uninstaller");
    }

    fn fake_release(&self, _work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>) {
        (
            format!("{DESKTOP_NAME}-{version}-setup.exe"),
            std::fs::read(installer).expect("read the fake installer"),
        )
    }

    fn was_updated(&self, _apps: &Path, _version: &str, record: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(record) else {
            return false;
        };
        text.lines().any(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .is_ok_and(|run| run["args"] == serde_json::json!(["/S"]))
        })
    }
}
