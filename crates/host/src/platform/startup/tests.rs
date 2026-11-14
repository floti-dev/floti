use std::path::Path;

use super::{HostStart, HostStartup, StartupKind, linux, macos, windows};

const WINDOWS_BINARY: &str = r"C:\Program Files\Floti App\floti.exe";
const UNIX_BINARY: &str = "/opt/Floti App/bin/floti";

/// The text between `<open>` and `</open>`.
fn section<'a>(contents: &'a str, open: &str) -> &'a str {
    let start = contents
        .find(&format!("<{open}>"))
        .unwrap_or_else(|| panic!("no <{open}> in:\n{contents}"));
    let end = contents
        .find(&format!("</{open}>"))
        .unwrap_or_else(|| panic!("no </{open}> in:\n{contents}"));
    &contents[start..end]
}

#[test]
fn windows_registers_nothing_and_names_the_logon_task_an_earlier_floti_left() {
    // Whatever the binary, and with no user to name: nothing is generated.
    for binary in [WINDOWS_BINARY, "floti.exe", ""] {
        assert_eq!(
            windows::Native.host_start(Path::new(binary)).unwrap(),
            HostStart::WhenNeeded {
                leftover: windows::LEFTOVER_TASK
            }
        );
    }
    assert_eq!(windows::LEFTOVER_TASK.name, "Floti host");
    assert_eq!(windows::LEFTOVER_TASK.file_name, "Floti host.xml");
}

#[test]
fn macos_definition_is_a_launch_agent_that_runs_the_path_as_one_argument() {
    let def = macos::launch_agent(Path::new(UNIX_BINARY)).unwrap();
    assert_eq!(def.kind, StartupKind::LaunchAgent);
    assert_eq!(def.name, "dev.floti.host");
    assert_eq!(def.file_name, "dev.floti.host.plist");
    assert_eq!(def.user_folder, "Library/LaunchAgents");
    let plist = &def.contents;

    assert!(
        plist.contains("<key>Label</key>\n\t<string>dev.floti.host</string>"),
        "{plist}"
    );
    // The path is one element of ProgramArguments, spaces and all, then `host`.
    let args = section(plist, "array");
    assert_eq!(
        args.matches("<string>").count(),
        2,
        "two arguments in:\n{args}"
    );
    assert!(
        args.contains("<string>/opt/Floti App/bin/floti</string>\n\t\t<string>host</string>"),
        "{args}"
    );
    assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"), "{plist}");
    assert!(
        plist.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"),
        "{plist}"
    );

    // Never a LaunchDaemon: no user or group to run as, no system folder.
    for absent in [
        "LaunchDaemons",
        "<key>UserName</key>",
        "<key>GroupName</key>",
        "<key>InitGroups</key>",
        "root",
    ] {
        assert!(!plist.contains(absent), "found {absent} in:\n{plist}");
    }
}

#[test]
fn macos_definition_escapes_the_path_and_refuses_a_relative_one() {
    let def = macos::launch_agent(Path::new("/Users/someone/R&D <x>/floti")).unwrap();
    assert!(
        def.contents
            .contains("<string>/Users/someone/R&amp;D &lt;x&gt;/floti</string>"),
        "{}",
        def.contents
    );
    assert_eq!(def.file_bytes(), def.contents.as_bytes());
    assert!(macos::launch_agent(Path::new("bin/floti")).is_err());
    assert!(macos::launch_agent(Path::new("/opt/floti\nhost")).is_err());
}

