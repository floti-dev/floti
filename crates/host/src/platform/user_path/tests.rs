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

#[test]
fn windows_add_appends_the_folder_to_the_value_as_written_with_no_empty_entry() {
    let folder = r"C:\Users\me\AppData\Local\Programs\Floti\bin";
    assert_eq!(
        windows::with_folder(Some(r"%USERPROFILE%\.local\bin;C:\tools;"), folder),
        format!(r"%USERPROFILE%\.local\bin;C:\tools;{folder}")
    );
    assert_eq!(windows::with_folder(Some(""), folder), folder);
    assert_eq!(windows::with_folder(None, folder), folder);
}

#[test]
fn an_install_says_what_it_changed_in_one_line() {
    let folder = Path::new("/home/me/.local/bin");
    assert_eq!(
        Added::Put { file: None }.sentence(folder).as_deref(),
        Some("Put /home/me/.local/bin on your PATH, for terminals and logins started from now on.")
    );
    assert_eq!(
        Added::Put {
            file: Some(PathBuf::from("/home/me/.zshenv"))
        }
        .sentence(folder)
        .as_deref(),
        Some(
            "Put /home/me/.local/bin on your PATH in /home/me/.zshenv, for terminals and logins started from now on."
        )
    );
    assert_eq!(Added::Nothing.sentence(folder), None);
    let by_hand = Added::ByHand {
        line: "echo it >> ~/.profile".to_owned(),
    }
    .sentence(folder)
    .unwrap();
    assert!(
        by_hand.contains("does not know which startup file")
            && by_hand.ends_with(": echo it >> ~/.profile"),
        "{by_hand}"
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
    let export = r#"echo 'export PATH="$HOME/.local/bin:$PATH" # added by floti' >> "#;
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
        r#"mkdir -p ~/.config/fish && echo 'fish_add_path -g "$HOME/.local/bin" # added by floti' >> ~/.config/fish/config.fish"#
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
        r#"echo 'export PATH="/opt/it'\''s \$here:$PATH" # added by floti' >> ~/.profile"#
    );
    // dash and the other shells that read ~/.profile, and one floti knows
    // nothing of, which doctor still gives ~/.profile.
    for shell in ["/bin/dash", "/bin/ksh", "/bin/tcsh"] {
        assert_eq!(
            posix::fix(&user(shell), &folder),
            format!("{export}~/.profile"),
            "{shell}"
        );
    }
}

