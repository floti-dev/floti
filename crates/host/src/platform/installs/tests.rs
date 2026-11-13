//! Each OS's installs, on every OS, over folders in a `TestDirs`: where the
//! desktop app is found and goes, which version it is, which processes are
//! its own, which floti is its own, and the renames that put a binary, an
//! AppImage or an app in place.

use std::cell::RefCell;
use std::ffi::OsString;
use std::io;
use std::path::Path;

use super::{
    DESKTOP_DIR_VAR, DISPLAY_TEST_VAR, Desktop, Installs, display_vars, leftovers, linux, macos,
    missing_display, not_offered, plist_string, windows,
};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::platform::startup::register::{OsCommand, Ran, SignInSystem};
use crate::testkit::TestDirs;

fn env(home: &Path, vars: &[(&str, &Path)]) -> UserEnv {
    UserEnv {
        home: Some(home.to_path_buf()),
        vars: vars
            .iter()
            .map(|(name, value)| (OsString::from(name), value.as_os_str().to_owned()))
            .collect(),
    }
}

fn row(name: &str, command_line: Option<&str>) -> ProcessRow {
    ProcessRow {
        pid: 10,
        parent_pid: 1,
        name: name.into(),
        command_line: command_line.map(str::to_owned),
    }
}

#[test]
fn windows_finds_the_per_user_install_and_not_a_portable_copy() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    let install = local.join("Programs").join("Floti").join("desktop");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("Floti.exe"), b"app").unwrap();
    let user = env(&dirs.home, &[("LOCALAPPDATA", &local)]);
    assert_eq!(
        windows::Native.desktop(&user),
        None,
        "no uninstaller: a portable copy"
    );
    std::fs::write(install.join("Uninstall Floti.exe"), b"uninstaller").unwrap();
    assert_eq!(
        windows::Native.desktop(&user),
        Some(Desktop {
            name: "Floti",
            path: install.clone()
        })
    );

    let moved = dirs.folder("apps dir");
    let user = env(
        &dirs.home,
        &[("LOCALAPPDATA", &local), (DESKTOP_DIR_VAR, &moved)],
    );
    assert_eq!(
        windows::Native.desktop(&user),
        None,
        "only the folder it names is looked in"
    );
}

#[test]
fn windows_puts_a_new_install_where_it_updates_one() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    let user = env(&dirs.home, &[("LOCALAPPDATA", &local)]);
    assert_eq!(windows::Native.desktop(&user), None);
    assert_eq!(
        windows::Native.desktop_target(&user),
        Some(Desktop {
            name: "Floti",
            path: local.join("Programs").join("Floti").join("desktop")
        })
    );
    // The installer runs install-self itself.
    assert_eq!(
        windows::Native.carried_floti(&Desktop {
            name: "Floti",
            path: local.join("Programs").join("Floti").join("desktop")
        }),
        None
    );
}

#[test]
fn windows_counts_the_floti_in_the_install_s_resources_as_the_app_s() {
    let desktop = Desktop {
        name: "Floti",
        path: r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop".into(),
    };
    assert!(windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop\resources\floti.exe")
    ));
    assert!(windows::Native.bundles(
        &desktop,
        Path::new(r"c:\users\some one\appdata\local\programs\FLOTI\DESKTOP\resources\floti.exe")
    ));
    assert!(!windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Floti\bin\floti.exe")
    ));
    assert!(!windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop Old\floti.exe")
    ));
}

