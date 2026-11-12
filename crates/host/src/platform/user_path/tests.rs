use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::posix;
use super::*;
use crate::testkit::TestDirs;

fn env(home: Option<&Path>, vars: &[(&str, &str)]) -> UserEnv {
    UserEnv {
        home: home.map(Path::to_path_buf),
        vars: vars
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect::<BTreeMap<_, _>>(),
    }
}

// ---------------------------------------------------------------------------
// Windows: the registry's two values, expanded
// ---------------------------------------------------------------------------

const FLOTI_BIN: &str = r"C:\Users\me\AppData\Local\Programs\Floti\bin";

#[test]
fn windows_finds_the_folder_in_the_users_value_with_its_variables_expanded_in_any_case() {
    let user = env(None, &[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")]);
    let system = r"C:\Windows\system32;C:\Windows";
    let value = r"%USERPROFILE%\.local\bin;%LocalAppData%\Programs\Floti\bin\";
    assert!(windows::names([system, value], &user, Path::new(FLOTI_BIN)));
    // Any case, and with forward slashes or quotes.
    assert!(windows::names(
        [r#""c:/users/ME/appdata/local/programs/floti/BIN""#],
        &user,
        Path::new(FLOTI_BIN)
    ));
}

#[test]
fn windows_does_not_find_a_folder_on_neither_value_nor_through_an_unknown_variable() {
    let user = env(None, &[]);
    assert!(!windows::names(
        [r"C:\Windows\system32", r"%NOT_SET%\Programs\Floti\bin;;"],
        &user,
        Path::new(FLOTI_BIN)
    ));
    assert!(!windows::names([], &user, Path::new(FLOTI_BIN)));
    // A folder inside one on the PATH is not on it.
    assert!(!windows::names(
        [r"C:\Users\me\AppData\Local\Programs\Floti"],
        &user,
        Path::new(FLOTI_BIN)
    ));
}

#[test]
fn windows_fix_appends_the_folder_to_the_users_value_in_powershell() {
    let fix = windows::Native.fix(&env(None, &[]), Path::new(r"C:\Users\o'neil\bin"));
    assert_eq!(
        fix,
        r"[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';C:\Users\o''neil\bin', 'User')"
    );
}

#[cfg(windows)]
#[test]
fn windows_reads_a_string_value_and_says_none_for_one_that_is_not_there() {
    // A value every Windows has, which is not a PATH: the reader is tested
    // without reading this computer's PATH.
    use windows::registry::{Hive, string_value};
    const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let product = string_value(Hive::LocalMachine, VERSION_KEY, "ProductName").unwrap();
    assert!(product.is_some_and(|name| name.contains("Windows")));
    assert_eq!(
        string_value(Hive::LocalMachine, VERSION_KEY, "No Such Value For Floti"),
        Ok(None)
    );
    assert_eq!(
        string_value(Hive::CurrentUser, r"Software\No Such Key For Floti", "Path"),
        Ok(None)
    );
}

// ---------------------------------------------------------------------------
// macOS and Linux: the system's list and the user's startup files, read
// ---------------------------------------------------------------------------

/// A computer's root and a home in the test's tree, and floti's folder in
/// that home.
struct Unix {
    dirs: TestDirs,
    root: PathBuf,
}

impl Unix {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let root = dirs.root().join("root");
        std::fs::create_dir_all(&root).unwrap();
        Self { dirs, root }
    }

    fn home(&self) -> &Path {
        &self.dirs.home
    }

    fn floti_bin(&self) -> PathBuf {
        self.home().join(".local/bin")
    }

    fn write(&self, under: &Path, file: &str, text: &str) {
        let path = under.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }

    fn has(&self, system: &posix::SystemPath, folder: &Path) -> Result<bool, String> {
        posix::has(system, &self.root, self.home(), folder)
    }
}

#[test]
fn a_standard_folder_is_on_every_login_shells_path() {
    let unix = Unix::new();
    assert_eq!(
        unix.has(&linux::SYSTEM, Path::new("/usr/local/bin/")),
        Ok(true)
    );
    assert_eq!(
        unix.has(&macos::SYSTEM, Path::new("/usr/local/bin")),
        Ok(true)
    );
    assert_eq!(unix.has(&linux::SYSTEM, &unix.floti_bin()), Ok(false));
}

#[test]
fn macos_reads_etc_paths_and_paths_d_and_linux_etc_environment() {
    let unix = Unix::new();
    let folder = Path::new("/opt/floti/bin");
    assert_eq!(unix.has(&macos::SYSTEM, folder), Ok(false));
    unix.write(&unix.root, "etc/paths.d/floti", "/opt/floti/bin\n");
    assert_eq!(unix.has(&macos::SYSTEM, folder), Ok(true));
    // Linux does not read macOS's list.
    assert_eq!(unix.has(&linux::SYSTEM, folder), Ok(false));
    unix.write(
        &unix.root,
        "etc/environment",
        "PATH=\"/usr/local/bin:/usr/bin:/opt/floti/bin\"\n",
    );
    assert_eq!(unix.has(&linux::SYSTEM, folder), Ok(true));
}

#[test]
fn a_path_line_in_a_startup_file_names_the_folder_under_home_however_home_is_written() {
    for line in [
        "if [ -d \"$HOME/.local/bin\" ] ; then\n    PATH=\"$HOME/.local/bin:$PATH\"\nfi\n",
        "export PATH=${HOME}/.local/bin:$PATH\n",
        "path+=(~/.local/bin)\nexport PATH\n",
        "fish_add_path ~/.local/bin\n",
    ] {
        let unix = Unix::new();
        let file = if line.contains("fish") {
            ".config/fish/config.fish"
        } else {
            ".zshrc"
        };
        unix.write(unix.home(), file, line);
        assert_eq!(
            unix.has(&linux::SYSTEM, &unix.floti_bin()),
            Ok(true),
            "{line}"
        );
    }
}

#[test]
fn a_startup_file_that_names_the_folder_only_in_a_comment_or_a_longer_name_does_not_count() {
    let unix = Unix::new();
    unix.write(
        unix.home(),
        ".profile",
        "# PATH=\"$HOME/.local/bin:$PATH\"\nexport PATH=\"$HOME/.local/binaries:$PATH\"\nalias l=\"ls $HOME/.local/bin\"\n",
    );
    assert_eq!(unix.has(&linux::SYSTEM, &unix.floti_bin()), Ok(false));
}

#[cfg(unix)]
#[test]
fn a_startup_file_that_cannot_be_read_is_could_not_tell_never_not_on_path() {
    let unix = Unix::new();
    // A folder where the file should be cannot be read as one.
    std::fs::create_dir_all(unix.home().join(".bashrc").join("inside")).unwrap();
    let told = unix.has(&linux::SYSTEM, &unix.floti_bin());
    assert!(
        told.as_ref().is_err_and(|why| why.contains(".bashrc")),
        "{told:?}"
    );
}

#[test]
fn the_unix_fix_appends_an_export_to_the_file_the_users_shell_reads() {
    let home = Path::new("/home/me");
    let folder = Path::new("/home/me/.local/bin");
    let bash = env(Some(home), &[("SHELL", "/bin/bash")]);
    assert_eq!(
        posix::fix(&bash, folder),
        r#"echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.profile"#
    );
    let zsh = env(Some(home), &[("SHELL", "/bin/zsh")]);
    assert_eq!(
        posix::fix(&zsh, folder),
        r#"echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshenv"#
    );
    let fish = env(Some(home), &[("SHELL", "/usr/bin/fish")]);
    assert_eq!(
        posix::fix(&fish, folder),
        "fish_add_path '/home/me/.local/bin'"
    );
    // A folder outside home with a quote in it is written so the shell
    // reads it back as it is.
    assert_eq!(
        posix::fix(&bash, Path::new("/opt/it's $here")),
        r#"echo 'export PATH="/opt/it'\''s \$here:$PATH"' >> ~/.profile"#
    );
}
