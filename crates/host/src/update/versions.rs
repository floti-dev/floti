//! Floti's versions, ordered as Semantic Versioning 2.0 orders them, so an
//! update never goes to a version older than or equal to the running one.
//! `2.5.0-preview.3` comes before `2.5.0`; build metadata (`+...`) is
//! ignored.

use std::cmp::Ordering;
use std::fmt;

/// A version: `MAJOR.MINOR.PATCH`, with an optional pre-release. Two
/// versions are equal when they order equal, build metadata aside.
#[derive(Debug, Clone)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Identifier>,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Identifier {
    Number(u64),
    Text(String),
}

impl Version {
    /// `text` as a version, or `None` when it is not one.
    pub fn parse(text: &str) -> Option<Self> {
        let without_build = text.split_once('+').map_or(text, |(version, _)| version);
        let (core, pre) = match without_build.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (without_build, None),
        };
        let mut numbers = core.split('.').map(number);
        let core = [numbers.next()??, numbers.next()??, numbers.next()??];
        if numbers.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|part| {
                    if part.is_empty()
                        || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    {
                        None
                    } else if part.chars().all(|c| c.is_ascii_digit()) {
                        number(part).map(Identifier::Number)
                    } else {
                        Some(Identifier::Text(part.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Self {
            core,
            pre,
            text: text.to_owned(),
        })
    }
}

/// A number with no leading zero, as SemVer writes them.
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    if !text.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release comes after its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (mine, theirs) in self.pre.iter().zip(&other.pre) {
                        let order = match (mine, theirs) {
                            (Identifier::Number(a), Identifier::Number(b)) => a.cmp(b),
                            (Identifier::Number(_), Identifier::Text(_)) => Ordering::Less,
                            (Identifier::Text(_), Identifier::Number(_)) => Ordering::Greater,
                            (Identifier::Text(a), Identifier::Text(b)) => a.cmp(b),
                        };
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            }
        })
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} is a version"))
    }

    #[test]
    fn versions_order_as_semver_orders_them() {
        let ascending = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.2.0",
            "2.4.1-preview.9",
            "2.4.1-preview.10",
            "2.4.1",
            "10.0.0",
        ];
        for pair in ascending.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("1.2.3+build.5").cmp(&v("1.2.3")), Ordering::Equal);
    }

    #[test]
    fn only_versions_parse() {
        for not in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "v1.2.3",
        ] {
            assert!(Version::parse(not).is_none(), "{not:?}");
        }
        assert_eq!(v("2.4.1-preview.10").to_string(), "2.4.1-preview.10");
    }
}
