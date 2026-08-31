//! One sigil at a time.
//!
//! Not a nicety. `sqex_chat` takes an exclusive `flock` on the account's store
//! for the life of a session, because two interactive clients would each keep
//! their own idea of the next message counter and reusing one costs the
//! confidentiality of two messages. A second sigil would therefore fail
//! anyway — this turns that into a clear refusal rather than a puzzling one,
//! and gives the running instance a chance to come forward instead.

use std::path::PathBuf;

/// Held for the life of the process. Dropping it releases the claim.
pub struct Instance {
    _file: std::fs::File,
    path: PathBuf,
}

impl Instance {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// Claim the right to be the running sigil, or say who already has it.
pub fn claim(dir: &std::path::Path) -> Result<Instance, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("sigil.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    #[cfg(unix)]
    {
        use std::io::{Read, Seek, Write};
        use std::os::unix::io::AsRawFd;
        // SAFETY: `file` owns the descriptor and outlives the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let mut held = String::new();
            let mut probe = &file;
            let _ = probe.read_to_string(&mut held);
            let who = held.trim();
            return Err(if who.is_empty() {
                "sigil is already running".to_string()
            } else {
                format!("sigil is already running (pid {who})")
            });
        }
        // Leave the pid behind so the next attempt can name it. Best effort:
        // failing to write it costs a less helpful message, nothing more.
        let mut w = &file;
        let _ = w.set_len(0);
        let _ = w.rewind();
        let _ = write!(w, "{}", std::process::id());
    }
    Ok(Instance { _file: file, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_is_refused_and_names_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let first = claim(dir.path()).expect("the first claim succeeds");
        let second = claim(dir.path());
        let why = second.err().expect("a second sigil must be refused");
        assert!(why.contains("already running"), "{why}");
        assert!(
            why.contains(&std::process::id().to_string()),
            "and says which process holds it: {why}"
        );
        drop(first);
    }

    #[test]
    fn releasing_lets_the_next_one_in() {
        let dir = tempfile::tempdir().unwrap();
        let first = claim(dir.path()).unwrap();
        drop(first);
        assert!(claim(dir.path()).is_ok(), "a released claim can be retaken");
    }
}