#[test]
fn windows_reads_the_app_s_version_from_its_own_uninstall_entry() {
    let desktop = Desktop {
        name: "Floti",
        path: r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop".into(),
    };
    let entry = |uninstall: &str, version: &str| windows::UninstallEntry {
        uninstall_string: uninstall.into(),
        display_version: version.into(),
    };
    let entries = [
        entry(r#""C:\Program Files\Other\uninstall.exe" /S"#, "9.9.9"),
        entry(
            r#""C:\Users\Some One\AppData\Local\Programs\Floti\desktop Copy\Uninstall Floti.exe" /currentuser"#,
            "1.0.0",
        ),
        entry(
            r#""C:\Users\some one\AppData\Local\Programs\Floti\desktop\Uninstall Floti.exe" /currentuser"#,
            "2.4.1-preview.11",
        ),
    ];
    assert_eq!(
        windows::Native::uninstall_version(&entries, &desktop).as_deref(),
        Some("2.4.1-preview.11")
    );
    let unquoted = [entry(
        r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop\Uninstall Floti.exe /currentuser",
        "2.5.0",
    )];
    assert_eq!(
        windows::Native::uninstall_version(&unquoted, &desktop).as_deref(),
        Some("2.5.0")
    );
    assert_eq!(
        windows::Native::uninstall_version(&entries[..2], &desktop),
        None
    );
}

#[test]
fn windows_counts_only_the_installed_program_s_processes() {
    let desktop = Desktop {
        name: "Floti",
        path: "C:\\Users\\Some One\\AppData\\Local\\Programs\\Floti\\desktop".into(),
    };
    let installed = r#""C:\Users\Some One\AppData\Local\Programs\Floti\desktop\Floti.exe" --type=renderer"#;
    assert!(windows::Native.is_desktop_process(&desktop, &row("Floti.exe", Some(installed))));
    assert!(
        windows::Native
            .is_desktop_process(&desktop, &row("floti.EXE", Some(&installed.to_uppercase())))
    );
    let unquoted = r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop\Floti.exe --type=gpu-process";
    assert!(windows::Native.is_desktop_process(&desktop, &row("Floti.exe", Some(unquoted))));
    assert!(!windows::Native.is_desktop_process(
        &desktop,
        &row("Floti.exe", Some(r"C:\Temp\portable\Floti.exe"))
    ));
    let host = r#""C:\Users\Some One\AppData\Local\Programs\Floti\desktop\resources\floti.exe" host"#;
    assert!(!windows::Native.is_desktop_process(&desktop, &row("floti.exe", Some(host))));
}

#[test]
fn windows_never_takes_the_host_for_the_app_though_their_names_differ_only_by_case() {
    let desktop = Desktop {
        name: "Floti",
        path: r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop".into(),
    };
    // The host, its keeper and a bridge, from floti's own folder.
    for command_line in [
        r#""C:\Users\Some One\AppData\Local\Programs\Floti\bin\floti.exe" host"#,
        r#"C:\Users\Some One\AppData\Local\Programs\Floti\bin\floti.exe keeper"#,
        r#""C:\Users\Some One\AppData\Local\Programs\Floti\bin\floti.exe" bridge"#,
    ] {
        assert!(
            !windows::Native.is_desktop_process(&desktop, &row("floti.exe", Some(command_line))),
            "{command_line}"
        );
    }
    // Whose command line could not be read: never the app, whatever its
    // image name says.
    for name in ["floti.exe", "Floti.exe", "FLOTI.EXE"] {
        assert!(!windows::Native.is_desktop_process(&desktop, &row(name, None)), "{name}");
    }
    // A program whose path only starts like the app's is another program.
    assert!(!windows::Native.is_desktop_process(
        &desktop,
        &row(
            "Floti.exe",
            Some(r"C:\Users\Some One\AppData\Local\Programs\Floti\desktop\Floti.exe.old --x")
        )
    ));
}

#[test]
fn macos_looks_in_the_user_s_applications_folder_then_the_one_named() {
    let dirs = TestDirs::new();
    let user_apps = dirs.home.join("Applications");
    let contents = user_apps.join("Floti.app").join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    let user = env(&dirs.home, &[]);
    assert_eq!(
        macos::Native.desktop(&user),
        None,
        "an app with no Info.plist is not one"
    );
    std::fs::write(contents.join("Info.plist"), "plist").unwrap();
    assert_eq!(
        macos::Native.desktop(&user).map(|desktop| desktop.path),
        Some(user_apps.join("Floti.app"))
    );
    let moved = dirs.folder("apps dir");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &moved)]);
    assert_eq!(macos::Native.desktop(&user), None);
}

#[test]
fn macos_counts_processes_run_from_inside_the_bundle() {
    let desktop = Desktop {
        name: "Floti",
        path: "/Applications/Floti.app".into(),
    };
    assert!(macos::Native.is_desktop_process(
        &desktop,
        &row("Floti", Some("/Applications/Floti.app/Contents/MacOS/Floti"))
    ));
    assert!(macos::Native.is_desktop_process(
        &desktop,
        &row(
            "Floti Helper (Renderer)",
            Some("/Applications/Floti.app/Contents/Frameworks/Floti Helper (Renderer).app/Contents/MacOS/Floti Helper (Renderer) --type=renderer")
        )
    ));
    assert!(macos::Native.is_desktop_process(&desktop, &row("Floti", None)));
    assert!(!macos::Native.is_desktop_process(
        &desktop,
        &row(
            "Floti",
            Some("/Users/someone/Downloads/Floti.app/Contents/MacOS/Floti")
        )
    ));
    // The host, its keeper and its Node runtime outlive the app.
    for (name, command_line) in [
        (
            "floti",
            "/Applications/Floti.app/Contents/Resources/floti host",
        ),
        (
            "floti",
            "/Applications/Floti.app/Contents/Resources/floti keeper /Users/someone/Library/Application Support/Floti /tmp/floti",
        ),
        (
            "node",
            "/Applications/Floti.app/Contents/Resources/node/bin/node service.mjs",
        ),
    ] {
        assert!(
            !macos::Native.is_desktop_process(&desktop, &row(name, Some(command_line))),
            "{command_line}"
        );
    }
}

#[test]
fn macos_reads_the_app_s_version_from_its_info_plist() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let app = apps.join("Floti.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    let desktop = Desktop {
        name: "Floti",
        path: app.clone(),
    };
    assert_eq!(macos::Native.desktop_version(&desktop), None, "no plist");
    std::fs::write(
        app.join("Contents").join("Info.plist"),
        macos::fake_plist("2.4.1-preview.11"),
    )
    .unwrap();
    assert_eq!(
        macos::Native.desktop_version(&desktop).as_deref(),
        Some("2.4.1-preview.11")
    );
}

