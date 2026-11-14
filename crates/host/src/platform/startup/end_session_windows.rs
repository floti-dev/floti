//! How a Windows host learns that the user is signing out or the computer is
//! shutting down: a hidden window of its own, on a thread of its own.
//!
//! Windows sends `CTRL_LOGOFF_EVENT` and `CTRL_SHUTDOWN_EVENT` only to a
//! console process that has not loaded user32 or gdi32 (the remarks on
//! `SetConsoleCtrlHandler`), and floti loads user32 for the notification
//! area and its question before stopping; a host Floti starts has no console
//! at all. What every process with user32 is sent instead goes to its
//! top-level windows: `WM_QUERYENDSESSION`, asking whether the session may
//! end, then `WM_ENDSESSION`, saying whether it does. So every Windows host
//! makes this window, with or without a desktop, and keeps it for as long as
//! it runs.
//!
//! The window is top-level, never shown, rather than message-only: a
//! message-only window is never sent either message. It is not the
//! notification area's, which exists only for a host a desktop started and
//! goes when the host stops showing it, and its class is its own.
//!
//! `WM_QUERYENDSESSION` is answered yes at once: the host never holds up a
//! sign-out. Once `WM_ENDSESSION` for an ending session returns, Windows may
//! end the process at any moment, so the window says why it is waiting (the
//! shutdown block reason, which Windows shows when it lists what is still
//! running), hands the reason to [`EndSession::recv`] and waits until the
//! host's stop is over: the host drops its [`Stopped`] once it is, whatever
//! stopped it, and the wait is bounded by [`STOP_BOUND`] in case it never
//! does.
//!
//! The host stops for the session ending as for an update (`run_host`): its
//! records freeze first, so the sessions Windows kills meanwhile, in no set
//! order, are not recorded as having exited, and each row is left running for
//! the next start to offer for restore once. A host Windows ended without
//! that would record every session it saw end as exited, and none of those
//! would be offered.

use std::ffi::c_void;
use std::io;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, mpsc};
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    ENDSESSION_CLOSEAPP, ENDSESSION_LOGOFF, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW, TranslateMessage, WM_APP,
    WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WM_NCCREATE, WM_QUERYENDSESSION, WNDCLASSEXW, WS_POPUP,
};

use super::END_SESSION_WINDOW_CLASS;
use crate::{Error, Result};

/// The longest `WM_ENDSESSION` waits for the host's stop. Windows ends a
/// process that takes longer after a timeout of its own anyway.
pub(super) const STOP_BOUND: Duration = Duration::from_secs(20);

/// What Windows shows beside the host while it waits for it.
const BLOCK_REASON: &str = "Floti is saving its sessions";

/// Takes the window away and ends its thread. `WM_CLOSE` does not: anyone may
/// send that to a top-level window, and the window is the host's.
const CLOSE: u32 = WM_APP + 1;

/// What the window does with a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reply {
    /// `WM_QUERYENDSESSION`: the session may end. Answered TRUE at once.
    MayEnd,
    /// `WM_ENDSESSION` for a session that is ending, for this reason: stop
    /// the host and wait for its stop.
    Ending(&'static str),
    /// `WM_ENDSESSION` for a session that is not ending after all: nothing.
    NotEnding,
    /// Any other message, which `DefWindowProcW` answers.
    Default,
}

/// What the window does with `message`, whose parameters are `wparam` and
/// `lparam`.
pub(super) fn reply(message: u32, wparam: WPARAM, lparam: LPARAM) -> Reply {
    match message {
        WM_QUERYENDSESSION => Reply::MayEnd,
        WM_ENDSESSION if wparam == 0 => Reply::NotEnding,
        WM_ENDSESSION => Reply::Ending(ending(lparam)),
        _ => Reply::Default,
    }
}

