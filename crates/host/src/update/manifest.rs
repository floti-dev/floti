//! `update.json`: what each channel's newest release is and where its files
//! are.
//!
//! ```json
//! {"schema": 1,
//!  "channels": {"stable": {"version": "2.6.0",
//!                          "url": "https://github.com/floti-dev/floti/releases/download/v2.6.0/",
//!                          "floti": {"windows-x64": {"file": "floti-2.6.0-windows-x64.exe", "sha256": "..."}},
//!                          "desktop": {"windows-x64": {"file": "Helm-2.6.0-setup.exe", "sha256": "..."}}}}}
//! ```
//!
//! Targets are named as floti's release files name them: `windows-x64`,
//! `windows-arm64`, `macos-arm64`, `linux-x64`, `linux-arm64`. A field this
//! floti does not know is ignored, so a later manifest can carry more. A
//! schema other than 1 is refused: it is a manifest this floti cannot read.
//!
//! Everything a file's name and address are made from is checked here,
//! before anything is downloaded: the folder `url` is `https` (or `http` to
//! this computer, for tests) and ends in `/`, each `file` is a plain file
//! name, so it can name nothing outside that folder or outside the folder it
//! is downloaded into, and each `sha256` is 64 hex digits.

use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use super::UpdateError;
use super::network::allowed_url;
use super::versions::Version;

/// The schema this floti reads.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Deserialize)]
struct ManifestFile {
    schema: u32,
    #[serde(default)]
    channels: BTreeMap<String, ReleaseFile>,
}

#[derive(Debug, Deserialize)]
struct ReleaseFile {
    version: String,
    url: String,
    #[serde(default)]
    floti: BTreeMap<String, FileEntry>,
    #[serde(default)]
    desktop: BTreeMap<String, FileEntry>,
}

/// One file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileEntry {
    /// Its name in the release's folder.
    pub file: String,
    /// Its SHA-256, as lowercase hex.
    pub sha256: String,
}

/// One channel's release, for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub channel: String,
    pub version: Version,
    /// The folder every file downloads from, ending in `/`.
    pub url: Url,
    /// The floti for the target, when the release has one.
    pub floti: Option<FileEntry>,
    /// The desktop app for the target, when the release has one.
    pub desktop: Option<FileEntry>,
}

impl Release {
    /// Where `file` downloads from: the release's folder and its name.
    pub fn file_url(&self, file: &str) -> Url {
        // Checked a plain file name, so it can only land in the folder.
        self.url
            .join(file)
            .expect("a plain file name joins onto the release's folder")
    }
}

/// The release `channel` names in the manifest `bytes`, for `target`.
pub fn release(bytes: &[u8], channel: &str, target: &str) -> Result<Release, UpdateError> {
    let manifest: ManifestFile = serde_json::from_slice(bytes)
        .map_err(|err| UpdateError::Manifest(format!("update.json cannot be read: {err}")))?;
    if manifest.schema != SCHEMA {
        return Err(UpdateError::Manifest(format!(
            "update.json is schema {}, which this floti does not read; install the newest floti by hand.",
            manifest.schema
        )));
    }
    let Some(found) = manifest.channels.get(channel) else {
        return Err(UpdateError::NoChannel {
            channel: channel.to_owned(),
            known: manifest.channels.keys().cloned().collect(),
        });
    };
    let version = Version::parse(&found.version).ok_or_else(|| {
        UpdateError::Manifest(format!(
            "update.json's {channel} channel names \"{}\", which is not a version.",
            found.version
        ))
    })?;
    let url = Url::parse(&found.url)
        .ok()
        .filter(|url| url.path().ends_with('/') && allowed_url(url) && url.query().is_none())
        .ok_or_else(|| {
            UpdateError::Manifest(format!(
                "update.json's {channel} channel downloads from \"{}\", which is not an https folder.",
                found.url
            ))
        })?;
    let entry = |files: &BTreeMap<String, FileEntry>| -> Result<Option<FileEntry>, UpdateError> {
        let Some(entry) = files.get(target) else {
            return Ok(None);
        };
        if !plain_file_name(&entry.file) {
            return Err(UpdateError::Manifest(format!(
                "update.json names \"{}\", which is not a plain file name.",
                entry.file
            )));
        }
        if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(UpdateError::Manifest(format!(
                "update.json gives {} a SHA-256 that is not 64 hex digits.",
                entry.file
            )));
        }
        Ok(Some(FileEntry {
            file: entry.file.clone(),
            sha256: entry.sha256.to_ascii_lowercase(),
        }))
    };
    Ok(Release {
        channel: channel.to_owned(),
        version,
        url,
        floti: entry(&found.floti)?,
        desktop: entry(&found.desktop)?,
    })
}

