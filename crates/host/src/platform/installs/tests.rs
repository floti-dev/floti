//! Each OS's installs, on every OS, over folders in a `TestDirs`: where the
//! desktop app is found and goes, which version it is, which processes are
//! its own, which floti is its own, and the renames that put a binary, an
//! AppImage or an app in place.

use std::ffi::OsString;
use std::path::Path;

use super::{
    DESKTOP_DIR_VAR, DISPLAY_TEST_VAR, Desktop, Installs, display_vars, leftovers, linux, macos,
    missing_display, plist_string, windows,
};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
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
    let install = local.join("Programs").join("Helm");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("Helm.exe"), b"app").unwrap();
    let user = env(&dirs.home, &[("LOCALAPPDATA", &local)]);
    assert_eq!(
        windows::Native.desktop(&user),
        None,
        "no uninstaller: a portable copy"
    );
    std::fs::write(install.join("Uninstall Helm.exe"), b"uninstaller").unwrap();
    assert_eq!(
        windows::Native.desktop(&user),
        Some(Desktop {
            name: "Helm",
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
            name: "Helm",
            path: local.join("Programs").join("Helm")
        })
    );
    // The installer runs install-self itself.
    assert_eq!(
        windows::Native.carried_floti(&Desktop {
            name: "Helm",
            path: local.join("Programs").join("Helm")
        }),
        None
    );
}

#[test]
fn windows_counts_the_floti_in_the_install_s_resources_as_the_app_s() {
    let desktop = Desktop {
        name: "Helm",
        path: r"C:\Users\Some One\AppData\Local\Programs\Helm".into(),
    };
    assert!(windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Helm\resources\floti.exe")
    ));
    assert!(windows::Native.bundles(
        &desktop,
        Path::new(r"c:\users\some one\appdata\local\programs\HELM\resources\floti.exe")
    ));
    assert!(!windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Floti\bin\floti.exe")
    ));
    assert!(!windows::Native.bundles(
        &desktop,
        Path::new(r"C:\Users\Some One\AppData\Local\Programs\Helm Old\floti.exe")
    ));
}

#[test]
fn windows_reads_the_app_s_version_from_its_own_uninstall_entry() {
    let install = Path::new(r"C:\Users\Some One\AppData\Local\Programs\Helm");
    let entry = |uninstall: &str, version: &str| windows::UninstallEntry {
        uninstall_string: uninstall.into(),
        display_version: version.into(),
    };
    let entries = [
        entry(r#""C:\Program Files\Other\uninstall.exe" /S"#, "9.9.9"),
        entry(
            r#""C:\Users\Some One\AppData\Local\Programs\Helm Copy\Uninstall Helm.exe" /currentuser"#,
            "1.0.0",
        ),
        entry(
            r#""C:\Users\some one\AppData\Local\Programs\Helm\Uninstall Helm.exe" /currentuser"#,
            "2.4.1-preview.11",
        ),
    ];
    assert_eq!(
        windows::Native::uninstall_version(&entries, install).as_deref(),
        Some("2.4.1-preview.11")
    );
    let unquoted = [entry(
        r"C:\Users\Some One\AppData\Local\Programs\Helm\Uninstall Helm.exe /currentuser",
        "2.5.0",
    )];
    assert_eq!(
        windows::Native::uninstall_version(&unquoted, install).as_deref(),
        Some("2.5.0")
    );
    assert_eq!(
        windows::Native::uninstall_version(&entries[..2], install),
        None
    );
}

#[test]
fn windows_counts_only_the_installed_program_s_processes() {
    let desktop = Desktop {
        name: "Helm",
        path: "C:\\Users\\Some One\\AppData\\Local\\Programs\\Helm".into(),
    };
    let installed = r#""C:\Users\Some One\AppData\Local\Programs\Helm\Helm.exe" --type=renderer"#;
    assert!(windows::Native.is_desktop_process(&desktop, &row("Helm.exe", Some(installed))));
    assert!(
        windows::Native
            .is_desktop_process(&desktop, &row("helm.EXE", Some(&installed.to_uppercase())))
    );
    assert!(
        windows::Native.is_desktop_process(&desktop, &row("Helm.exe", None)),
        "unreadable counts"
    );
    assert!(!windows::Native.is_desktop_process(
        &desktop,
        &row("Helm.exe", Some(r"C:\Temp\portable\Helm.exe"))
    ));
    let host = r#""C:\Users\Some One\AppData\Local\Programs\Helm\resources\floti.exe" host"#;
    assert!(!windows::Native.is_desktop_process(&desktop, &row("floti.exe", Some(host))));
}

#[test]
fn macos_looks_in_the_user_s_applications_folder_then_the_one_named() {
    let dirs = TestDirs::new();
    let user_apps = dirs.home.join("Applications");
    let contents = user_apps.join("Helm.app").join("Contents");
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
        Some(user_apps.join("Helm.app"))
    );
    let moved = dirs.folder("apps dir");
    let user = env(&dirs.home, &[(DESKTOP_DIR_VAR, &moved)]);
    assert_eq!(macos::Native.desktop(&user), None);
}

