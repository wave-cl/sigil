//! What a release says about its files, and the signature that makes it
//! worth reading.
//!
//! The manifest is a small JSON document -- the tag, the version, and for
//! every published file its size and SHA-256 -- and beside it a detached
//! Ed25519 signature over the **exact bytes of that file**. Verification
//! checks the bytes it was given, never a re-serialisation: two JSON
//! writers that agree on meaning need not agree on bytes, and a signature
//! is over bytes.
//!
//! [`Manifest::to_canonical_bytes`] is what the signing tool writes --
//! compact, keys sorted -- so that building the manifest twice from the same
//! files gives the same bytes. That is a convenience for reproducing a
//! release, not something verification depends on.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Error;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct Manifest {
    /// `v0.1.6` -- must equal the release's tag, so a manifest cannot be
    /// carried from one release to another.
    pub tag: String,
    /// `0.1.6`.
    pub version: String,
    /// By file name, sorted.
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct Asset {
    pub bytes: u64,
    /// 64 lowercase hex characters.
    pub sha256: String,
}

/// The manifest's own file names, by tag: `sigil-v0.1.6-manifest.json` and
/// `.sig` beside it.
pub fn file_names(tag: &str) -> (String, String) {
    (
        format!("sigil-{tag}-manifest.json"),
        format!("sigil-{tag}-manifest.sig"),
    )
}

impl Manifest {
    /// Describe every regular file in `dir` except the manifest's own two.
    pub fn build(tag: &str, dir: &Path) -> Result<Manifest, Error> {
        let version = crate::Version::parse(tag)
            .ok_or_else(|| Error::BadManifest(format!("{tag} is not a version tag")))?;
        let (own, sig) = file_names(tag);
        let mut assets = BTreeMap::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == own || name == sig {
                continue;
            }
            let (sha256, bytes) = sha256_hex(&entry.path())?;
            assets.insert(name, Asset { bytes, sha256 });
        }
        Ok(Manifest {
            tag: tag.to_owned(),
            version: version.to_string(),
            assets,
        })
    }

    /// Compact JSON with sorted keys: the same files give the same bytes.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a manifest serialises")
    }

    /// The file's entry, checked against the file: its size, then its digest.
    pub fn check_file(&self, name: &str, path: &Path) -> Result<(), Error> {
        let want = self
            .assets
            .get(name)
            .ok_or_else(|| Error::NoBuild(name.to_owned()))?;
        let (sha256, bytes) = sha256_hex(path)?;
        if bytes != want.bytes || sha256 != want.sha256 {
            return Err(Error::ShaMismatch(name.to_owned()));
        }
        Ok(())
    }
}

/// Sign these exact bytes with the seed; 64 bytes back.
pub fn sign(seed: &[u8; 32], bytes: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(seed).sign(bytes).to_bytes()
}

/// The signature file's contents: 128 hex characters and a newline.
pub fn signature_file(signature: &[u8; 64]) -> String {
    format!("{}\n", hex::encode(signature))
}

/// Check the signature over these exact bytes, then read them as a manifest.
///
/// The order matters: nothing is parsed until it is known to have been
/// signed, so a malformed document from a stranger is refused as unsigned
/// rather than as malformed.
pub fn verify(key: &VerifyingKey, bytes: &[u8], signature: &str) -> Result<Manifest, Error> {
    let signature = hex::decode(signature.trim()).map_err(|_| Error::BadSignature)?;
    let signature = Signature::from_slice(&signature).map_err(|_| Error::BadSignature)?;
    key.verify(bytes, &signature)
        .map_err(|_| Error::BadSignature)?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|e| Error::BadManifest(e.to_string()))?;
    for (name, asset) in &manifest.assets {
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::BadManifest(format!(
                "{name}: sha256 is not 64 hex characters"
            )));
        }
    }
    Ok(manifest)
}

/// The key the app trusts, as a verifier.
pub fn verifying_key(public: &[u8; 32]) -> VerifyingKey {
    VerifyingKey::from_bytes(public).expect("PUBLIC_KEY is a valid Ed25519 public key")
}

/// `SIGIL_UPDATE_KEY`: 64 hex characters, the 32-byte seed; surrounding
/// whitespace tolerated because secrets pasted into a form often carry a
/// newline.
pub fn parse_seed(hex64: &str) -> Result<[u8; 32], Error> {
    let bytes =
        hex::decode(hex64.trim()).map_err(|_| Error::BadManifest("the key is not hex".into()))?;
    bytes
        .try_into()
        .map_err(|_| Error::BadManifest("the key is not 32 bytes".into()))
}

