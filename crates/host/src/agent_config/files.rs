//! A file as the host's readers and its snapshot-first writer see it: its
//! bytes and their hash, the guard a surface writes behind, and the key a
//! file's snapshots are kept under.

use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use floti_protocol::channels::workspace::{ConfigFileContent, Millis};

use crate::sha256;

/// `path` made absolute and with `.` and `..` folded away, without asking the
/// disk, so a path that climbs out of a scope is seen to before anything
/// opens it.
pub fn normalize(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never above the root: `/..` is `/`.
                if !matches!(
                    out.components().next_back(),
                    None | Some(Component::RootDir | Component::Prefix(_))
                ) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `path`'s components below `base`, or `None` when it is not under it.
fn below(base: &Path, path: &Path) -> Option<Vec<String>> {
    path.strip_prefix(base).ok().map(|rest| {
        rest.components()
            .filter_map(|part| match part {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    })
}

/// The time a file was last written, in epoch ms; zero when the OS cannot
/// say.
pub fn mtime_ms(meta: &std::fs::Metadata) -> Millis {
    Millis(
        meta.modified()
            .ok()
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map_or(0.0, |at| at.as_secs_f64() * 1_000.0),
    )
}

/// The hash a write of `content` would leave on disk.
pub fn hash_content(content: &str) -> String {
    sha256::hex_digest(content.as_bytes())
}

/// A file as it is now. The hash is over the bytes on disk rather than over
/// decoded text, so a CRLF file an editor turned LF is a different file, as
/// a conflict check needs it to be. Bytes that hold a NUL or are not UTF-8
/// are binary: their content is not sent, and no write replaces them.
pub fn read_file(path: &Path) -> ConfigFileContent {
    let path = normalize(path);
    let shown = path.display().to_string();
    let Ok(meta) = std::fs::metadata(&path) else {
        return ConfigFileContent {
            path: shown,
            exists: false,
            content: String::new(),
            hash: String::new(),
            size: 0,
            mtime_ms: Millis(0.0),
            binary: false,
        };
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return ConfigFileContent {
            path: shown,
            exists: true,
            content: String::new(),
            hash: String::new(),
            size: meta.len(),
            mtime_ms: mtime_ms(&meta),
            binary: true,
        };
    };
    let hash = sha256::hex_digest(&bytes);
    let text = if bytes.contains(&0) {
        None
    } else {
        String::from_utf8(bytes).ok()
    };
    ConfigFileContent {
        path: shown,
        exists: true,
        binary: text.is_none(),
        content: text.unwrap_or_default(),
        hash,
        size: meta.len(),
        mtime_ms: mtime_ms(&meta),
    }
}

/// Which paths one surface may write through the
/// [`SnapshotWriter`](super::SnapshotWriter): the writer's snapshot, conflict
/// and text rules are the same for every surface, and the guard is the one
/// thing each decides for itself.
pub trait PathGuard: Send + Sync {
    /// Refuses, in a sentence, a write of `path` under `scope`.
    fn check(&self, scope: &Path, path: &Path) -> Result<(), String>;
}

/// The key a file's snapshots are kept under: its path below the scope,
/// forward-slashed.
pub fn snapshot_key(scope: &Path, path: &Path) -> String {
    let scope = normalize(scope);
    let path = normalize(path);
    match below(&scope, &path) {
        Some(segments) => segments.join("/"),
        None => path.display().to_string().replace('\\', "/"),
    }
}