/// Why the session is ending, as `WM_ENDSESSION`'s `lparam` says it and the
/// host's stop line says it after "Floti host is stopping: ".
fn ending(lparam: LPARAM) -> &'static str {
    // The flags are the low 32 bits; ENDSESSION_LOGOFF is the top one of
    // them, so the parameter is read as unsigned.
    let flags = lparam as usize as u32;
    if flags & ENDSESSION_LOGOFF != 0 {
        "signing out"
    } else if flags & ENDSESSION_CLOSEAPP != 0 {
        "Windows asking it to close"
    } else {
        "the computer shutting down"
    }
}

/// Whether the host's stop is over, and the wait for it.
#[derive(Debug, Default)]
pub(super) struct Handshake {
    stopped: Mutex<bool>,
    changed: Condvar,
}

impl Handshake {
    /// The host's stop is over: every wait returns, and every later one at
    /// once.
    pub(super) fn finish(&self) {
        *self.stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    /// Waits until [`finish`], for at most `bound`. Whether it finished.
    ///
    /// [`finish`]: Self::finish
    pub(super) fn wait(&self, bound: Duration) -> bool {
        let stopped = self.stopped.lock().unwrap_or_else(PoisonError::into_inner);
        let (stopped, _) = self
            .changed
            .wait_timeout_while(stopped, bound, |stopped| !*stopped)
            .unwrap_or_else(PoisonError::into_inner);
        *stopped
    }
}

/// The window, as an address: a window handle may be used from any thread,
/// though only its own thread runs its messages. Dropped by the last of
/// [`EndSession`] and its [`Stopped`] handles, so it outlives whichever of
/// them goes first.
struct Window(isize);

impl Drop for Window {
    fn drop(&mut self) {
        // Posted, not sent, and the thread is not joined: nothing waits on
        // it, so the host's exit never does. A window that has gone already
        // refuses the post, which is fine.
        // SAFETY: the handle is this host's window; a stale one is refused.
        unsafe { PostMessageW(self.0 as HWND, CLOSE, 0, 0) };
    }
}

/// The host's end-session window: the reasons it hands on, and the handshake
/// it waits on.
pub(super) struct EndSession {
    ends: UnboundedReceiver<&'static str>,
    handshake: Arc<Handshake>,
    window: Option<Arc<Window>>,
}

impl EndSession {
    /// Makes the window on a thread of its own, and returns once it is made.
    /// A window that cannot be made is logged, and the host runs on without
    /// it: it then hears no sign-out, as before, and nothing else changes.
    pub(super) fn start() -> Self {
        let (sender, ends) = unbounded_channel();
        let handshake = Arc::new(Handshake::default());
        let window = match make_on_own_thread(sender, Arc::clone(&handshake)) {
            Ok(window) => Some(Arc::new(Window(window))),
            Err(err) => {
                log_line!(
                    "floti host: it will not hear Windows signing out or shutting down: {err}"
                );
                None
            }
        };
        Self {
            ends,
            handshake,
            window,
        }
    }

    /// Waits for the next ending session and says why it is ending; `None`
    /// once the window has gone, or where it was never made.
    pub(super) async fn recv(&mut self) -> Option<&'static str> {
        self.ends.recv().await
    }

    /// A handle whose drop tells the window that the host's stop is over.
    pub(super) fn stopped(&self) -> Stopped {
        Stopped {
            handshake: Arc::clone(&self.handshake),
            _window: self.window.clone(),
        }
    }
}

/// Dropped once the host's stop is over, however it stopped: a
/// `WM_ENDSESSION` waiting for it then returns, and one arriving later
/// returns at once. It keeps the window until then.
pub(super) struct Stopped {
    handshake: Arc<Handshake>,
    _window: Option<Arc<Window>>,
}

impl Drop for Stopped {
    fn drop(&mut self) {
        // Before the window may go, which the field's drop does after this.
        self.handshake.finish();
    }
}

