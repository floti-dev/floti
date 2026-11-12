use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;

use super::*;
use crate::platform::startup::{linux, macos, windows};
use crate::testkit::TestDirs;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn it_takes_nothing_or_purge_and_nothing_else() {
    assert_eq!(parse(&args(&[])), Some(Options { purge: false }));
    assert_eq!(parse(&args(&["--purge"])), Some(Options { purge: true }));
    for wrong in [
        &["--purge", "--purge"][..],
        &["purge"],
        &["--yes"],
        &["--purge", "desktop"],
    ] {
        assert_eq!(parse(&args(wrong)), None, "{wrong:?}");
    }
}

#[test]
fn the_host_line_says_how_many_sessions_it_ended() {
    assert_eq!(ending(Some(0)), "ending no session");
    assert_eq!(ending(Some(1)), "ending 1 session");
    assert_eq!(ending(Some(3)), "ending 3 sessions");
    assert_eq!(ending(None), "ending every session it held");
}

#[test]
fn each_oss_entry_is_named_as_a_person_calls_it() {
    let binary = Path::new("/home/me/.local/bin/floti");
    assert_eq!(
        entry_name(&windows::logon_task(Path::new(r"C:\Floti\floti.exe"), "me").unwrap()),
        "logon task \"Floti host\""
    );
    assert_eq!(
        entry_name(&macos::launch_agent(binary).unwrap()),
        "LaunchAgent dev.floti.host"
    );
    assert_eq!(
        entry_name(&linux::user_unit(binary).unwrap()),
        "systemd user unit floti.service"
    );
}

fn row(pid: u32, parent_pid: u32, name: &str) -> ProcessRow {
    ProcessRow {
        pid,
        parent_pid,
        name: name.to_owned(),
        command_line: None,
    }
}

#[test]
fn a_floti_among_its_parents_is_a_session_it_would_end() {
    // floti keeper > pwsh > floti uninstall
    let in_session = [
        row(10, 1, "floti.exe"),
        row(20, 10, "pwsh.exe"),
        row(30, 20, "floti.exe"),
    ];
    assert!(inside_floti(&in_session, 30));
    // The same on macOS and Linux, where the name has no extension.
    let unix = [
        row(10, 1, "floti"),
        row(20, 10, "zsh"),
        row(30, 20, "floti"),
    ];
    assert!(inside_floti(&unix, 30));
}

#[test]
fn a_terminal_floti_did_not_start_is_not_a_session() {
    // explorer > Windows Terminal > pwsh > floti uninstall: only itself is floti.
    let terminal = [
        row(4, 0, "explorer.exe"),
        row(10, 4, "WindowsTerminal.exe"),
        row(20, 10, "pwsh.exe"),
        row(30, 20, "floti.exe"),
    ];
    assert!(!inside_floti(&terminal, 30));
    // A parent missing from the table, or a loop of parents, ends the walk.
    assert!(!inside_floti(&[row(30, 99, "floti")], 30));
    assert!(!inside_floti(
        &[row(10, 20, "bash"), row(20, 10, "sh"), row(30, 10, "floti")],
        30
    ));
    assert!(!inside_floti(&[], 30));
}

// ---------------------------------------------------------------------------
// Closing the desktop app
// ---------------------------------------------------------------------------

/// A process table's processes, ended one at a time: `others` belong to
/// another user and refuse, `gone` exited before they were ended.
struct FakeEnder {
    running: RefCell<HashSet<u32>>,
    others: HashSet<u32>,
    gone: HashSet<u32>,
    ended: RefCell<Vec<u32>>,
}

impl FakeEnder {
    fn of(rows: &[ProcessRow], others: &[u32], gone: &[u32]) -> Self {
        Self {
            running: RefCell::new(
                rows.iter()
                    .map(|row| row.pid)
                    .filter(|pid| !gone.contains(pid))
                    .collect(),
            ),
            others: others.iter().copied().collect(),
            gone: gone.iter().copied().collect(),
            ended: RefCell::default(),
        }
    }
}

impl ProcessEnder for FakeEnder {
    fn end(&self, pid: u32) -> io::Result<()> {
        if self.others.contains(&pid) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if self.gone.contains(&pid) {
            return Err(io::Error::other("no such process"));
        }
        self.ended.borrow_mut().push(pid);
        self.running.borrow_mut().remove(&pid);
        Ok(())
    }

    fn alive(&self, pid: u32) -> bool {
        self.running.borrow().contains(&pid)
    }
}

