//! Each OS's installs, on every OS, over folders in a `TestDirs`: where the
//! desktop app is found and goes, which version it is, which processes are
//! its own, which floti is its own, and the renames that put a binary, an
//! AppImage or an app in place.

use std::ffi::OsString;
use std::path::Path;

use super::{DESKTOP_DIR_VAR, Desktop, Installs, leftovers, linux, macos, plist_string, windows};
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
