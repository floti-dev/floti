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

#[test]
fn a_purge_never_removes_the_home_folder_or_anything_holding_claude_or_ssh() {
    let dirs = TestDirs::new();
    let env = dirs.user_env();
    let home = &dirs.home;
    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join(".local").join("share").join("floti")).unwrap();

    assert!(refusal(&env, home).is_some(), "the home folder");
    assert!(
        refusal(&env, dirs.root()).is_some(),
        "a folder holding home"
    );
    assert!(refusal(&env, &home.join(".claude")).is_some());
    assert!(
        refusal(&env, &home.join(".claude").join("projects")).is_some(),
        "inside ~/.claude"
    );
    assert!(refusal(&env, &home.join(".ssh")).is_some());
    assert!(refusal(&env, Path::new("/")).is_some(), "a root");
    assert_eq!(
        refusal(&env, &home.join(".local").join("share").join("floti")),
        None
    );
    assert_eq!(refusal(&env, &dirs.data), None);
}
