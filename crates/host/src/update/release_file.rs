//! One release's files: the folder they download from, and the download of
//! one of them, checked against its signature and its SHA-256 before it is
//! used. `floti update` fetches through it, and so does anything else that
//! installs a release, such as the desktop app's first install.
//!
//! A file `F` of a release downloads from `<folder>F`, and its signature
//! from `<folder>F.minisig`, which is fetched first: a signature that is
//! missing, unreadable, for another file or version, or made with another
//! key stops the download before the file is fetched. Once the file is on
//! disk it is checked against that signature, whose trusted comment must be
//! `floti <version> F`, and against its SHA-256, both over the bytes as they
//! lie on disk. A file that fails a check is removed.
//!
//! Every release's folder is `<releases>v<version>/` under the project's
//! GitHub releases ([`RELEASES_URL`]), so a release of any version, preview
//! or stable, is reached without the manifest ([`ReleaseFolder::of_version`]).
//! A small file, such as a release's SHA-256 list, is fetched whole and
//! checked against its signature, which is fetched first
//! ([`ReleaseFolder::fetch_whole`]).

use std::fs::File;
use std::io::Write;
use std::path::Path;

use minisign_verify::PublicKey;
use url::Url;

use super::UpdateError;
use super::manifest::FileEntry;
use super::network::{FetchError, Network, allowed_url, get_whole};
use super::signature::{FileCheck, check_whole, file_comment};
use super::versions::Version;

/// The largest signature fetched.
const SIGNATURE_LIMIT: u64 = 16 * 1024;

/// The largest release file fetched.
const FILE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Where every release's folder is, as `v<version>/` under it.
pub const RELEASES_URL: &str = "https://github.com/floti-dev/floti/releases/download/";

/// Where every release's folder is: [`RELEASES_URL`], or, in a build with
/// the testkit only, the one the marker in the data folder `data` names
/// (`releases`).
pub fn releases_url(data: &Path) -> Url {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(url) =
        super::signature::test_marker(data, "releases").and_then(|text| Url::parse(&text).ok())
    {
        return url;
    }
    let _ = data;
    Url::parse(RELEASES_URL).expect("the releases' address is a URL")
}

/// The folder a release's files download from, and the version they are of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFolder {
    url: Url,
    version: Version,
}

impl ReleaseFolder {
    /// The folder at `url`, holding `version`'s files, when `url` is one
    /// floti downloads from: `https` (or, in a build with the testkit,
    /// `http` to this computer), ending in `/`, with no query.
    pub fn new(url: &str, version: Version) -> Option<Self> {
        let url = Url::parse(url)
            .ok()
            .filter(|url| url.path().ends_with('/') && url.query().is_none() && allowed_url(url))?;
        Some(Self { url, version })
    }

    /// The folder of `version`'s release under `releases`
    /// ([`releases_url`]): `<releases>v<version>/`.
    pub fn of_version(releases: &Url, version: Version) -> Option<Self> {
        Self::new(&format!("{releases}v{version}/"), version)
    }

    /// The version this folder's files are of.
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Where `file` downloads from: the folder and its name.
    pub fn file_url(&self, file: &str) -> Url {
        // A FileEntry's name is a plain file name, so it lands in the folder.
        self.url
            .join(file)
            .expect("a plain file name joins onto the release's folder")
    }

    /// Downloads `file`, at most `limit` bytes of it, after its signature,
    /// and answers its bytes once they verify with `key` and the trusted
    /// comment `floti <version> <file>`. Not there (404) is
    /// [`UpdateError::MissingSignature`] for its signature and
    /// [`UpdateError::Network`] for the file.
    pub fn fetch_whole(
        &self,
        network: &dyn Network,
        key: &PublicKey,
        file: &str,
        limit: u64,
    ) -> Result<Vec<u8>, UpdateError> {
        let url = self.file_url(file);
        let signature = fetch_signature(network, file, &url)?;
        let comment = file_comment(&self.version.to_string(), file);
        // A signature made with another key is refused before the file is
        // fetched.
        FileCheck::new(key, &signature, file, &comment)?;
        let bytes =
            get_whole(network, &url, limit).map_err(|err| fetch_refusal(file, &url, err))?;
        check_whole(key, &bytes, &signature, file, &comment)?;
        Ok(bytes)
    }

