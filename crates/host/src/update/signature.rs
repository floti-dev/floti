//! The checks every download passes before it is used: its minisign
//! signature, made with the key whose public half is built into this
//! binary (`minisign.pub` beside this file), and, for a release file, the
//! SHA-256 `update.json` gives it.
//!
//! A signature's trusted comment is signed with it, and must be exactly
//! `floti update.json` for the manifest and `floti <version> <file>` for a
//! release file, so a genuine signature cannot be moved to another file or
//! another version. Only minisign's pre-hashed signatures, its default, are
//! accepted.
//!
//! A release build has no way to swap the key: there is no switch,
//! variable or flag for it. Only a build with the testkit (the crate's own
//! tests) reads a test's key from a marker in the data folder
//! ([`TEST_UPDATE_MARKER`]), as the secret seal does.

use minisign_verify::{Error as MinisignError, PublicKey, Signature};

use super::UpdateError;
use crate::sha256::Sha256;

/// The public key releases are signed with, in minisign's two-line form.
pub const BUILT_IN_KEY: &str = include_str!("minisign.pub");

/// The trusted comment `update.json`'s signature carries.
pub const MANIFEST_COMMENT: &str = "floti update.json";

/// The trusted comment a release file's signature carries.
pub fn file_comment(version: &str, file: &str) -> String {
    format!("floti {version} {file}")
}

/// The key built into this binary.
pub fn built_in_key() -> PublicKey {
    PublicKey::decode(BUILT_IN_KEY).expect("minisign.pub holds a minisign public key")
}

/// The file in a data folder that, in a build with the testkit only, names
/// the manifest's address and the key to check with in place of the
/// built-in ones: `{"manifest": "<url>", "key": "<minisign.pub text>"}`.
#[cfg(any(test, feature = "testkit"))]
pub const TEST_UPDATE_MARKER: &str = "floti-test-update.json";

/// The test's manifest address and key, when `data` holds the marker.
#[cfg(any(test, feature = "testkit"))]
pub fn test_overrides(data: &std::path::Path) -> Option<(url::Url, PublicKey)> {
    let text = std::fs::read_to_string(data.join(TEST_UPDATE_MARKER)).ok()?;
    let marker: serde_json::Value = serde_json::from_str(&text).ok()?;
    let manifest = url::Url::parse(marker["manifest"].as_str()?).ok()?;
    let key = PublicKey::decode(marker["key"].as_str()?).ok()?;
    Some((manifest, key))
}

/// `text` as a signature of `file`, with exactly `comment` as its trusted
/// comment. The comment is proved genuine when the signature is verified.
fn decode(text: &[u8], file: &str, comment: &str) -> Result<Signature, UpdateError> {
    let signature = std::str::from_utf8(text)
        .ok()
        .and_then(|text| Signature::decode(text).ok())
        .ok_or_else(|| UpdateError::BadSignature {
            file: file.to_owned(),
        })?;
    if signature.trusted_comment() != comment {
        return Err(UpdateError::WrongComment {
            file: file.to_owned(),
            found: signature.trusted_comment().to_owned(),
            wanted: comment.to_owned(),
        });
    }
    Ok(signature)
}

fn refusal(err: &MinisignError, file: &str) -> UpdateError {
    match err {
        MinisignError::UnexpectedKeyId => UpdateError::WrongKey {
            file: file.to_owned(),
        },
        _ => UpdateError::BadSignature {
            file: file.to_owned(),
        },
    }
}

/// Checks `bytes`, all of `file`, against its signature `signature`.
pub fn check_whole(
    key: &PublicKey,
    bytes: &[u8],
    signature: &[u8],
    file: &str,
    comment: &str,
) -> Result<(), UpdateError> {
    let signature = decode(signature, file, comment)?;
    key.verify(bytes, &signature, false)
        .map_err(|err| refusal(&err, file))
}

/// The checks of one release file, made once it is on disk.
pub struct FileCheck {
    signature: Signature,
    file: String,
}

impl FileCheck {
    /// The checks of `file`, signed by `signature` with `comment`. A
    /// signature made with another key is refused here, before anything is
    /// downloaded.
    pub fn new(
        key: &PublicKey,
        signature: &[u8],
        file: &str,
        comment: &str,
    ) -> Result<Self, UpdateError> {
        let signature = decode(signature, file, comment)?;
        key.verify_stream(&signature)
            .map_err(|err| refusal(&err, file))?;
        Ok(Self {
            signature,
            file: file.to_owned(),
        })
    }

    /// Checks the file at `path` against its signature, then against
    /// `sha256`. Both are taken over the file as it is on disk, so what is
    /// checked is what will be used.
    pub fn finish(
        self,
        key: &PublicKey,
        path: &std::path::Path,
        sha256: &str,
    ) -> Result<(), UpdateError> {
        let unreadable = |err: std::io::Error| UpdateError::Disk {
            reason: format!("could not read {} back: {err}", path.display()),
        };
        let mut verifier = key
            .verify_stream(&self.signature)
            .map_err(|err| refusal(&err, &self.file))?;
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(path).map_err(unreadable)?;
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer).map_err(unreadable)?;
            if read == 0 {
                break;
            }
            verifier.update(&buffer[..read]);
            hasher.update(&buffer[..read]);
        }
        verifier
            .finalize()
            .map_err(|err| refusal(&err, &self.file))?;
        if hasher.finish_hex() != sha256 {
            return Err(UpdateError::WrongSha256 { file: self.file });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_key_is_a_minisign_public_key() {
        let _ = built_in_key();
        assert_eq!(BUILT_IN_KEY.lines().count(), 2, "{BUILT_IN_KEY}");
        assert!(BUILT_IN_KEY.starts_with("untrusted comment: "));
    }
}