/// Starts the window's thread and waits until it says whether it made the
/// window.
fn make_on_own_thread(
    ends: UnboundedSender<&'static str>,
    handshake: Arc<Handshake>,
) -> Result<isize> {
    let (ready, made) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("end session".to_owned())
        .spawn(move || serve(State { ends, handshake }, &ready))
        .map_err(|err| Error::io("start the end-session window's thread", err))?;
    match made.recv() {
        Ok(made) => made,
        Err(_) => {
            let _ = thread.join();
            Err(Error::io(
                "make the end-session window",
                io::Error::other("its thread ended before it made its window"),
            ))
        }
    }
}

/// What the window's messages need, kept on its thread.
struct State {
    ends: UnboundedSender<&'static str>,
    handshake: Arc<Handshake>,
}

/// The window's thread: makes the window, says whether it could, and runs
/// its messages until it is closed.
fn serve(state: State, ready: &mpsc::Sender<Result<isize>>) {
    match make_window(&state) {
        Ok(window) => {
            let _ = ready.send(Ok(window as isize));
            run_messages();
        }
        Err(err) => {
            let _ = ready.send(Err(err));
        }
    }
}

fn run_messages() {
    // SAFETY: a zeroed MSG is what GetMessageW fills.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: this thread's messages, into `msg`; 0 is WM_QUIT, -1 an error.
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: `msg` is the message just read.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The hidden window, with `state` in its user data from its first message.
fn make_window(state: &State) -> Result<HWND> {
    static REGISTERED: OnceLock<std::result::Result<(), u32>> = OnceLock::new();
    let class = wide(END_SESSION_WINDOW_CLASS);
    // SAFETY: this module's own image.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let registered = REGISTERED.get_or_init(|| {
        // SAFETY: a zeroed class is filled below with what it needs.
        let mut class_info: WNDCLASSEXW = unsafe { std::mem::zeroed() };
        class_info.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        class_info.lpfnWndProc = Some(window_proc);
        class_info.hInstance = instance;
        class_info.lpszClassName = class.as_ptr();
        // SAFETY: a filled class whose name outlives the call; the system
        // copies it.
        if unsafe { RegisterClassExW(&class_info) } != 0 {
            return Ok(());
        }
        // SAFETY: reads this thread's last error.
        match unsafe { GetLastError() } {
            ERROR_CLASS_ALREADY_EXISTS => Ok(()),
            code => Err(code),
        }
    });
    if let Err(code) = registered {
        return Err(Error::io(
            "register the end-session window",
            io::Error::from_raw_os_error(*code as i32),
        ));
    }
    // What Windows names the host by when it lists what is still running.
    let title = wide("Floti host");
    // SAFETY: the class above, a nul-terminated title, no parent, and
    // `state`, which outlives the window: the thread destroys the window
    // before `serve` returns and drops it.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::from_ref(state).cast::<c_void>(),
        )
    };
    if window.is_null() {
        return Err(Error::io(
            "make the end-session window",
            io::Error::last_os_error(),
        ));
    }
    Ok(window)
}

