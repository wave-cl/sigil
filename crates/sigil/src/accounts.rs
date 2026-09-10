//! Every identity sigil is holding, and which one is on screen.
//!
//! # Why several at once
//!
//! One person is often several accounts — a personal key and a work key, or one
//! per exchange, since an account's channels live at exactly one exchange and
//! nothing moves them. Holding one at a time would mean a message arriving for
//! the other is a message nobody is told about until they think to look, which
//! is the failure a desktop client exists to prevent.
//!
//! So **every unlocked account stays live**: its own connection, its own store,
//! its own lock, its own event stream -- which is what a ring arrives on, and
//! why one reaches somebody looking at another identity. [`Accounts::active`]
//! decides only what is *drawn*. Badges and notifications come from all of them
//! and say which.
//!
//! # The generation counter
//!
//! Apps cache things per identity — a chat session, a listener, a view. When
//! the roster changes they have to reconcile, and the failure when they do not
//! is invisible: a session started for the previous key keeps running, keeps
//! succeeding, and keeps being the wrong person. Nothing errors.
//!
//! [`Accounts::generation`] bumps on every change worth reconciling for.
//! An app compares it against what it last saw and rebuilds. It is a *"reconcile
//! now"* signal and deliberately not a diff: a diff has to be complete to be
//! safe, and this only has to be noticed.

use std::path::{Path, PathBuf};

use sqnr_core::PubKey;

use crate::account::Account;

/// One identity's line in the remembered roster.
///
/// Paths and exchange names. **Never a seed** — a seed on disk outside
/// `~/.sqnr` would be a second copy of somebody's identity, in a file nobody
/// chose to create and no passphrase protects.
#[derive(serde::Serialize, serde::Deserialize)]
struct Remembered {
    path: PathBuf,
    #[serde(default)]
    exchanges: Vec<String>,
}

/// One identity, and the exchanges it is connected to.
///
/// **An account, in the sense somebody means it, is a pair**: an identity *at*
/// an exchange. The identity is the same key everywhere, but its conversations,
/// its channel keys and its SIP-17 counters belong to one exchange and do not
/// move. So the two are held together here rather than the identity alone.
#[derive(Debug)]
pub struct Held {
    account: Account,
    /// Exchanges beyond the one discovery finds, as the person typed them: a
    /// domain, or `host:port`.
    ///
    /// The default is not in this list. It has no name to be in it by — it is
    /// whatever the identity's own SIP-38 handle and `~/.sqnr/config` resolve
    /// to, which is the answer for almost everybody and needs no configuring.
    extra: Vec<String>,
}

impl Held {
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Every exchange this identity should be connected to.
    ///
    /// The empty string is the default one, and is always first. It is a
    /// *name* for an exchange rather than an exchange: what it resolves to is
    /// not known until something dials it, which is why sessions are keyed on
    /// this and report the key they reached.
    pub fn exchanges(&self) -> Vec<String> {
        let mut all = vec![String::new()];
        all.extend(self.extra.iter().cloned());
        all
    }
}

/// The identities sigil is holding, and the one being shown.
#[derive(Debug)]
pub struct Accounts {
    entries: Vec<Held>,
    active: usize,
    generation: u64,
}

impl Accounts {
    /// Start from whatever is at `path`, or the default `~/.sqnr/identity`.
    pub fn discover(path: Option<PathBuf>) -> Accounts {
        Accounts::of(vec![Account::discover(path)])
    }

    /// Build from accounts already in hand. Empty is not a state: the roster
    /// always has somewhere to point, even if it points at a missing file.
    pub fn of(entries: Vec<Account>) -> Accounts {
        let entries = if entries.is_empty() {
            vec![Account::discover(None)]
        } else {
            entries
        };
        let entries = entries
            .into_iter()
            .map(|account| Held {
                account,
                extra: Vec::new(),
            })
            .collect();
        Accounts {
            entries,
            active: 0,
            generation: 0,
        }
    }

    /// Re-open a remembered roster. Paths only — a seed is never written here.
    ///
    /// A path that has since been deleted comes back as [`Account::Missing`]
    /// rather than being dropped, so somebody who moved an identity file is
    /// told where it used to be instead of finding the account silently gone.
    pub fn restore(paths: &[PathBuf]) -> Accounts {
        Accounts::of(
            paths
                .iter()
                .cloned()
                .map(|p| Account::discover(Some(p)))
                .collect(),
        )
    }

