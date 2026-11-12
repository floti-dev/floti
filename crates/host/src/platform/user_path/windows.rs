//! Windows: the system's and the user's `Path` in the registry.

use std::path::Path;

use super::UserPath;
use crate::platform::folders::UserEnv;
use crate::platform::windows_path::{comparable, split};

/// The Windows user `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// Where the system's `Path` is kept, under `HKEY_LOCAL_MACHINE`.
pub const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Where the user's `Path` is kept, under `HKEY_CURRENT_USER`.
pub const USER_KEY: &str = "Environment";

impl UserPath for Native {
    fn has(&self, env: &UserEnv, floti: &Path) -> Result<bool, String> {
        let system = registry::string_value(registry::Hive::LocalMachine, SYSTEM_KEY, "Path")?;
        let user = registry::string_value(registry::Hive::CurrentUser, USER_KEY, "Path")?;
        Ok(names(
            [system.as_deref(), user.as_deref()].into_iter().flatten(),
            env,
            floti,
        ))
    }

    fn fix(&self, _env: &UserEnv, folder: &Path) -> String {
        let folder = folder.display().to_string().replace('\'', "''");
        // The value as written, variables and all, and back as an
        // expandable string: [Environment]::SetEnvironmentVariable would
        // write the expanded value as a plain one, freezing every %VARIABLE%.
        format!(
            "$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); $path = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames').TrimEnd(';'); $key.SetValue('Path', (($path, '{folder}') -join ';').TrimStart(';'), 'ExpandString')"
        )
    }

    fn add(&self, _env: &UserEnv, folder: &Path) -> Result<bool, String> {
        let written = registry::string_value(registry::Hive::CurrentUser, USER_KEY, "Path")?;
        let value = with_folder(written.as_deref(), &folder.display().to_string());
        registry::set_expandable_string(USER_KEY, "Path", &value)?;
        registry::announce_environment();
        Ok(true)
    }

    fn remove(&self, env: &UserEnv, folder: &Path) -> Result<bool, String> {
        let written = registry::string_value(registry::Hive::CurrentUser, USER_KEY, "Path")?;
        let Some(left) = without_folder(written.as_deref(), &folder.display().to_string(), env)
        else {
            return Ok(false);
        };
        if left.is_empty() {
            registry::delete_value(USER_KEY, "Path")?;
        } else {
            registry::set_expandable_string(USER_KEY, "Path", &left)?;
        }
        registry::announce_environment();
        Ok(true)
    }
}

/// The user's `Path` value `written`, as the registry keeps it, without each
/// entry that names `folder`: as written, or with its `%VARIABLE%`s expanded
/// from `env`, compared without case, quotes or a trailing separator, as
/// Windows finds programs. The other entries stay as written, in their
/// order, and empty entries go with it. `None` when no entry names it.
pub fn without_folder(written: Option<&str>, folder: &str, env: &UserEnv) -> Option<String> {
    let wanted = comparable(folder);
    let names_it = |entry: &str| {
        let entry = entry.trim().trim_matches('"');
        comparable(entry) == wanted || comparable(&expand(entry, env)) == wanted
    };
    let entries: Vec<&str> = written.unwrap_or_default().split(';').collect();
    if !entries.iter().any(|entry| names_it(entry)) {
        return None;
    }
    Some(
        entries
            .into_iter()
            .filter(|entry| !entry.trim().is_empty() && !names_it(entry))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// The user's `Path` value `written`, as the registry keeps it (its
/// `%VARIABLE%`s unexpanded), with `folder` appended, as [`UserPath::fix`]'s
/// line appends it: no empty entry at either end.
pub fn with_folder(written: Option<&str>, folder: &str) -> String {
    let written = written.unwrap_or_default().trim_end_matches(';');
    format!("{written};{folder}")
        .trim_start_matches(';')
        .to_owned()
}

/// Whether one of `values`, each a `Path` value as the registry keeps it,
/// has `floti`: an entry, with its `%VARIABLE%`s expanded from `env`, that is
/// floti's own folder, compared without case and without a trailing
/// separator as Windows finds programs, or one whose `floti.exe` is this
/// one by another name.
pub fn names<'a>(values: impl IntoIterator<Item = &'a str>, env: &UserEnv, floti: &Path) -> bool {
    let written = floti.to_string_lossy();
    let (folder, name) = split(&written);
    let wanted = folder.map(comparable);
    let this = std::fs::canonicalize(floti).ok();
    values
        .into_iter()
        .flat_map(|value| value.split(';'))
        .map(|entry| expand(entry.trim().trim_matches('"'), env))
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            wanted.as_deref() == Some(comparable(&entry).as_str())
                || this.as_ref().is_some_and(|this| {
                    std::fs::canonicalize(Path::new(&entry).join(&name))
                        .ok()
                        .as_ref()
                        == Some(this)
                })
        })
}

