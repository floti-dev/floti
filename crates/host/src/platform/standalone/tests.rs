use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::testkit::TestDirs;

fn env(home: Option<&Path>, vars: &[(&str, &Path)]) -> UserEnv {
    UserEnv {
        home: home.map(Path::to_path_buf),
        vars: vars
            .iter()
            .map(|(name, value)| (OsString::from(name), value.as_os_str().to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

/// The files in `folder`, by name.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn windows_puts_it_in_localappdata_programs_floti_bin() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    assert_eq!(
        windows::Native.folder(&env(Some(&dirs.home), &[("LOCALAPPDATA", &local)])),
        Some(local.join("Programs").join("Floti").join("bin"))
    );
    assert_eq!(
        windows::Native.binary(&env(Some(&dirs.home), &[("LOCALAPPDATA", &local)])),
        Some(
            local
                .join("Programs")
                .join("Floti")
                .join("bin")
                .join("floti.exe")
        )
    );
    // No guess from the profile folder where install.ps1 would find none.
    assert_eq!(windows::Native.folder(&env(Some(&dirs.home), &[])), None);
    assert_eq!(windows::Native.binary(&env(Some(&dirs.home), &[])), None);
}

#[test]
fn macos_and_linux_put_it_in_local_bin() {
    let dirs = TestDirs::new();
    let wanted = Some(dirs.home.join(".local").join("bin"));
    for place in [&macos::Native as &dyn Standalone, &linux::Native] {
        assert_eq!(place.folder(&env(Some(&dirs.home), &[])), wanted);
        assert_eq!(
            place.binary(&env(Some(&dirs.home), &[])),
            wanted.as_ref().map(|folder| folder.join("floti"))
        );
        assert_eq!(place.folder(&env(None, &[])), None);
    }
}

#[test]
fn windows_puts_the_old_copy_aside_and_removes_it_when_nothing_runs_it() {
    let dirs = TestDirs::new();
    let bin = dirs.folder("bin");
    let target = bin.join("floti.exe");
    let new = bin.join(".floti-install.1.floti.exe");

    std::fs::write(&new, b"first").unwrap();
    windows::Native.replace(&target, &new).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"first");
    assert_eq!(names(&bin), ["floti.exe"]);

    std::fs::write(&new, b"second").unwrap();
    windows::Native.replace(&target, &new).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"second");
    // Nothing ran the old copy, so it is gone already.
    assert_eq!(names(&bin), ["floti.exe"]);

    // Copies an earlier install or update left, under either name, go once
    // nothing runs them; another program's copy stays.
    for name in ["floti.old-11-0.exe", "floti.old-12.exe", "node.old-1-0.exe"] {
        std::fs::write(bin.join(name), b"left").unwrap();
    }
    assert_eq!(
        windows::Native.remove_asides(&target),
        Vec::<PathBuf>::new()
    );
    assert_eq!(names(&bin), ["floti.exe", "node.old-1-0.exe"]);
}

#[test]
fn windows_leaves_the_old_copy_in_place_when_the_new_one_cannot_take_its_name() {
    let dirs = TestDirs::new();
    let bin = dirs.folder("bin");
    let target = bin.join("floti.exe");
    std::fs::write(&target, b"old").unwrap();
    let missing = bin.join(".floti-install.1.floti.exe");
    assert!(windows::Native.replace(&target, &missing).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    assert_eq!(names(&bin), ["floti.exe"]);
}

#[test]
fn macos_and_linux_rename_the_new_file_over_the_old_and_put_nothing_aside() {
    for place in [&macos::Native as &dyn Standalone, &linux::Native] {
        let dirs = TestDirs::new();
        let bin = dirs.folder("bin");
        let target = bin.join("floti");
        std::fs::write(&target, b"old").unwrap();
        let new = bin.join(".floti-install.1.floti");
        std::fs::write(&new, b"new").unwrap();
        place.replace(&target, &new).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(names(&bin), ["floti"]);
        assert!(place.remove_asides(&target).is_empty());
    }
}

#[cfg(unix)]
#[test]
fn macos_and_linux_make_the_new_file_executable() {
    use std::os::unix::fs::PermissionsExt;
    for place in [&macos::Native as &dyn Standalone, &linux::Native] {
        let dirs = TestDirs::new();
        let bin = dirs.folder("bin");
        let target = bin.join("floti");
        let new = bin.join(".floti-install.1.floti");
        std::fs::write(&new, b"new").unwrap();
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o600)).unwrap();
        place.replace(&target, &new).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_puts_floti_in_place_without_the_dmgs_quarantine() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let dirs = TestDirs::new();
    let bin = dirs.folder("bin");
    let target = bin.join("floti");
    let new = bin.join(".floti-install.1.floti");
    std::fs::write(&new, b"new").unwrap();
    let file = CString::new(new.as_os_str().as_bytes()).unwrap();
    let name = CString::new(macos::QUARANTINE).unwrap();
    let value = b"0083;00000000;Safari;";
    // SAFETY: NUL-terminated names and a value buffer of the length given.
    let set = unsafe {
        libc::setxattr(
            file.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    assert_eq!(set, 0, "{}", std::io::Error::last_os_error());
    macos::Native.replace(&target, &new).unwrap();
    let placed = CString::new(target.as_os_str().as_bytes()).unwrap();
    // SAFETY: NUL-terminated names; a null buffer asks only for the size.
    let size = unsafe {
        libc::getxattr(
            placed.as_ptr(),
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            0,
        )
    };
    assert_eq!(size, -1, "the quarantine flag is gone");
}

/// Ends the process it holds when the test ends, however it ends.
#[cfg(windows)]
struct Running(std::process::Child);

#[cfg(windows)]
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(windows)]
#[test]
fn windows_puts_a_running_copy_aside_and_keeps_it_until_nothing_runs_it() {
    use std::process::{Command, Stdio};

    let dirs = TestDirs::new();
    let bin = dirs.folder("bin");
    let target = bin.join("floti.exe");
    // Any program that keeps running stands in for a running host.
    let system = std::env::var_os("SystemRoot").expect("SystemRoot is set");
    std::fs::copy(
        PathBuf::from(system).join("System32").join("PING.EXE"),
        &target,
    )
    .unwrap();
    let mut running = Running(
        Command::new(&target)
            .args(["-n", "600", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let new = bin.join(".floti-install.1.floti.exe");
    std::fs::write(&new, b"new").unwrap();
    windows::Native.replace(&target, &new).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    assert!(running.0.try_wait().unwrap().is_none(), "it still runs");
    assert_eq!(names(&bin).len(), 2, "the running copy is kept aside");
    let left = windows::Native.remove_asides(&target);
    assert_eq!(left.len(), 1, "the running copy is kept: {left:?}");

    running.0.kill().unwrap();
    running.0.wait().unwrap();
    assert!(windows::Native.remove_asides(&target).is_empty());
    assert_eq!(names(&bin), ["floti.exe"]);
}
