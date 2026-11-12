//! Windows paths as Windows reads them, worked as text, so a concern's
//! Windows rule gives the same answer on whichever OS its tests run:
//! `std::path` on macOS or Linux takes all of `C:\Users\me\floti.exe` for
//! one part, with no folder, no file name and no root.
//!
//! Windows separates parts with `\` or `/` and compares names without
//! letter case. A path with no `\` in it and a `/` is one this OS gave,
//! such as a test's own folder on macOS or Linux, and is joined with `/`.

use std::path::{Path, PathBuf};

/// `folder` with `parts` after it, each after the separator `folder` uses:
/// `\`, or `/` for a folder written with `/` alone.
pub fn join(folder: &Path, parts: &[&str]) -> PathBuf {
    let text = folder.to_string_lossy();
    let separator = if !text.contains('\\') && text.contains('/') {
        '/'
    } else {
        '\\'
    };
    let mut joined = text.trim_end_matches(['\\', '/']).to_owned();
    for part in parts {
        joined.push(separator);
        joined.push_str(part);
    }
    PathBuf::from(joined)
}

/// Whether `path` is a full path: one this OS takes for one, or one
/// Windows does (`C:\…`, `C:/…` or `\\server\…`).
pub fn is_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let drive = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('\\' | '/')) if letter.is_ascii_alphabetic()
    );
    path.is_absolute() || drive || text.starts_with("\\\\")
}

/// `path` as Windows compares folders and files: `/` as `\`, no trailing
/// separator, any case. A drive's root keeps its separator: `C:\` and `C:`
/// are not one folder.
pub fn comparable(path: &str) -> String {
    let path = path.replace('/', "\\");
    let trimmed = path.trim_end_matches('\\');
    let trimmed = if trimmed.ends_with(':') {
        path.as_str()
    } else {
        trimmed
    };
    trimmed.to_lowercase()
}

/// Whether `path` is inside `folder`, and not `folder` itself, as Windows
/// names files.
pub fn inside(path: &Path, folder: &Path) -> bool {
    let path = comparable(&path.to_string_lossy());
    let folder = comparable(&folder.to_string_lossy());
    let folder = folder.trim_end_matches('\\');
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('\\') && rest.len() > 1)
}

/// A path's folder and last part, split on its last `\` or `/`. A drive's
/// root keeps its separator (`C:\`), as [`comparable`] keeps it.
pub fn split(path: &str) -> (Option<&str>, String) {
    match path.rfind(['\\', '/']) {
        Some(at) => {
            let folder = &path[..at];
            let folder = if folder.ends_with(':') {
                &path[..=at]
            } else {
                folder
            };
            (Some(folder), path[at + 1..].to_owned())
        }
        None => (None, path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: &str = r"C:\Users\Some One\AppData\Local";

    #[test]
    fn a_windows_path_is_joined_split_and_compared_as_windows_does_on_every_os() {
        assert_eq!(
            join(Path::new(LOCAL), &["Programs", "Helm"]),
            PathBuf::from(r"C:\Users\Some One\AppData\Local\Programs\Helm")
        );
        assert_eq!(
            join(Path::new(r"C:\"), &["floti.exe"]),
            PathBuf::from(r"C:\floti.exe")
        );
        // A folder this OS gave with `/` keeps `/`.
        assert_eq!(
            join(Path::new("/tmp/floti test/Local"), &["Programs"]),
            PathBuf::from("/tmp/floti test/Local/Programs")
        );

        assert_eq!(
            split(r"C:\Users\me\floti.exe"),
            (Some(r"C:\Users\me"), "floti.exe".to_owned())
        );
        assert_eq!(split("C:/floti.exe"), (Some("C:/"), "floti.exe".to_owned()));
        assert_eq!(split("floti.exe"), (None, "floti.exe".to_owned()));

        assert_eq!(comparable(r"C:/Users/ME\Floti\"), r"c:\users\me\floti");
        assert_eq!(comparable(r"C:\"), r"c:\");
    }

    #[test]
    fn inside_is_below_the_folder_in_any_case_and_never_beside_it() {
        let helm = Path::new(r"C:\Users\Some One\AppData\Local\Programs\Helm");
        assert!(inside(
            Path::new(r"c:/users/some one/appdata/local/programs/HELM/resources/floti.exe"),
            helm
        ));
        assert!(!inside(helm, helm));
        assert!(!inside(
            Path::new(r"C:\Users\Some One\AppData\Local\Programs\Helm Old\floti.exe"),
            helm
        ));
    }

    #[test]
    fn a_full_path_is_one_windows_or_this_os_takes_for_one() {
        for full in [r"C:\Users\me", "c:/users/me", r"\\server\share"] {
            assert!(is_absolute(Path::new(full)), "{full}");
        }
        for relative in [r"Users\me", "C:", ""] {
            assert!(!is_absolute(Path::new(relative)), "{relative}");
        }
    }
}