/// `entry` with each `%NAME%` replaced by that variable of `env`, and left
/// as written where `env` has no such variable, as Windows leaves it.
fn expand(entry: &str, env: &UserEnv) -> String {
    let mut out = String::with_capacity(entry.len());
    let mut rest = entry;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match env.var_any_case(name) {
                    Some(value) => out.push_str(&value.to_string_lossy()),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(windows)]
pub(super) mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_EXPAND_SZ, RRF_NOEXPAND,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    /// The hive a key is read from.
    #[derive(Debug, Clone, Copy)]
    pub enum Hive {
        LocalMachine,
        CurrentUser,
    }

    impl Hive {
        fn key(self) -> HKEY {
            match self {
                Self::LocalMachine => HKEY_LOCAL_MACHINE,
                Self::CurrentUser => HKEY_CURRENT_USER,
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::LocalMachine => "HKEY_LOCAL_MACHINE",
                Self::CurrentUser => "HKEY_CURRENT_USER",
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The string value `value` of `subkey` in `hive`, as written, its
    /// variables not expanded; `None` where the key or the value is not
    /// there. Read only: `RegGetValueW` opens the key for reading and closes
    /// it.
    pub fn string_value(hive: Hive, subkey: &str, value: &str) -> Result<Option<String>, String> {
        let subkey_w = wide(subkey);
        let value_w = wide(value);
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut buffer: Vec<u16> = vec![0; 2048];
        loop {
            let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
            // SAFETY: both names are NUL-terminated UTF-16, and `bytes` is
            // the size of `buffer` in bytes, which RegGetValueW writes within.
            let status = unsafe {
                RegGetValueW(
                    hive.key(),
                    subkey_w.as_ptr(),
                    value_w.as_ptr(),
                    flags,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &raw mut bytes,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    let units = (bytes as usize / 2).min(buffer.len());
                    let text = String::from_utf16_lossy(&buffer[..units]);
                    return Ok(Some(text.trim_end_matches('\0').to_owned()));
                }
                ERROR_FILE_NOT_FOUND => return Ok(None),
                ERROR_MORE_DATA => buffer = vec![0; (bytes as usize).div_ceil(2) + 1],
                code => {
                    return Err(format!(
                        "could not read Path in {}\\{subkey}: {}",
                        hive.name(),
                        std::io::Error::from_raw_os_error(code as i32)
                    ));
                }
            }
        }
    }

    /// Writes `text` as the expandable string value `value` of `subkey` in
    /// `HKEY_CURRENT_USER`, making the key when it is not there, so each
    /// `%VARIABLE%` in it is expanded when a process's environment is built
    /// from it rather than frozen now.
    pub fn set_expandable_string(subkey: &str, value: &str, text: &str) -> Result<(), String> {
        let subkey_w = wide(subkey);
        let value_w = wide(value);
        let data = wide(text);
        let bytes =
            u32::try_from(data.len() * 2).map_err(|_| format!("{value} is too long to write"))?;
        // SAFETY: both names and the data are NUL-terminated UTF-16, and
        // `bytes` is the data's size in bytes, its NUL included.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                subkey_w.as_ptr(),
                value_w.as_ptr(),
                REG_EXPAND_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "could not write {value} in HKEY_CURRENT_USER\\{subkey}: {}",
                std::io::Error::from_raw_os_error(status as i32)
            ))
        }
    }

    /// Deletes the value `value` of `subkey` in `HKEY_CURRENT_USER`; one
    /// that is not there is already deleted.
    pub fn delete_value(subkey: &str, value: &str) -> Result<(), String> {
        let subkey_w = wide(subkey);
        let value_w = wide(value);
        // SAFETY: both names are NUL-terminated UTF-16 that outlive the call.
        let status =
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey_w.as_ptr(), value_w.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!(
                "could not delete {value} in HKEY_CURRENT_USER\\{subkey}: {}",
                std::io::Error::from_raw_os_error(status as i32)
            ))
        }
    }

    /// Tells the programs that build environments for new processes, such
    /// as Explorer, that the user's environment changed, so a terminal
    /// opened from them now has it. A program that does not answer within
    /// five seconds is not waited for.
    pub fn announce_environment() {
        let environment = wide("Environment");
        let mut result: usize = 0;
        // SAFETY: the message's text is a NUL-terminated UTF-16 string that
        // outlives the call, and `result` is a valid place to write.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5000,
                &raw mut result,
            );
        }
    }
}

#[cfg(not(windows))]
pub(super) mod registry {
    /// The hive a key is read from.
    #[derive(Debug, Clone, Copy)]
    pub enum Hive {
        LocalMachine,
        CurrentUser,
    }

    /// There is no registry off Windows.
    pub fn string_value(
        _hive: Hive,
        _subkey: &str,
        _value: &str,
    ) -> Result<Option<String>, String> {
        Err("there is no Windows registry on this computer".to_owned())
    }

    /// There is no registry off Windows.
    pub fn set_expandable_string(_subkey: &str, _value: &str, _text: &str) -> Result<(), String> {
        Err("there is no Windows registry on this computer".to_owned())
    }

    /// There is no registry off Windows.
    pub fn delete_value(_subkey: &str, _value: &str) -> Result<(), String> {
        Err("there is no Windows registry on this computer".to_owned())
    }

    /// There is nobody to tell off Windows.
    pub fn announce_environment() {}
}
