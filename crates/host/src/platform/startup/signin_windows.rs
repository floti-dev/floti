//! The host's place in the user's sign-in on Windows: whether it runs as a
//! service account or elevated, whether the user has a desktop session, and
//! what stops it: the console's events (Ctrl+C, Ctrl+Break, the console
//! closing) and the session ending, signing out or shutting down, which its
//! end-session window hears
//! ([`end_session_windows`](super::end_session_windows)).

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr;

use tokio::signal::windows::{CtrlBreak, CtrlC, CtrlClose};
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::Account;
use super::end_session_windows::EndSession;
pub(super) use super::end_session_windows::Stopped;
use crate::{Error, Result};

/// The machine's Windows build number, from `RtlGetVersion`, which answers
/// the real version where `GetVersionEx` answers what the program's manifest
/// claims to support.
pub(super) fn windows_build() -> Option<u32> {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    // SAFETY: a zeroed OSVERSIONINFOW with its size set is what
    // RtlGetVersion fills; it writes only inside it.
    let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    let status = unsafe { RtlGetVersion(&mut info) };
    (status == 0).then_some(info.dwBuildNumber)
}

/// The service accounts a host must never run as, by their well-known SIDs.
const SERVICE_ACCOUNTS: [(&str, &str); 3] = [
    ("S-1-5-18", "the SYSTEM account"),
    ("S-1-5-19", "the LOCAL SERVICE account"),
    ("S-1-5-20", "the NETWORK SERVICE account"),
];

pub(super) fn account() -> Account {
    match token_user_sid() {
        Ok(sid) => SERVICE_ACCOUNTS
            .iter()
            .find(|(known, _)| *known == sid)
            .map_or(Account::User, |(_, name)| Account::Privileged(name)),
        Err(_) => Account::Unknown,
    }
}

/// Whether this process's token is elevated (`TokenElevation`): an
/// administrator's full token, which OpenSSH gives an administrator's login
/// and "Run as administrator" gives a program. `None` when Windows would not
/// say.
pub(super) fn elevated() -> Option<bool> {
    let token = process_token().ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut written = 0u32;
    // SAFETY: `elevation` is a TOKEN_ELEVATION, the size passed is its own,
    // and the token is open for querying while `token` lives.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut written,
        )
    };
    (ok != 0).then_some(elevation.TokenIsElevated != 0)
}

/// This process's token, open for querying.
fn process_token() -> io::Result<OwnedHandle> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle needs no closing; `raw` receives a token
    // handle, owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken succeeded, so `raw` is a handle we own.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) })
}

/// The SID of the user this process's token belongs to, as text.
fn token_user_sid() -> io::Result<String> {
    let token = process_token()?;
    let token = token.as_raw_handle() as HANDLE;

    let mut needed = 0u32;
    // SAFETY: a size query with no buffer; it fails and sets `needed`.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buffer` holds at least `needed` bytes, aligned for TOKEN_USER.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetTokenInformation(TokenUser) wrote a TOKEN_USER at the start
    // of the buffer, whose SID points into the same buffer.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is valid while `buffer` lives; on success `text` is a
    // NUL-terminated string from LocalAlloc, freed below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is NUL-terminated.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `len` units precede the NUL.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(text.cast::<c_void>()) };
    Ok(sid)
}

/// Whether this user (`USERDOMAIN\USERNAME`) has a desktop session on this
/// computer: a session other than session 0, where services run, whose
/// user it is, at the console or over remote desktop, connected or not. An
/// ssh login runs outside any such session, so it never counts.
pub(super) fn desktop_session() -> io::Result<bool> {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSActive, WTSDisconnected, WTSDomainName,
        WTSEnumerateSessionsW, WTSFreeMemory, WTSUserName,
    };

    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
    };
    let Some(user) = var("USERNAME").filter(|user| !user.is_empty()) else {
        return Err(io::Error::other(
            "USERNAME is not set, so the user's sessions cannot be told apart",
        ));
    };
    let domain = var("USERDOMAIN").unwrap_or_default();

    let query = |session: u32, class| -> Option<String> {
        let mut text: windows_sys::core::PWSTR = ptr::null_mut();
        let mut bytes = 0u32;
        // SAFETY: on success `text` is a NUL-terminated string the API
        // allocated, freed below.
        let ok = unsafe {
            windows_sys::Win32::System::RemoteDesktop::WTSQuerySessionInformationW(
                WTS_CURRENT_SERVER_HANDLE,
                session,
                class,
                &mut text,
                &mut bytes,
            )
        };
        if ok == 0 || text.is_null() {
            return None;
        }
        // SAFETY: `text` is NUL-terminated.
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        // SAFETY: `len` units precede the NUL.
        let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        // SAFETY: allocated by WTSQuerySessionInformationW.
        unsafe { WTSFreeMemory(text.cast()) };
        Some(value)
    };

    let mut sessions: *mut WTS_SESSION_INFOW = ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: on success `sessions` points at `count` entries the API
    // allocated, freed below.
    if unsafe { WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut sessions, &mut count) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `count` entries were written at `sessions`.
    let listed = unsafe { std::slice::from_raw_parts(sessions, count as usize) };
    let found = listed.iter().any(|session| {
        session.SessionId != 0
            && (session.State == WTSActive || session.State == WTSDisconnected)
            && query(session.SessionId, WTSUserName)
                .is_some_and(|name| name.eq_ignore_ascii_case(&user))
            && (domain.is_empty()
                || query(session.SessionId, WTSDomainName)
                    .is_some_and(|name| name.eq_ignore_ascii_case(&domain)))
    });
    // SAFETY: allocated by WTSEnumerateSessionsW.
    unsafe { WTSFreeMemory(sessions.cast()) };
    Ok(found)
}

