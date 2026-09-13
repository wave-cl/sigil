//! A newer sigil, found on GitHub, proven, and put in place.
//!
//! Three things have to be true before anything on the disc changes, and
//! each has its own module: the release exists and is newer ([`release`]),
//! what it says about its files is signed by the key this build trusts
//! ([`manifest`]), and the file that arrived is the one the manifest names
//! ([`download`]). Only then does [`install`] touch anything, and it never
//! writes over a running binary -- it puts a new one beside it and renames,
//! so the process that is running keeps the inode it started with.
//!
//! The signing tool in `bin/` shares every one of these paths, which is the
//! point of it being here rather than a shell script in the workflow: what
//! the release job signs is byte for byte what the app verifies.

pub mod checker;
pub mod download;
pub mod install;
pub mod manifest;
pub mod relaunch;
pub mod release;
pub mod tool;
pub mod version;

pub use checker::{UpdateState, Updater};
pub use install::Install;
pub use version::Version;

/// The key a release must be signed by for this build to install it.
///
/// Ed25519, 32 bytes. Printed by `scripts/update-key` along with the seed
/// that goes in the `SIGIL_UPDATE_KEY` repository secret. Changing it means
/// no release signed by the old key is installed by builds carrying the
/// new one -- which is what rotation has to go through: a release signed by
/// the old key that carries the new one.
///
/// hex: 04275ff15afcdeae31579404e4f4844098521e2aedc21aa856401c6579e6ac18
pub const PUBLIC_KEY: [u8; 32] = [
    0x04, 0x27, 0x5f, 0xf1, 0x5a, 0xfc, 0xde, 0xae, 0x31, 0x57, 0x94, 0x04, 0xe4, 0xf4, 0x84, 0x40,
    0x98, 0x52, 0x1e, 0x2a, 0xed, 0xc2, 0x1a, 0xa8, 0x56, 0x40, 0x1c, 0x65, 0x79, 0xe6, 0xac, 0x18,
];

/// What can go wrong between "is there a newer one" and "it is installed",
/// in words somebody can read on the Desktop pane.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach {0}")]
    Unreachable(String),
    #[error("the server answered {0}")]
    Http(u16),
    #[error("the manifest's signature does not verify")]
    BadSignature,
    #[error("the manifest is not what was expected: {0}")]
    BadManifest(String),
    #[error("no build for {0} in this release")]
    NoBuild(String),
    #[error("{0} is not the file the manifest describes")]
    ShaMismatch(String),
    #[error("{0} is larger than the manifest says")]
    TooLarge(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Install(String),
}