    /// What to remember. Paths and exchange names, never seeds.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .map(|h| h.account.path().to_path_buf())
            .collect()
    }

    /// What is written to disk: where each identity lives and which exchanges
    /// it is connected to.
    fn remembered(&self) -> Vec<Remembered> {
        self.entries
            .iter()
            .map(|h| Remembered {
                path: h.account.path().to_path_buf(),
                exchanges: h.extra.clone(),
            })
            .collect()
    }

    /// Bumped whenever an app should reconcile. See the module note.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        // Never true — `of` refuses an empty roster — but clippy asks, and a
        // reader should not have to go and check.
        self.entries.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &Account {
        &self.entries[self.active].account
    }

    /// The identity being shown, with its exchanges.
    pub fn active_held(&self) -> &Held {
        &self.entries[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Account {
        &mut self.entries[self.active].account
    }

    pub fn get(&self, i: usize) -> Option<&Account> {
        self.entries.get(i).map(|h| &h.account)
    }

    pub fn held(&self, i: usize) -> Option<&Held> {
        self.entries.get(i)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Account> {
        self.entries.iter().map(|h| &h.account)
    }

    /// Every identity with the exchanges it is connected to.
    pub fn all(&self) -> impl Iterator<Item = &Held> {
        self.entries.iter()
    }

    /// Every account that is open for use, with its key.
    ///
    /// This is the set an app keeps sessions for — *not* just the active one.
    pub fn unlocked(&self) -> impl Iterator<Item = (PubKey, &crate::account::Unlocked)> {
        self.entries
            .iter()
            .filter_map(|h| h.account.unlocked().map(|u| (u.me(), u)))
    }

    /// Connect the identity at `i` to another exchange as well.
    ///
    /// The identity is the same key there, and nothing else is: conversations,
    /// channel keys and SIP-17 counters all belong to one exchange. This is
    /// therefore closer to adding an account than to changing a setting, and
    /// the interface should say so.
    pub fn add_exchange(&mut self, i: usize, named: &str) -> bool {
        let named = named.trim().to_string();
        let Some(held) = self.entries.get_mut(i) else {
            return false;
        };
        // The empty name is the default exchange and is always present, so it
        // cannot be added; and one already here is not added twice, which
        // would give two sessions racing for one lock.
        if named.is_empty() || held.extra.contains(&named) {
            return false;
        }
        held.extra.push(named);
        self.generation += 1;
        true
    }

    /// Stop connecting the identity at `i` to `named`.
    ///
    /// The default exchange cannot be dropped: an identity connected to
    /// nothing is not an identity anybody can use, and there would be no way
    /// back to it from the interface.
    pub fn drop_exchange(&mut self, i: usize, named: &str) -> bool {
        let Some(held) = self.entries.get_mut(i) else {
            return false;
        };
        let before = held.extra.len();
        held.extra.retain(|e| e != named);
        if held.extra.len() == before {
            return false;
        }
        self.generation += 1;
        true
    }

    /// Show a different account. No effect on which are live.
    pub fn switch_to(&mut self, i: usize) -> bool {
        if i >= self.entries.len() || i == self.active {
            return false;
        }
        self.active = i;
        self.generation += 1;
        true
    }

    /// Add an identity, or switch to it if it is already held.
    ///
    /// Adding the same path twice would otherwise give two accounts on one
    /// store, and the second would be refused its lock and sit there broken
    /// for a reason that looks like somebody else's fault.
    pub fn add(&mut self, path: PathBuf) -> usize {
        if let Some(i) = self.entries.iter().position(|h| h.account.path() == path) {
            self.switch_to(i);
            return i;
        }
        self.entries.push(Held {
            account: Account::discover(Some(path)),
            extra: Vec::new(),
        });
        self.active = self.entries.len() - 1;
        self.generation += 1;
        self.active
    }

    /// Put one away: its seed is wiped and its apps reconcile it shut.
    ///
    /// The last account is closed rather than removed — the roster always has
    /// somewhere to point.
    pub fn remove(&mut self, i: usize) -> bool {
        if i >= self.entries.len() {
            return false;
        }
        if self.entries.len() == 1 {
            self.entries[0].account.lock();
        } else {
            self.entries.remove(i);
            if self.active >= self.entries.len() {
                self.active = self.entries.len() - 1;
            } else if self.active > i {
                self.active -= 1;
            }
        }
        self.generation += 1;
        true
    }

    /// Try a passphrase on the account at `i`.
    ///
    /// Bumps the generation on success, which is what starts that identity's
    /// session — unlocking is the moment an account becomes usable, and an app
    /// that only reconciled on add would never notice it happen.
    pub fn unlock(&mut self, i: usize, passphrase: &str) -> bool {
        let Some(account) = self.entries.get_mut(i) else {
            return false;
        };
        let opened = account.account.unlock(passphrase);
        if opened {
            self.generation += 1;
        }
        opened
    }

    /// Close the account at `i` without removing it from the roster.
    pub fn lock(&mut self, i: usize) -> bool {
        let Some(account) = self.entries.get_mut(i) else {
            return false;
        };
        if !account.account.is_unlocked() {
            return false;
        }
        account.account.lock();
        self.generation += 1;
        true
    }

    /// Re-read every account's file. For after something changed on disk.
    pub fn rediscover(&mut self) {
        let paths: Vec<PathBuf> = self.paths();
        for (account, path) in self.entries.iter_mut().zip(paths) {
            if !account.account.is_unlocked() {
                account.account = Account::discover(Some(path));
            }
        }
        self.generation += 1;
    }

    /// Every identity file in `~/.sqnr`, oldest name first.
    ///
    /// # Why a scan and not a list
    ///
    /// sqnr writes identities into one directory and nothing keeps an index of
    /// them, so the directory *is* the index. `identity` is the default one
    /// and `identity-<something>` is the rest; a name with a dot in it is a
    /// sidecar (`identity.handles`) and not an identity, and `config` and
    /// `known_servers` are not either.
    ///
    /// Nothing here is opened, and nothing needs a passphrase: this answers
    /// *what is there*, which is what somebody choosing between them needs
    /// before they have typed anything.
    pub fn found() -> Vec<PathBuf> {
        let Some(dir) = sqnr::identity::default_identity_path()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    (n == "identity" || n.starts_with("identity-")) && !n.contains('.')
                })
            })
            .collect();
        // The default first, then the rest by name, so the order is the same
        // every time somebody opens the program. A directory listing is not.
        out.sort_by_key(|p| {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            (name != "identity", name)
        });
        out
    }

    /// Make `path` the account on screen, adding it to the roster if it is new.
    ///
    /// Returns its index. An identity already held is switched to rather than
    /// opened twice — a second `Account` for one key would take a second store
    /// lock against itself and be refused, which is the right refusal in the
    /// wrong place.
    pub fn use_path(&mut self, path: PathBuf) -> usize {
        if let Some(i) = self
            .entries
            .iter()
            .position(|h| h.account.path() == path.as_path())
        {
            self.switch_to(i);
            return i;
        }
        let i = self.add(path);
        self.switch_to(i);
        i
    }

    /// Where the roster is remembered, if this machine has anywhere to put it.
    pub fn remembered_at() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("sigil").join("accounts.json"))
    }

    /// Re-open the accounts this machine was last holding.
    ///
    /// Falls back to the default identity, so a first run and a lost file look
    /// the same and both work.
    pub fn load() -> Accounts {
        let Some(path) = Accounts::remembered_at() else {
            return Accounts::discover(None);
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Accounts::discover(None);
        };
        // The current shape first, then the one written before identities
        // could hold more than one exchange. A file already on somebody's
        // machine is what every upgrade actually reads, and a roster silently
        // reset to one account is a worse outcome than any of the parsing.
        if let Ok(remembered) = serde_json::from_str::<Vec<Remembered>>(&text)
            && !remembered.is_empty()
        {
            return Accounts::of_remembered(remembered);
        }
        match serde_json::from_str::<Vec<PathBuf>>(&text) {
            Ok(paths) if !paths.is_empty() => Accounts::restore(&paths),
            // Unreadable rather than absent. Say so and carry on with the
            // default: refusing to start because a convenience file is corrupt
            // would be the file deciding whether sigil runs.
            Ok(_) => Accounts::discover(None),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot read the remembered accounts");
                Accounts::discover(None)
            }
        }
    }

    fn of_remembered(remembered: Vec<Remembered>) -> Accounts {
        Accounts {
            entries: remembered
                .into_iter()
                .map(|r| Held {
                    account: Account::discover(Some(r.path)),
                    extra: r.exchanges,
                })
                .collect(),
            active: 0,
            generation: 0,
        }
    }

    /// Remember which identities to re-open. **Paths, never seeds.**
    ///
    /// A seed on disk outside `~/.sqnr` would be a second copy of somebody's
    /// identity, in a file nobody chose to create and no passphrase protects.
    pub fn save(&self) {
        let Some(path) = Accounts::remembered_at() else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(path = %parent.display(), error = %e, "cannot make the settings directory");
            return;
        }
        match serde_json::to_string_pretty(&self.remembered()) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!(path = %path.display(), error = %e, "cannot remember the accounts");
                }
            }
            Err(e) => tracing::warn!(error = %e, "cannot encode the accounts"),
        }
    }

    /// A short label for the switcher: the SIP-38 handle if one is known, else
    /// the start of the key, else the file name.
    ///
    /// Deliberately not a bare abbreviated key on its own line — the full key
    /// belongs beside it wherever there is room (SIP-21).
    pub fn label(&self, i: usize) -> String {
        let Some(account) = self.entries.get(i) else {
            return String::new();
        };
        match account.account.unlocked() {
            Some(u) => {
                let key = u.me().to_string();
                format!("{}…", &key[..key.len().min(10)])
            }
            None => Path::new(account.account.path())
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| account.account.path().display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(n: u8) -> Account {
        Account::unlocked_for_test([n; 32])
    }

    #[test]
    fn a_roster_is_never_empty() {
        let accounts = Accounts::of(vec![]);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts.active_index(), 0);
    }

    #[test]
    fn switching_changes_what_is_shown_and_bumps_the_generation() {
        let mut accounts = Accounts::of(vec![held(1), held(2)]);
        let was = accounts.generation();
        assert!(accounts.switch_to(1));
        assert_eq!(accounts.active_index(), 1);
        assert!(accounts.generation() > was, "a switch must be noticed");
        // Switching to where we already are is not a change.
        assert!(!accounts.switch_to(1));
        assert!(!accounts.switch_to(9));
    }

    #[test]
    fn every_unlocked_account_stays_live_not_just_the_active_one() {
        let accounts = Accounts::of(vec![held(1), held(2), held(3)]);
        assert_eq!(accounts.unlocked().count(), 3);
        assert_eq!(accounts.active_index(), 0);
    }

    #[test]
    fn adding_a_path_already_held_switches_rather_than_duplicating() {
        let mut accounts = Accounts::of(vec![held(1), held(2)]);
        let path = accounts.get(0).unwrap().path().to_path_buf();
        let i = accounts.add(path);
        assert_eq!(i, 0);
        assert_eq!(accounts.len(), 2, "must not hold one store twice");
        assert_eq!(accounts.active_index(), 0);
    }

    #[test]
    fn removing_keeps_the_active_index_pointing_at_the_same_account() {
        let mut accounts = Accounts::of(vec![held(1), held(2), held(3)]);
        accounts.switch_to(2);
        assert!(accounts.remove(0));
        assert_eq!(accounts.len(), 2);
        // Was showing the third; after dropping the first it is the second.
        assert_eq!(accounts.active_index(), 1);
    }

    #[test]
    fn removing_the_last_account_closes_it_rather_than_emptying_the_roster() {
        let mut accounts = Accounts::of(vec![held(1)]);
        assert!(accounts.remove(0));
        assert_eq!(accounts.len(), 1);
        assert!(!accounts.active().is_unlocked(), "its seed must be gone");
    }

    #[test]
    fn locking_an_account_bumps_the_generation_so_apps_shut_it_down() {
        let mut accounts = Accounts::of(vec![held(1), held(2)]);
        let was = accounts.generation();
        assert!(accounts.lock(0));
        assert!(accounts.generation() > was);
        assert_eq!(accounts.unlocked().count(), 1);
        // Locking one that is already shut is not a change to reconcile for.
        let now = accounts.generation();
        assert!(!accounts.lock(0));
        assert_eq!(accounts.generation(), now);
    }

    #[test]
    fn what_is_remembered_is_paths_and_only_paths() {
        let accounts = Accounts::of(vec![held(1), held(2)]);
        let paths = accounts.paths();
        assert_eq!(paths.len(), 2);
        // Nothing in a remembered roster can reconstruct a seed.
        let restored = Accounts::restore(&paths);
        assert_eq!(restored.len(), 2);
        assert!(restored.iter().all(|a| !a.is_unlocked()));
    }
}

