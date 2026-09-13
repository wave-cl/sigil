//! What the signing tool does, as functions, so the release job's steps
//! are tested here and not only on the day.
//!
//! Three refusals, each of which would otherwise be a release that looks
//! right and installs nowhere: a manifest for a tag that is not this
//! build's version; a manifest over a set of files that is not the eight
//! a release has; and a signature from a key that is not the one the app
//! carries.

use std::path::Path;

use crate::Error;
use crate::manifest::{self, Manifest};

/// What every release publishes, less the manifest's own two files.
pub const EXPECTED_ASSETS: [&str; 8] = [
    "x86_64-linux-gnu.deb",
    "x86_64-linux-gnu.rpm",
    "x86_64-linux-gnu.tar.gz",
    "aarch64-linux-gnu.deb",
    "aarch64-linux-gnu.rpm",
    "aarch64-linux-gnu.tar.gz",
    "aarch64-apple-darwin.zip",
    "x86_64-apple-darwin.zip",
];

/// Write `out` describing the files in `dir`, refusing a tag that is not
/// `built_version`'s or a directory that is not exactly a release.
pub fn manifest(tag: &str, dir: &Path, out: &Path, built_version: &str) -> Result<Manifest, Error> {
    let want_tag = format!("v{built_version}");
    if tag != want_tag {
        return Err(Error::BadManifest(format!(
            "the tag is {tag} but this tree is version {built_version}; bump Cargo.toml before tagging"
        )));
    }
    let manifest = Manifest::build(tag, dir)?;
    let mut want: Vec<String> = EXPECTED_ASSETS
        .iter()
        .map(|suffix| format!("sigil-{tag}-{suffix}"))
        .collect();
    want.sort();
    let have: Vec<String> = manifest.assets.keys().cloned().collect();
    if have != want {
        return Err(Error::BadManifest(format!(
            "expected exactly {want:?}, found {have:?}"
        )));
    }
    std::fs::write(out, manifest.to_canonical_bytes())?;
    Ok(manifest)
}

/// Sign the exact bytes of `json` with `seed`, writing `.sig` beside it --
/// only if `seed` is the key `trusted` builds carry.
pub fn sign(json: &Path, seed: &[u8; 32], trusted: &[u8; 32]) -> Result<std::path::PathBuf, Error> {
    if &manifest::public_of(seed) != trusted {
        return Err(Error::BadSignature);
    }
    let bytes = std::fs::read(json)?;
    let sig = manifest::sign(seed, &bytes);
    let out = json.with_extension("sig");
    std::fs::write(&out, manifest::signature_file(&sig))?;
    Ok(out)
}

/// Check `json` against `.sig` beside it with `trusted`, then every file
/// it names against the file in `dir`.
pub fn verify(json: &Path, dir: &Path, trusted: &[u8; 32]) -> Result<Manifest, Error> {
    let bytes = std::fs::read(json)?;
    let sig = std::fs::read_to_string(json.with_extension("sig"))?;
    let key = manifest::verifying_key(trusted);
    let manifest = manifest::verify(&key, &bytes, &sig)?;
    for name in manifest.assets.keys() {
        manifest.check_file(name, &dir.join(name))?;
    }
    Ok(manifest)
}

