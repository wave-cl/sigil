//! `MAJOR.MINOR.PATCH`, and nothing else.
//!
//! sigil's tags are `vX.Y.Z` and its manifests say `X.Y.Z`; a pre-release
//! suffix, a missing part, or a bare `v` is not a version and is refused
//! rather than read as one -- an unparsable tag must never look like `0.0.0`,
//! which every real version is newer than.

use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `0.1.6` or `v0.1.6`; anything else is `None`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.');
        let mut next = || parts.next().and_then(number);
        let (major, minor, patch) = (next()?, next()?, next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    /// The version of this build: the workspace version, which every crate
    /// shares, so any crate's `CARGO_PKG_VERSION` is sigil's.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION"))
            .expect("the workspace version is MAJOR.MINOR.PATCH")
    }

    /// `v0.1.6`: the tag a release of this version carries.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

/// Digits only: `"1"` but not `"1a"`, `"+1"` or `""`.
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_with_or_without_the_v_and_orders_numerically() {
        assert_eq!(v("0.1.6"), v("v0.1.6"));
        assert!(v("0.1.6") > v("0.1.5"));
        assert!(v("0.2.0") > v("0.1.99"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("0.1.6").to_string(), "0.1.6");
        assert_eq!(v("0.1.6").tag(), "v0.1.6");
    }

    #[test]
    fn anything_that_is_not_three_numbers_is_refused() {
        for s in [
            "",
            "v",
            "0.1",
            "0.1.6-rc1",
            "0.1.6.1",
            "a.b.c",
            "0.+1.6",
            "0..6",
            " 0.1.6",
        ] {
            assert_eq!(Version::parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn the_current_version_is_the_workspace_version() {
        assert_eq!(Version::current(), v(env!("CARGO_PKG_VERSION")));
    }
}