#[test]
fn a_plist_string_is_the_one_after_its_key() {
    let plist = "<dict>\n\t<key>CFBundleVersion</key>\n\t<string>7</string>\n\t<key>CFBundleShortVersionString</key>\n\t<string> 2.6.0 </string>\n</dict>";
    assert_eq!(
        plist_string(plist, "CFBundleShortVersionString").as_deref(),
        Some("2.6.0")
    );
    assert_eq!(plist_string(plist, "CFBundleVersion").as_deref(), Some("7"));
    assert_eq!(plist_string(plist, "CFBundleName"), None);
    assert_eq!(
        plist_string("<key>A</key><integer>1</integer>", "A"),
        None,
        "not a string"
    );
}

#[test]
fn macos_puts_a_new_app_in_place_or_moves_the_old_one_aside_first() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let target = apps.join("Floti.app");
    let old = apps.join(".Floti.app.floti-old");

    let new = dirs.folder("unpacked").join("Floti.app");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("marker"), b"first").unwrap();
    macos::Native::swap_in(&new, &target, &old).unwrap();
    assert_eq!(std::fs::read(target.join("marker")).unwrap(), b"first");
    assert!(!old.exists(), "nothing was there to move aside");

    let new = dirs.folder("unpacked again").join("Floti.app");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("marker"), b"second").unwrap();
    macos::Native::swap_in(&new, &target, &old).unwrap();
    assert_eq!(std::fs::read(target.join("marker")).unwrap(), b"second");
    assert_eq!(std::fs::read(old.join("marker")).unwrap(), b"first");
}

#[test]
fn macos_counts_a_floti_inside_the_bundle_as_the_app_s() {
    let desktop = Desktop {
        name: "Floti",
        path: "/Applications/Floti.app".into(),
    };
    assert!(macos::Native.bundles(
        &desktop,
        Path::new("/Applications/Floti.app/Contents/Resources/floti")
    ));
    assert!(!macos::Native.bundles(&desktop, Path::new("/Users/someone/.local/bin/floti")));
    assert!(!macos::Native.bundles(&desktop, Path::new("/Applications/Floti.application/floti")));
}

#[test]
fn macos_installs_a_first_app_in_the_user_s_applications_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let target = macos::Native.desktop_target(&user).unwrap();
    assert_eq!(target.path, dirs.home.join("Applications").join("Floti.app"));
}

#[test]
fn linux_finds_the_app_image_in_the_applications_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    assert_eq!(linux::Native.desktop(&user), None);
    let apps = dirs.home.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    assert_eq!(
        linux::Native.desktop_target(&user),
        Some(Desktop {
            name: "Floti",
            path: apps.join("Floti.AppImage")
        }),
        "a new install goes there"
    );
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("Floti.AppImage"), b"app").unwrap();
    assert_eq!(
        linux::Native.desktop(&user),
        Some(Desktop {
            name: "Floti",
            path: apps.join("Floti.AppImage")
        })
    );
    assert_eq!(
        linux::Native.desktop_version(&linux::Native.desktop(&user).unwrap()),
        None,
        "an AppImage's version is unknown"
    );
}

#[test]
fn linux_counts_the_app_image_s_runtime() {
    let desktop = Desktop {
        name: "Floti",
        path: "/home/someone/Applications/Floti.AppImage".into(),
    };
    assert!(
        linux::Native.is_desktop_process(&desktop, &row("Floti.AppImage", Some("./Floti.AppImage")))
    );
    assert!(linux::Native.is_desktop_process(
        &desktop,
        &row(
            "AppRun",
            Some("/home/someone/Applications/Floti.AppImage --no-sandbox")
        )
    ));
    assert!(!linux::Native.is_desktop_process(
        &desktop,
        &row("floti", Some("/home/someone/.local/bin/floti host"))
    ));
}

#[test]
fn linux_writes_the_new_app_image_beside_the_old_and_renames_it_over() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let app = apps.join("Floti.AppImage");
    std::fs::write(&app, b"old").unwrap();
    let release = dirs.folder("download").join("Floti-2.1.0-x86_64.AppImage");
    std::fs::write(&release, b"new").unwrap();
    let desktop = Desktop {
        name: "Floti",
        path: app.clone(),
    };
    linux::Native.install_desktop(&desktop, &release).unwrap();
    assert_eq!(std::fs::read(&app).unwrap(), b"new");
    let names: Vec<_> = std::fs::read_dir(&apps)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![OsString::from("Floti.AppImage")],
        "nothing left beside it"
    );
}

#[test]
fn linux_installs_a_first_app_image_making_its_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let release = dirs.folder("download").join("Floti-2.1.0-x86_64.AppImage");
    std::fs::write(&release, b"new").unwrap();
    let target = linux::Native.desktop_target(&user).unwrap();
    linux::Native.install_desktop(&target, &release).unwrap();
    assert_eq!(
        std::fs::read(dirs.home.join("Applications").join("Floti.AppImage")).unwrap(),
        b"new"
    );
    assert_eq!(linux::Native.desktop(&user), Some(target));
}

#[test]
fn linux_counts_a_floti_on_the_app_image_s_mount_as_the_app_s() {
    let desktop = Desktop {
        name: "Floti",
        path: "/home/someone/Applications/Floti.AppImage".into(),
    };
    assert!(linux::Native.bundles(
        &desktop,
        Path::new("/tmp/.mount_FlotiAb12Cd/resources/floti")
    ));
    assert!(!linux::Native.bundles(&desktop, Path::new("/home/someone/.local/bin/floti")));
    assert!(!linux::Native.bundles(&desktop, Path::new("/tmp/.mount_OtherX/resources/floti")));
}