#[cfg(test)]
mod exchanges {
    use super::*;

    fn held(n: u8) -> Account {
        Account::unlocked_for_test([n; 32])
    }

    #[test]
    fn an_identity_starts_at_one_exchange_and_it_has_no_name() {
        let accounts = Accounts::of(vec![held(1)]);
        // The default is whatever the identity's own handle and config
        // resolve to. It is in the list, and it is the empty name, because
        // there is nothing for somebody to have typed.
        assert_eq!(accounts.active_held().exchanges(), vec![String::new()]);
    }

    #[test]
    fn adding_one_makes_it_a_second_exchange_not_a_replacement() {
        let mut accounts = Accounts::of(vec![held(1)]);
        let was = accounts.generation();
        assert!(accounts.add_exchange(0, "indra.org"));
        assert_eq!(
            accounts.active_held().exchanges(),
            vec![String::new(), "indra.org".to_string()]
        );
        assert!(accounts.generation() > was, "apps have to reconcile");
    }

    #[test]
    fn the_same_exchange_is_not_added_twice() {
        // Two sessions for one pair would race for one store lock, and the
        // loser would report that somebody else is using the account.
        let mut accounts = Accounts::of(vec![held(1)]);
        assert!(accounts.add_exchange(0, "indra.org"));
        assert!(!accounts.add_exchange(0, "indra.org"));
        assert!(!accounts.add_exchange(0, "  indra.org  "));
        assert_eq!(accounts.active_held().exchanges().len(), 2);
    }

