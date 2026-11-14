use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use floti_protocol::Knowable;

use super::{
    BREAKAWAY_REFUSED, Ending, Native, OneProcess, Outliving, ProcessTree, descendants,
    start_outliving, tree_under,
};
use crate::session::tests::fake_agent_exe;

#[test]
fn the_host_itself_is_alive() {
    assert_eq!(Native.is_alive(std::process::id()), Knowable::Known(true));
}

#[test]
fn a_process_that_has_exited_is_not_alive() {
    let mut child = Command::new(fake_agent_exe())
        .arg("--fake-exit=0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert_eq!(Native.is_alive(pid), Knowable::Known(false));
}

/// A pid of 0 is a process that never started; ending it signals nothing,
/// least of all the host's own group.
#[test]
fn ending_no_process_does_nothing() {
    Native.end_tree(0, Ending::Forced).unwrap();
    Native.end_tree(0, Ending::Polite).unwrap();
    assert_eq!(Native.is_alive(std::process::id()), Knowable::Known(true));
}

/// Every OS ties what this process starts to its life, and asking again
/// gives the same answer.
#[test]
fn what_this_process_starts_ends_with_it_on_every_os() {
    assert_eq!(Native.end_tree_with_this_process(), Knowable::Known(true));
    assert_eq!(Native.end_tree_with_this_process(), Knowable::Known(true));
}

/// The tree under a process is its children and theirs, found by parent,
/// and never a process the walk is told is not really a child.
#[test]
fn the_tree_under_a_process_is_its_children_theirs_and_no_others() {
    let table = [
        (1, 0),
        (10, 1),
        (11, 1),
        (20, 10),
        (30, 20),
        (40, 99),
        // A row naming itself as its parent ends no walk.
        (50, 50),
    ];
    let mut tree = tree_under(&table, 1, |_, _| true);
    tree.sort_unstable();
    assert_eq!(tree, [10, 11, 20, 30]);

    // 20 names 10 as its parent only because 10's pid was handed on.
    let mut tree = tree_under(&table, 1, |parent, pid| (parent, pid) != (10, 20));
    tree.sort_unstable();
    assert_eq!(tree, [10, 11]);

    assert_eq!(tree_under(&table, 40, |_, _| true), Vec::<u32>::new());
}

/// A program started as a tree's root holds nothing of this process's but
/// its standard three. A descriptor open without close-on-exec when it
/// starts - on macOS, a pipe another thread's spawn has just made, before
/// it is marked - is not held by the program: the pipe's reader sees its end
/// once this process lets go of its own copy.
#[cfg(unix)]
#[test]
fn a_root_holds_no_descriptor_of_the_host_but_its_standard_three() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let mut ends = [0; 2];
    // SAFETY: `pipe` fills the two descriptors it makes, without close-on-exec.
    assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
    // SAFETY: both were just made and nothing else owns them.
    let (reader, writer) =
        unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };

    let mut command = Command::new(fake_agent_exe());
    command
        .arg("--fake-child=60000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    Native.prepare_root(&mut command);
    let mut agent = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(agent.stdout.take().expect("the agent's output"))
        .read_line(&mut line)
        .expect("read the agent's child");
    let grandchild: Option<u32> = line
        .trim()
        .strip_prefix("child: ")
        .and_then(|pid| pid.parse().ok());

    // The program is running, so whatever it holds it holds now.
    drop(writer);
    let mut byte = 0u8;
    // SAFETY: reads at most one byte into `byte`; the pipe is non-blocking.
    let read = unsafe {
        let raw = reader.as_raw_fd();
        libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK);
        libc::read(raw, (&raw mut byte).cast(), 1)
    };
    let held = std::io::Error::last_os_error();

    drop(agent.stdin.take());
    agent.wait().unwrap();
    if let Some(pid) = grandchild {
        Native.end_tree(pid, Ending::Forced).unwrap();
    }
    assert_eq!(read, 0, "the program holds the pipe's write end: {held}");
}