#[test]
fn macos_hands_its_own_floti_to_install_self_and_the_appimage_none() {
    let app = Desktop {
        name: "Floti",
        path: std::path::PathBuf::from("/Users/me/Applications/Floti.app"),
    };
    assert_eq!(
        macos::Native.carried_floti(&app),
        Some(app.path.join("Contents").join("Resources").join("floti"))
    );
    assert_eq!(linux::Native.carried_floti(&app), None);
}

#[test]
fn each_os_opens_the_installed_app_by_its_own_program() {
    let windows_app = Desktop {
        name: "Floti",
        path: std::path::PathBuf::from(r"C:\Users\Me\AppData\Local\Programs\Floti\desktop"),
    };
    assert_eq!(
        windows::Native.opener(&windows_app),
        (
            std::path::PathBuf::from(r"C:\Users\Me\AppData\Local\Programs\Floti\desktop\Floti.exe"),
            Vec::new()
        )
    );
    let mac_app = Desktop {
        name: "Floti",
        path: std::path::PathBuf::from("/Users/me/Applications/Floti.app"),
    };
    assert_eq!(
        macos::Native.opener(&mac_app),
        (
            std::path::PathBuf::from("/usr/bin/open"),
            vec!["/Users/me/Applications/Floti.app".to_owned()]
        )
    );
    let image = Desktop {
        name: "Floti",
        path: std::path::PathBuf::from("/home/me/Applications/Floti.AppImage"),
    };
    assert_eq!(
        linux::Native.opener(&image),
        (image.path.clone(), Vec::new())
    );
}

#[test]
fn what_an_install_left_beside_the_app_is_found_with_the_process_that_left_it() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let app = apps.join("Floti.app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(apps.join(".Floti.app.floti-old-11").join("Contents")).unwrap();
    std::fs::create_dir_all(apps.join(".Floti.app.floti-update-12")).unwrap();
    for name in [
        ".Floti.app.floti-old-x",
        ".Floti.app.floti-olden-14",
        ".Other.app.floti-old-15",
        "Floti.app.floti-old-16",
    ] {
        std::fs::write(apps.join(name), b"not one").unwrap();
    }
    let desktop = Desktop {
        name: "Floti",
        path: app,
    };
    let mut found = leftovers(&desktop);
    found.sort();
    assert_eq!(
        found,
        vec![
            (apps.join(".Floti.app.floti-old-11"), 11),
            (apps.join(".Floti.app.floti-update-12"), 12),
        ]
    );
    let nowhere = Desktop {
        name: "Floti",
        path: dirs.root().join("no such folder").join("Floti.AppImage"),
    };
    assert_eq!(leftovers(&nowhere), Vec::new());
}

#[test]
fn only_the_display_and_session_bus_variables_are_taken_from_the_app_s_environment() {
    let environ =
        b"PATH=/usr/bin\0DISPLAY=:0\0WAYLAND_DISPLAY=wayland-0\0XAUTHORITY=/run/user/1000/xauth\0\
DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus\0LD_PRELOAD=/tmp/evil.so\0HOME=/home/me\0\
DISPLAY_EXTRA=x\0XAUTHORITY=\0\xff\xfe=bad\0";
    assert_eq!(
        display_vars(environ),
        vec![
            ("DISPLAY".to_owned(), ":0".to_owned()),
            ("WAYLAND_DISPLAY".to_owned(), "wayland-0".to_owned()),
            ("XAUTHORITY".to_owned(), "/run/user/1000/xauth".to_owned()),
            (
                "DBUS_SESSION_BUS_ADDRESS".to_owned(),
                "unix:path=/run/user/1000/bus".to_owned()
            ),
        ]
    );
    assert!(display_vars(b"").is_empty());
}

#[test]
fn only_linux_reads_an_app_s_display_variables() {
    // No process has id 0's environment to give, on any OS.
    assert!(windows::Native.display_env(std::process::id()).is_empty());
    assert!(macos::Native.display_env(std::process::id()).is_empty());
    assert!(linux::Native.display_env(0).is_empty());
}

#[test]
fn linux_has_a_display_with_either_display_variable_and_none_without() {
    let dirs = TestDirs::new();
    let none = env(&dirs.home, &[]);
    assert_eq!(
        linux::Native.no_display(&none).as_deref(),
        Some("this session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set)")
    );
    for name in ["DISPLAY", "WAYLAND_DISPLAY"] {
        let with = env(&dirs.home, &[(name, Path::new(":0"))]);
        assert_eq!(linux::Native.no_display(&with), None, "{name}");
    }
    let empty = env(&dirs.home, &[("DISPLAY", Path::new(""))]);
    assert!(
        linux::Native.no_display(&empty).is_some(),
        "an empty one is unset"
    );
}

#[test]
fn windows_has_no_display_in_session_0_alone() {
    assert!(windows::Native::no_display_in(Some(0)).is_some());
    assert_eq!(windows::Native::no_display_in(Some(1)), None);
    assert_eq!(
        windows::Native::no_display_in(None),
        None,
        "unknown counts as a display"
    );
}