#[test]
fn each_shells_fix_is_read_back_as_on_path() {
    for shell in [
        "/bin/sh",
        "/bin/dash",
        "/bin/bash",
        "/bin/zsh",
        "/usr/bin/fish",
    ] {
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

// ---------------------------------------------------------------------------
// Taking floti's entry off again, for floti uninstall
// ---------------------------------------------------------------------------

const FLOTI_BIN: &str = r"C:\Users\me\AppData\Local\Programs\Floti\bin";

#[test]
fn windows_takes_out_only_floti_s_entry_and_keeps_the_rest_as_written() {
    let user = env(None, &[]);
    assert_eq!(
        windows::without_folder(
            Some(r"%USERPROFILE%\.local\bin;C:\Users\me\AppData\Local\Programs\Floti\bin;D:\tools"),
            FLOTI_BIN,
            &user
        ),
        Some(r"%USERPROFILE%\.local\bin;D:\tools".to_owned())
    );
    // What install-self appended, to a value that was empty or absent.
    assert_eq!(
        windows::without_folder(Some(FLOTI_BIN), FLOTI_BIN, &user),
        Some(String::new())
    );
    assert_eq!(
        windows::without_folder(
            Some(windows::with_folder(None, FLOTI_BIN).as_str()),
            FLOTI_BIN,
            &user
        ),
        Some(String::new())
    );
    // Not there: nothing to change.
    assert_eq!(
        windows::without_folder(Some(r"C:\tools;D:\bin"), FLOTI_BIN, &user),
        None
    );
    assert_eq!(windows::without_folder(None, FLOTI_BIN, &user), None);
}

#[test]
fn windows_knows_the_entry_in_any_case_quoted_with_a_trailing_separator_or_by_its_variable() {
    let user = env(None, &[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")]);
    for written in [
        r"c:\users\ME\appdata\local\programs\floti\BIN",
        r"C:\Users\me\AppData\Local\Programs\Floti\bin\",
        r#""C:\Users\me\AppData\Local\Programs\Floti\bin""#,
        r"%LocalAppData%\Programs\Floti\bin",
    ] {
        let value = format!(r"C:\tools;{written};;D:\bin;");
        assert_eq!(
            windows::without_folder(Some(&value), FLOTI_BIN, &user),
            Some(r"C:\tools;D:\bin".to_owned()),
            "{written}"
        );
    }
    // Twice is taken out twice.
    let twice = format!(r"{FLOTI_BIN};C:\tools;{FLOTI_BIN}\");
    assert_eq!(
        windows::without_folder(Some(&twice), FLOTI_BIN, &user),
        Some(r"C:\tools".to_owned())
    );
    // A folder inside it, or one that only starts the same, is not it.
    let near = format!(r"{FLOTI_BIN}\node;{FLOTI_BIN}2");
    assert_eq!(windows::without_folder(Some(&near), FLOTI_BIN, &user), None);
}

#[test]
fn on_macos_and_linux_a_path_line_the_user_wrote_stays() {
    let dirs = TestDirs::new();
    let profile = dirs.home.join(".profile");
    let line = "PATH=\"$HOME/.local/bin:$PATH\"\n";
    std::fs::write(&profile, line).unwrap();
    let user = env(Some(&dirs.home), &[("SHELL", "/bin/bash")]);
    let folder = dirs.home.join(".local").join("bin");
    assert_eq!(linux::Native.remove(&user, &folder), Ok(false));
    assert_eq!(macos::Native.remove(&user, &folder), Ok(false));
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), line);
}

#[test]
fn uninstall_takes_flotis_line_and_only_it_out_of_every_shells_files() {
    let unix = Unix::new();
    let folder = unix.folder();
    let (export, _) = appended(&posix::fix(&unix.user("/bin/bash"), &folder));
    let (fish, _) = appended(&posix::fix(&unix.user("/usr/bin/fish"), &folder));
    let (elsewhere, _) = appended(&posix::fix(
        &unix.user("/bin/bash"),
        Path::new("/opt/floti/bin"),
    ));
    // The user's own lines: the same PATH line without the marker, the
    // marker alone, and floti's line for another folder.
    let own = format!(
        "export PATH=\"$HOME/.local/bin:$PATH\"\n{}\n{elsewhere}\n",
        posix::MARKER
    );
    unix.write(
        unix.home(),
        ".profile",
        &format!("umask 022\r\n{export}\r\n{own}{export}\n# the end"),
    );
    // Added while the user's shell was another.
    unix.write(unix.home(), ".zshrc", &format!("{export}\n"));
    unix.write(
        unix.home(),
        ".config/fish/config.fish",
        &format!("set -x EDITOR vi\n{fish}\n"),
    );

    let user = unix.user("/bin/zsh");
    assert_eq!(posix::remove(&user, &folder), Ok(true));
    assert_eq!(
        unix.read(".profile"),
        format!("umask 022\r\n{own}# the end")
    );
    assert_eq!(unix.read(".zshrc"), "");
    assert_eq!(unix.read(".config/fish/config.fish"), "set -x EDITOR vi\n");
    // Nothing is left beside them, and a second uninstall finds nothing.
    let mut left: Vec<String> = std::fs::read_dir(unix.home())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, [".config", ".profile", ".zshrc"]);
    assert_eq!(posix::remove(&user, &folder), Ok(false));
}

// ---------------------------------------------------------------------------
// macOS and Linux: floti's line, added once by an install
// ---------------------------------------------------------------------------

/// Every startup file a shell floti knows reads, under the home folder.
const STARTUP_FILES: [&str; 9] = [
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".bashrc",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".zlogin",
    ".config/fish/config.fish",
];

/// The user `PATH` of a Linux computer whose root is the test's.
struct LinuxAt<'a>(&'a Path);

impl UserPath for LinuxAt<'_> {
    fn has(&self, env: &UserEnv, floti: &Path) -> Result<bool, String> {
        let home = env.require_home().map_err(|err| err.to_string())?;
        posix::has(&linux::SYSTEM, self.0, env, home, floti)
    }

    fn fix(&self, env: &UserEnv, folder: &Path) -> String {
        posix::fix(env, folder)
    }

    fn add(&self, env: &UserEnv, folder: &Path) -> Result<Added, String> {
        posix::add(env, folder)
    }

    fn remove(&self, env: &UserEnv, folder: &Path) -> Result<bool, String> {
        posix::remove(env, folder)
    }
}

impl Unix {
    fn user(&self, shell: &str) -> UserEnv {
        env(Some(self.home()), &[("SHELL", shell)])
    }

    fn folder(&self) -> PathBuf {
        self.floti().parent().unwrap().to_path_buf()
    }

    fn at(&self, file: &str) -> PathBuf {
        file.split('/')
            .fold(self.home().to_path_buf(), |path, part| path.join(part))
    }

    fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.at(file)).unwrap()
    }
}

