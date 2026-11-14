//! The local socket on Windows: a named pipe whose access list grants this
//! user only, refusing remote clients.
//!
//! The pipe's security descriptor has a protected access list with a single
//! entry, full access for the user the host runs as. No entry for Everyone,
//! Authenticated Users, anonymous logons, administrators or SYSTEM, so no
//! other account on the machine can open it. Its mandatory label is Medium,
//! so a host running elevated still admits the same user's clients, which
//! run at Medium (`UserOnlyDescriptor`). `PIPE_REJECT_REMOTE_CLIENTS`
//! refuses a connection that arrives over the network (`\\server\pipe\...`),
//! even from this user.
//!
//! The first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, which
//! fails when any process already has a pipe of that name, so a second host,
//! or a process squatting on the name, makes `bind` refuse. A pipe exists
//! only while a process holds a handle to it, so a killed host leaves nothing
//! behind to replace.

use std::ffi::{OsString, c_void};
use std::fmt;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;
use std::time::Instant;

use floti_protocol::Knowable;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions,
};
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{CONNECT_PATIENCE, Endpoint, LocalListener, LocalSocket, LocalStream, already_running};
use crate::{Error, Result};

/// How long `connect` waits before trying a busy pipe again.
const BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(20);

/// The Windows implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// A listening pipe. It always has one instance waiting for the next client,
/// created with the same user-only descriptor as the first.
pub struct Listener {
    name: OsString,
    descriptor: UserOnlyDescriptor,
    waiting: NamedPipeServer,
}

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Listener {
    /// A new instance of the pipe for the next client.
    fn instance(&self) -> io::Result<NamedPipeServer> {
        create_instance(&self.name, &self.descriptor, false)
    }
}

impl LocalSocket for Native {
    type Listener = Listener;

    /// A pipe in the local pipe namespace. Pipes are not files, and that
    /// namespace is one for the whole machine, so the name carries a hash of
    /// the runtime folder: each user's folder is their own, which keeps two
    /// users' hosts apart, and moving the folder moves the pipe as it moves a
    /// socket file elsewhere. Windows paths ignore case, so the hash does too.
    fn endpoint(&self, runtime_dir: &Path, name: &str) -> Endpoint {
        // FNV-1a, 64 bits: stable across builds, unlike std's hasher.
        let folder = runtime_dir.to_string_lossy().to_lowercase();
        let hash = folder
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Endpoint::new(format!(r"\\.\pipe\{name}-{hash:016x}"))
    }

    fn bind(&self, endpoint: &Endpoint) -> Result<Listener> {
        let name = endpoint.as_path().as_os_str().to_owned();
        let descriptor = UserOnlyDescriptor::for_current_user()
            .map_err(|e| Error::io("build the pipe's access list for this user", e))?;
        let waiting = create_instance(&name, &descriptor, true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                already_running(endpoint)
            } else {
                Error::path("listen on", endpoint.as_path(), e)
            }
        })?;
        Ok(Listener {
            name,
            descriptor,
            waiting,
        })
    }

    async fn connect(&self, endpoint: &Endpoint) -> Result<LocalStream> {
        Ok(Box::pin(open_client(endpoint).await?))
    }

    /// The pipe's server process, from `GetNamedPipeServerProcessId` on this
    /// end of it.
    async fn connect_naming_server(
        &self,
        endpoint: &Endpoint,
    ) -> Result<(LocalStream, Knowable<u32>)> {
        let client = open_client(endpoint).await?;
        let mut pid = 0u32;
        // SAFETY: the handle is this client's end of the pipe, open for the
        // call, and `pid` is written by it.
        let named =
            unsafe { GetNamedPipeServerProcessId(client.as_raw_handle() as HANDLE, &raw mut pid) };
        let server = if named != 0 && pid != 0 {
            Knowable::Known(pid)
        } else {
            Knowable::Unknown
        };
        Ok((Box::pin(client), server))
    }
}