/// A fresh key: the seed for the secret, and the public half as the Rust
/// the app compiles in.
pub fn keygen() -> Result<(String, String), Error> {
    let mut seed = [0u8; 32];
    // The kernel's generator, read directly: this runs once, on a
    // developer's machine, and a dependency for thirty-two bytes would be
    // a dependency for ever.
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut seed)?;
    let public = manifest::public_of(&seed);
    let rust = format!(
        "/// hex: {}\npub const PUBLIC_KEY: [u8; 32] = [\n    {},\n];",
        hex::encode(public),
        public
            .chunks(8)
            .map(|row| row
                .iter()
                .map(|b| format!("0x{b:02x}"))
                .collect::<Vec<_>>()
                .join(", "))
            .collect::<Vec<_>>()
            .join(",\n    ")
    );
    Ok((hex::encode(seed), rust))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_dir(tag: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for suffix in EXPECTED_ASSETS {
            std::fs::write(dir.path().join(format!("sigil-{tag}-{suffix}")), suffix).unwrap();
        }
        dir
    }

    #[test]
    fn the_tool_makes_signs_and_verifies_a_release() {
        let dir = release_dir("v1.2.3");
        let out = dir.path().join("sigil-v1.2.3-manifest.json");
        manifest("v1.2.3", dir.path(), &out, "1.2.3").unwrap();
        let seed = [5u8; 32];
        let public = manifest::public_of(&seed);
        let sig = sign(&out, &seed, &public).unwrap();
        assert_eq!(sig, dir.path().join("sigil-v1.2.3-manifest.sig"));
        assert_eq!(std::fs::read_to_string(&sig).unwrap().trim().len(), 128);
        let back = verify(&out, dir.path(), &public).unwrap();
        assert_eq!(back.assets.len(), 8);
        // A second manifest over the same files, with the first's own two
        // files now present, is byte for byte the same.
        let again = dir.path().join("again.json");
        manifest("v1.2.3", dir.path(), &again, "1.2.3").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&again).unwrap());
    }

    #[test]
    fn the_tool_refuses_a_tag_that_is_not_this_version() {
        let dir = release_dir("v1.2.4");
        let out = dir.path().join("m.json");
        match manifest("v1.2.4", dir.path(), &out, "1.2.3") {
            Err(Error::BadManifest(why)) => assert!(why.contains("bump Cargo.toml"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(!out.exists());
    }

    #[test]
    fn the_tool_refuses_a_release_that_is_not_eight_files() {
        let dir = release_dir("v1.2.3");
        std::fs::remove_file(dir.path().join("sigil-v1.2.3-x86_64-apple-darwin.zip")).unwrap();
        let out = dir.path().join("m.json");
        assert!(matches!(
            manifest("v1.2.3", dir.path(), &out, "1.2.3"),
            Err(Error::BadManifest(_))
        ));
        std::fs::write(dir.path().join("sigil-v1.2.3-x86_64-apple-darwin.zip"), "z").unwrap();
        std::fs::write(dir.path().join("stray.txt"), "?").unwrap();
        assert!(matches!(
            manifest("v1.2.3", dir.path(), &out, "1.2.3"),
            Err(Error::BadManifest(_))
        ));
    }

    #[test]
    fn the_tool_refuses_to_sign_with_a_key_the_build_does_not_trust() {
        let dir = release_dir("v1.2.3");
        let out = dir.path().join("sigil-v1.2.3-manifest.json");
        manifest("v1.2.3", dir.path(), &out, "1.2.3").unwrap();
        let trusted = manifest::public_of(&[5u8; 32]);
        assert!(matches!(
            sign(&out, &[6u8; 32], &trusted),
            Err(Error::BadSignature)
        ));
        assert!(!out.with_extension("sig").exists());
    }

    #[test]
    fn verify_reads_the_files_not_only_the_signature() {
        let dir = release_dir("v1.2.3");
        let out = dir.path().join("sigil-v1.2.3-manifest.json");
        manifest("v1.2.3", dir.path(), &out, "1.2.3").unwrap();
        let seed = [5u8; 32];
        let public = manifest::public_of(&seed);
        sign(&out, &seed, &public).unwrap();
        std::fs::write(
            dir.path().join("sigil-v1.2.3-aarch64-linux-gnu.rpm"),
            "changed",
        )
        .unwrap();
        assert!(matches!(
            verify(&out, dir.path(), &public),
            Err(Error::ShaMismatch(_))
        ));
    }

    #[test]
    fn keygen_prints_a_seed_the_app_could_trust() {
        let (seed_hex, rust) = keygen().unwrap();
        let seed = manifest::parse_seed(&seed_hex).unwrap();
        let public = manifest::public_of(&seed);
        assert!(
            rust.contains(&format!("hex: {}", hex::encode(public))),
            "{rust}"
        );
        assert!(
            rust.contains("pub const PUBLIC_KEY: [u8; 32] = ["),
            "{rust}"
        );
        assert_eq!(rust.matches("0x").count(), 32);
        let (other, _) = keygen().unwrap();
        assert_ne!(seed_hex, other);
    }
}