/// The seed's own public key, for saying whether a secret is the key a
/// build trusts.
pub fn public_of(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// SHA-256 of a file as lowercase hex, and its length, in one pass.
pub fn sha256_hex(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex::encode(hasher.finalize()), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> [u8; 32] {
        [7u8; 32]
    }

    fn dir_of_assets() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in [
            ("sigil-v0.1.6-x86_64-linux-gnu.deb", "deb"),
            ("sigil-v0.1.6-aarch64-apple-darwin.zip", "zip bytes"),
        ] {
            std::fs::write(dir.path().join(name), body).unwrap();
        }
        dir
    }

    #[test]
    fn a_manifest_signed_with_the_key_verifies_and_names_every_file() {
        let dir = dir_of_assets();
        let manifest = Manifest::build("v0.1.6", dir.path()).unwrap();
        let bytes = manifest.to_canonical_bytes();
        let sig = signature_file(&sign(&seed(), &bytes));
        let key = verifying_key(&public_of(&seed()));
        let back = verify(&key, &bytes, &sig).unwrap();
        assert_eq!(back, manifest);
        assert_eq!(back.version, "0.1.6");
        assert_eq!(back.assets.len(), 2);
        assert_eq!(back.assets["sigil-v0.1.6-x86_64-linux-gnu.deb"].bytes, 3);
        for name in back.assets.keys() {
            back.check_file(name, &dir.path().join(name)).unwrap();
        }
    }

    #[test]
    fn a_changed_byte_or_another_key_does_not_verify() {
        let dir = dir_of_assets();
        let bytes = Manifest::build("v0.1.6", dir.path())
            .unwrap()
            .to_canonical_bytes();
        let sig = signature_file(&sign(&seed(), &bytes));
        let key = verifying_key(&public_of(&seed()));

        let mut tampered = bytes.clone();
        // "0.1.6" -> "0.1.7" somewhere in the document.
        let at = tampered.windows(5).position(|w| w == b"0.1.6").unwrap();
        tampered[at + 4] = b'7';
        assert!(matches!(
            verify(&key, &tampered, &sig),
            Err(Error::BadSignature)
        ));

        let other = verifying_key(&public_of(&[9u8; 32]));
        assert!(matches!(
            verify(&other, &bytes, &sig),
            Err(Error::BadSignature)
        ));

        assert!(matches!(
            verify(&key, &bytes, "not hex"),
            Err(Error::BadSignature)
        ));
        assert!(matches!(
            verify(&key, &bytes, "abcd"),
            Err(Error::BadSignature)
        ));
    }

    #[test]
    fn a_file_that_is_not_what_the_manifest_says_is_refused() {
        let dir = dir_of_assets();
        let manifest = Manifest::build("v0.1.6", dir.path()).unwrap();
        let deb = dir.path().join("sigil-v0.1.6-x86_64-linux-gnu.deb");
        // Same length, different bytes.
        std::fs::write(&deb, "dab").unwrap();
        assert!(matches!(
            manifest.check_file("sigil-v0.1.6-x86_64-linux-gnu.deb", &deb),
            Err(Error::ShaMismatch(_))
        ));
        // Longer.
        std::fs::write(&deb, "deb!").unwrap();
        assert!(matches!(
            manifest.check_file("sigil-v0.1.6-x86_64-linux-gnu.deb", &deb),
            Err(Error::ShaMismatch(_))
        ));
        // Not in the manifest at all.
        assert!(matches!(
            manifest.check_file("sigil-v0.1.6-nothing.rpm", &deb),
            Err(Error::NoBuild(_))
        ));
    }

    #[test]
    fn canonical_bytes_are_compact_sorted_and_reproducible() {
        let dir = dir_of_assets();
        let a = Manifest::build("v0.1.6", dir.path())
            .unwrap()
            .to_canonical_bytes();
        let b = Manifest::build("v0.1.6", dir.path())
            .unwrap()
            .to_canonical_bytes();
        assert_eq!(a, b);
        let text = String::from_utf8(a).unwrap();
        assert!(!text.contains('\n') && !text.contains(": "), "{text}");
        let zip = text.find("aarch64-apple-darwin.zip").unwrap();
        let deb = text.find("x86_64-linux-gnu.deb").unwrap();
        assert!(zip < deb, "keys are sorted: {text}");
        assert!(
            text.starts_with(r#"{"tag":"v0.1.6","version":"0.1.6","assets":{"#),
            "{text}"
        );
    }

    #[test]
    fn the_manifest_leaves_its_own_files_out_and_refuses_a_bad_tag() {
        let dir = dir_of_assets();
        std::fs::write(dir.path().join("sigil-v0.1.6-manifest.json"), "{}").unwrap();
        std::fs::write(dir.path().join("sigil-v0.1.6-manifest.sig"), "00").unwrap();
        let manifest = Manifest::build("v0.1.6", dir.path()).unwrap();
        assert_eq!(manifest.assets.len(), 2);
        assert!(matches!(
            Manifest::build("v0.1.6-rc1", dir.path()),
            Err(Error::BadManifest(_))
        ));
    }

    #[test]
    fn a_seed_is_sixty_four_hex_characters() {
        let hex = hex::encode(seed());
        assert_eq!(parse_seed(&hex).unwrap(), seed());
        assert_eq!(parse_seed(&format!("  {hex}\n")).unwrap(), seed());
        assert!(parse_seed(&hex[..63]).is_err());
        assert!(parse_seed(&format!("{}zz", &hex[..62])).is_err());
        assert!(parse_seed("BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=").is_err());
    }

    #[test]
    fn a_manifest_with_a_malformed_digest_is_refused_even_when_signed() {
        let bytes =
            br#"{"tag":"v0.1.6","version":"0.1.6","assets":{"a":{"bytes":1,"sha256":"abc"}}}"#;
        let sig = signature_file(&sign(&seed(), bytes));
        let key = verifying_key(&public_of(&seed()));
        assert!(matches!(
            verify(&key, bytes, &sig),
            Err(Error::BadManifest(_))
        ));
    }
}