/// The app and its helpers, the host it started (its child), the host's
/// keeper, and a session in the keeper.
fn desktop_with_its_host() -> Vec<ProcessRow> {
    vec![
        row(100, 1, "Helm.exe"),
        row(101, 100, "Helm.exe"),
        row(102, 100, "Helm.exe"),
        row(200, 100, "floti.exe"),
        row(300, 200, "floti.exe"),
        row(400, 300, "claude.exe"),
    ]
}

fn is_helm(row: &ProcessRow) -> bool {
    row.name == "Helm.exe"
}

#[test]
fn the_app_is_closed_process_by_process_and_the_host_it_started_is_left_to_stop() {
    let rows = desktop_with_its_host();
    let ender = FakeEnder::of(&rows, &[], &[]);
    let mut paused = Vec::new();

    let closed = close_processes(&rows, &is_helm, &ender, CLOSE_WAIT, &mut |pause| {
        paused.push(pause);
    })
    .unwrap();
    assert_eq!(
        closed,
        Closed {
            ended: 3,
            skipped: 0
        }
    );
    assert_eq!(*ender.ended.borrow(), [100, 101, 102]);
    // The host, its keeper and the session run on, for the host's stop.
    for pid in [200, 300, 400] {
        assert!(ender.alive(pid), "{pid} still runs");
    }
    assert!(paused.is_empty(), "they went at once");
}

#[test]
fn another_users_app_is_left_and_counted_and_one_already_gone_is_not() {
    let mut rows = desktop_with_its_host();
    rows.push(row(900, 1, "Helm.exe"));
    let ender = FakeEnder::of(&rows, &[900], &[102]);

    let closed = close_processes(&rows, &is_helm, &ender, CLOSE_WAIT, &mut |_| {}).unwrap();
    assert_eq!(
        closed,
        Closed {
            ended: 2,
            skipped: 1
        }
    );
    assert_eq!(*ender.ended.borrow(), [100, 101]);
    assert!(ender.alive(900), "another user's runs on");
}

#[test]
fn no_app_running_ends_nothing() {
    let rows = vec![row(200, 1, "floti.exe")];
    let ender = FakeEnder::of(&rows, &[], &[]);
    let closed = close_processes(&rows, &is_helm, &ender, CLOSE_WAIT, &mut |_| {}).unwrap();
    assert_eq!(closed, Closed::default());
    assert!(ender.ended.borrow().is_empty());
}

#[test]
fn an_app_that_does_not_go_fails_after_the_patience_in_stepped_time() {
    struct Stubborn;
    impl ProcessEnder for Stubborn {
        fn end(&self, _pid: u32) -> io::Result<()> {
            Ok(())
        }
        fn alive(&self, _pid: u32) -> bool {
            true
        }
    }
    let rows = vec![row(100, 1, "Helm.exe")];
    let mut waited = Duration::ZERO;
    let failed = close_processes(&rows, &is_helm, &Stubborn, CLOSE_WAIT, &mut |pause| {
        waited += pause;
    })
    .unwrap_err();
    assert!(failed.contains("still running"), "{failed}");
    assert_eq!(waited, CLOSE_WAIT);
}

// ---------------------------------------------------------------------------
// What a purge may remove
// ---------------------------------------------------------------------------

/// A purge for the test's folders, with `layout` as the folders floti uses
/// there with no override.
fn plan(
    dirs: &TestDirs,
    env: &UserEnv,
    layout: Option<Folders>,
) -> Result<Vec<PurgeTarget>, String> {
    let folders = dirs.folders();
    let definition = linux::user_unit(Path::new("/home/me/.local/bin/floti")).unwrap();
    purge_plan(&Uninstall {
        env,
        folders: &folders,
        layout,
        running: Path::new("floti"),
        definition: &definition,
        secret_entry: StoreEntry::installed(),
        purge: true,
    })
}

fn actions(planned: &[PurgeTarget]) -> Vec<(&'static str, PurgeAction)> {
    planned
        .iter()
        .map(|target| (target.item, target.action.clone()))
        .collect()
}

#[test]
fn the_folders_floti_uses_are_purged_without_a_mark() {
    let dirs = TestDirs::new();
    let planned = plan(&dirs, &dirs.user_env(), Some(dirs.folders())).unwrap();
    assert_eq!(
        actions(&planned),
        [
            ("data", PurgeAction::Remove),
            ("logs", PurgeAction::Remove),
            ("runtime", PurgeAction::Remove),
            ("config", PurgeAction::Remove),
        ]
    );
}