/// Ending one process alone ends that process at once, and nothing it
/// started: its child runs on.
#[test]
fn ending_one_process_ends_it_alone_and_leaves_what_it_started_running() {
    let mut command = Command::new(fake_agent_exe());
    command
        .arg("--fake-child=60000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A root of its own, as a host is: its group is its tree.
    Native.prepare_root(&mut command);
    let mut agent = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(agent.stdout.take().expect("the agent's output"))
        .read_line(&mut line)
        .expect("read the agent's child");
    let grandchild: u32 = line
        .trim()
        .strip_prefix("child: ")
        .and_then(|pid| pid.parse().ok())
        .unwrap_or_else(|| panic!("no child reported: {line:?}"));

    let one = OneProcess::hold(agent.id()).expect("hold the agent");
    assert_eq!(one.pid(), agent.id());
    one.end_now().expect("end the agent");
    agent.wait().expect("the agent ended");
    let child_alive = Native.is_alive(grandchild);
    Native.end_tree(grandchild, Ending::Forced).unwrap();
    assert_eq!(
        child_alive,
        Knowable::Known(true),
        "its child was ended too"
    );
    // Ending one that has already exited is no error.
    one.end_now().expect("end it again");
}

#[test]
fn this_process_and_no_process_are_not_ones_to_end() {
    assert!(OneProcess::hold(0).is_err());
    assert!(OneProcess::hold(std::process::id()).is_err());
}

/// A program this process started, and the child that one started, are
/// both found under this process.
#[test]
fn the_descendants_of_this_process_include_its_childs_child() {
    let mut agent = Command::new(fake_agent_exe())
        .arg("--fake-child=60000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let agent_pid = agent.id();
    let mut line = String::new();
    BufReader::new(agent.stdout.take().expect("the agent's output"))
        .read_line(&mut line)
        .expect("read the agent's child");
    let grandchild: Option<u32> = line
        .trim()
        .strip_prefix("child: ")
        .and_then(|pid| pid.parse().ok());

    let found = descendants(std::process::id());
    // Its stdin closes and it exits; its child is ended here.
    drop(agent.stdin.take());
    agent.wait().unwrap();
    if let Some(pid) = grandchild {
        Native.end_tree(pid, Ending::Forced).unwrap();
    }

    let grandchild = grandchild.unwrap_or_else(|| panic!("no child reported: {line:?}"));
    let Knowable::Known(found) = found else {
        panic!("the OS would not list this process's descendants");
    };
    assert!(found.contains(&agent_pid), "{agent_pid} not in {found:?}");
    assert!(found.contains(&grandchild), "{grandchild} not in {found:?}");
    assert!(!found.contains(&std::process::id()), "{found:?}");
}

/// A start through [`start_outliving`] whose breakaway start answers
/// `breakaway` and whose start inside the job answers `inside`, with the
/// job ending what it holds when `ends_with_job`: what it answered, and each
/// start it was asked for (true for out of the job).
fn outliving(
    breakaway: fn() -> std::io::Result<()>,
    inside: fn() -> std::io::Result<()>,
    ends_with_job: bool,
) -> (std::io::Result<Outliving>, Vec<bool>) {
    let mut asked = Vec::new();
    let answer = start_outliving(
        &mut |out_of_job| {
            asked.push(out_of_job);
            if out_of_job { breakaway() } else { inside() }
        },
        &|| ends_with_job,
    );
    (answer, asked)
}

fn refused() -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(BREAKAWAY_REFUSED))
}

#[test]
fn a_start_the_job_lets_go_runs_apart_and_is_asked_once() {
    let (answer, asked) = outliving(|| Ok(()), || unreachable!("never inside"), true);
    assert_eq!(answer.unwrap(), Outliving::Apart);
    assert_eq!(asked, [true]);
}

#[test]
fn a_job_that_refuses_breakaway_gets_it_started_inside_and_says_whether_it_ends_it() {
    for ends_with_job in [false, true] {
        let (answer, asked) = outliving(refused, || Ok(()), ends_with_job);
        assert_eq!(answer.unwrap(), Outliving::InJob { ends_with_job });
        assert_eq!(asked, [true, false], "out of the job first, then inside");
    }
}