/// The window's messages.
///
/// # Safety
///
/// Called by Windows for windows of [`END_SESSION_WINDOW_CLASS`], whose user data is null
/// or the [`State`] their thread keeps alive while it runs their messages.
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: as the function's contract says, throughout.
    unsafe {
        if message == WM_NCCREATE {
            let create = lparam as *const CREATESTRUCTW;
            SetWindowLongPtrW(window, GWLP_USERDATA, (*create).lpCreateParams as isize);
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const State;
        let Some(state) = state.as_ref() else {
            return DefWindowProcW(window, message, wparam, lparam);
        };
        match message {
            CLOSE => {
                DestroyWindow(window);
                return 0;
            }
            WM_CLOSE => return 0,
            WM_DESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        match reply(message, wparam, lparam) {
            Reply::MayEnd => 1,
            Reply::Ending(why) => {
                let reason = wide(BLOCK_REASON);
                ShutdownBlockReasonCreate(window, reason.as_ptr());
                // A host whose stop is over already, or under way for another
                // reason, has dropped the receiver or ignores it; either way
                // the wait below is what holds Windows back.
                let _ = state.ends.send(why);
                if !state.handshake.wait(STOP_BOUND) {
                    log_line!(
                        "floti host: its stop was not over {STOP_BOUND:?} after Windows said the session was ending ({why}); Windows may end it before its sessions are recorded"
                    );
                }
                ShutdownBlockReasonDestroy(window);
                0
            }
            Reply::NotEnding => 0,
            Reply::Default => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGOFF: LPARAM = ENDSESSION_LOGOFF as LPARAM;

    #[test]
    fn a_query_is_answered_yes_whatever_is_ending() {
        assert_eq!(reply(WM_QUERYENDSESSION, 0, 0), Reply::MayEnd);
        assert_eq!(reply(WM_QUERYENDSESSION, 0, LOGOFF), Reply::MayEnd);
    }

    #[test]
    fn an_ending_session_stops_the_host_saying_why() {
        assert_eq!(
            reply(WM_ENDSESSION, 1, LOGOFF),
            Reply::Ending("signing out")
        );
        assert_eq!(
            reply(WM_ENDSESSION, 1, 0),
            Reply::Ending("the computer shutting down")
        );
        assert_eq!(
            reply(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP as LPARAM),
            Reply::Ending("Windows asking it to close")
        );
        assert_eq!(
            reply(
                WM_ENDSESSION,
                1,
                (ENDSESSION_LOGOFF | ENDSESSION_CLOSEAPP) as LPARAM
            ),
            Reply::Ending("signing out")
        );
    }

    #[test]
    fn a_session_that_is_not_ending_after_all_changes_nothing() {
        assert_eq!(reply(WM_ENDSESSION, 0, LOGOFF), Reply::NotEnding);
        assert_eq!(reply(WM_ENDSESSION, 0, 0), Reply::NotEnding);
    }

    #[test]
    fn other_messages_are_left_to_windows() {
        assert_eq!(reply(WM_CLOSE, 0, 0), Reply::Default);
        assert_eq!(reply(WM_APP + 7, 1, LOGOFF), Reply::Default);
    }

    #[test]
    fn the_wait_ends_when_the_stop_is_over_and_not_before() {
        let handshake = Handshake::default();
        assert!(!handshake.wait(Duration::ZERO), "nothing has stopped yet");
        handshake.finish();
        assert!(handshake.wait(Duration::ZERO));
        assert!(handshake.wait(Duration::ZERO), "and stays over");
    }

    #[test]
    fn a_wait_under_way_returns_once_another_thread_says_the_stop_is_over() {
        let handshake = Arc::new(Handshake::default());
        let (waiting, started) = mpsc::channel();
        let waiter = {
            let handshake = Arc::clone(&handshake);
            std::thread::spawn(move || {
                let _ = waiting.send(());
                handshake.wait(STOP_BOUND)
            })
        };
        started.recv().unwrap();
        handshake.finish();
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn dropping_a_stopped_handle_finishes_the_handshake_whatever_stopped_the_host() {
        let handshake = Arc::new(Handshake::default());
        let stopped = Stopped {
            handshake: Arc::clone(&handshake),
            _window: None,
        };
        assert!(!handshake.wait(Duration::ZERO));
        drop(stopped);
        assert!(handshake.wait(Duration::ZERO));
    }

    #[test]
    fn a_host_whose_window_cannot_be_heard_from_still_waits_on_its_other_stop_requests() {
        // The window's sender gone, as when its thread has ended: the reason
        // stream ends rather than blocking the host's select.
        let (sender, ends) = unbounded_channel::<&'static str>();
        drop(sender);
        let mut end_session = EndSession {
            ends,
            handshake: Arc::new(Handshake::default()),
            window: None,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(runtime.block_on(end_session.recv()), None);
    }
}