/// A Mac as launchd answers for user 501: with a GUI domain or without, or
/// unable to tell the user's id or to run launchctl at all. It records each
/// command line asked.
struct FakeLaunchd {
    id: Option<&'static str>,
    domain: Option<bool>,
    asked: RefCell<Vec<String>>,
}

impl FakeLaunchd {
    fn new(id: Option<&'static str>, domain: Option<bool>) -> Self {
        Self {
            id,
            domain,
            asked: RefCell::default(),
        }
    }
}

impl SignInSystem for FakeLaunchd {
    fn run(&self, command: &OsCommand) -> io::Result<Ran> {
        let args: Vec<String> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        self.asked
            .borrow_mut()
            .push(format!("{} {}", command.program, args.join(" ")));
        let answered = |success: bool, stdout: &str| Ran {
            success,
            stdout: stdout.into(),
            stderr: String::new(),
        };
        match command.program.as_str() {
            "id" => match self.id {
                Some(id) => Ok(answered(true, id)),
                None => Ok(answered(false, "")),
            },
            "launchctl" => match self.domain {
                Some(there) => Ok(answered(there, "")),
                None => Err(io::Error::new(io::ErrorKind::NotFound, "no launchctl")),
            },
            other => panic!("only id and launchctl are asked, not {other}"),
        }
    }

    fn desktop_session(&self) -> io::Result<bool> {
        panic!("only the Windows logon task asks for a desktop session")
    }
}

#[test]
fn a_mac_has_a_display_only_when_launchd_has_a_gui_domain_for_the_user() {
    let signed_in = FakeLaunchd::new(Some("501\n"), Some(true));
    assert_eq!(macos::no_display_with(&signed_in), None);
    assert_eq!(
        *signed_in.asked.borrow(),
        ["id -u", "launchctl print gui/501"]
    );

    let nobody = FakeLaunchd::new(Some("501\n"), Some(false));
    assert_eq!(
        macos::no_display_with(&nobody).as_deref(),
        Some("nobody is signed in at this Mac's screen (launchd has no GUI domain for this user)")
    );
    assert_eq!(
        macos::no_display_with(&nobody).as_deref(),
        Some(macos::NO_GUI_DOMAIN)
    );
}

#[test]
fn a_mac_that_cannot_be_asked_counts_as_having_a_display() {
    for (id, domain) in [
        (None, Some(false)),
        (Some("501\n"), None),
        (Some("me"), Some(false)),
    ] {
        let unknown = FakeLaunchd::new(id, domain);
        assert_eq!(
            macos::no_display_with(&unknown),
            None,
            "id {id:?}, domain {domain:?}"
        );
    }
}

#[test]
fn a_wsl_distro_is_not_offered_the_app_though_it_may_have_a_display() {
    let dirs = TestDirs::new();
    let no_interop = dirs.root().join("no interop");
    let distro = env(
        &dirs.home,
        &[
            ("WSL_DISTRO_NAME", Path::new("Ubuntu")),
            ("DISPLAY", Path::new(":0")),
        ],
    );
    assert_eq!(
        linux::not_offered_in(&distro, &no_interop).as_deref(),
        Some("this is a WSL distro, whose desktop is Windows' own")
    );
    assert_eq!(
        linux::Native.no_display(&distro),
        None,
        "WSLg's display is there, so floti add desktop still installs"
    );

    let interop = dirs.root().join("WSLInterop");
    std::fs::write(&interop, b"enabled").unwrap();
    let plain = env(&dirs.home, &[("DISPLAY", Path::new(":0"))]);
    assert_eq!(
        linux::not_offered_in(&plain, &interop).as_deref(),
        Some(linux::IN_WSL)
    );
    assert_eq!(linux::not_offered_in(&plain, &no_interop), None);

    assert_eq!(windows::Native.not_offered(&plain), None);
    assert_eq!(macos::Native.not_offered(&plain), None);
}

#[test]
fn a_test_build_s_display_switch_also_answers_whether_the_app_is_offered() {
    let dirs = TestDirs::new();
    let left_out = |vars: &[(&str, &Path)]| {
        let mut user = env(&dirs.home, vars);
        user.vars.insert("WSL_DISTRO_NAME".into(), "Ubuntu".into());
        not_offered(&linux::Native, &user)
    };
    assert!(left_out(&[]).is_some());
    assert_eq!(left_out(&[(DISPLAY_TEST_VAR, Path::new("present"))]), None);
}

#[test]
fn a_test_build_s_display_switch_answers_before_the_os() {
    let dirs = TestDirs::new();
    let none = env(&dirs.home, &[(DISPLAY_TEST_VAR, Path::new("none"))]);
    assert!(missing_display(&macos::Native, &none).is_some());
    let present = env(&dirs.home, &[(DISPLAY_TEST_VAR, Path::new("present"))]);
    assert_eq!(missing_display(&linux::Native, &present), None);
    assert!(missing_display(&linux::Native, &env(&dirs.home, &[])).is_some());
}