#[test]
fn linux_definition_is_a_user_unit_wanted_by_default_target_with_the_path_quoted() {
    let def = linux::user_unit(Path::new(UNIX_BINARY)).unwrap();
    assert_eq!(def.kind, StartupKind::SystemdUserUnit);
    assert_eq!(def.name, "floti.service");
    assert_eq!(def.file_name, "floti.service");
    assert_eq!(def.user_folder, ".config/systemd/user");
    let unit = &def.contents;

    let exec: Vec<&str> = unit
        .lines()
        .filter(|line| line.starts_with("ExecStart="))
        .collect();
    assert_eq!(
        exec,
        [r#"ExecStart="/opt/Floti App/bin/floti" host"#],
        "{unit}"
    );

    let wanted: Vec<&str> = unit
        .lines()
        .filter(|line| line.starts_with("WantedBy="))
        .collect();
    assert_eq!(wanted, ["WantedBy=default.target"], "{unit}");

    // The host alone is signalled and killed, so the session keeper in the
    // unit's control group outlives a host that dies, and only a host that
    // ended with a failure (a crash or a kill) is started again.
    let service: Vec<&str> = unit
        .lines()
        .filter(|line| {
            [
                "KillMode=",
                "Restart=",
                "RestartSec=",
                "SuccessExitStatus=",
                "RestartPreventExitStatus=",
            ]
            .iter()
            .any(|key| line.starts_with(key))
        })
        .collect();
    // A host that exits because another already runs on its folders is not
    // a failure, and is not started again every second.
    assert_eq!(
        service,
        [
            "KillMode=process",
            "Restart=on-failure",
            "RestartSec=1",
            "SuccessExitStatus=3",
            "RestartPreventExitStatus=3"
        ],
        "{unit}"
    );
    assert_eq!(crate::ALREADY_RUNNING_EXIT, 3);

    for absent in [
        "User=",
        "Group=",
        "multi-user.target",
        "graphical.target",
        "sysinit.target",
        "RequiredBy=",
    ] {
        assert!(!unit.contains(absent), "found {absent} in:\n{unit}");
    }
}

#[test]
fn linux_definition_escapes_what_systemd_would_expand() {
    let def = linux::user_unit(Path::new(r#"/home/someone/50% "$HOME" \x/floti"#)).unwrap();
    assert!(
        def.contents
            .contains(r#"ExecStart="/home/someone/50%% \"$$HOME\" \\x/floti" host"#),
        "{}",
        def.contents
    );
    assert_eq!(def.file_bytes(), def.contents.as_bytes());
    assert!(linux::user_unit(Path::new("floti")).is_err());
    assert!(linux::user_unit(Path::new("/opt/floti\nExecStartPre=/bin/x")).is_err());
}

#[test]
fn macos_and_linux_start_the_host_at_sign_in_through_their_definitions() {
    let binary = Path::new(UNIX_BINARY);
    assert_eq!(
        macos::Native.host_start(binary).unwrap(),
        HostStart::AtSignIn(macos::launch_agent(binary).unwrap())
    );
    assert_eq!(
        linux::Native.host_start(binary).unwrap(),
        HostStart::AtSignIn(linux::user_unit(binary).unwrap())
    );
    assert!(linux::Native.host_start(Path::new("floti")).is_err());
}

#[test]
fn native_start_runs_this_binary_where_it_starts_at_sign_in() {
    let binary = std::env::current_exe().unwrap();
    match super::Native.host_start(&binary).unwrap() {
        HostStart::AtSignIn(def) => {
            assert!(def.contents.contains("host"), "{}", def.contents);
        }
        HostStart::WhenNeeded { leftover } => assert_eq!(leftover, windows::LEFTOVER_TASK),
    }
}

/// Generating a definition registers nothing: the generators compose text and
/// reach neither the file system, nor a process, nor the OS's own tools.
#[test]
fn generating_any_definition_registers_nothing_with_the_os() {
    let sources = [
        ("mod.rs", include_str!("mod.rs")),
        ("windows.rs", include_str!("windows.rs")),
        ("macos.rs", include_str!("macos.rs")),
        ("linux.rs", include_str!("linux.rs")),
    ];
    let forbidden = [
        "std::process",
        "Command::new",
        "std::fs",
        "fs::",
        "tokio::",
        "std::net",
        "unsafe",
        "windows_sys",
        "libc::",
        "schtasks",
        "launchctl",
        "systemctl",
    ];
    for (file, source) in sources {
        for (number, line) in source.lines().enumerate() {
            for word in forbidden {
                assert!(
                    !line.contains(word),
                    "startup/{file}:{} uses {word}: {line}",
                    number + 1
                );
            }
        }
    }
}

fn env_with(vars: &[(&str, &str)]) -> crate::platform::folders::UserEnv {
    crate::platform::folders::UserEnv {
        home: None,
        vars: vars
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
    }
}

/// One sentence, ending at the only full stop, that says where the host
/// starts.
fn assert_one_sentence_about_sign_in(refusal: &str) {
    assert!(
        refusal.starts_with("The Floti host starts at your sign-in"),
        "{refusal}"
    );
    assert_eq!(refusal.matches(". ").count(), 0, "{refusal}");
    assert!(refusal.ends_with('.'), "{refusal}");
}

#[test]
fn s5_a_start_from_an_ssh_session_is_refused_in_one_sentence() {
    for name in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
        let refusal = super::launch_refusal(
            &env_with(&[(name, "203.0.113.9 50022 192.0.2.1 22")]),
            super::Account::User,
        )
        .unwrap_or_else(|| panic!("{name} set, yet the host would start"));
        assert_one_sentence_about_sign_in(&refusal);
        assert!(refusal.contains("SSH"), "{refusal}");
    }
    assert_eq!(
        super::launch_refusal(&env_with(&[("SSH_CONNECTION", "")]), super::Account::User),
        None,
        "an empty variable is no SSH session"
    );
}

#[test]
fn s5_a_start_as_root_or_a_service_account_is_refused_in_one_sentence() {
    for name in ["root", "the SYSTEM account"] {
        let refusal = super::launch_refusal(&env_with(&[]), super::Account::Privileged(name))
            .unwrap_or_else(|| panic!("running as {name}, yet the host would start"));
        assert_one_sentence_about_sign_in(&refusal);
        assert!(refusal.contains(name), "{refusal}");
    }
}

#[test]
fn s5_a_start_at_the_users_sign_in_is_not_refused() {
    assert_eq!(
        super::launch_refusal(&env_with(&[]), super::Account::User),
        None
    );
    assert_eq!(
        super::launch_refusal(&env_with(&[]), super::Account::Unknown),
        None
    );
    // The tests run as an ordinary user, never root or a service account.
    assert!(
        !matches!(super::account(), super::Account::Privileged(_)),
        "{:?}",
        super::account()
    );
}

// ---------------------------------------------------------------------------
// Taking an entry out again, for floti uninstall
// ---------------------------------------------------------------------------

mod unregistering {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io;
    use std::path::{Path, PathBuf};

    use super::super::register::{
        OsCommand, Ran, SignInSystem, WANTS_FOLDER, leftovers_at_start, leftovers_line,
        lingering_on, remove_leftover_task, remove_leftovers, unregister,
    };
    use super::super::{HostStart, StartupDefinition, linux, macos, windows};
    use super::UNIX_BINARY;
    use crate::platform::folders::{CONFIG_READ_ONLY_VAR, UserEnv};
    use crate::testkit::TestDirs;

    /// An OS that answers each command line it is given from `answers`,
    /// succeeding with nothing written where it has none, and records every
    /// command it was given.
    #[derive(Default)]
    struct FakeOs {
        answers: HashMap<String, Ran>,
        ran: RefCell<Vec<String>>,
    }

    impl FakeOs {
        /// Each command line with whether it succeeds and what it prints;
        /// one that fails says `<line> refused` on standard error.
        fn answering(answers: &[(&str, bool, &str)]) -> Self {
            Self {
                answers: answers
                    .iter()
                    .map(|(line, success, stdout)| {
                        let ran = Ran {
                            success: *success,
                            stdout: (*stdout).to_owned(),
                            stderr: if *success {
                                String::new()
                            } else {
                                format!("{line} refused")
                            },
                        };
                        ((*line).to_owned(), ran)
                    })
                    .collect(),
                ran: RefCell::default(),
            }
        }

        fn ran(&self) -> Vec<String> {
            self.ran.borrow().clone()
        }
    }

    impl SignInSystem for FakeOs {
        fn run(&self, command: &OsCommand) -> io::Result<Ran> {
            let line = command.described();
            self.ran.borrow_mut().push(line.clone());
            Ok(self.answers.get(&line).cloned().unwrap_or(Ran {
                success: true,
                ..Ran::default()
            }))
        }

        fn desktop_session(&self) -> io::Result<bool> {
            Ok(true)
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const QUERY: &str = "schtasks /Query /FO CSV /NH";
    const DELETE: &str = "schtasks /Delete /TN Floti host /F";
    const LISTED: &str =
        "\"\\Floti host\",\"N/A\",\"Running\"\r\n\"\\Other\",\"N/A\",\"Ready\"\r\n";

    const TASK: super::super::LeftoverTask = windows::LEFTOVER_TASK;

    /// Never `schtasks /End` or `/Run`: deleting a task leaves the host it
    /// started running, and starts nothing.
    fn assert_only_query_and_delete(os: &FakeOs) {
        for line in os.ran() {
            assert!(line == QUERY || line == DELETE, "ran {line}");
        }
    }

    #[test]
    fn a_listed_leftover_logon_task_is_deleted_and_its_xml_and_folder_go() {
        let dirs = TestDirs::new();
        let xml = dirs.data.join("startup").join("Floti host.xml");
        write(&xml, "<Task/>");
        let os = FakeOs::answering(&[(QUERY, true, LISTED)]);

        assert_eq!(remove_leftover_task(&os, &TASK, &dirs.data), Ok(true));
        assert_eq!(os.ran(), [QUERY, DELETE]);
        assert!(!xml.exists());
        assert!(!dirs.data.join("startup").exists(), "its emptied folder");
        assert_only_query_and_delete(&os);
    }

    #[test]
    fn a_leftover_task_that_is_not_listed_is_not_found_and_nothing_is_deleted() {
        let dirs = TestDirs::new();
        let os = FakeOs::answering(&[(QUERY, true, "\"\\Other\",\"N/A\",\"Ready\"\r\n")]);
        assert_eq!(remove_leftover_task(&os, &TASK, &dirs.data), Ok(false));
        assert_eq!(os.ran(), [QUERY]);
        // A startup folder holding something else stays.
        let note = dirs.data.join("startup").join("note.txt");
        write(&note, "mine");
        assert_eq!(remove_leftover_task(&os, &TASK, &dirs.data), Ok(false));
        assert!(note.exists());
    }

    #[test]
    fn a_leftover_xml_whose_task_is_gone_still_goes() {
        let dirs = TestDirs::new();
        let xml = dirs.data.join("startup").join("Floti host.xml");
        write(&xml, "<Task/>");
        let os = FakeOs::answering(&[(QUERY, true, "")]);
        assert_eq!(remove_leftover_task(&os, &TASK, &dirs.data), Ok(true));
        assert_eq!(os.ran(), [QUERY]);
        assert!(!xml.exists());
    }

    #[test]
    fn a_refused_delete_or_listing_says_schtasks_own_last_line() {
        let dirs = TestDirs::new();
        let os = FakeOs::answering(&[(QUERY, true, LISTED), (DELETE, false, "")]);
        assert_eq!(
            remove_leftover_task(&os, &TASK, &dirs.data),
            Err(format!("{DELETE} refused"))
        );
        let os = FakeOs::answering(&[(QUERY, false, "")]);
        assert_eq!(
            remove_leftover_task(&os, &TASK, &dirs.data),
            Err(format!("{QUERY} refused"))
        );
    }

    #[test]
    fn deleting_the_leftover_task_is_said_in_one_line_and_a_refusal_is_a_warning_naming_the_fix() {
        let dirs = TestDirs::new();
        let when_needed = HostStart::WhenNeeded { leftover: TASK };
        let os = FakeOs::answering(&[(QUERY, true, LISTED)]);
        assert_eq!(
            leftovers_line(&os, &when_needed, &dirs.data).as_deref(),
            Some(
                "Deleted the logon task \"Floti host\" an earlier floti registered; the Floti host starts when Floti or a connection needs it."
            )
        );
        let nothing = FakeOs::answering(&[(QUERY, true, "")]);
        assert_eq!(leftovers_line(&nothing, &when_needed, &dirs.data), None);
        let refused = FakeOs::answering(&[(QUERY, true, LISTED), (DELETE, false, "")]);
        assert_eq!(
            leftovers_line(&refused, &when_needed, &dirs.data).as_deref(),
            Some(
                "Warning: the logon task \"Floti host\" an earlier floti registered was not deleted (schtasks /Delete /TN Floti host /F refused); schtasks /Delete /TN \"Floti host\" /F deletes it."
            )
        );
        assert_only_query_and_delete(&refused);
        let at_sign_in = FakeOs::answering(&[]);
        assert_eq!(
            leftovers_line(&at_sign_in, &HostStart::AtSignIn(unit()), &dirs.data),
            None
        );
        assert!(at_sign_in.ran().is_empty());
    }

    #[test]
    fn a_host_deletes_the_leftover_task_at_its_start_unless_it_runs_read_only() {
        let dirs = TestDirs::new();
        let when_needed = HostStart::WhenNeeded { leftover: TASK };
        let user = |vars: &[(&str, &str)]| UserEnv {
            home: None,
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into()))
                .collect(),
        };

        // A development or check host beside the installed Floti asks nothing.
        let read_only = FakeOs::answering(&[(QUERY, true, LISTED)]);
        let env = user(&[(CONFIG_READ_ONLY_VAR, "1")]);
        assert_eq!(
            leftovers_at_start(&read_only, &when_needed, &dirs.data, &env),
            None
        );
        assert!(read_only.ran().is_empty(), "{:?}", read_only.ran());

        let os = FakeOs::answering(&[(QUERY, true, LISTED)]);
        let line = leftovers_at_start(&os, &when_needed, &dirs.data, &user(&[])).unwrap();
        assert!(
            line.starts_with("Deleted the logon task \"Floti host\""),
            "{line}"
        );
        assert_eq!(os.ran(), [QUERY, DELETE]);
        assert_only_query_and_delete(&os);
    }

    #[test]
    fn only_a_start_when_needed_has_leftovers_to_remove() {
        let dirs = TestDirs::new();
        let os = FakeOs::answering(&[(QUERY, true, LISTED)]);
        let at_sign_in = HostStart::AtSignIn(unit());
        assert_eq!(remove_leftovers(&os, &at_sign_in, &dirs.data), Ok(false));
        assert!(os.ran().is_empty(), "{:?}", os.ran());
        let when_needed = HostStart::WhenNeeded { leftover: TASK };
        assert_eq!(remove_leftovers(&os, &when_needed, &dirs.data), Ok(true));
        assert_eq!(os.ran(), [QUERY, DELETE]);
    }

    fn agent() -> StartupDefinition {
        macos::launch_agent(Path::new(UNIX_BINARY)).unwrap()
    }

    fn plist(dirs: &TestDirs) -> PathBuf {
        dirs.home
            .join("Library")
            .join("LaunchAgents")
            .join("dev.floti.host.plist")
    }

    #[test]
    fn a_loaded_launch_agent_is_booted_out_and_its_property_list_goes() {
        let dirs = TestDirs::new();
        write(&plist(&dirs), "<plist/>");
        let os = FakeOs::answering(&[("id -u", true, "501\n")]);

        assert_eq!(unregister(&os, &agent(), &dirs.home), Ok(true));
        assert_eq!(
            os.ran(),
            [
                "id -u",
                "launchctl print gui/501",
                "launchctl print gui/501/dev.floti.host",
                "launchctl bootout gui/501/dev.floti.host"
            ]
        );
        assert!(!plist(&dirs).exists());
    }

    #[test]
    fn with_nobody_at_the_screen_the_property_list_still_goes_and_nothing_else_runs() {
        let dirs = TestDirs::new();
        write(&plist(&dirs), "<plist/>");
        let os = FakeOs::answering(&[
            ("id -u", true, "501\n"),
            ("launchctl print gui/501", false, ""),
        ]);
        assert_eq!(unregister(&os, &agent(), &dirs.home), Ok(true));
        assert_eq!(os.ran(), ["id -u", "launchctl print gui/501"]);
        assert!(!plist(&dirs).exists());
        // Again: neither loaded nor there.
        assert_eq!(unregister(&os, &agent(), &dirs.home), Ok(false));
    }

    #[test]
    fn a_launch_agent_launchd_has_not_loaded_is_not_booted_out() {
        let dirs = TestDirs::new();
        let os = FakeOs::answering(&[
            ("id -u", true, "501\n"),
            ("launchctl print gui/501/dev.floti.host", false, ""),
        ]);
        assert_eq!(unregister(&os, &agent(), &dirs.home), Ok(false));
        assert!(!os.ran().iter().any(|line| line.contains("bootout")));
    }

    fn unit() -> StartupDefinition {
        linux::user_unit(Path::new(UNIX_BINARY)).unwrap()
    }

    /// The unit's file and the link `systemctl enable` makes for it.
    fn unit_files(dirs: &TestDirs) -> (PathBuf, PathBuf) {
        let folder = dirs.home.join(".config").join("systemd").join("user");
        (
            folder.join("floti.service"),
            folder.join(WANTS_FOLDER).join("floti.service"),
        )
    }

    #[test]
    fn an_enabled_unit_is_disabled_and_stopped_its_file_goes_and_the_manager_reloads() {
        let dirs = TestDirs::new();
        let (file, wants) = unit_files(&dirs);
        write(&file, "[Unit]");
        write(&wants, "the link systemctl enable made");
        let os = FakeOs::default();

        assert_eq!(unregister(&os, &unit(), &dirs.home), Ok(true));
        assert_eq!(
            os.ran(),
            [
                "systemctl --user show-environment",
                "systemctl --user disable --now floti.service",
                "systemctl --user daemon-reload"
            ]
        );
        assert!(!file.exists() && !wants.exists());
        // Again: nothing there, nothing run.
        let again = FakeOs::default();
        assert_eq!(unregister(&again, &unit(), &dirs.home), Ok(false));
        assert!(again.ran().is_empty());
    }

    #[test]
    fn with_no_user_manager_the_unit_and_its_link_go_by_hand() {
        let dirs = TestDirs::new();
        let (file, wants) = unit_files(&dirs);
        write(&file, "[Unit]");
        write(&wants, "the link systemctl enable made");
        let os = FakeOs::answering(&[("systemctl --user show-environment", false, "")]);

        assert_eq!(unregister(&os, &unit(), &dirs.home), Ok(true));
        assert_eq!(os.ran(), ["systemctl --user show-environment"]);
        assert!(!file.exists() && !wants.exists());
    }

    #[test]
    fn a_link_left_without_its_unit_goes_without_asking_systemctl() {
        let dirs = TestDirs::new();
        let (_, wants) = unit_files(&dirs);
        write(&wants, "a link to a unit that is gone");
        let os = FakeOs::default();
        assert_eq!(unregister(&os, &unit(), &dirs.home), Ok(true));
        assert!(os.ran().is_empty(), "{:?}", os.ran());
        assert!(!wants.exists());
    }

    #[test]
    fn lingering_is_only_read_and_on_only_when_loginctl_says_yes() {
        const SHOW: &str = "loginctl show-user 1000 --property=Linger --value";
        let on = FakeOs::answering(&[("id -u", true, "1000\n"), (SHOW, true, "yes\n")]);
        assert!(lingering_on(&on));
        assert_eq!(on.ran(), ["id -u", SHOW]);
        let off = FakeOs::answering(&[("id -u", true, "1000\n"), (SHOW, true, "no\n")]);
        assert!(!lingering_on(&off));
        let unknown = FakeOs::answering(&[("id -u", false, "")]);
        assert!(!lingering_on(&unknown));
    }
}