/// This user's client end of the pipe at `endpoint`, trying again while it is
/// busy, for at most [`CONNECT_PATIENCE`].
async fn open_client(endpoint: &Endpoint) -> Result<NamedPipeClient> {
    let deadline = Instant::now() + CONNECT_PATIENCE;
    loop {
        match ClientOptions::new().open(endpoint.as_path()) {
            Ok(client) => return Ok(client),
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < deadline =>
            {
                tokio::time::sleep(BUSY_RETRY).await;
            }
            Err(e) => return Err(Error::path("connect to", endpoint.as_path(), e)),
        }
    }
}

impl LocalListener for Listener {
    async fn accept(&mut self) -> Result<LocalStream> {
        let connected = self.waiting.connect().await;
        // Whatever happened to that instance, the next client needs a fresh
        // one waiting before this returns.
        let next = self
            .instance()
            .map_err(|e| Error::path("listen on", Path::new(&self.name), e))?;
        let served = std::mem::replace(&mut self.waiting, next);
        connected.map_err(|e| Error::io("accept a client on the local pipe", e))?;
        Ok(Box::pin(served))
    }
}

/// One instance of the pipe, open to this user and local clients only.
fn create_instance(
    name: &OsString,
    descriptor: &UserOnlyDescriptor,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor
    // lives as long as `descriptor`, which outlives this call; CreateNamedPipe
    // copies what it needs.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Byte)
            .create_with_security_attributes_raw(name, (&raw mut attributes).cast::<c_void>())
    }
}

/// A self-relative security descriptor whose protected access list has one
/// entry, full access for the current user, and whose mandatory label is
/// Medium integrity.
///
/// The label is what lets a host that runs elevated (an administrator's SSH
/// login gets the full token, at High integrity) serve the same user's
/// desktop client, which runs at Medium. Without an explicit one, whether an
/// object a High process makes admits a Medium writer is the OS's default to
/// decide; with it, the pipe, the keeper's pipe and every owner-only file
/// admit the user's Medium processes by construction, and still refuse Low
/// ones writing (`NW`, no write up). A label at or below the creator's own
/// integrity may be set as the object is made, which needs no privilege
/// (`SeSecurityPrivilege` is for reading or writing audit entries, which this
/// has none of).
pub(crate) struct UserOnlyDescriptor(PSECURITY_DESCRIPTOR);

/// The descriptor's text for the user whose SID is `sid`: `D:P` is a
/// protected access list, so nothing is inherited into it; `(A;;GA;;;sid)`
/// allows generic all to the user, and nobody else; `S:(ML;;NW;;;ME)` is a
/// Medium mandatory label that refuses a lower integrity writing.
fn user_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})S:(ML;;NW;;;ME)")
}

// SAFETY: the descriptor is heap memory from LocalAlloc that nothing mutates
// after it is built; it is only read, by CreateNamedPipe and CreateFileW.
unsafe impl Send for UserOnlyDescriptor {}
// SAFETY: as above, shared reads of immutable memory.
unsafe impl Sync for UserOnlyDescriptor {}

impl UserOnlyDescriptor {
    pub(crate) fn for_current_user() -> io::Result<Self> {
        let sid = CurrentUser::query()?.sid_string()?;
        let sddl: Vec<u16> = user_only_sddl(&sid).encode_utf16().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; on success the API allocates the
        // descriptor with LocalAlloc, freed in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }

    /// The descriptor, valid while `self` lives.
    pub(crate) fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.0
    }
}

impl Drop for UserOnlyDescriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.0) };
    }
}

/// The user this process runs as: its token's TOKEN_USER, which holds the
/// SID.
pub(crate) struct CurrentUser {
    /// A TOKEN_USER and the SID it points into, in u64s for alignment.
    buffer: Vec<u64>,
}

impl CurrentUser {
    pub(crate) fn query() -> io::Result<Self> {
        let mut raw: HANDLE = ptr::null_mut();
        // SAFETY: the pseudo handle needs no closing; `raw` receives a token
        // handle, owned below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken succeeded, so `raw` is a handle we own.
        let token = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
        let token = std::os::windows::io::AsRawHandle::as_raw_handle(&token) as HANDLE;

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
        Ok(Self { buffer })
    }