#[test]
fn linux_adds_a_menu_entry_and_its_icon_under_the_data_home() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("my apps");
    let desktop = Desktop {
        name: "Floti",
        path: apps.join("Floti.AppImage"),
    };
    let share = dirs.home.join(".local").join("share");
    let entry = linux::Native
        .add_launcher(&env(&dirs.home, &[]), &desktop)
        .unwrap()
        .unwrap();
    assert_eq!(entry, share.join("applications").join("dev.floti.desktop.desktop"));
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
    for line in [
        "Type=Application",
        "Name=Floti",
        "Icon=dev.floti.desktop",
        "Terminal=false",
        "Categories=Development;",
        // A Windows path's backslashes are each four in the entry.
        &format!("Exec=\"{}\"", desktop.path.display()).replace('\\', r"\\\\"),
    ] {
        assert!(text.lines().any(|found| found == line), "{line} in {text}");
    }
    let icon = share
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join("dev.floti.desktop.png");
    let png = std::fs::read(&icon).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "the app's icon");

    // XDG_DATA_HOME, when absolute, is where they go instead; written again
    // over what is there.
    let data_home = dirs.folder("data home");
    let moved = linux::Native
        .add_launcher(&env(&dirs.home, &[("XDG_DATA_HOME", &data_home)]), &desktop)
        .unwrap()
        .unwrap();
    assert_eq!(moved, data_home.join("applications").join("dev.floti.desktop.desktop"));
    linux::Native
        .add_launcher(&env(&dirs.home, &[("XDG_DATA_HOME", &data_home)]), &desktop)
        .unwrap();
    let left: Vec<_> = std::fs::read_dir(data_home.join("applications"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        left,
        vec![OsString::from("dev.floti.desktop.desktop")],
        "nothing beside it"
    );
}

#[test]
fn linux_quotes_the_app_image_s_path_in_its_menu_entry() {
    let text = linux::Native::entry_text(Path::new("/home/a b/$x \"q\" 100%/Floti.AppImage"));
    assert!(
        text.lines()
            .any(|line| line == r#"Exec="/home/a b/\\$x \\"q\\" 100%%/Floti.AppImage""#),
        "{text}"
    );
    let text = linux::Native::entry_text(Path::new(r"/home/back\slash/Floti.AppImage"));
    assert!(
        text.lines()
            .any(|line| line == r#"Exec="/home/back\\\\slash/Floti.AppImage""#),
        "{text}"
    );
}

#[test]
fn windows_and_macos_leave_the_launcher_to_their_installs() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let desktop = Desktop {
        name: "Floti",
        path: dirs.folder("apps").join("Floti"),
    };
    assert_eq!(windows::Native.add_launcher(&user, &desktop).unwrap(), None);
    assert_eq!(macos::Native.add_launcher(&user, &desktop).unwrap(), None);
    assert!(!dirs.home.join(".local").exists(), "nothing is written");
}

/// The quarantine flag on `path`, as `getxattr` reads it.
#[cfg(target_os = "macos")]
fn quarantined(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let file = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(crate::platform::standalone::macos::QUARANTINE).unwrap();
    // SAFETY: NUL-terminated names; a null buffer asks only for the size.
    let size =
        unsafe { libc::getxattr(file.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
    size >= 0
}

#[cfg(target_os = "macos")]
#[test]
fn macos_installs_an_app_from_a_quarantined_zip_with_no_quarantine_flag() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    use super::FakeDesktop;

    let dirs = TestDirs::new();
    let floti = dirs.root().join("a floti");
    std::fs::write(&floti, b"floti").unwrap();
    let (file, bytes) = macos::Native.fake_release(&dirs.folder("work"), "3.0.0", &floti);
    let release = dirs.folder("download").join(file);
    std::fs::write(&release, bytes).unwrap();
    let zip = CString::new(release.as_os_str().as_bytes()).unwrap();
    let name = CString::new(crate::platform::standalone::macos::QUARANTINE).unwrap();
    let value = b"0083;00000000;Safari;";
    // SAFETY: NUL-terminated names and a value buffer of the length given.
    let set = unsafe {
        libc::setxattr(
            zip.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    assert_eq!(set, 0, "{}", std::io::Error::last_os_error());
    assert!(quarantined(&release));

    // Only the test's own Applications folder, never /Applications.
    let apps = dirs.folder("apps");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &apps)]);
    let target = macos::Native.desktop_target(&user).unwrap();
    assert_eq!(target.path, apps.join("Floti.app"));
    macos::Native.install_desktop(&target, &release).unwrap();
    assert_eq!(
        macos::Native.desktop_version(&target).as_deref(),
        Some("3.0.0")
    );
    for path in [
        target.path.clone(),
        target.path.join("Contents"),
        target.path.join("Contents").join("Info.plist"),
    ] {
        assert!(
            !quarantined(&path),
            "{} carries no quarantine flag",
            path.display()
        );
    }
}

#[test]
fn linux_writes_a_missing_menu_entry_for_an_installed_app_image_and_leaves_one_that_is_there() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let desktop = Desktop {
        name: "Floti",
        path: dirs.folder("apps").join("Floti.AppImage"),
    };
    let entry = dirs
        .home
        .join(".local")
        .join("share")
        .join("applications")
        .join("dev.floti.desktop.desktop");
    assert_eq!(
        linux::Native.repair_launcher(&user, &desktop).unwrap(),
        Some(entry.clone())
    );
    assert!(entry.is_file());
    std::fs::write(&entry, b"the user's own").unwrap();
    assert_eq!(
        linux::Native.repair_launcher(&user, &desktop).unwrap(),
        None
    );
    assert_eq!(std::fs::read(&entry).unwrap(), b"the user's own");
    assert_eq!(
        windows::Native.repair_launcher(&user, &desktop).unwrap(),
        None
    );
    assert_eq!(
        macos::Native.repair_launcher(&user, &desktop).unwrap(),
        None
    );
}

