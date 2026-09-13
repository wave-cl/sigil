//! Fetching a release's file and proving it is the one the manifest names.
//!
//! The file streams through the hash as it lands in `<name>.part`, and only
//! a digest that matches gets the file renamed to `<name>` -- so a torn or
//! wrong download never looks finished, and what the installer is handed
//! has been checked by the time it has a name.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Error;
use crate::manifest::Asset;
use crate::release::Client;

/// Where downloads land: `<local data>/sigil/updates`, beside the roster
/// and the instance lock.
pub fn staging_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("sigil")
        .join("updates")
}

/// A place on the same volume as `app`, for what has to be renamed into
/// its place: `<app's directory>/.sigil-update-<pid>`.
pub fn staging_dir_beside(app: &Path) -> PathBuf {
    let dir = app.parent().unwrap_or(Path::new("/"));
    dir.join(format!(".sigil-update-{}", std::process::id()))
}

/// Fetch `url` into `into/<name>`, checking it against `expect` on the way.
/// `progress(done, total)` is called as bytes land.
pub fn download(
    client: &Client,
    url: &str,
    name: &str,
    expect: &Asset,
    into: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(into)?;
    let part = into.join(format!("{name}.part"));
    let done = into.join(name);
    let _ = std::fs::remove_file(&part);
    let _ = std::fs::remove_file(&done);

    let mut response = client.get_big(url)?;
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(&part)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut received = 0u64;
    progress(0, expect.bytes);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Unreachable(format!("while fetching {name}: {e}")))?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > expect.bytes {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err(Error::TooLarge(name.to_owned()));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        progress(received, expect.bytes);
    }
    file.flush()?;
    drop(file);
    let digest = hex::encode(hasher.finalize());
    if received != expect.bytes || digest != expect.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(Error::ShaMismatch(name.to_owned()));
    }
    std::fs::rename(&part, &done)?;
    Ok(done)
}