#[test]
fn a_start_that_fails_inside_the_job_too_answers_that_failure() {
    let (answer, asked) = outliving(
        refused,
        || Err(std::io::Error::other("no such program")),
        false,
    );
    assert_eq!(answer.unwrap_err().to_string(), "no such program");
    assert_eq!(asked, [true, false]);
}

#[test]
fn any_other_failure_of_the_breakaway_start_is_not_tried_inside() {
    let (answer, asked) = outliving(
        || Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        || unreachable!("never inside"),
        false,
    );
    assert_eq!(answer.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert_eq!(asked, [true]);
}

/// What, near where a program is started, makes it open no console window
/// when the host starting it has no console: the launcher's `windowless` or
/// a `prepare_*` here (each `CREATE_NO_WINDOW` on Windows), or the flag
/// itself, or `DETACHED_PROCESS`, which gives it no console at all.
const NO_WINDOW_MARKS: [&str; 6] = [
    "windowless(",
    "prepare_root(",
    "prepare_lasting_root(",
    "prepare_orphan_root(",
    "CREATE_NO_WINDOW",
    "DETACHED_PROCESS",
];

/// The starts that need none of them, by file, and why.
const OPENS_NO_CONSOLE: [(&str, &str); 3] = [
    (
        "platform/installs/windows.rs",
        "the desktop app's installer, a GUI program, which has no console",
    ),
    (
        "platform/tray/windows.rs",
        "the desktop app itself, a GUI program, which has no console",
    ),
    (
        "platform/socket/windows.rs",
        "netstat in a function only test builds have",
    ),
];

/// Files compiled only on macOS and Linux, where no program has a console
/// window to open.
fn unix_only(file: &str) -> bool {
    [
        "unix.rs",
        "macos.rs",
        "linux.rs",
        "posix.rs",
        "signin_unix.rs",
    ]
    .iter()
    .any(|name| file.ends_with(name))
}

/// Every `.rs` file under `folder` but tests and the testkit, by its path
/// from `src`, with its text up to its test module.
fn host_sources(
    folder: &std::path::Path,
    src: &std::path::Path,
    found: &mut Vec<(String, String)>,
) {
    for entry in std::fs::read_dir(folder).expect("read the host's sources") {
        let path = entry.expect("a source entry").path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            if name != "tests" && name != "testkit" {
                host_sources(&path, src, found);
            }
            continue;
        }
        if !name.ends_with(".rs") || name == "tests.rs" || name == "testkit.rs" {
            continue;
        }
        let file = path
            .strip_prefix(src)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        // A Windows checkout may hold CRLF line ends.
        let text = std::fs::read_to_string(&path)
            .expect("read a source file")
            .replace('\r', "");
        let text = match text.find("#[cfg(test)]\nmod tests {") {
            Some(at) => text[..at].to_owned(),
            None => text,
        };
        found.push((file, text));
    }
}

/// Every program the host starts outside a terminal is started so that,
/// run from a host with no console (every host Floti starts on Windows),
/// it opens no console window: a console program a process with no console
/// starts without `CREATE_NO_WINDOW` opens one. A new start that is not
/// says where it is.
#[test]
fn every_program_the_host_starts_opens_no_console_window() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    host_sources(&src, &src, &mut sources);
    assert!(sources.len() > 50, "found only {} files", sources.len());
    let mut unmarked = Vec::new();
    let mut starts = 0;
    for (file, text) in &sources {
        if unix_only(file) || OPENS_NO_CONSOLE.iter().any(|(named, _)| file == named) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let started = code.match_indices("Command::new(").any(|(i, _)| {
                !code[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            });
            if !started {
                continue;
            }
            starts += 1;
            let near = &lines[at..lines.len().min(at + 20)];
            if !near
                .iter()
                .any(|line| NO_WINDOW_MARKS.iter().any(|mark| line.contains(mark)))
            {
                unmarked.push(format!("{file}:{}: {}", at + 1, code));
            }
        }
    }
    assert!(starts > 10, "found only {starts} starts");
    assert!(
        unmarked.is_empty(),
        "these start a program that would open a console window from a host with no console:\n{}",
        unmarked.join("\n")
    );
}