// ---------------------------------------------------------------------------
// Removing the app, for floti uninstall
// ---------------------------------------------------------------------------

#[test]
fn windows_removes_the_app_by_its_own_uninstaller_run_silently_in_place() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    let user = env(&dirs.home, &[("LOCALAPPDATA", &local)]);
    assert_eq!(windows::Native.removal(&user), None, "nothing installed");
    let install = local.join("Programs").join("Floti").join("desktop");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("Floti.exe"), b"app").unwrap();
    assert_eq!(windows::Native.removal(&user), None, "a portable copy");
    std::fs::write(install.join("Uninstall Floti.exe"), b"uninstaller").unwrap();

    let removal = windows::Native.removal(&user).expect("an install");
    let (uninstaller, line) = removal.uninstaller.expect("its uninstaller");
    assert_eq!(uninstaller, install.join("Uninstall Floti.exe"));
    // `_?=` last and unquoted, spaces and all ("Local App Data").
    assert_eq!(line, format!("/S _?={}", install.display()));
    assert_eq!(removal.gone_after, Some(install.join("Floti.exe")));
    assert_eq!(removal.remove, std::slice::from_ref(&install));
    assert_eq!(removal.shown, install);
}

#[test]
fn macos_removes_the_app_bundle_wherever_it_was_found() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &apps)]);
    assert_eq!(macos::Native.removal(&user), None);
    let app = apps.join("Floti.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    std::fs::write(app.join("Contents").join("Info.plist"), b"<plist/>").unwrap();

    let removal = macos::Native.removal(&user).expect("an install");
    assert_eq!(removal.uninstaller, None);
    assert_eq!(removal.gone_after, None);
    assert_eq!(removal.remove, std::slice::from_ref(&app));
    assert_eq!(removal.shown, app);
}

#[test]
fn linux_removes_the_app_image_its_menu_entry_and_icon_whichever_are_there() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("apps dir");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &apps)]);
    assert_eq!(linux::Native.removal(&user), None, "nothing there");
    let app_image = apps.join("Floti.AppImage");
    let desktop = Desktop {
        name: "Floti",
        path: app_image.clone(),
    };
    let entry = linux::Native
        .add_launcher(&user, &desktop)
        .unwrap()
        .unwrap();
    let icon = dirs
        .home
        .join(".local")
        .join("share")
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join("dev.floti.desktop.png");

    // A menu entry left by an app removed by hand is still the app's.
    let removal = linux::Native.removal(&user).expect("its menu entry");
    assert_eq!(removal.remove, [icon.clone(), entry.clone()]);
    assert_eq!(removal.shown, app_image);

    std::fs::write(&app_image, b"AppImage").unwrap();
    let removal = linux::Native.removal(&user).expect("an install");
    assert_eq!(removal.uninstaller, None);
    assert_eq!(removal.gone_after, None);
    assert_eq!(removal.remove, [app_image.clone(), icon, entry]);
}

#[test]
fn windows_reads_no_app_folder_from_a_relative_localappdata() {
    let dirs = TestDirs::new();
    let relative = env(&dirs.home, &[("LOCALAPPDATA", Path::new("AppData Local"))]);
    assert_eq!(windows::Native.desktop_target(&relative), None);
    assert_eq!(windows::Native.desktop(&relative), None);
    assert_eq!(windows::Native.removal(&relative), None);
}

#[test]
fn windows_finds_an_install_from_before_the_rename_and_prefers_floti_s() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    let programs = local.join("Programs");
    let helm = programs.join("Helm");
    std::fs::create_dir_all(&helm).unwrap();
    std::fs::write(helm.join("Helm.exe"), b"app").unwrap();
    std::fs::write(helm.join("Uninstall Helm.exe"), b"uninstaller").unwrap();
    let user = env(&dirs.home, &[("LOCALAPPDATA", &local)]);
    let old = Desktop {
        name: "Helm",
        path: helm.clone(),
    };
    assert_eq!(windows::Native.desktop(&user), Some(old.clone()));
    assert_eq!(windows::Native.desktop_target(&user), Some(old.clone()));
    assert!(windows::Native.is_desktop_process(
        &old,
        &row("Helm.exe", Some(&format!("\"{}\"", helm.join("Helm.exe").display())))
    ));
    assert!(!windows::Native.is_desktop_process(&old, &row("Floti.exe", None)));
    let removal = windows::Native.removal(&user).unwrap();
    assert_eq!(removal.uninstaller.unwrap().0, helm.join("Uninstall Helm.exe"));

    // What the installer leaves in its place: the app in its new folder.
    let floti = Desktop {
        name: "Floti",
        path: programs.join("Floti").join("desktop"),
    };
    assert_eq!(windows::Native::installed_over(&old), floti);
    assert_eq!(windows::Native::installed_over(&floti), floti);

    std::fs::create_dir_all(&floti.path).unwrap();
    std::fs::write(floti.path.join("Floti.exe"), b"app").unwrap();
    std::fs::write(floti.path.join("Uninstall Floti.exe"), b"uninstaller").unwrap();
    assert_eq!(windows::Native.desktop(&user), Some(floti.clone()));
    assert_eq!(
        windows::Native.opener(&floti).0,
        floti.path.join("Floti.exe")
    );
}