/// What stops the host, listened for while this lives: the console's events,
/// for `floti host` run in a terminal, and the end-session window.
///
/// The console's logoff and shutdown events are not among them: Windows
/// sends those only to a console process that has not loaded user32 or
/// gdi32, and floti loads user32, so signing out and shutting down reach the
/// host as window messages, with or without a console.
pub(super) struct Signals {
    ctrl_c: CtrlC,
    ctrl_break: CtrlBreak,
    close: CtrlClose,
    end_session: EndSession,
}

impl Signals {
    pub(super) fn register() -> Result<Self> {
        let failed = |err| Error::io("listen for the console's stop events", err);
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c().map_err(failed)?,
            ctrl_break: tokio::signal::windows::ctrl_break().map_err(failed)?,
            close: tokio::signal::windows::ctrl_close().map_err(failed)?,
            end_session: EndSession::start(),
        })
    }

    pub(super) async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.ctrl_c.recv() => "Ctrl+C",
            _ = self.ctrl_break.recv() => "Ctrl+Break",
            _ = self.close.recv() => "the console closing",
            Some(why) = self.end_session.recv() => why,
        }
    }

    pub(super) fn stopped(&self) -> Stopped {
        self.end_session.stopped()
    }
}

/// Makes `command` a process [`interrupt`] can reach: the console sends
/// Ctrl+Break to a process group, so it gets one of its own.
#[cfg(any(test, feature = "testkit"))]
pub fn stoppable(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

/// Interrupts process `pid`, started through [`stoppable`], as the keyboard
/// would: Ctrl+Break, the interrupt a console delivers to a process group of
/// its own.
#[cfg(any(test, feature = "testkit"))]
pub fn interrupt(pid: u32) -> io::Result<()> {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    // A group id of 0 would mean every process on this console, the caller
    // included.
    if pid == 0 {
        return Err(io::Error::other("no process to interrupt"));
    }
    // SAFETY: plain call; `pid` names the group `stoppable` created.
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Asks process `pid` to terminate. Windows has no SIGTERM for a console
/// program; the request it gets is a console event, as [`interrupt`] sends.
#[cfg(any(test, feature = "testkit"))]
pub fn terminate(pid: u32) -> io::Result<()> {
    interrupt(pid)
}

/// Makes `command` start with no console at all, as Floti starts a host
/// (`DETACHED_PROCESS`), in a process group of its own as [`stoppable`]
/// gives it.
#[cfg(any(test, feature = "testkit"))]
pub fn without_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// The top-level windows of class `class` that process `pid` made, as
/// addresses: what a test sends Windows' session messages to.
#[cfg(any(test, feature = "testkit"))]
pub fn top_level_windows(pid: u32, class: &str) -> floti_protocol::Knowable<Vec<isize>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    let class: Vec<u16> = class.encode_utf16().chain(std::iter::once(0)).collect();
    let mut found = Vec::new();
    let mut after = ptr::null_mut();
    loop {
        // SAFETY: top-level windows (no parent) of a nul-terminated class,
        // after the last one found.
        after = unsafe { FindWindowExW(ptr::null_mut(), after, class.as_ptr(), ptr::null()) };
        if after.is_null() {
            return floti_protocol::Knowable::Known(found);
        }
        let mut owner = 0u32;
        // SAFETY: a window just found, and a place for its process id.
        unsafe { GetWindowThreadProcessId(after, &mut owner) };
        if owner == pid {
            found.push(after as isize);
        }
    }
}

/// Sends `window` `message` as Windows does when the session ends, and
/// returns its answer, failing when it has not answered within `within`.
#[cfg(any(test, feature = "testkit"))]
pub fn send_session_message(
    window: isize,
    message: super::SessionMessage,
    within: std::time::Duration,
) -> io::Result<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ENDSESSION_LOGOFF, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_ENDSESSION, WM_QUERYENDSESSION,
    };
    let flags = |sign_out: bool| {
        if sign_out {
            ENDSESSION_LOGOFF as usize as isize
        } else {
            0
        }
    };
    let (id, wparam, lparam) = match message {
        super::SessionMessage::Query { sign_out } => (WM_QUERYENDSESSION, 0, flags(sign_out)),
        super::SessionMessage::End { ends, sign_out } => {
            (WM_ENDSESSION, usize::from(ends), flags(sign_out))
        }
    };
    let ms = u32::try_from(within.as_millis()).unwrap_or(u32::MAX);
    let mut answer = 0usize;
    // SAFETY: a window handle, which a stale one fails; the answer is
    // written to `answer`.
    let sent = unsafe {
        SendMessageTimeoutW(
            window as windows_sys::Win32::Foundation::HWND,
            id,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            ms,
            &mut answer,
        )
    };
    if sent == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(answer as isize)
}
