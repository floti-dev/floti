use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::posix;
use super::*;
use crate::platform::links::{FolderLinks, Native as NativeLinks};
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

const FLOTI_EXE: &str = r"C:\Users\me\AppData\Local\Programs\Floti\bin\floti.exe";

#[test]
fn windows_finds_the_folder_in_the_users_value_with_its_variables_expanded_in_any_case() {
    let user = env(None, &[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")]);
    let system = r"C:\Windows\system32;C:\Windows";
    let value = r"%USERPROFILE%\.local\bin;%LocalAppData%\Programs\Floti\bin\";
    assert!(windows::names([system, value], &user, Path::new(FLOTI_EXE)));
    // Any case, and with forward slashes or quotes.
    assert!(windows::names(
        [r#""c:/users/ME/appdata/local/programs/floti/BIN""#],
        &user,
        Path::new(FLOTI_EXE)
    ));
}

#[test]
fn windows_does_not_find_a_folder_on_neither_value_nor_through_an_unknown_variable() {
    let user = env(None, &[]);
    assert!(!windows::names(
        [r"C:\Windows\system32", r"%NOT_SET%\Programs\Floti\bin;;"],
        &user,
        Path::new(FLOTI_EXE)
    ));
    assert!(!windows::names([], &user, Path::new(FLOTI_EXE)));
    // A folder above or below floti's is not floti's.
    assert!(!windows::names(
        [
            r"C:\Users\me\AppData\Local\Programs\Floti",
            r"C:\Users\me\AppData\Local\Programs\Floti\bin\tools"
        ],
        &user,
        Path::new(FLOTI_EXE)
    ));
}

#[test]
fn a_path_folder_whose_floti_is_this_one_by_another_name_counts() {
    let dirs = TestDirs::new();
    let real = dirs.root().join("real bin");
    std::fs::create_dir_all(&real).unwrap();
    let floti = real.join("floti.exe");
    std::fs::write(&floti, b"floti").unwrap();
    let linked = dirs.root().join("linked bin");
    NativeLinks
        .link_folder(&real, &linked)
        .expect("link the folder");
    let user = env(None, &[]);
    let value = linked.display().to_string();
    assert!(windows::names([value.as_str()], &user, &floti));
    // Another floti of the same name is not this one.
    let other = dirs.root().join("other bin");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("floti.exe"), b"another").unwrap();
    let value = other.display().to_string();
    assert!(!windows::names([value.as_str()], &user, &floti));
}

#[test]
fn windows_fix_appends_the_folder_to_the_users_value_as_written_and_keeps_it_expandable() {
    let fix = windows::Native.fix(&env(None, &[]), Path::new(r"C:\Users\o'neil\bin"));
    assert_eq!(
        fix,
        r"$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); $path = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames').TrimEnd(';'); $key.SetValue('Path', (($path, 'C:\Users\o''neil\bin') -join ';').TrimStart(';'), 'ExpandString')"
    );
    assert!(!fix.contains("SetEnvironmentVariable"));
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

/// A computer's root and a home in the test's tree, with floti in that
/// home's `~/.local/bin`.
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

    fn floti(&self) -> PathBuf {
        self.home().join(".local").join("bin").join("floti")
    }

    fn write(&self, under: &Path, file: &str, text: &str) {
        let path = file
            .split('/')
            .fold(under.to_path_buf(), |path, part| path.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }

    fn has_with(
        &self,
        system: &posix::SystemPath,
        shell: &str,
        floti: &Path,
    ) -> Result<bool, String> {
        let user = env(Some(self.home()), &[("SHELL", shell)]);
        posix::has(system, &self.root, &user, self.home(), floti)
    }

    fn has(&self, shell: &str) -> Result<bool, String> {
        self.has_with(&linux::SYSTEM, shell, &self.floti())
    }
}

#[test]
fn a_standard_folder_is_on_every_login_shells_path() {
    let unix = Unix::new();
    let in_usr = Path::new("/usr/local/bin/floti");
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/sh", in_usr), Ok(true));
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", in_usr), Ok(true));
    assert_eq!(unix.has("/bin/sh"), Ok(false));
}

#[test]
fn macos_reads_etc_paths_and_paths_d_and_linux_etc_environment() {
    let unix = Unix::new();
    let floti = Path::new("/opt/floti/bin/floti");
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", floti), Ok(false));
    unix.write(&unix.root, "etc/paths.d/floti", "/opt/floti/bin\n");
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", floti), Ok(true));
    // Linux does not read macOS's list.
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/bash", floti), Ok(false));
    unix.write(
        &unix.root,
        "etc/environment",
        "PATH=\"/usr/local/bin:/usr/bin:/opt/floti/bin\"\n",
    );
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/bash", floti), Ok(true));
}

#[test]
fn a_path_line_in_a_file_the_shell_reads_names_the_folder_however_home_is_written() {
    for (shell, file, line) in [
        // Ubuntu's own ~/.profile: added only where the folder exists at
        // sign-in, which it does once floti is in it.
        (
            "/bin/sh",
            ".profile",
            "if [ -d \"$HOME/.local/bin\" ] ; then\n    PATH=\"$HOME/.local/bin:$PATH\"\nfi\n",
        ),
        (
            "/bin/bash",
            ".bashrc",
            "export PATH=${HOME}/.local/bin/:$PATH\n",
        ),
        ("/bin/zsh", ".zshrc", "path+=(~/.local/bin)\nexport PATH\n"),
        (
            "/usr/bin/fish",
            ".config/fish/config.fish",
            "fish_add_path -g ~/.local/bin\n",
        ),
    ] {
        let unix = Unix::new();
        unix.write(unix.home(), file, line);
        assert_eq!(unix.has(shell), Ok(true), "{line}");
    }
}

#[test]
fn only_the_files_the_users_shell_reads_count() {
    let unix = Unix::new();
    unix.write(
        unix.home(),
        ".zshrc",
        "export PATH=\"$HOME/.local/bin:$PATH\"\n",
    );
    assert_eq!(unix.has("/bin/zsh"), Ok(true));
    assert_eq!(unix.has("/bin/bash"), Ok(false), "bash reads no .zshrc");

    // A login bash reads ~/.bash_profile and skips ~/.profile once it exists.
    let unix = Unix::new();
    unix.write(unix.home(), ".profile", "PATH=\"$HOME/.local/bin:$PATH\"\n");
    assert_eq!(unix.has("/bin/bash"), Ok(true));
    unix.write(unix.home(), ".bash_profile", "source ~/.bashrc\n");
    assert_eq!(unix.has("/bin/bash"), Ok(false));
    // sh still reads it.
    assert_eq!(unix.has("/bin/sh"), Ok(true));
}

#[test]
fn a_folder_below_flotis_or_another_variable_does_not_count() {
    let unix = Unix::new();
    unix.write(
        unix.home(),
        ".profile",
        "# PATH=\"$HOME/.local/bin:$PATH\"\nexport PATH=\"$HOME/.local/bin/tools:$PATH\"\nexport MANPATH=\"$HOME/.local/bin:$MANPATH\"\nPYTHONPATH=$HOME/.local/bin\nalias l=\"ls $HOME/.local/bin\"\n",
    );
    assert_eq!(unix.has("/bin/sh"), Ok(false));
}

#[test]
fn brew_shellenv_puts_that_homebrews_bin_on_path() {
    let unix = Unix::new();
    let floti = Path::new("/opt/homebrew/bin/floti");
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", floti), Ok(false));
    unix.write(
        unix.home(),
        ".zprofile",
        "eval \"$(/opt/homebrew/bin/brew shellenv)\"\n",
    );
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", floti), Ok(true));
}

#[test]
fn a_quoted_folder_with_spaces_is_one_folder_in_a_path_line_and_in_brew_shellenv() {
    let unix = Unix::new();
    let floti = unix.home().join("my tools").join("bin").join("floti");
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/sh", &floti), Ok(false));
    unix.write(
        unix.home(),
        ".profile",
        "export PATH=\"$HOME/my tools\"/bin:$PATH\n",
    );
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/sh", &floti), Ok(true));

    let brewed = Path::new("/opt/my brew/bin/floti");
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", brewed), Ok(false));
    unix.write(
        unix.home(),
        ".zprofile",
        "eval \"$(\"/opt/my brew/bin/brew\" shellenv)\"\n",
    );
    assert_eq!(unix.has_with(&macos::SYSTEM, "/bin/zsh", brewed), Ok(true));
}

#[test]
fn a_windows_path_splits_into_its_folder_and_name_on_every_os() {
    assert_eq!(
        windows::split_windows_path(FLOTI_EXE),
        (
            Some(r"C:\Users\me\AppData\Local\Programs\Floti\bin"),
            "floti.exe".to_owned()
        )
    );
    assert_eq!(
        windows::split_windows_path("C:/floti.exe"),
        (Some("C:/"), "floti.exe".to_owned())
    );
    assert_eq!(
        windows::split_windows_path("floti.exe"),
        (None, "floti.exe".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn a_floti_linked_from_a_path_folder_is_this_one() {
    // Homebrew's bin holds a link to the floti in its Cellar, and this floti
    // runs as the Cellar's.
    let unix = Unix::new();
    let cellar = unix
        .dirs
        .root()
        .join("Cellar")
        .join("floti")
        .join("3.0.0")
        .join("bin");
    std::fs::create_dir_all(&cellar).unwrap();
    let floti = cellar.join("floti");
    std::fs::write(&floti, b"floti").unwrap();
    let bin = unix.dirs.root().join("linuxbrew").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&floti, bin.join("floti")).unwrap();
    assert_eq!(
        unix.has_with(&linux::SYSTEM, "/bin/bash", &floti),
        Ok(false)
    );
    unix.write(
        unix.home(),
        ".bashrc",
        // Quoted, as a shell needs a folder with spaces to be: the test's
        // tree has them.
        &format!("eval \"$(\"{}/brew\" shellenv)\"\n", bin.display()),
    );
    assert_eq!(unix.has_with(&linux::SYSTEM, "/bin/bash", &floti), Ok(true));
}

#[cfg(unix)]
#[test]
fn a_startup_file_that_cannot_be_read_is_could_not_tell_never_not_on_path() {
    let unix = Unix::new();
    // A folder where the file should be cannot be read as one.
    std::fs::create_dir_all(unix.home().join(".bashrc").join("inside")).unwrap();
    let told = unix.has("/bin/bash");
    assert!(
        told.as_ref().is_err_and(|why| why.contains(".bashrc")),
        "{told:?}"
    );
}

#[test]
fn the_unix_fix_appends_to_the_first_file_the_users_shell_reads() {
    let unix = Unix::new();
    let folder = unix.home().join(".local").join("bin");
    let user = |shell: &str| env(Some(unix.home()), &[("SHELL", shell)]);
    let export = r#"echo 'export PATH="$HOME/.local/bin:$PATH"' >> "#;
    assert_eq!(
        posix::fix(&user("/bin/sh"), &folder),
        format!("{export}~/.profile")
    );
    assert_eq!(
        posix::fix(&user("/bin/zsh"), &folder),
        format!("{export}~/.zshenv")
    );
    assert_eq!(
        posix::fix(&user("/usr/bin/fish"), &folder),
        r#"mkdir -p ~/.config/fish && echo 'fish_add_path -g "$HOME/.local/bin"' >> ~/.config/fish/config.fish"#
    );
    // bash: ~/.profile until a ~/.bash_profile shadows it.
    assert_eq!(
        posix::fix(&user("/bin/bash"), &folder),
        format!("{export}~/.profile")
    );
    unix.write(unix.home(), ".bash_profile", "");
    assert_eq!(
        posix::fix(&user("/bin/bash"), &folder),
        format!("{export}~/.bash_profile")
    );
    // A folder outside home with a quote in it is written so the shell
    // reads it back as it is.
    assert_eq!(
        posix::fix(&user("/bin/sh"), Path::new("/opt/it's $here")),
        r#"echo 'export PATH="/opt/it'\''s \$here:$PATH"' >> ~/.profile"#
    );
}

#[test]
fn each_shells_fix_is_read_back_as_on_path() {
    for shell in ["/bin/sh", "/bin/bash", "/bin/zsh", "/usr/bin/fish"] {
        let unix = Unix::new();
        let folder = unix.floti().parent().unwrap().to_path_buf();
        let user = env(Some(unix.home()), &[("SHELL", shell)]);
        let fix = posix::fix(&user, &folder);
        let (line, file) = appended(&fix);
        unix.write(
            unix.home(),
            file.strip_prefix("~/").unwrap(),
            &format!("{line}\n"),
        );
        assert_eq!(unix.has(shell), Ok(true), "{shell}: {fix}");
    }
}

/// The line `fix` appends and the file it appends it to, read back from its
/// `echo '<line>' >> <file>`.
fn appended(fix: &str) -> (String, String) {
    let echo = fix.rsplit("&& ").next().unwrap();
    let (quoted, file) = echo
        .strip_prefix("echo ")
        .unwrap()
        .rsplit_once(" >> ")
        .unwrap();
    let line = quoted
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap()
        .replace("'\\''", "'");
    (line, file.to_owned())
}
