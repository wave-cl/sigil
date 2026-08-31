//! The identity sigil acts as, and getting it unlocked without blocking.
//!
//! # Why this is not just a call to `identity::load`
//!
//! An identity file is usually sealed with a passphrase, and the CLI asks for
//! one with `rpassword::prompt_password` — a synchronous read from the
//! terminal. A window has no terminal, and a frame that blocks on stdin is a
//! frame that never gets drawn.
//!
//! So unlocking is a **state machine** the interface drives: it reads the file,
//! reports what it found, and waits to be handed a passphrase. Nothing here
//! prompts, sleeps, or blocks.
//!
//! # Software identities only
//!
//! Voice and chat both act *as* an identity on the transport (SIP-3), and a
//! YubiKey signs but never releases a seed, so it cannot be a transport key.
//! Chat additionally derives its store key from the seed. A card is therefore
//! not a harder case to support — it is one the protocol excludes, and saying
//! so plainly at the first screen is kinder than a failure four steps later.

use std::path::{Path, PathBuf};

use sqnr_core::{PubKey, Signer, SoftwareSigner};

/// Where the identity has got to.
#[derive(Debug)]
pub enum Account {
    /// Nothing at that path yet.
    Missing { path: PathBuf },
    /// A sealed identity, waiting for a passphrase.
    ///
    /// `trouble` carries the last failed attempt, so the interface can say
    /// "that passphrase did not open it" rather than silently clearing the
    /// field and looking broken.
    Locked {
        path: PathBuf,
        trouble: Option<String>,
    },
    /// Open, and this is who we are.
    Unlocked(Unlocked),
    /// The file could not be read or made sense of at all.
    Broken { path: PathBuf, trouble: String },
}

/// An identity that is open for use.
///
/// Holds the seed rather than a signer, because a signer cannot be cloned and
/// every call wants one of its own. Minting one is a key expansion and costs
/// nothing worth caring about.
pub struct Unlocked {
    seed: [u8; 32],
    me: PubKey,
    path: PathBuf,
}