    #[test]
    fn the_default_exchange_cannot_be_dropped() {
        // An identity connected to nothing is one nothing in the interface can
        // get back to.
        let mut accounts = Accounts::of(vec![held(1)]);
        assert!(!accounts.drop_exchange(0, ""));
        assert_eq!(accounts.active_held().exchanges().len(), 1);
    }

    #[test]
    fn dropping_an_added_one_leaves_the_rest() {
        let mut accounts = Accounts::of(vec![held(1)]);
        accounts.add_exchange(0, "indra.org");
        accounts.add_exchange(0, "squic.org");
        assert!(accounts.drop_exchange(0, "indra.org"));
        assert_eq!(
            accounts.active_held().exchanges(),
            vec![String::new(), "squic.org".to_string()]
        );
    }

    #[test]
    fn a_roster_written_before_exchanges_existed_still_loads() {
        // What every upgrade actually reads. A roster silently reset to one
        // account would be a worse outcome than any parse error.
        let old = serde_json::to_string(&vec![
            std::path::PathBuf::from("/one"),
            std::path::PathBuf::from("/two"),
        ])
        .unwrap();
        let remembered: Vec<PathBuf> = serde_json::from_str(&old).unwrap();
        let accounts = Accounts::restore(&remembered);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts.active_held().exchanges(), vec![String::new()]);
    }

    #[test]
    fn what_is_written_carries_the_exchanges_and_no_seed() {
        let mut accounts = Accounts::of(vec![held(1)]);
        accounts.add_exchange(0, "indra.org");
        let written = serde_json::to_string(&accounts.remembered()).unwrap();
        assert!(written.contains("indra.org"));
        // Round-trips.
        let back: Vec<Remembered> = serde_json::from_str(&written).unwrap();
        let reopened = Accounts::of_remembered(back);
        assert_eq!(
            reopened.active_held().exchanges(),
            vec![String::new(), "indra.org".to_string()]
        );
        assert!(
            !reopened.active().is_unlocked(),
            "nothing written can reconstruct a seed"
        );
    }
}