    /// Downloads `entry` to `dest` and checks it, or removes it and says
    /// which check it failed.
    pub fn fetch(
        &self,
        network: &dyn Network,
        key: &PublicKey,
        entry: &FileEntry,
        dest: &Path,
    ) -> Result<(), UpdateError> {
        let url = self.file_url(&entry.file);
        let signature = fetch_signature(network, &entry.file, &url)?;
        let comment = file_comment(&self.version.to_string(), &entry.file);
        let check = FileCheck::new(key, &signature, &entry.file, &comment)?;
        let result = (|| {
            let unwritable = |err: std::io::Error| UpdateError::Disk {
                reason: format!("could not write {}: {err}", dest.display()),
            };
            let mut file = File::create(dest).map_err(unwritable)?;
            network
                .get(&url, FILE_LIMIT, &mut |piece| file.write_all(piece))
                .map_err(|err| fetch_refusal(&entry.file, &url, err))?;
            file.sync_all().map_err(unwritable)?;
            drop(file);
            check.finish(key, dest, &entry.sha256)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }
}

/// `url` with `.minisig` after it.
pub fn signature_url(url: &Url) -> Url {
    Url::parse(&format!("{url}.minisig")).expect("a URL with .minisig after it is a URL")
}

/// The signature of `file`, which downloads from `url`. Not there (404) is
/// [`UpdateError::MissingSignature`].
pub fn fetch_signature(
    network: &dyn Network,
    file: &str,
    url: &Url,
) -> Result<Vec<u8>, UpdateError> {
    let signature_url = signature_url(url);
    get_whole(network, &signature_url, SIGNATURE_LIMIT).map_err(|err| match err {
        FetchError::NotFound => UpdateError::MissingSignature {
            file: file.to_owned(),
        },
        other => fetch_refusal(&format!("{file}.minisig"), &signature_url, other),
    })
}

/// Why `file`, from `url`, did not download.
pub fn fetch_refusal(file: &str, url: &Url, err: FetchError) -> UpdateError {
    match err {
        FetchError::Cut(reason) => UpdateError::CutOff {
            file: file.to_owned(),
            reason,
        },
        other => UpdateError::Network {
            file: file.to_owned(),
            url: url.to_string(),
            reason: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version() -> Version {
        Version::parse("2.6.0").unwrap()
    }

    #[test]
    fn a_folder_is_https_ends_in_a_slash_and_has_no_query() {
        let folder = ReleaseFolder::new("https://example.com/download/v2.6.0/", version()).unwrap();
        assert_eq!(folder.version().to_string(), "2.6.0");
        assert_eq!(
            folder.file_url("floti-2.6.0-linux-x64").as_str(),
            "https://example.com/download/v2.6.0/floti-2.6.0-linux-x64"
        );
        for bad in [
            "http://example.com/download/",
            "https://example.com/download",
            "https://example.com/download/?x=1",
            "file:///etc/",
            "not a url",
        ] {
            assert_eq!(ReleaseFolder::new(bad, version()), None, "{bad}");
        }
    }

    #[test]
    fn a_version_s_folder_is_v_and_its_version_under_the_releases() {
        let releases = Url::parse(RELEASES_URL).unwrap();
        let folder =
            ReleaseFolder::of_version(&releases, Version::parse("3.1.0-preview.2").unwrap())
                .unwrap();
        assert_eq!(
            folder.file_url("Helm-3.1.0-preview.2-SHA256SUMS").as_str(),
            "https://github.com/floti-dev/floti/releases/download/v3.1.0-preview.2/Helm-3.1.0-preview.2-SHA256SUMS"
        );
    }

    #[test]
    fn a_signature_s_address_is_the_file_s_with_minisig_after_it() {
        let url = Url::parse("https://example.com/v/floti").unwrap();
        assert_eq!(
            signature_url(&url).as_str(),
            "https://example.com/v/floti.minisig"
        );
    }
}
