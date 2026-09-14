//! Asking GitHub what the latest release is, and fetching small things
//! from it.
//!
//! The API base is a parameter so a test can stand a listener up on
//! 127.0.0.1 and have every line of this -- the request, the parsing, the
//! asset lookup -- run against it. Nothing in the tests is a second path.

use std::io::Read;
use std::time::Duration;

use serde::Deserialize;

use crate::Error;
use crate::install::{Install, PackageKind};

/// The real one.
pub const GITHUB_API: &str = "https://api.github.com/repos/wave-cl/sigil";

/// A small fetch's ceiling: the manifest is a few hundred bytes, the API
/// answer a few kilobytes.
pub const SMALL: u64 = 64 * 1024;

pub struct Client {
    /// For the API and the manifest: everything is over in seconds.
    small: ureq::Agent,
    /// For the file itself: tens of megabytes, on whatever line this is.
    big: ureq::Agent,
    api_base: String,
}

/// What `/releases/latest` says, the parts that matter.
#[derive(Deserialize, Debug, Clone)]
pub struct Latest {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<ApiAsset>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ApiAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

impl Latest {
    pub fn asset(&self, name: &str) -> Option<&ApiAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

impl Client {
    /// Short timeouts, on purpose: on a machine where a firewall drops the
    /// packets of a program it has not been told about, a request that
    /// waits for ever looks exactly like a program that never asked.
    pub fn new(api_base: impl Into<String>) -> Client {
        let base = || {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(15)))
                .user_agent(format!(
                    "sigil/{} (+https://github.com/wave-cl/sigil)",
                    crate::Version::current()
                ))
                .http_status_as_error(false)
        };
        let small = base().timeout_global(Some(Duration::from_secs(15))).build();
        let big = base()
            .timeout_recv_body(Some(Duration::from_secs(15 * 60)))
            .build();
        Client {
            small: ureq::Agent::new_with_config(small),
            big: ureq::Agent::new_with_config(big),
            api_base: api_base.into(),
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// `GET {api_base}/releases/latest`.
    pub fn latest(&self) -> Result<Latest, Error> {
        let url = format!("{}/releases/latest", self.api_base);
        let bytes = self.fetch_small(&url)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::BadManifest(format!("releases/latest: {e}")))
    }

    /// A whole small body, or an error that says which way it went wrong.
    pub fn fetch_small(&self, url: &str) -> Result<Vec<u8>, Error> {
        let mut response = self.get(url)?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(SMALL + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        if bytes.len() as u64 > SMALL {
            return Err(Error::TooLarge(url.to_owned()));
        }
        Ok(bytes)
    }

    /// The response to a GET, once its status is known to be 200.
    pub fn get(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
        Self::get_with(&self.small, url)
    }

    /// The same, with the patience a large file needs.
    pub fn get_big(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
        Self::get_with(&self.big, url)
    }

    fn get_with(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let response = agent
            .get(url)
            .header("Accept", "application/vnd.github+json, */*")
            .call()
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::Http(status));
        }
        Ok(response)
    }
}

/// The file in a release that this install would take, by the release's
/// naming: `sigil-<tag>-<arch>-<platform>.<ext>`.
pub fn asset_name(tag: &str, install: &Install, arch: &str) -> Option<String> {
    let tail = match install {
        Install::MacBundle { .. } => "apple-darwin.zip",
        Install::LinuxBinary { .. } => "linux-gnu.tar.gz",
        Install::LinuxPackage {
            kind: PackageKind::Deb,
            ..
        } => "linux-gnu.deb",
        Install::LinuxPackage {
            kind: PackageKind::Rpm,
            ..
        } => "linux-gnu.rpm",
        Install::Unsupported { .. } => return None,
    };
    Some(format!("sigil-{tag}-{arch}-{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_asset_is_named_as_the_release_names_it() {
        let tag = "v0.1.6";
        assert_eq!(
            asset_name(
                tag,
                &Install::MacBundle {
                    app: "/a.app".into()
                },
                "aarch64"
            )
            .unwrap(),
            "sigil-v0.1.6-aarch64-apple-darwin.zip"
        );
        assert_eq!(
            asset_name(tag, &Install::LinuxBinary { exe: "/x".into() }, "x86_64").unwrap(),
            "sigil-v0.1.6-x86_64-linux-gnu.tar.gz"
        );
        assert_eq!(
            asset_name(
                tag,
                &Install::LinuxPackage {
                    kind: PackageKind::Deb,
                    exe: "/usr/bin/sigil".into()
                },
                "x86_64"
            )
            .unwrap(),
            "sigil-v0.1.6-x86_64-linux-gnu.deb"
        );
        assert_eq!(
            asset_name(
                tag,
                &Install::LinuxPackage {
                    kind: PackageKind::Rpm,
                    exe: "/usr/bin/sigil".into()
                },
                "aarch64"
            )
            .unwrap(),
            "sigil-v0.1.6-aarch64-linux-gnu.rpm"
        );
        assert_eq!(
            asset_name(tag, &Install::Unsupported { why: String::new() }, "x86_64"),
            None
        );
    }
}
