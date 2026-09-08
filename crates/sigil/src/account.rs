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
use zeroize::Zeroize;

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

/// The seed is wiped when an account is closed or replaced.
///
/// Switching identities is the reason this exists: sigil now holds several
/// accounts at once, and one being put away must not leave its seed sitting in
/// a freed allocation for the rest of the process. `Drop` rather than a method
/// so that every path out — switch, forget, quit, panic — gets it, including
/// the ones nobody remembered to write.
impl Drop for Unlocked {
    fn drop(&mut self) {
        self.seed.zeroize();
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

    /// The account's public key, whether or not it is open.
    ///
    /// A sealed identity still names its key in the clear — sqnr writes it
    /// beside the encrypted seed, and `read_public` reads it without the
    /// passphrase. That is what lets somebody be shown *which* identity they
    /// are about to open, by the same mark it will carry once it is open.
    ///
    /// `None` for an identity whose file is missing or unreadable, which is a
    /// state with no key to show rather than a key that failed to load.
    pub fn public(&self) -> Option<PubKey> {
        match self {
            Account::Unlocked(open) => Some(open.me()),
            Account::Locked { path, .. } => sqnr::identity::read_public(path).ok(),
            Account::Missing { .. } | Account::Broken { .. } => None,
        }
    }

    fn unlocked_from(signer: SoftwareSigner, path: PathBuf) -> Account {
        Account::Unlocked(Unlocked {
            seed: signer.seed(),
            me: PubKey::new(signer.public()),
            path,
        })
    }

    /// An open account from a fixed seed, touching no filesystem.
    ///
    /// For snapshots. `sqnr::identity::generate` mints a **random** key, and
    /// any view that shows the key in full — which the call screen does, and
    /// should, since a key is the only thing that identifies somebody — then
    /// renders differently on every run. A snapshot of that can never pass
    /// twice, and mine did not: it was written by `UPDATE_SNAPSHOTS` and never
    /// checked until CI checked it.
    ///
    /// Tests that do not draw the key should keep using a generated identity;
    /// a fixed seed is a worse default everywhere else.
    #[doc(hidden)]
    pub fn unlocked_for_test(seed: [u8; 32]) -> Account {
        let signer = SoftwareSigner::new(ed25519_dalek::SigningKey::from_bytes(&seed));
        Account::unlocked_from(signer, PathBuf::from("/dev/null/test-identity"))
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

    /// Close this account, wiping the seed and returning it to whatever state
    /// its file is in now.
    ///
    /// The counterpart `unlock` never had. Without it an account, once open,
    /// stayed open for the life of the process — fine when there was exactly
    /// one, wrong the moment somebody can switch away from it or sign out.
    ///
    /// Re-reads the file rather than assuming `Locked`: an identity can be
    /// deleted or replaced while sigil is running, and reporting it as sealed
    /// when it is gone sends somebody looking for a passphrase that will never
    /// work.
    pub fn lock(&mut self) {
        let path = self.path().to_path_buf();
        // The old `Unlocked` drops here, and its `Drop` wipes the seed.
        *self = Account::discover(Some(path));
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

    /// A sealed identity still says which identity it is.
    ///
    /// sqnr writes the public key beside the encrypted seed, so a client can
    /// show *which* account it is about to open — by the same mark it will
    /// carry once open — without asking for anything first. Without this, the
    /// only honest thing to draw for a locked account is a blank.
    #[test]
    fn a_sealed_identity_still_names_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_identity(dir.path(), Some("open sesame"));

        let sealed = Account::discover(Some(path.clone()));
        assert!(matches!(sealed, Account::Locked { .. }));
        let named = sealed.public().expect("a sealed identity names its key");

        // The same key the passphrase eventually produces, or the mark shown
        // before unlocking would be a different account's.
        let mut open = Account::discover(Some(path));
        assert!(open.unlock("open sesame"));
        assert_eq!(named, open.public().expect("an open one names it too"));
        assert_eq!(named, open.unlocked().expect("open").me());
    }

    /// There is no key to name when there is no file.
    #[test]
    fn a_missing_identity_names_no_key() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Account::discover(Some(dir.path().join("nothing-here"))).public(),
            None
        );
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