/// A file name of letters, digits, `.`, `-`, `_` and `+`, not starting
/// with `.`: nothing a path or a URL reads as anything but the one name.
fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "AB00000000000000000000000000000000000000000000000000000000000000";

    fn manifest(release: serde_json::Value) -> Vec<u8> {
        serde_json::json!({"schema": 1, "channels": {"stable": release}, "later": true})
            .to_string()
            .into_bytes()
    }

    fn good() -> serde_json::Value {
        serde_json::json!({
            "version": "2.6.0",
            "url": "https://example.com/download/v2.6.0/",
            "notes": "ignored",
            "floti": {"linux-x64": {"file": "floti-2.6.0-linux-x64", "sha256": SHA, "size": 1}},
            "desktop": {"windows-x64": {"file": "Helm-2.6.0-setup.exe", "sha256": SHA}}
        })
    }

    #[test]
    fn a_channel_s_release_for_a_target_is_read_and_unknown_fields_are_ignored() {
        let release = release(&manifest(good()), "stable", "linux-x64").unwrap();
        assert_eq!(release.version.to_string(), "2.6.0");
        let floti = release.floti.clone().unwrap();
        assert_eq!(floti.file, "floti-2.6.0-linux-x64");
        assert_eq!(floti.sha256, SHA.to_ascii_lowercase());
        assert_eq!(release.desktop, None);
        assert_eq!(
            release.file_url(&floti.file).as_str(),
            "https://example.com/download/v2.6.0/floti-2.6.0-linux-x64"
        );
    }

    #[test]
    fn a_missing_channel_is_an_error_naming_it_and_the_ones_there_are() {
        let err = release(&manifest(good()), "beta", "linux-x64").unwrap_err();
        assert!(
            matches!(&err, UpdateError::NoChannel { channel, known } if channel == "beta" && known == &["stable"])
        );
        assert_eq!(
            err.to_string(),
            "update.json has no channel \"beta\"; it has stable."
        );
    }

    #[test]
    fn another_schema_is_refused() {
        let bytes = serde_json::json!({"schema": 2, "channels": {}}).to_string();
        let err = release(bytes.as_bytes(), "stable", "linux-x64").unwrap_err();
        assert!(err.to_string().contains("schema 2"), "{err}");
    }

    #[test]
    fn names_addresses_and_hashes_that_could_reach_elsewhere_are_refused() {
        let with = |key: &str, value: serde_json::Value| {
            let mut release = good();
            release[key] = value;
            manifest(release)
        };
        let bad = [
            with("url", "http://example.com/download/".into()),
            with("url", "https://example.com/download".into()),
            with("url", "file:///etc/".into()),
            with("version", "latest".into()),
            with(
                "floti",
                serde_json::json!({"linux-x64": {"file": "../floti", "sha256": SHA}}),
            ),
            with(
                "floti",
                serde_json::json!({"linux-x64": {"file": "a/b", "sha256": SHA}}),
            ),
            with(
                "floti",
                serde_json::json!({"linux-x64": {"file": "floti?x=1", "sha256": SHA}}),
            ),
            with(
                "floti",
                serde_json::json!({"linux-x64": {"file": "floti", "sha256": "abc"}}),
            ),
        ];
        for bytes in bad {
            let err = release(&bytes, "stable", "linux-x64").unwrap_err();
            assert!(
                matches!(err, UpdateError::Manifest(_)),
                "{}: {err}",
                String::from_utf8_lossy(&bytes)
            );
        }
        let loopback = with("url", "http://127.0.0.1:4000/v/".into());
        assert!(release(&loopback, "stable", "linux-x64").is_ok());
    }
}