    /// The user's SID, valid while `self` lives.
    pub(crate) fn sid(&self) -> PSID {
        // SAFETY: GetTokenInformation(TokenUser) wrote a TOKEN_USER at the
        // start of the buffer.
        unsafe { (*self.buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// The SID as text, such as `S-1-5-21-...`.
    fn sid_string(&self) -> io::Result<String> {
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: `sid()` is a valid SID; on success `text` is a
        // NUL-terminated string from LocalAlloc, freed below.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `text` is NUL-terminated.
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        // SAFETY: `len` u16s are readable at `text`.
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        // SAFETY: allocated by ConvertSidToStringSidW.
        unsafe { LocalFree(text.cast()) };
        Ok(sid)
    }
}

/// Every TCP socket listening and every UDP socket bound in process `pid`,
/// one line each, read from `netstat -ano`. A TCP socket is listening when
/// its remote address is the unspecified one, which holds whatever language
/// netstat prints its states in.
#[cfg(any(test, feature = "testkit"))]
pub fn network_listeners(pid: u32) -> io::Result<Vec<String>> {
    let output = std::process::Command::new("netstat").arg("-ano").output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "netstat -ano failed: {}",
            output.status
        )));
    }
    let pid = pid.to_string();
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["TCP", _, remote, _, owner] => *owner == pid && remote.ends_with(":0"),
                ["UDP", _, _, owner] => *owner == pid,
                _ => false,
            }
        })
        .map(|line| line.trim().to_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, SE_DACL_PROTECTED,
    };

    use super::*;
    use crate::testkit::{TestDirs, unique_name};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// ACCESS_ALLOWED_ACE_TYPE, from winnt.h.
    const ACCESS_ALLOWED: u8 = 0;

    /// The entries of the access list on `handle`'s object, as (ACE type,
    /// access mask, whether the SID is the current user's), and whether the
    /// list is protected from inheritance.
    fn access_list(handle: HANDLE) -> (bool, Vec<(u8, u32, bool)>) {
        let user = CurrentUser::query().expect("read this process's user");
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `handle` is a live pipe handle; the out pointers are valid.
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS, "read the pipe's security descriptor");
        assert!(!dacl.is_null(), "the pipe has an access list, not none");

        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `descriptor` came from GetSecurityInfo.
        let ok = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
        assert_ne!(ok, 0);

        let mut size = ACL_SIZE_INFORMATION::default();
        // SAFETY: `dacl` points into `descriptor`; `size` is the right type.
        let ok = unsafe {
            GetAclInformation(
                dacl,
                (&raw mut size).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        };
        assert_ne!(ok, 0);

        let entries = (0..size.AceCount)
            .map(|index| {
                let mut ace: *mut c_void = ptr::null_mut();
                // SAFETY: `index` is below AceCount.
                assert_ne!(unsafe { GetAce(dacl, index, &mut ace) }, 0);
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                // SAFETY: every ACE starts with an ACE_HEADER; the SID of an
                // allowed or denied ACE starts at SidStart.
                unsafe {
                    let kind = (*ace).Header.AceType;
                    let sid = (&raw mut (*ace).SidStart).cast::<c_void>();
                    (kind, (*ace).Mask, EqualSid(sid, user.sid()) != 0)
                }
            })
            .collect();
        // SAFETY: allocated by GetSecurityInfo.
        unsafe { LocalFree(descriptor) };
        (control & SE_DACL_PROTECTED != 0, entries)
    }

    fn handle_of(listener: &Listener) -> HANDLE {
        std::os::windows::io::AsRawHandle::as_raw_handle(&listener.waiting) as HANDLE
    }

    #[tokio::test]
    async fn the_pipes_access_list_grants_the_current_user_only() {
        let dirs = TestDirs::new();
        let endpoint = dirs.endpoint();
        let mut listener = Native.bind(&endpoint).expect("bind");

        let (protected, entries) = access_list(handle_of(&listener));
        assert!(protected, "the access list inherits nothing");
        assert_eq!(
            entries.len(),
            1,
            "one entry and no other, for Everyone, Authenticated Users, anonymous or anyone: {entries:?}"
        );
        let (kind, mask, is_user) = entries[0];
        assert_eq!(kind, ACCESS_ALLOWED);
        assert!(is_user, "the one entry is the current user's");
        assert_ne!(mask, 0);

        // Every instance after the first carries the same list.
        let client = Native.connect(&endpoint).await.expect("connect");
        let _served = listener.accept().await.expect("accept");
        let (protected, entries) = access_list(handle_of(&listener));
        assert!(protected);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].2);
        drop(client);
    }

    #[tokio::test]
    async fn the_pipe_refuses_remote_clients() {
        let dirs = TestDirs::new();
        let endpoint = dirs.endpoint();
        let name = endpoint
            .as_path()
            .file_name()
            .expect("a pipe name")
            .to_string_lossy()
            .into_owned();
        let mut listener = Native.bind(&endpoint).expect("bind");

        // `\\localhost\pipe\...` goes through the network redirector, so the
        // pipe sees a remote client even though it is this machine.
        let remote = format!(r"\\localhost\pipe\{name}");
        let refused = ClientOptions::new().open(&remote);
        let refused = refused.expect_err("a remote client is refused");
        assert_eq!(
            refused.raw_os_error(),
            Some(ERROR_ACCESS_DENIED as i32),
            "refused by the pipe, not unreachable: {refused}"
        );

        // The refusal used no instance: a local client still connects.
        let mut client = Native.connect(&endpoint).await.expect("connect locally");
        let mut served = listener.accept().await.expect("accept");
        client.write_all(b"local").await.unwrap();
        let mut buf = [0u8; 5];
        served.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"local");
    }

    #[tokio::test]
    async fn the_same_pipe_without_the_flag_takes_remote_clients() {
        // The control for the test above: the redirector path does reach a
        // pipe that does not refuse remote clients, so the refusal there is
        // the flag's doing.
        let name = unique_name();
        let local = format!(r"\\.\pipe\{name}");
        let descriptor = UserOnlyDescriptor::for_current_user().unwrap();
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // SAFETY: as in create_instance.
        let server = unsafe {
            ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(false)
                .create_with_security_attributes_raw(&local, (&raw mut attributes).cast())
        }
        .expect("create a control pipe");
        let remote = format!(r"\\localhost\pipe\{name}");
        let client = ClientOptions::new().open(&remote);
        assert!(
            client.is_ok(),
            "the redirector reaches a pipe that takes remote clients: {client:?}"
        );
        drop(server);
    }

    #[test]
    fn a_pipe_name_already_held_is_refused() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();
        let name = unique_name();
        let endpoint = Native.endpoint(Path::new("unused"), &name);
        // Someone else's pipe of that name, with default security.
        let squatter = ServerOptions::new()
            .first_pipe_instance(true)
            .create(endpoint.as_path())
            .expect("create the squatting pipe");
        let refused = Native.bind(&endpoint).expect_err("the name is taken");
        assert!(refused.to_string().contains("already running"), "{refused}");
        drop(squatter);
        Native.bind(&endpoint).expect("the name is free again");
    }

    /// The pipe's Medium label, and an elevated host's pipe taking the same
    /// user's client at Medium integrity, as Floti at the desktop is.
    mod integrity {
        use std::io::Write;

        use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
        use windows_sys::Win32::Security::{
            CreateRestrictedToken, DuplicateTokenEx, GetLengthSid, ImpersonateLoggedOnUser,
            LABEL_SECURITY_INFORMATION, RevertToSelf, SID_AND_ATTRIBUTES,
            SYSTEM_MANDATORY_LABEL_ACE, SecurityImpersonation, SetTokenInformation,
            TOKEN_ALL_ACCESS, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TokenImpersonation,
            TokenIntegrityLevel,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

        // The pipe module's items and the tests' imports beside them.
        use super::*;

        /// SYSTEM_MANDATORY_LABEL_ACE_TYPE, from winnt.h.
        const MANDATORY_LABEL: u8 = 0x11;
        /// SYSTEM_MANDATORY_LABEL_NO_WRITE_UP, from winnt.h.
        const NO_WRITE_UP: u32 = 0x1;
        /// SE_GROUP_INTEGRITY, from winnt.h.
        const GROUP_INTEGRITY: u32 = 0x20;
        const MEDIUM: &str = "S-1-16-8192";
        const HIGH: &str = "S-1-16-12288";

        /// A SID made from its text, freed when dropped.
        struct Sid(PSID);

        impl Sid {
            fn from_text(text: &str) -> Self {
                let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let mut sid: PSID = ptr::null_mut();
                // SAFETY: `wide` is NUL-terminated; on success `sid` is
                // allocated with LocalAlloc, freed in Drop.
                assert_ne!(
                    unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) },
                    0,
                    "make the SID {text}"
                );
                Self(sid)
            }
        }

        impl Drop for Sid {
            fn drop(&mut self) {
                // SAFETY: allocated by ConvertStringSidToSidW.
                unsafe { LocalFree(self.0) };
            }
        }

        /// `sid` as text.
        fn text_of(sid: PSID) -> String {
            let mut text: *mut u16 = ptr::null_mut();
            // SAFETY: `sid` is valid for the call; on success `text` is a
            // NUL-terminated string from LocalAlloc, freed below.
            assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut text) }, 0);
            // SAFETY: `text` is NUL-terminated.
            let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
            // SAFETY: `len` u16s are readable at `text`.
            let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
            // SAFETY: allocated by ConvertSidToStringSidW.
            unsafe { LocalFree(text.cast()) };
            sid
        }

        /// The integrity level of `token`, as its SID's text.
        fn integrity_of(token: HANDLE) -> String {
            let mut needed = 0u32;
            // SAFETY: a size query with no buffer; it fails and sets `needed`.
            unsafe {
                GetTokenInformation(token, TokenIntegrityLevel, ptr::null_mut(), 0, &mut needed)
            };
            assert_ne!(needed, 0, "{}", io::Error::last_os_error());
            let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
            // SAFETY: `buffer` holds `needed` bytes, aligned for the label.
            let ok = unsafe {
                GetTokenInformation(
                    token,
                    TokenIntegrityLevel,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            };
            assert_ne!(ok, 0, "{}", io::Error::last_os_error());
            // SAFETY: a TOKEN_MANDATORY_LABEL starts the buffer, its SID in it.
            text_of(unsafe { (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid })
        }

        /// The integrity level this test process runs at.
        fn this_process_integrity() -> String {
            let mut raw: HANDLE = ptr::null_mut();
            // SAFETY: `raw` receives a token handle, owned below.
            assert_ne!(
                unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) },
                0
            );
            // SAFETY: OpenProcessToken succeeded.
            let token = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
            integrity_of(token.as_raw_handle() as HANDLE)
        }

        /// The mandatory label entries on `handle`'s object, as (ACE type,
        /// mask, SID text).
        fn labels(handle: HANDLE) -> Vec<(u8, u32, String)> {
            let mut sacl: *mut ACL = ptr::null_mut();
            let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
            // SAFETY: `handle` is a live pipe handle; the out pointers are
            // valid. Reading the label needs READ_CONTROL only, no privilege.
            let status = unsafe {
                GetSecurityInfo(
                    handle,
                    SE_KERNEL_OBJECT,
                    LABEL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut sacl,
                    &mut descriptor,
                )
            };
            assert_eq!(status, ERROR_SUCCESS, "read the pipe's label");
            let mut found = Vec::new();
            if !sacl.is_null() {
                let mut size = ACL_SIZE_INFORMATION::default();
                // SAFETY: `sacl` points into `descriptor`.
                let ok = unsafe {
                    GetAclInformation(
                        sacl,
                        (&raw mut size).cast(),
                        size_of::<ACL_SIZE_INFORMATION>() as u32,
                        AclSizeInformation,
                    )
                };
                assert_ne!(ok, 0);
                for index in 0..size.AceCount {
                    let mut ace: *mut c_void = ptr::null_mut();
                    // SAFETY: `index` is below AceCount.
                    assert_ne!(unsafe { GetAce(sacl, index, &mut ace) }, 0);
                    let ace = ace.cast::<SYSTEM_MANDATORY_LABEL_ACE>();
                    // SAFETY: every ACE starts with its header; a label ACE's
                    // SID starts at SidStart.
                    found.push(unsafe {
                        (
                            (*ace).Header.AceType,
                            (*ace).Mask,
                            text_of((&raw mut (*ace).SidStart).cast()),
                        )
                    });
                }
            }
            // SAFETY: allocated by GetSecurityInfo.
            unsafe { LocalFree(descriptor) };
            found
        }

        /// This process's token made what a non-elevated desktop client's
        /// is, as an impersonation token: the same user, Administrators for
        /// deny only, at Medium integrity.
        fn medium_client_token() -> OwnedHandle {
            let own = |raw: HANDLE| {
                // SAFETY: each caller passes a handle a call just opened.
                unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) }
            };
            let mut raw: HANDLE = ptr::null_mut();
            // SAFETY: `raw` receives a token handle, owned below.
            assert_ne!(
                unsafe {
                    OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw)
                },
                0
            );
            let process = own(raw);
            // SAFETY: plain call on an open token; `raw` receives the copy.
            assert_ne!(
                unsafe {
                    DuplicateTokenEx(
                        process.as_raw_handle() as HANDLE,
                        TOKEN_ALL_ACCESS,
                        ptr::null(),
                        SecurityImpersonation,
                        TokenImpersonation,
                        &mut raw,
                    )
                },
                0,
                "{}",
                io::Error::last_os_error()
            );
            let duplicate = own(raw);
            let administrators = Sid::from_text("S-1-5-32-544");
            let deny_only = SID_AND_ATTRIBUTES {
                Sid: administrators.0,
                Attributes: 0,
            };
            // SAFETY: one SID to disable, nothing else; `raw` receives the
            // new token, of the same type and access as the one it is made
            // from.
            assert_ne!(
                unsafe {
                    CreateRestrictedToken(
                        duplicate.as_raw_handle() as HANDLE,
                        0,
                        1,
                        &deny_only,
                        0,
                        ptr::null(),
                        0,
                        ptr::null(),
                        &mut raw,
                    )
                },
                0,
                "{}",
                io::Error::last_os_error()
            );
            let client = own(raw);
            let medium = Sid::from_text(MEDIUM);
            let label = TOKEN_MANDATORY_LABEL {
                Label: SID_AND_ATTRIBUTES {
                    Sid: medium.0,
                    Attributes: GROUP_INTEGRITY,
                },
            };
            // SAFETY: a label and the SID it points at, sized together as
            // the call requires. Lowering a token's integrity needs no
            // privilege.
            let size =
                size_of::<TOKEN_MANDATORY_LABEL>() as u32 + unsafe { GetLengthSid(medium.0) };
            assert_ne!(
                unsafe {
                    SetTokenInformation(
                        client.as_raw_handle() as HANDLE,
                        TokenIntegrityLevel,
                        (&raw const label).cast(),
                        size,
                    )
                },
                0,
                "{}",
                io::Error::last_os_error()
            );
            client
        }

        /// Opens the pipe `name` for reading and writing on a thread of its
        /// own that impersonates `token`, writes `said` to it, and says what
        /// the thread's integrity was, with the opened pipe or the error.
        fn open_as(token: &OwnedHandle, name: &str, said: &[u8]) -> (String, io::Result<()>) {
            let token = token.as_raw_handle() as usize;
            let name = name.to_owned();
            let said = said.to_vec();
            std::thread::spawn(move || {
                // SAFETY: `token` is an impersonation token that outlives the
                // thread; the thread reverts before it ends.
                assert_ne!(unsafe { ImpersonateLoggedOnUser(token as HANDLE) }, 0);
                let mut raw: HANDLE = ptr::null_mut();
                // SAFETY: `raw` receives the thread's token, owned below.
                assert_ne!(
                    unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) },
                    0
                );
                // SAFETY: OpenThreadToken succeeded.
                let thread = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
                let integrity = integrity_of(thread.as_raw_handle() as HANDLE);
                let opened = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&name)
                    .and_then(|mut pipe| pipe.write_all(&said));
                // SAFETY: ends this thread's impersonation.
                assert_ne!(unsafe { RevertToSelf() }, 0);
                (integrity, opened)
            })
            .join()
            .expect("the client thread")
        }

        #[test]
        fn the_descriptor_grants_the_user_alone_and_labels_it_medium() {
            assert_eq!(
                user_only_sddl("S-1-5-21-1-2-3-1001"),
                "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)S:(ML;;NW;;;ME)"
            );
        }

        /// A label at or below the creator's integrity is set as the pipe is
        /// made, with no privilege, so binding works in an elevated process
        /// and in one that is not.
        #[tokio::test]
        async fn the_pipe_binds_with_a_medium_label_needing_no_privilege() {
            let integrity = this_process_integrity();
            eprintln!("this test process runs at integrity {integrity}");
            let dirs = TestDirs::new();
            let listener = Native.bind(&dirs.endpoint()).expect("bind");
            let handle = listener.waiting.as_raw_handle() as HANDLE;
            assert_eq!(
                labels(handle),
                [(MANDATORY_LABEL, NO_WRITE_UP, MEDIUM.to_owned())],
                "one Medium label that refuses a lower integrity writing"
            );
        }

        /// What AC4 asks: a host started from an elevated SSH login serves
        /// the same user's desktop client, which runs at Medium. It needs an
        /// elevated test process, as on CI's Windows runners; anywhere else
        /// it says so and checks nothing.
        #[tokio::test]
        async fn an_elevated_hosts_pipe_takes_the_same_users_medium_integrity_client() {
            let integrity = this_process_integrity();
            if integrity != HIGH {
                eprintln!(
                    "SKIPPED: this test needs a test process at High integrity (elevated), as on CI's Windows runners; this one runs at {integrity}"
                );
                return;
            }
            eprintln!("this test process runs elevated, at High integrity");
            let client = medium_client_token();

            let dirs = TestDirs::new();
            let endpoint = dirs.endpoint();
            let name = endpoint.as_path().to_string_lossy().into_owned();
            let mut listener = Native.bind(&endpoint).expect("bind");
            let (as_client, opened) = open_as(&client, &name, b"medium");
            assert_eq!(as_client, MEDIUM, "the client runs at Medium");
            opened.expect("the Medium client opens the elevated host's pipe");
            let mut served = listener.accept().await.expect("accept");
            let mut buf = [0u8; 6];
            served.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"medium");

            // The control: a pipe the same user made with a High label
            // refuses that client writing, so the client really is at Medium
            // and the label is what lets it in.
            let sid = CurrentUser::query().unwrap().sid_string().unwrap();
            let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})S:(ML;;NW;;;HI)")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
            // SAFETY: as in UserOnlyDescriptor::for_current_user.
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        SDDL_REVISION_1,
                        &mut descriptor,
                        ptr::null_mut(),
                    )
                },
                0
            );
            let high = UserOnlyDescriptor(descriptor);
            let control = format!(r"\\.\pipe\{}", crate::testkit::unique_name());
            let _server = create_instance(&OsString::from(&control), &high, true)
                .expect("create the High-labelled control pipe");
            let (_, refused) = open_as(&client, &control, b"medium");
            let refused = refused.expect_err("a High label refuses a Medium writer");
            assert_eq!(
                refused.raw_os_error(),
                Some(ERROR_ACCESS_DENIED as i32),
                "{refused}"
            );
        }
    }
}
