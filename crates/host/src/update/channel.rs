//! The channel `floti update` follows: `stable` until `floti update
//! --channel <name>` chooses another, which is remembered in
//! `update-channel.json` in Floti's data folder (`{"channel": "beta"}`) for
//! every later run. A file that cannot be read is `stable`.
//!
//! It is a file of its own rather than a row of `floti.db`, which only the
//! host opens, because `floti update` runs whether a host runs or not.

use std::path::Path;

use super::UpdateError;

/// The channel followed when none was chosen.
pub const DEFAULT_CHANNEL: &str = "stable";

/// The file in the data folder that remembers the chosen channel.
pub const CHANNEL_FILE: &str = "update-channel.json";

/// Whether `name` can be a channel: lowercase letters, digits and `-`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The channel remembered in `data`, or [`DEFAULT_CHANNEL`].
pub fn remembered(data: &Path) -> String {
    std::fs::read_to_string(data.join(CHANNEL_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["channel"].as_str().map(str::to_owned))
        .filter(|name| valid_name(name))
        .unwrap_or_else(|| DEFAULT_CHANNEL.to_owned())
}

/// Remembers `channel` in `data`, written whole and renamed into place.
pub fn remember(data: &Path, channel: &str) -> Result<(), UpdateError> {
    let failed = |err: std::io::Error| UpdateError::Disk {
        reason: format!(
            "could not remember the channel in {}: {err}",
            data.display()
        ),
    };
    std::fs::create_dir_all(data).map_err(failed)?;
    let file = data.join(CHANNEL_FILE);
    let partial = data.join(format!("{CHANNEL_FILE}.{}.partial", std::process::id()));
    let text = serde_json::json!({ "channel": channel }).to_string();
    std::fs::write(&partial, text).map_err(failed)?;
    std::fs::rename(&partial, &file).map_err(|err| {
        let _ = std::fs::remove_file(&partial);
        failed(err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TestDirs;

    #[test]
    fn a_chosen_channel_is_remembered_and_stable_is_the_default() {
        let dirs = TestDirs::new();
        assert_eq!(remembered(&dirs.data), "stable");
        remember(&dirs.data, "beta").unwrap();
        assert_eq!(remembered(&dirs.data), "beta");
        std::fs::write(dirs.data.join(CHANNEL_FILE), "not json").unwrap();
        assert_eq!(remembered(&dirs.data), "stable");
    }

    #[test]
    fn channel_names_are_plain() {
        assert!(valid_name("beta"));
        assert!(valid_name("nightly-2"));
        for not in ["", "Beta", "../x", "a b", "a/b"] {
            assert!(!valid_name(not), "{not:?}");
        }
    }
}
