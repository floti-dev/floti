//! The process table and listening sockets on Windows.
//!
//! A Toolhelp snapshot lists every process with its parent and image name.
//! Each command line is asked of the process itself
//! (`NtQueryInformationProcess` with `ProcessCommandLineInformation`, opened
//! with `PROCESS_QUERY_LIMITED_INFORMATION`, the least access there is); a
//! process this user may not open, or a protected one that refuses, has none.
//! `GetExtendedTcpTable` lists the listening sockets of both families with
//! their owning pids.
//!
//! Windows hands a dead process's id to a new process without telling the
//! dead one's children, and Toolhelp still names it as their parent. So the
//! same handle that gives a process's command line gives when it started, and
//! a process that started before the one now holding its parent's id is
//! listed with parent 0, as one whose parent has gone: otherwise it would be
//! counted in the tree of whatever program got that id.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NOT_SUPPORTED, FILETIME, HANDLE,
    INVALID_HANDLE_VALUE, NO_ERROR, STATUS_INFO_LENGTH_MISMATCH, UNICODE_STRING,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{PortRow, ProcessRow, ProcessTable};
use crate::{Error, Result};

/// The Windows implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl ProcessTable for Native {
    fn processes(&self) -> Result<Vec<ProcessRow>> {
        let mut rows = list_processes().map_err(|err| Error::io("list the processes", err))?;
        // One buffer for every command line, grown when one needs more.
        let mut buffer = Vec::new();
        let mut started = HashMap::with_capacity(rows.len());
        for row in &mut rows {
            let Some(process) = Opened::query(row.pid) else {
                continue;
            };
            row.command_line = command_line(&process, &mut buffer);
            if let Some(at) = started_at(&process) {
                started.insert(row.pid, at);
            }
        }
        unlink_reused_parents(&mut rows, &started);
        Ok(rows)
    }

    fn listening(&self) -> Result<Vec<PortRow>> {
        let mut rows = listening_v4().map_err(|err| Error::io("list the IPv4 listeners", err))?;
        rows.extend(listening_v6().map_err(|err| Error::io("list the IPv6 listeners", err))?);
        Ok(rows)
    }
}

/// Every process, without command lines.
fn list_processes() -> io::Result<Vec<ProcessRow>> {
    // SAFETY: takes plain values and returns a snapshot handle or
    // INVALID_HANDLE_VALUE.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut rows = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).expect("an entry's size fits"),
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: `snapshot` is open, `entry` is sized as the calls require, and
    // the snapshot is closed once after the walk.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let name = &entry.szExeFile;
        let len = name
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(name.len());
        rows.push(ProcessRow {
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
            name: String::from_utf16_lossy(&name[..len]),
            command_line: None,
        });
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    Ok(rows)
}

/// How much room a command line gets at first: most fit.
const FIRST_COMMAND_LINE_BYTES: usize = 4096;

/// A process opened with the least access there is, closed when dropped.
struct Opened(HANDLE);

impl Opened {
    /// Process `pid`, or `None` where Windows will not open it for this user.
    fn query(pid: u32) -> Option<Self> {
        if pid == 0 {
            return None;
        }
        // SAFETY: OpenProcess takes plain values and returns a handle or null.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: the handle was opened in `query` and is closed once, here.
        unsafe { CloseHandle(self.0) };
    }
}

/// When `process` started, in 100-nanosecond ticks since 1601, or `None`
/// when Windows will not say.
fn started_at(process: &Opened) -> Option<u64> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the handle is open with the access GetProcessTimes needs, and
    // each pointer is to a FILETIME it may write.
    let read =
        unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) };
    (read != 0)
        .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// Lists with parent 0 each process that started before the process holding
/// its parent's id, given when each started (`started`, by pid): that id
/// was handed on after its own parent had gone. A link with either start
/// unknown is kept, since nothing says it is wrong.
fn unlink_reused_parents(rows: &mut [ProcessRow], started: &HashMap<u32, u64>) {
    for row in rows {
        if let (Some(own), Some(parent)) = (started.get(&row.pid), started.get(&row.parent_pid))
            && own < parent
        {
            row.parent_pid = 0;
        }
    }
}

/// `process`'s command line, or `None` where Windows will not give it to
/// this user. `buffer` is reused between calls.
fn command_line(process: &Opened, buffer: &mut Vec<u64>) -> Option<String> {
    let process = process.0;
    if buffer.is_empty() {
        buffer.resize(FIRST_COMMAND_LINE_BYTES / 8, 0);
    }
    let mut read = None;
    for _ in 0..2 {
        let capacity = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: `process` is open with the access this class needs, and
        // `buffer` is `capacity` bytes, aligned for the UNICODE_STRING the
        // call writes at its start with the text after it.
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                buffer.as_mut_ptr().cast(),
                capacity,
                &mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH && needed as usize > buffer.len() * 8 {
            buffer.resize((needed as usize).div_ceil(8), 0);
            continue;
        }
        if status >= 0 {
            read = Some(unicode_string_in(buffer));
        }
        break;
    }
    read.flatten().filter(|line| !line.is_empty())
}

