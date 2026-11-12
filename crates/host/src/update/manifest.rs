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
//! before anything is downloaded: the folder `url` is `https` (or, in a
//! build with the testkit, `http` to this computer) and ends in `/`
//! ([`ReleaseFolder`]), each `file` is a plain file
//! name, so it can name nothing outside that folder or outside the folder it
//! is downloaded into, and each `sha256` is 64 hex digits.

use std::collections::BTreeMap;

use super::UpdateError;
use super::release_file::ReleaseFolder;
use super::versions::Version;
use serde::Deserialize;

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

impl FileEntry {
    /// The entry for `file` with `sha256`, when `file` is a plain file name
    /// and `sha256` is 64 hex digits; the digest is kept in lowercase.
    pub fn checked(file: &str, sha256: &str) -> Result<Self, UpdateError> {
        if !plain_file_name(file) {
            return Err(UpdateError::Manifest(format!(
                "update.json names \"{file}\", which is not a plain file name."
            )));
        }
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(UpdateError::Manifest(format!(
                "update.json gives {file} a SHA-256 that is not 64 hex digits."
            )));
        }
        Ok(Self {
            file: file.to_owned(),
            sha256: sha256.to_ascii_lowercase(),
        })
    }
}

/// One channel's release, for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub channel: String,
    /// The folder every file downloads from, and the release's version.
    pub folder: ReleaseFolder,
    /// The floti for the target, when the release has one.
    pub floti: Option<FileEntry>,
    /// The desktop app for the target, when the release has one.
    pub desktop: Option<FileEntry>,
}

impl Release {
    /// The release's version.
    pub fn version(&self) -> &Version {
        self.folder.version()
    }
}

/// The release `channel` names in the manifest `bytes`, for `target`.
pub fn release(bytes: &[u8], channel: &str, target: &str) -> Result<Release, UpdateError> {
    Manifest::read(bytes)?.release(channel, target)
}

/// A manifest, read. Its signature is checked before it is read.
#[derive(Debug)]
pub struct Manifest {
    file: ManifestFile,
}

impl Manifest {
    /// The manifest `bytes`, when this floti reads its schema.
    pub fn read(bytes: &[u8]) -> Result<Self, UpdateError> {
        let file: ManifestFile = serde_json::from_slice(bytes)
            .map_err(|err| UpdateError::Manifest(format!("update.json cannot be read: {err}.")))?;
        if file.schema != SCHEMA {
            return Err(UpdateError::Manifest(format!(
                "update.json is schema {}, which this floti does not read; install the newest floti by hand.",
                file.schema
            )));
        }
        Ok(Self { file })
    }

    /// Each channel, by name, with the version it names when that is a
    /// version.
    pub fn versions(&self) -> Vec<(String, Version)> {
        self.file
            .channels
            .iter()
            .filter_map(|(name, found)| Some((name.clone(), Version::parse(&found.version)?)))
            .collect()
    }

    /// The release `channel` names, for `target`.
    pub fn release(&self, channel: &str, target: &str) -> Result<Release, UpdateError> {
        let Some(found) = self.file.channels.get(channel) else {
            return Err(UpdateError::NoChannel {
                channel: channel.to_owned(),
                known: self.file.channels.keys().cloned().collect(),
            });
        };
        let version = Version::parse(&found.version).ok_or_else(|| {
            UpdateError::Manifest(format!(
                "update.json's {channel} channel names \"{}\", which is not a version.",
                found.version
            ))
        })?;
        let folder = ReleaseFolder::new(&found.url, version).ok_or_else(|| {
            UpdateError::Manifest(format!(
                "update.json's {channel} channel downloads from \"{}\", which is not an https folder.",
                found.url
            ))
        })?;
        let entry = |files: &BTreeMap<String, FileEntry>| {
            files
                .get(target)
                .map(|entry| FileEntry::checked(&entry.file, &entry.sha256))
                .transpose()
        };
        Ok(Release {
            channel: channel.to_owned(),
            folder,
            floti: entry(&found.floti)?,
            desktop: entry(&found.desktop)?,
        })
    }
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
        assert_eq!(release.version().to_string(), "2.6.0");
        let floti = release.floti.clone().unwrap();
        assert_eq!(floti.file, "floti-2.6.0-linux-x64");
        assert_eq!(floti.sha256, SHA.to_ascii_lowercase());
        assert_eq!(release.desktop, None);
        assert_eq!(
            release.folder.file_url(&floti.file).as_str(),
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
    fn each_channel_s_version_is_listed_and_one_that_is_not_a_version_left_out() {
        let bytes = serde_json::json!({"schema": 1, "channels": {
            "stable": good(),
            "preview": {"version": "3.0.0-preview.2", "url": "https://example.com/v/"},
            "odd": {"version": "latest", "url": "https://example.com/v/"},
        }})
        .to_string();
        let versions: Vec<(String, String)> = Manifest::read(bytes.as_bytes())
            .unwrap()
            .versions()
            .into_iter()
            .map(|(name, version)| (name, version.to_string()))
            .collect();
        assert_eq!(
            versions,
            vec![
                ("preview".to_owned(), "3.0.0-preview.2".to_owned()),
                ("stable".to_owned(), "2.6.0".to_owned()),
            ]
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
        // A test build fetches from this computer; a release build does not
        // (`network::allowed`).
        let loopback = with("url", "http://127.0.0.1:4000/v/".into());
        assert!(release(&loopback, "stable", "linux-x64").is_ok());
    }

    /// The manifest the release workflow writes for floti.dev
    /// (`packages/desktop/scripts/floti-dev.mjs`), as that script's unit test
    /// writes it for 3.0.0 and compares it with this file byte for byte.
    const RELEASE_MANIFEST: &[u8] = include_bytes!("fixtures/update.json");

    #[test]
    fn the_manifest_a_release_writes_is_read_for_every_target() {
        let folder = "https://github.com/floti-dev/floti/releases/download/v3.0.0/";
        let targets = [
            ("windows-x64", ".exe", Some("Helm-3.0.0-setup.exe")),
            ("windows-arm64", ".exe", None),
            ("macos-arm64", "", Some("Helm-3.0.0-arm64-mac.zip")),
            ("linux-x64", "", Some("Helm-3.0.0-linux-x64.AppImage")),
            ("linux-arm64", "", None),
        ];
        for (target, extension, desktop) in targets {
            let release = release(RELEASE_MANIFEST, "stable", target).unwrap();
            assert_eq!(release.channel, "stable");
            assert_eq!(release.version().to_string(), "3.0.0");
            let floti = release.floti.clone().unwrap();
            assert_eq!(floti.file, format!("floti-3.0.0-{target}{extension}"));
            assert_eq!(
                release.folder.file_url(&floti.file).as_str(),
                format!("{folder}{}", floti.file)
            );
            assert_eq!(
                release.desktop.clone().map(|entry| entry.file).as_deref(),
                desktop,
                "{target}"
            );
        }
    }
}