#[test]
fn the_install_appends_doctors_line_to_the_file_doctor_names_for_each_shell_and_makes_no_other() {
    for (shell, file) in [
        ("/bin/sh", ".profile"),
        ("/usr/bin/dash", ".profile"),
        ("/bin/bash", ".profile"),
        ("/bin/zsh", ".zshenv"),
        ("/usr/bin/fish", ".config/fish/config.fish"),
    ] {
        let unix = Unix::new();
        let user = unix.user(shell);
        let path = LinuxAt(&unix.root);
        assert_eq!(path.has(&user, &unix.floti()), Ok(false), "{shell}");
        assert_eq!(
            put_on_path(&path, &user, &unix.floti()),
            Ok(Added::Put {
                file: Some(unix.at(file))
            }),
            "{shell}"
        );
        let (line, named) = appended(&path.fix(&user, &unix.folder()));
        assert_eq!(named, format!("~/{file}"), "{shell}");
        assert!(line.ends_with(posix::MARKER), "{line}");
        assert_eq!(unix.read(file), format!("{line}\n"), "{shell}");
        assert_eq!(path.has(&user, &unix.floti()), Ok(true), "{shell}");
        for other in STARTUP_FILES.iter().filter(|other| **other != file) {
            assert!(!unix.at(other).exists(), "{shell} made {other}");
        }
    }
}

#[test]
fn an_existing_file_gets_the_line_at_its_end_and_keeps_every_line_it_had() {
    let unix = Unix::new();
    // A login bash reads ~/.bash_profile here, so ~/.profile is not made.
    unix.write(unix.home(), ".bash_profile", "umask 022\nsource ~/.bashrc");
    let user = unix.user("/bin/bash");
    assert_eq!(
        posix::add(&user, &unix.folder()),
        Ok(Added::Put {
            file: Some(unix.at(".bash_profile"))
        })
    );
    let (line, _) = appended(&posix::fix(&user, &unix.folder()));
    assert_eq!(
        unix.read(".bash_profile"),
        format!("umask 022\nsource ~/.bashrc\n{line}\n")
    );
    assert!(!unix.at(".profile").exists());
}

#[test]
fn a_second_install_adds_nothing() {
    let unix = Unix::new();
    unix.write(unix.home(), ".zshenv", "export EDITOR=vi\n");
    let user = unix.user("/bin/zsh");
    let path = LinuxAt(&unix.root);
    assert!(matches!(
        put_on_path(&path, &user, &unix.floti()),
        Ok(Added::Put { .. })
    ));
    let once = unix.read(".zshenv");
    assert_eq!(put_on_path(&path, &user, &unix.floti()), Ok(Added::Nothing));
    // Asked to add it again, a file with floti's line keeps it once.
    assert_eq!(posix::add(&user, &unix.folder()), Ok(Added::Nothing));
    assert_eq!(unix.read(".zshenv"), once);
}

#[test]
fn a_folder_the_shells_startup_files_put_on_path_already_gets_no_line() {
    let unix = Unix::new();
    let ubuntu = "if [ -d \"$HOME/.local/bin\" ] ; then\n    PATH=\"$HOME/.local/bin:$PATH\"\nfi\n";
    unix.write(unix.home(), ".profile", ubuntu);
    assert_eq!(
        put_on_path(&LinuxAt(&unix.root), &unix.user("/bin/bash"), &unix.floti()),
        Ok(Added::Nothing)
    );
    assert_eq!(unix.read(".profile"), ubuntu);
}

#[test]
fn a_shell_floti_knows_no_startup_file_of_is_given_the_line_and_no_file_changes() {
    for vars in [&[("SHELL", "/bin/tcsh")][..], &[][..]] {
        let unix = Unix::new();
        let user = env(Some(unix.home()), vars);
        assert_eq!(
            posix::add(&user, &unix.folder()),
            Ok(Added::ByHand {
                line: posix::fix(&user, &unix.folder())
            }),
            "{vars:?}"
        );
        assert!(std::fs::read_dir(unix.home()).unwrap().next().is_none());
    }
}

#[cfg(unix)]
#[test]
fn a_linked_startup_file_is_changed_through_its_link_and_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let unix = Unix::new();
    let dotfiles = unix.dirs.root().join("my dotfiles");
    std::fs::create_dir_all(&dotfiles).unwrap();
    let target = dotfiles.join("zshenv");
    std::fs::write(&target, "export EDITOR=vi\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&target, unix.at(".zshenv")).unwrap();
    let still_linked = || {
        assert!(
            std::fs::symlink_metadata(unix.at(".zshenv"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    };
    let user = unix.user("/bin/zsh");
    let folder = unix.folder();

    assert_eq!(
        posix::add(&user, &folder),
        Ok(Added::Put {
            file: Some(unix.at(".zshenv"))
        })
    );
    let (line, _) = appended(&posix::fix(&user, &folder));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        format!("export EDITOR=vi\n{line}\n")
    );
    still_linked();

    assert_eq!(posix::remove(&user, &folder), Ok(true));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "export EDITOR=vi\n"
    );
    still_linked();
}