#[test]
fn macos_counts_processes_run_from_inside_the_bundle() {
    let desktop = Desktop {
        name: "Helm",
        path: "/Applications/Helm.app".into(),
    };
    assert!(macos::Native.is_desktop_process(
        &desktop,
        &row("Helm", Some("/Applications/Helm.app/Contents/MacOS/Helm"))
    ));
    assert!(macos::Native.is_desktop_process(
        &desktop,
        &row(
            "Helm Helper (Renderer)",
            Some("/Applications/Helm.app/Contents/Frameworks/Helm Helper (Renderer).app/Contents/MacOS/Helm Helper (Renderer) --type=renderer")
        )
    ));
    assert!(macos::Native.is_desktop_process(&desktop, &row("Helm", None)));
    assert!(!macos::Native.is_desktop_process(
        &desktop,
        &row(
            "Helm",
            Some("/Users/someone/Downloads/Helm.app/Contents/MacOS/Helm")
        )
    ));
    // The host, its keeper and its Node runtime outlive the app.
    for (name, command_line) in [
        (
            "floti",
            "/Applications/Helm.app/Contents/Resources/floti host",
        ),
        (
            "floti",
            "/Applications/Helm.app/Contents/Resources/floti keeper /Users/someone/Library/Application Support/Floti /tmp/floti",
        ),
        (
            "node",
            "/Applications/Helm.app/Contents/Resources/node/bin/node service.mjs",
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
    let app = apps.join("Helm.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    let desktop = Desktop {
        name: "Helm",
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
    let target = apps.join("Helm.app");
    let old = apps.join(".Helm.app.floti-old");

    let new = dirs.folder("unpacked").join("Helm.app");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("marker"), b"first").unwrap();
    macos::Native::swap_in(&new, &target, &old).unwrap();
    assert_eq!(std::fs::read(target.join("marker")).unwrap(), b"first");
    assert!(!old.exists(), "nothing was there to move aside");

    let new = dirs.folder("unpacked again").join("Helm.app");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("marker"), b"second").unwrap();
    macos::Native::swap_in(&new, &target, &old).unwrap();
    assert_eq!(std::fs::read(target.join("marker")).unwrap(), b"second");
    assert_eq!(std::fs::read(old.join("marker")).unwrap(), b"first");
}

#[test]
fn macos_counts_a_floti_inside_the_bundle_as_the_app_s() {
    let desktop = Desktop {
        name: "Helm",
        path: "/Applications/Helm.app".into(),
    };
    assert!(macos::Native.bundles(
        &desktop,
        Path::new("/Applications/Helm.app/Contents/Resources/floti")
    ));
    assert!(!macos::Native.bundles(&desktop, Path::new("/Users/someone/.local/bin/floti")));
    assert!(!macos::Native.bundles(&desktop, Path::new("/Applications/Helm.application/floti")));
}

#[test]
fn macos_installs_a_first_app_in_the_user_s_applications_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let target = macos::Native.desktop_target(&user).unwrap();
    assert_eq!(target.path, dirs.home.join("Applications").join("Helm.app"));
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
            name: "Helm",
            path: apps.join("Helm.AppImage")
        }),
        "a new install goes there"
    );
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("Helm.AppImage"), b"app").unwrap();
    assert_eq!(
        linux::Native.desktop(&user),
        Some(Desktop {
            name: "Helm",
            path: apps.join("Helm.AppImage")
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
        name: "Helm",
        path: "/home/someone/Applications/Helm.AppImage".into(),
    };
    assert!(
        linux::Native.is_desktop_process(&desktop, &row("Helm.AppImage", Some("./Helm.AppImage")))
    );
    assert!(linux::Native.is_desktop_process(
        &desktop,
        &row(
            "AppRun",
            Some("/home/someone/Applications/Helm.AppImage --no-sandbox")
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
    let app = apps.join("Helm.AppImage");
    std::fs::write(&app, b"old").unwrap();
    let release = dirs.folder("download").join("Helm-2.1.0-x86_64.AppImage");
    std::fs::write(&release, b"new").unwrap();
    let desktop = Desktop {
        name: "Helm",
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
        vec![OsString::from("Helm.AppImage")],
        "nothing left beside it"
    );
}

#[test]
fn linux_installs_a_first_app_image_making_its_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let release = dirs.folder("download").join("Helm-2.1.0-x86_64.AppImage");
    std::fs::write(&release, b"new").unwrap();
    let target = linux::Native.desktop_target(&user).unwrap();
    linux::Native.install_desktop(&target, &release).unwrap();
    assert_eq!(
        std::fs::read(dirs.home.join("Applications").join("Helm.AppImage")).unwrap(),
        b"new"
    );
    assert_eq!(linux::Native.desktop(&user), Some(target));
}

#[test]
fn linux_counts_a_floti_on_the_app_image_s_mount_as_the_app_s() {
    let desktop = Desktop {
        name: "Helm",
        path: "/home/someone/Applications/Helm.AppImage".into(),
    };
    assert!(linux::Native.bundles(
        &desktop,
        Path::new("/tmp/.mount_HelmAb12Cd/resources/floti")
    ));
    assert!(!linux::Native.bundles(&desktop, Path::new("/home/someone/.local/bin/floti")));
    assert!(!linux::Native.bundles(&desktop, Path::new("/tmp/.mount_OtherX/resources/floti")));
}

#[test]
fn macos_hands_its_own_floti_to_install_self_and_the_appimage_none() {
    let app = Desktop {
        name: "Helm",
        path: std::path::PathBuf::from("/Users/me/Applications/Helm.app"),
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
        name: "Helm",
        path: std::path::PathBuf::from(r"C:\Users\Me\AppData\Local\Programs\Helm"),
    };
    assert_eq!(
        windows::Native.opener(&windows_app),
        (
            std::path::PathBuf::from(r"C:\Users\Me\AppData\Local\Programs\Helm\Helm.exe"),
            Vec::new()
        )
    );
    let mac_app = Desktop {
        name: "Helm",
        path: std::path::PathBuf::from("/Users/me/Applications/Helm.app"),
    };
    assert_eq!(
        macos::Native.opener(&mac_app),
        (
            std::path::PathBuf::from("/usr/bin/open"),
            vec!["/Users/me/Applications/Helm.app".to_owned()]
        )
    );
    let image = Desktop {
        name: "Helm",
        path: std::path::PathBuf::from("/home/me/Applications/Helm.AppImage"),
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
    let app = apps.join("Helm.app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(apps.join(".Helm.app.floti-old-11").join("Contents")).unwrap();
    std::fs::create_dir_all(apps.join(".Helm.app.floti-update-12")).unwrap();
    for name in [
        ".Helm.app.floti-old-x",
        ".Helm.app.floti-olden-14",
        ".Other.app.floti-old-15",
        "Helm.app.floti-old-16",
    ] {
        std::fs::write(apps.join(name), b"not one").unwrap();
    }
    let desktop = Desktop {
        name: "Helm",
        path: app,
    };
    let mut found = leftovers(&desktop);
    found.sort();
    assert_eq!(
        found,
        vec![
            (apps.join(".Helm.app.floti-old-11"), 11),
            (apps.join(".Helm.app.floti-update-12"), 12),
        ]
    );
    let nowhere = Desktop {
        name: "Helm",
        path: dirs.root().join("no such folder").join("Helm.AppImage"),
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
fn windows_has_no_display_in_session_0_alone_and_macos_always_has_one() {
    assert!(windows::Native::no_display_in(Some(0)).is_some());
    assert_eq!(windows::Native::no_display_in(Some(1)), None);
    assert_eq!(
        windows::Native::no_display_in(None),
        None,
        "unknown counts as a display"
    );
    let dirs = TestDirs::new();
    assert_eq!(macos::Native.no_display(&env(&dirs.home, &[])), None);
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
        name: "Helm",
        path: apps.join("Helm.AppImage"),
    };
    let share = dirs.home.join(".local").join("share");
    let entry = linux::Native
        .add_launcher(&env(&dirs.home, &[]), &desktop)
        .unwrap()
        .unwrap();
    assert_eq!(entry, share.join("applications").join("helm.desktop"));
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
    for line in [
        "Type=Application",
        "Name=Helm",
        "Icon=helm",
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
        .join("helm.png");
    let png = std::fs::read(&icon).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "the app's icon");

    // XDG_DATA_HOME, when absolute, is where they go instead; written again
    // over what is there.
    let data_home = dirs.folder("data home");
    let moved = linux::Native
        .add_launcher(&env(&dirs.home, &[("XDG_DATA_HOME", &data_home)]), &desktop)
        .unwrap()
        .unwrap();
    assert_eq!(moved, data_home.join("applications").join("helm.desktop"));
    linux::Native
        .add_launcher(&env(&dirs.home, &[("XDG_DATA_HOME", &data_home)]), &desktop)
        .unwrap();
    let left: Vec<_> = std::fs::read_dir(data_home.join("applications"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        left,
        vec![OsString::from("helm.desktop")],
        "nothing beside it"
    );
}

#[test]
fn linux_quotes_the_app_image_s_path_in_its_menu_entry() {
    let text = linux::Native::entry_text(Path::new("/home/a b/$x \"q\" 100%/Helm.AppImage"));
    assert!(
        text.lines()
            .any(|line| line == r#"Exec="/home/a b/\\$x \\"q\\" 100%%/Helm.AppImage""#),
        "{text}"
    );
    let text = linux::Native::entry_text(Path::new(r"/home/back\slash/Helm.AppImage"));
    assert!(
        text.lines()
            .any(|line| line == r#"Exec="/home/back\\\\slash/Helm.AppImage""#),
        "{text}"
    );
}

#[test]
fn windows_and_macos_leave_the_launcher_to_their_installs() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    let desktop = Desktop {
        name: "Helm",
        path: dirs.folder("apps").join("Helm"),
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
    assert_eq!(target.path, apps.join("Helm.app"));
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
        name: "Helm",
        path: dirs.folder("apps").join("Helm.AppImage"),
    };
    let entry = dirs
        .home
        .join(".local")
        .join("share")
        .join("applications")
        .join("helm.desktop");
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