#[test]
fn an_overrides_folder_is_purged_only_with_a_mark_of_flotis() {
    let dirs = TestDirs::new();
    let env = dirs.user_env();
    // No layout matches: each folder is an override's, refused in turn
    // until it carries what floti writes there.
    let refused = plan(&dirs, &env, None).unwrap_err();
    assert!(refused.starts_with("data: "), "{refused}");
    assert!(
        refused.contains("holds nothing floti writes there"),
        "{refused}"
    );
    assert!(refused.ends_with("floti uninstall without --purge removes the rest."));

    std::fs::write(dirs.data.join(HOST_KEY_FILE), b"key").unwrap();
    let refused = plan(&dirs, &env, None).unwrap_err();
    assert!(refused.starts_with("logs: "), "{refused}");

    std::fs::write(dirs.logs.join(LOG_FILE), b"log").unwrap();
    let refused = plan(&dirs, &env, None).unwrap_err();
    assert!(refused.starts_with("config: "), "{refused}");

    std::fs::create_dir_all(dirs.config.join("templates")).unwrap();
    let planned = plan(&dirs, &env, None).unwrap();
    assert_eq!(
        actions(&planned),
        [
            ("data", PurgeAction::Remove),
            ("logs", PurgeAction::Remove),
            (
                "runtime",
                PurgeAction::Leave("it is not the folder floti uses".to_owned())
            ),
            ("config", PurgeAction::Remove),
        ]
    );
}

#[test]
fn the_data_folder_is_marked_by_its_host_key_or_its_database() {
    let dirs = TestDirs::new();
    assert!(!marked("data", &dirs.data));
    std::fs::write(dirs.data.join(HOST_KEY_FILE), b"key").unwrap();
    assert!(marked("data", &dirs.data));
    std::fs::remove_file(dirs.data.join(HOST_KEY_FILE)).unwrap();
    std::fs::write(dirs.data.join(DB_FILE), b"db").unwrap();
    assert!(marked("data", &dirs.data));
    assert!(
        !marked("runtime", &dirs.runtime),
        "the socket's folder has no mark"
    );
}

#[test]
fn a_purge_with_no_home_folder_is_refused_outright() {
    let dirs = TestDirs::new();
    let homeless = UserEnv::default();
    let refused = plan(&dirs, &homeless, Some(dirs.folders())).unwrap_err();
    assert!(
        refused.starts_with("--purge needs your home folder"),
        "{refused}"
    );
    assert!(refused.contains("Nothing was changed"), "{refused}");
}

#[test]
fn a_folder_holding_a_repository_is_refused_even_where_floti_would_use_it() {
    let dirs = TestDirs::new();
    std::fs::create_dir_all(dirs.config.join(".git")).unwrap();
    let refused = plan(&dirs, &dirs.user_env(), Some(dirs.folders())).unwrap_err();
    assert!(refused.starts_with("config: "), "{refused}");
    assert!(refused.contains("it holds a .git"), "{refused}");
}

#[test]
fn the_home_folder_a_folder_holding_it_and_claude_and_ssh_are_never_purged() {
    let dirs = TestDirs::new();
    let home = &dirs.home;
    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join(".local").join("share").join("floti")).unwrap();

    assert!(refusal(home, home).is_some(), "the home folder");
    assert!(
        refusal(home, dirs.root()).is_some(),
        "a folder holding home"
    );
    assert!(refusal(home, &home.join(".claude")).is_some());
    assert!(
        refusal(home, &home.join(".claude").join("projects")).is_some(),
        "inside ~/.claude"
    );
    assert!(refusal(home, &home.join(".ssh")).is_some());
    assert!(refusal(home, Path::new("/")).is_some(), "a root");
    assert_eq!(
        refusal(home, &home.join(".local").join("share").join("floti")),
        None
    );
    assert_eq!(refusal(home, &dirs.data), None);

    // Where floti would use it, an override's home folder is still refused.
    let mut folders = dirs.folders();
    folders.data = home.clone();
    let definition = linux::user_unit(Path::new("/home/me/.local/bin/floti")).unwrap();
    let env = dirs.user_env();
    let refused = purge_plan(&Uninstall {
        env: &env,
        folders: &folders,
        layout: Some(folders.clone()),
        running: Path::new("floti"),
        definition: &definition,
        secret_entry: StoreEntry::installed(),
        purge: true,
    })
    .unwrap_err();
    assert!(refused.starts_with("data: "), "{refused}");
    assert!(refused.contains("your home folder"), "{refused}");
}
