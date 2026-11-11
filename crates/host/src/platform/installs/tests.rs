//! Each OS's installs, on every OS, over folders in a `TestDirs`: where the
//! desktop app is found, which processes are its own, and the renames that
//! put a binary or an AppImage in place.

use std::ffi::OsString;
use std::path::Path;

use super::{DESKTOP_DIR_VAR, Desktop, Installs, linux, macos, windows};
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
fn windows_moves_the_running_binary_aside_and_the_new_one_into_its_name() {
    let dirs = TestDirs::new();
    let bin = dirs.folder("bin dir");
    let running = bin.join("floti.exe");
    std::fs::write(&running, b"old").unwrap();
    std::fs::write(bin.join("floti.old.exe"), b"from an earlier update").unwrap();
    let new = bin.join(".floti-2.1.0.1.exe");
    std::fs::write(&new, b"new").unwrap();
    windows::Native.replace_binary(&running, &new).unwrap();
    assert_eq!(std::fs::read(&running).unwrap(), b"new");
    assert!(!new.exists());
    // Nothing runs the old copy here, so it is removed as well.
    assert!(!bin.join("floti.old.exe").exists());
}

#[test]
fn windows_puts_the_old_binary_back_when_the_new_one_cannot_move_in() {
    let dirs = TestDirs::new();
    let bin = dirs.folder("bin dir");
    let running = bin.join("floti.exe");
    std::fs::write(&running, b"old").unwrap();
    let err = windows::Native
        .replace_binary(&running, &bin.join("not there.exe"))
        .unwrap_err();
    assert!(err.to_string().contains("replace"), "{err}");
    assert_eq!(std::fs::read(&running).unwrap(), b"old");
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
}

#[test]
fn linux_finds_the_app_image_in_the_applications_folder() {
    let dirs = TestDirs::new();
    let user = env(&dirs.home, &[]);
    assert_eq!(linux::Native.desktop(&user), None);
    let apps = dirs.home.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("Floti.AppImage"), b"app").unwrap();
    assert_eq!(
        linux::Native.desktop(&user),
        Some(Desktop {
            name: "Floti",
            path: apps.join("Floti.AppImage")
        })
    );
}

#[test]
fn linux_counts_the_app_image_s_runtime() {
    let desktop = Desktop {
        name: "Floti",
        path: "/home/someone/Applications/Floti.AppImage".into(),
    };
    assert!(
        linux::Native
            .is_desktop_process(&desktop, &row("Floti.AppImage", Some("./Floti.AppImage")))
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
    linux::Native.replace_desktop(&desktop, &release).unwrap();
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
fn macos_and_linux_rename_the_new_binary_over_the_running_one() {
    for installs in [&linux::Native as &dyn Installs, &macos::Native] {
        let dirs = TestDirs::new();
        let bin = dirs.folder("bin dir");
        let running = bin.join("floti");
        std::fs::write(&running, b"old").unwrap();
        let new = bin.join(".floti-2.1.0.1");
        std::fs::write(&new, b"new").unwrap();
        installs.replace_binary(&running, &new).unwrap();
        assert_eq!(std::fs::read(&running).unwrap(), b"new");
        assert!(!new.exists());
    }
}