/// The UNICODE_STRING at the start of `buffer`, whose text the call put in
/// the same buffer.
fn unicode_string_in(buffer: &[u64]) -> Option<String> {
    let start = buffer.as_ptr().cast::<u8>();
    let end = start.wrapping_add(buffer.len() * 8);
    // SAFETY: the buffer is at least as large as, and aligned for, a
    // UNICODE_STRING, which the call wrote there.
    let text = unsafe { std::ptr::read(buffer.as_ptr().cast::<UNICODE_STRING>()) };
    let units = usize::from(text.Length) / 2;
    let at = text.Buffer.cast::<u8>().cast_const();
    if units == 0 {
        return Some(String::new());
    }
    // The text has to lie inside the buffer; anything else is not trusted.
    if text.Buffer.is_null() || at < start || at.wrapping_add(units * 2) > end {
        return None;
    }
    // SAFETY: checked above to be `units` UTF-16 units inside `buffer`.
    let slice = unsafe { std::slice::from_raw_parts(text.Buffer.cast_const(), units) };
    Some(String::from_utf16_lossy(slice))
}

/// The table of listening sockets of `family`, as `GetExtendedTcpTable`
/// writes it, or `None` when the OS has no such family.
fn tcp_table(family: u16) -> io::Result<Option<Vec<u64>>> {
    let mut buffer: Vec<u64> = vec![0; 512];
    loop {
        let mut size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        // SAFETY: `buffer` is `size` bytes, aligned for the table; the call
        // writes at most that much and says how much it needs when it is
        // not enough.
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                u32::from(family),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match status {
            NO_ERROR => return Ok(Some(buffer)),
            ERROR_INSUFFICIENT_BUFFER => buffer.resize((size as usize).div_ceil(8) + 64, 0),
            ERROR_NOT_SUPPORTED => return Ok(None),
            other => {
                return Err(io::Error::from_raw_os_error(
                    i32::try_from(other).unwrap_or(i32::MAX),
                ));
            }
        }
    }
}

/// The rows of a table written at the start of `buffer`: a count, then that
/// many rows of `Row`, laid out as `Table` says.
fn rows_of<Table, Row: Copy>(buffer: &[u64], count_of: fn(&Table) -> u32) -> Vec<Row> {
    let table = buffer.as_ptr().cast::<Table>();
    // SAFETY: the call wrote a `Table` at the start of `buffer`.
    let count = count_of(unsafe { &*table }) as usize;
    let offset = size_of::<Table>() - size_of::<Row>();
    let room = (buffer.len() * 8).saturating_sub(offset) / size_of::<Row>();
    let first = buffer
        .as_ptr()
        .cast::<u8>()
        .wrapping_add(offset)
        .cast::<Row>();
    // SAFETY: the rows follow the count, as the table's one-row array
    // declares, and no more are read than the buffer holds.
    unsafe { std::slice::from_raw_parts(first, count.min(room)) }.to_vec()
}

fn port_of(raw: u32) -> u16 {
    // The low 16 bits, in network byte order.
    u16::from_be(raw as u16)
}

fn listening_v4() -> io::Result<Vec<PortRow>> {
    let Some(buffer) = tcp_table(AF_INET)? else {
        return Ok(Vec::new());
    };
    let rows: Vec<MIB_TCPROW_OWNER_PID> =
        rows_of::<MIB_TCPTABLE_OWNER_PID, _>(&buffer, |table| table.dwNumEntries);
    Ok(rows
        .iter()
        .map(|row| PortRow {
            pid: row.dwOwningPid,
            port: port_of(row.dwLocalPort),
            // Stored in network byte order, as it lies in memory.
            address: Some(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string()),
        })
        .collect())
}

fn listening_v6() -> io::Result<Vec<PortRow>> {
    let Some(buffer) = tcp_table(AF_INET6)? else {
        return Ok(Vec::new());
    };
    let rows: Vec<MIB_TCP6ROW_OWNER_PID> =
        rows_of::<MIB_TCP6TABLE_OWNER_PID, _>(&buffer, |table| table.dwNumEntries);
    Ok(rows
        .iter()
        .map(|row| PortRow {
            pid: row.dwOwningPid,
            port: port_of(row.dwLocalPort),
            address: Some(Ipv6Addr::from(row.ucLocalAddr).to_string()),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The System process is one no unelevated user may ask for a command
    /// line: it is listed with none, and the pass goes on.
    #[test]
    fn a_process_that_will_not_give_its_command_line_is_listed_without_one() {
        let rows = Native.processes().expect("the process table");
        let system = rows
            .iter()
            .find(|row| row.pid == 4)
            .expect("the System process");
        assert_eq!(system.command_line, None, "{system:?}");
        let this = rows
            .iter()
            .find(|row| row.pid == std::process::id())
            .expect("this process");
        assert!(this.command_line.is_some(), "{this:?}");
    }

    fn row(pid: u32, parent_pid: u32) -> ProcessRow {
        ProcessRow {
            pid,
            parent_pid,
            name: "a.exe".into(),
            command_line: None,
        }
    }

    /// A process whose parent's id now belongs to a later process is no
    /// child of that one; a real child, one started with its parent, and one
    /// either start of which is unknown keep their parent.
    #[test]
    fn a_process_older_than_the_holder_of_its_parents_id_is_listed_without_a_parent() {
        let mut rows = vec![
            row(10, 1),
            row(20, 10),
            row(30, 10),
            row(40, 10),
            row(50, 10),
            row(60, 70),
        ];
        let started = HashMap::from([(10, 500), (20, 400), (30, 600), (40, 500), (60, 100)]);
        unlink_reused_parents(&mut rows, &started);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.parent_pid))
                .collect::<Vec<_>>(),
            [(10, 1), (20, 0), (30, 10), (40, 10), (50, 10), (60, 70)]
        );
    }
}
