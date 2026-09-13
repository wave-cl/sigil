//! The release job's half of self-update: make the manifest, sign it, check
//! it. And `keygen`, for `scripts/update-key`.
//!
//!     sigil-update-tool keygen
//!     sigil-update-tool manifest --tag v0.1.6 --dir dist --out dist/sigil-v0.1.6-manifest.json
//!     sigil-update-tool sign dist/sigil-v0.1.6-manifest.json      # SIGIL_UPDATE_KEY in the environment
//!     sigil-update-tool verify dist/sigil-v0.1.6-manifest.json dist
//!
//! Every path here is the one the app takes: `verify` is what a sigil does
//! before it installs, with the key it was built with.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sigil_update::{PUBLIC_KEY, manifest, tool};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sigil-update-tool: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let (seed, rust) = tool::keygen().map_err(|e| e.to_string())?;
            println!("# The public half, for crates/sigil-update/src/lib.rs:\n{rust}\n");
            println!("# The secret half, for the repository secret (never commit it):");
            println!("#   gh secret set SIGIL_UPDATE_KEY --repo wave-cl/sigil");
            println!("SIGIL_UPDATE_KEY={seed}");
            Ok(())
        }
        Some("manifest") => {
            let tag = flag(args, "--tag")?;
            let dir = PathBuf::from(flag(args, "--dir")?);
            let out = PathBuf::from(flag(args, "--out")?);
            let m = tool::manifest(&tag, &dir, &out, env!("CARGO_PKG_VERSION"))
                .map_err(|e| e.to_string())?;
            println!("{}: {} files", out.display(), m.assets.len());
            Ok(())
        }
        Some("sign") => {
            let json = Path::new(args.get(1).ok_or("sign <manifest.json>")?);
            let seed = std::env::var("SIGIL_UPDATE_KEY")
                .map_err(|_| "SIGIL_UPDATE_KEY is not set".to_string())?;
            let seed = manifest::parse_seed(&seed).map_err(|e| e.to_string())?;
            let sig = tool::sign(json, &seed, &PUBLIC_KEY).map_err(|e| match e {
                sigil_update::Error::BadSignature => {
                    "SIGIL_UPDATE_KEY is not the key this build trusts; a release signed with it could never be installed".to_string()
                }
                other => other.to_string(),
            })?;
            println!("{}", sig.display());
            Ok(())
        }
        Some("verify") => {
            let json = Path::new(args.get(1).ok_or("verify <manifest.json> <dir>")?);
            let dir = Path::new(args.get(2).ok_or("verify <manifest.json> <dir>")?);
            let m = tool::verify(json, dir, &PUBLIC_KEY).map_err(|e| e.to_string())?;
            println!("{}: signed for {}, {} files match", json.display(), m.tag, m.assets.len());
            Ok(())
        }
        _ => Err("usage: sigil-update-tool keygen | manifest --tag T --dir D --out F | sign F | verify F D".into()),
    }
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| format!("{name} is required"))
}
