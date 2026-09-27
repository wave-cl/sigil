//! What sigil was packaged into, when that is not sigil.
//!
//! **Two version numbers, and the phone showed the wrong one.** The Settings
//! screen printed `sigil 0.1.47` -- this workspace's version, the code. The
//! handset's launcher, its App info, F-Droid and `adb` all said `0.1.11`,
//! which is `sigil-android`'s: a separate repository, separate history,
//! separate tags. So the one version a person can read inside the app named
//! a release that does not exist for the thing they installed, and a report
//! quoting it sent whoever read it to the wrong repository. Asked on this
//! handset and answered with `dumpsys`, which is not a thing to ask of
//! somebody reporting a fault.
//!
//! The packager says which package it is, once, at startup. Nothing needs to
//! be plumbed from the platform: `sigil-android`'s own `CARGO_PKG_VERSION`
//! **is** the APK's `versionName` by construction -- Gradle reads the
//! version out of the same `[workspace.package]`.
//!
//! Unset on a desktop, where sigil is what was installed and one number is
//! the whole truth.

use std::sync::OnceLock;

static PACKAGE: OnceLock<String> = OnceLock::new();

/// Called once by whatever packaged sigil, before the first frame.
///
/// Later calls are ignored rather than refused: this is a label, and a
/// second packager is not a thing that happens.
pub fn packaged_as(version: &str) {
    let _ = PACKAGE.set(version.to_string());
}

/// The version line for an "about" footer: sigil's own, and the package's
/// beside it when they are not the same thing.
///
/// `sigil` is given rather than read from this crate so the line names the
/// version of the application, not of one library inside it.
pub fn version_line(sigil: &str) -> String {
    line(sigil, PACKAGE.get().map(String::as_str))
}

/// The wording, with nothing global in it.
///
/// Separate so it can be tested by value. A test that reached the `OnceLock`
/// would pass under `nextest`, which gives each test a process, and race
/// under `cargo test`, which does not -- one test setting the package would
/// decide what another one saw.
fn line(sigil: &str, package: Option<&str>) -> String {
    match package {
        Some(package) if package != sigil => format!("sigil {sigil} · build {package}"),
        _ => format!("sigil {sigil}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing packaged it, or it packaged as itself: one number, said once.
    #[test]
    fn one_number_is_said_once() {
        assert_eq!(line("0.1.47", None), "sigil 0.1.47");
        assert_eq!(line("0.1.11", Some("0.1.11")), "sigil 0.1.11");
    }

    /// A package of its own is named beside it, because that is the number
    /// the launcher, the store and `adb` all show -- and the one somebody
    /// quotes in a report.
    #[test]
    fn a_package_of_its_own_is_named_beside_it() {
        assert_eq!(
            line("0.1.47", Some("0.1.11")),
            "sigil 0.1.47 · build 0.1.11"
        );
    }

    /// And the global reaches the line, which is the only thing the
    /// `OnceLock` is for. Its own test, because it cannot be undone.
    #[test]
    fn what_the_packager_said_reaches_the_line() {
        packaged_as("9.9.9");
        assert_eq!(version_line("0.1.47"), "sigil 0.1.47 · build 9.9.9");
    }
}
