//! Windows: the system's and the user's `Path` in the registry.

use std::path::Path;

use super::UserPath;
use crate::platform::folders::UserEnv;

/// The Windows user `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// Where the system's `Path` is kept, under `HKEY_LOCAL_MACHINE`.
pub const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Where the user's `Path` is kept, under `HKEY_CURRENT_USER`.
pub const USER_KEY: &str = "Environment";

impl UserPath for Native {
    fn has(&self, env: &UserEnv, folder: &Path) -> Result<bool, String> {
        let system = registry::string_value(registry::Hive::LocalMachine, SYSTEM_KEY, "Path")?;
        let user = registry::string_value(registry::Hive::CurrentUser, USER_KEY, "Path")?;
        Ok(names(
            [system.as_deref(), user.as_deref()].into_iter().flatten(),
            env,
            folder,
        ))
    }

    fn fix(&self, _env: &UserEnv, folder: &Path) -> String {
        let folder = folder.display().to_string().replace('\'', "''");
        format!(
            "[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';{folder}', 'User')"
        )
    }
}

/// Whether one of `values`, each a `Path` value as the registry keeps it,
/// names `folder`: each entry with its `%VARIABLE%`s expanded from `env`,
/// compared without case and without a trailing separator, as Windows finds
/// programs.
pub fn names<'a>(values: impl IntoIterator<Item = &'a str>, env: &UserEnv, folder: &Path) -> bool {
    let wanted = comparable(&folder.display().to_string());
    values
        .into_iter()
        .flat_map(|value| value.split(';'))
        .map(|entry| comparable(&expand(entry.trim().trim_matches('"'), env)))
        .any(|entry| !entry.is_empty() && entry == wanted)
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

/// `path` as Windows compares folders: forward slashes as backslashes, no
/// trailing separator, any case.
fn comparable(path: &str) -> String {
    let path = path.replace('/', "\\");
    let trimmed = path.trim_end_matches('\\');
    // A drive's root keeps its separator: `C:\` and `C:` are not one folder.
    let trimmed = if trimmed.ends_with(':') {
        path.as_str()
    } else {
        trimmed
    };
    trimmed.to_lowercase()
}

#[cfg(windows)]
pub(super) mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ, RegGetValueW,
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
}