impl std::fmt::Debug for Unlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the seed. A Debug print that leaks a secret is a secret leaked
        // into every log that ever captured it.
        f.debug_struct("Unlocked")
            .field("me", &self.me)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Unlocked {
    /// Who we are, in full. Shown somewhere reachable rather than abbreviated
    /// away: a name is an assertion, a key is not (SIP-21).
    pub fn me(&self) -> PubKey {
        self.me
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A signer for one call. Each takes ownership of its own.
    pub fn signer(&self) -> SoftwareSigner {
        SoftwareSigner::new(ed25519_dalek::SigningKey::from_bytes(&self.seed))
    }
}

impl Account {
    /// Look at `path`, or the default `~/.sqnr/identity`, and report what is
    /// there. Opens nothing that needs a passphrase.
    pub fn discover(path: Option<PathBuf>) -> Account {
        let path = match path.or_else(|| sqnr::identity::default_identity_path().ok()) {
            Some(p) => p,
            None => {
                return Account::Broken {
                    path: PathBuf::from("~/.sqnr/identity"),
                    trouble: "cannot work out where identities live".into(),
                };
            }
        };
        if !path.exists() {
            return Account::Missing { path };
        }
        match sqnr::identity::is_encrypted(&path) {
            Ok(true) => Account::Locked {
                path,
                trouble: None,
            },
            // Unsealed: nothing to ask for, so open it now.
            Ok(false) => match sqnr::identity::load(&path, None) {
                Ok(signer) => Account::unlocked_from(signer, path),
                Err(trouble) => Account::Broken { path, trouble },
            },
            Err(trouble) => Account::Broken { path, trouble },
        }
    }

    fn unlocked_from(signer: SoftwareSigner, path: PathBuf) -> Account {
        Account::Unlocked(Unlocked {
            seed: signer.seed(),
            me: PubKey::new(signer.public()),
            path,
        })
    }

    /// Try `passphrase`. On failure the account stays locked and remembers why.
    ///
    /// Returns whether it opened, so the interface can clear the field on
    /// success and leave it alone on failure — retyping a long passphrase
    /// because the program threw it away is its own small cruelty.
    pub fn unlock(&mut self, passphrase: &str) -> bool {
        let path = match self {
            Account::Locked { path, .. } => path.clone(),
            _ => return self.is_unlocked(),
        };
        match sqnr::identity::load(&path, Some(passphrase)) {
            Ok(signer) => {
                *self = Account::unlocked_from(signer, path);
                true
            }
            Err(trouble) => {
                *self = Account::Locked {
                    path,
                    trouble: Some(trouble),
                };
                false
            }
        }
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(self, Account::Unlocked(_))
    }

    pub fn unlocked(&self) -> Option<&Unlocked> {
        match self {
            Account::Unlocked(u) => Some(u),
            _ => None,
        }
    }

    /// The path this account is about, whatever state it is in.
    pub fn path(&self) -> &Path {
        match self {
            Account::Missing { path }
            | Account::Locked { path, .. }
            | Account::Broken { path, .. } => path,
            Account::Unlocked(u) => &u.path,
        }
    }

    /// What to tell somebody looking at this for the first time.
    pub fn describe(&self) -> String {
        match self {
            Account::Missing { path } => format!(
                "No identity at {}. Run `sqnr keygen` to make one.",
                path.display()
            ),
            Account::Locked { trouble: None, .. } => {
                "This identity is sealed. Enter its passphrase.".into()
            }
            Account::Locked {
                trouble: Some(t), ..
            } => {
                format!("That did not open it: {t}")
            }
            Account::Unlocked(u) => format!("You are {}", u.me()),
            Account::Broken { path, trouble } => {
                format!("Cannot use {}: {trouble}", path.display())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real identity, written by sqnr's own `generate`.
    ///
    /// Hand-rolling the file format here would test a guess at it; this tests
    /// the format sqnr actually writes, and lets the sealed case use a genuine
    /// passphrase rather than a placeholder that only `is_encrypted` believes.
    fn write_identity(dir: &Path, passphrase: Option<&str>) -> PathBuf {
        let path = dir.join("identity");
        sqnr::identity::generate(&path, passphrase).expect("generate an identity");
        path
    }

    #[test]
    fn a_missing_file_says_how_to_make_one() {
        let dir = tempfile::tempdir().unwrap();
        let account = Account::discover(Some(dir.path().join("nothing-here")));
        assert!(matches!(account, Account::Missing { .. }));
        assert!(
            account.describe().contains("sqnr keygen"),
            "it says what to do: {}",
            account.describe()
        );
        assert!(!account.is_unlocked());
    }

    #[test]
    fn discovering_never_blocks_on_a_sealed_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), Some("open sesame"));
        let account = Account::discover(Some(path));
        // The point: it came back at all, with something to show, rather than
        // sitting on a terminal read that a window cannot answer.
        assert!(matches!(account, Account::Locked { trouble: None, .. }));
        assert!(account.describe().contains("passphrase"));
    }

    #[test]
    fn a_wrong_passphrase_leaves_it_locked_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), Some("open sesame"));
        let mut account = Account::discover(Some(path));
        assert!(!account.unlock("not the passphrase"));
        match &account {
            Account::Locked {
                trouble: Some(_), ..
            } => {}
            other => panic!("should still be locked, with a reason: {other:?}"),
        }
        assert!(
            account.describe().contains("did not open it"),
            "{}",
            account.describe()
        );

        // And the right one still works afterwards: a failed attempt must not
        // leave the account in a state that refuses the correct passphrase.
        assert!(account.unlock("open sesame"), "{}", account.describe());
        assert!(account.is_unlocked());
    }

    #[test]
    fn unlocking_something_already_open_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), None);
        let mut account = Account::discover(Some(path));
        assert!(account.is_unlocked(), "{}", account.describe());
        assert!(account.unlock("irrelevant"), "already open stays open");
    }

    /// Every call needs a signer of its own, and `SoftwareSigner` is not
    /// `Clone`. Minting from the seed is how that works, so it had better be
    /// the same identity every time.
    #[test]
    fn each_minted_signer_is_the_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), None);
        let account = Account::discover(Some(path));
        let u = account.unlocked().expect("plain identity opens");
        assert_eq!(PubKey::new(u.signer().public()), u.me());
        assert_eq!(u.signer().seed(), u.signer().seed());
    }

    /// A Debug print that leaks a seed leaks it into every log that captured
    /// it. This is cheap to assert and expensive to discover.
    #[test]
    fn debug_never_prints_the_seed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), None);
        let account = Account::discover(Some(path));
        let seed = account.unlocked().unwrap().signer().seed();
        let printed = format!("{account:?}");
        let seed_b58 = bs58::encode(seed).into_string();
        assert!(
            !printed.contains(&seed_b58),
            "seed leaked into Debug: {printed}"
        );
        assert!(
            !printed.contains(&hex::encode(seed)),
            "seed leaked into Debug: {printed}"
        );
        assert!(!printed.to_lowercase().contains("seed"), "{printed}");
    }
}