#[test]
fn macos_finds_a_helm_app_from_before_the_rename_and_prefers_floti_s() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("Applications");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &apps)]);
    let helm = apps.join("Helm.app");
    std::fs::create_dir_all(helm.join("Contents")).unwrap();
    std::fs::write(helm.join("Contents").join("Info.plist"), macos::fake_plist("2.4.0")).unwrap();
    assert_eq!(
        macos::Native.desktop(&user),
        Some(Desktop {
            name: "Helm",
            path: helm.clone()
        })
    );
    let floti = apps.join("Floti.app");
    std::fs::create_dir_all(floti.join("Contents")).unwrap();
    std::fs::write(floti.join("Contents").join("Info.plist"), macos::fake_plist("3.0.0")).unwrap();
    assert_eq!(
        macos::Native.desktop(&user),
        Some(Desktop {
            name: "Floti",
            path: floti
        })
    );
}

#[test]
fn linux_updates_a_helm_app_image_into_floti_s_and_carries_its_menu_entry_over() {
    let dirs = TestDirs::new();
    let apps = dirs.folder("my apps");
    let data_home = dirs.folder("data home");
    let user = env(
        &dirs.home,
        &[(DESKTOP_DIR_VAR, &apps), ("XDG_DATA_HOME", &data_home)],
    );
    let helm = apps.join("Helm.AppImage");
    std::fs::write(&helm, b"old app").unwrap();
    let old = linux::Native.desktop(&user).unwrap();
    assert_eq!(
        old,
        Desktop {
            name: "Helm",
            path: helm.clone()
        }
    );
    // The entry floti wrote for it, as it wrote it then.
    let old_entry = linux::Native.add_launcher(&user, &old).unwrap().unwrap();
    assert_eq!(old_entry, data_home.join("applications").join("helm.desktop"));
    let old_text = std::fs::read_to_string(&old_entry).unwrap();
    assert!(old_text.lines().any(|line| line == "Name=Helm"), "{old_text}");
    assert!(old_text.lines().any(|line| line == "Icon=helm"), "{old_text}");

    let release = dirs.folder("download").join("Floti-3.0.0-linux-x64.AppImage");
    std::fs::write(&release, b"new app").unwrap();
    let installed = linux::Native.install_desktop(&old, &release).unwrap();

    assert_eq!(
        installed,
        Desktop {
            name: "Floti",
            path: apps.join("Floti.AppImage")
        }
    );
    assert_eq!(std::fs::read(&installed.path).unwrap(), b"new app");
    assert!(helm.exists(), "the old AppImage stays until it is removed apart");
    linux::Native.remove_replaced(&old, &installed).unwrap();
    assert!(!helm.exists(), "the old AppImage is removed");
    linux::Native.remove_replaced(&installed, &installed).unwrap();
    assert!(installed.path.is_file(), "the app is never removed as its own old copy");
    let entry = linux::Native
        .carry_launcher(&user, &old, &installed)
        .unwrap()
        .unwrap();
    assert_eq!(
        entry,
        data_home.join("applications").join("dev.floti.desktop.desktop")
    );
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(text.lines().any(|line| line == "Name=Floti"), "{text}");
    assert!(text.lines().any(|line| line == "StartupWMClass=dev.floti.desktop"), "{text}");
    assert!(!old_entry.exists(), "the old entry is removed");
    let icons = data_home.join("icons").join("hicolor").join("256x256").join("apps");
    assert!(!icons.join("helm.png").exists());
    assert!(icons.join("dev.floti.desktop.png").is_file());
    assert_eq!(linux::Native.desktop(&user), Some(installed));
}

#[test]
fn linux_writes_no_menu_entry_for_an_app_image_floti_never_wrote_one_for() {
    let dirs = TestDirs::new();
    let data_home = dirs.folder("data home");
    let user = env(&dirs.home, &[("XDG_DATA_HOME", &data_home)]);
    let old = Desktop {
        name: "Helm",
        path: dirs.folder("apps").join("Helm.AppImage"),
    };
    let installed = Desktop {
        name: "Floti",
        path: dirs.folder("apps").join("Floti.AppImage"),
    };
    assert_eq!(
        linux::Native.carry_launcher(&user, &old, &installed).unwrap(),
        None
    );
    assert!(!data_home.join("applications").exists());
    assert_eq!(
        linux::Native.carry_launcher(&user, &installed, &installed).unwrap(),
        None
    );
}

#[test]
fn macos_and_linux_install_a_first_app_under_floti_s_name() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    assert_eq!(
        macos::Native.desktop_target(&user).unwrap().path,
        dirs.home.join("Applications").join("Floti.app")
    );
    assert_eq!(
        linux::Native.desktop_target(&user).unwrap().path,
        dirs.home.join("Applications").join("Floti.AppImage")
    );
}
