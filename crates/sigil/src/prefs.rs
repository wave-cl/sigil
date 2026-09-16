//! What this machine prefers, when it is not about any one conversation or
//! identity: how a call connects, for now.
//!
//! Beside [`Quiet`](crate::quiet::Quiet) rather than inside it: a mute is
//! about what is said out loud, and this is not.

use std::cell::Cell;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    /// A direct-message call asks the exchange for a SIP-25 introduction
    /// and goes straight between the two people when it can, relayed by
    /// the exchange when it cannot. On by default; off, every call is
    /// relayed. Off is a choice worth offering because an introduction
    /// discloses this machine's address to the other person.
    #[serde(default = "yes")]
    pub direct_calls: bool,
    /// Something changed since it was last written; the shell writes it.
    #[serde(skip)]
    changed: Cell<bool>,
}

fn yes() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            direct_calls: true,
            changed: Cell::new(false),
        }
    }
}

impl Prefs {
    pub fn set_direct_calls(&mut self, on: bool) {
        if self.direct_calls != on {
            self.direct_calls = on;
            self.changed.set(true);
        }
    }

    /// Whether something changed since the last [`save`](Prefs::save), and
    /// forget that it did.
    pub fn take_changed(&self) -> bool {
        self.changed.replace(false)
    }

    /// Where this is remembered, beside the roster.
    pub fn remembered_at() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("sigil").join("prefs.json"))
    }

    /// What this machine remembers; the defaults on a first run or a lost
    /// file.
    pub fn load() -> Prefs {
        Prefs::remembered_at()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Prefs::remembered_at() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Direct calls are on until somebody turns them off, and a change is
    /// noticed once.
    #[test]
    fn direct_calls_are_on_by_default_and_a_change_is_noticed_once() {
        let mut prefs = Prefs::default();
        assert!(prefs.direct_calls);
        assert!(!prefs.take_changed());
        prefs.set_direct_calls(true);
        assert!(!prefs.take_changed(), "nothing changed");
        prefs.set_direct_calls(false);
        assert!(!prefs.direct_calls);
        assert!(prefs.take_changed());
        assert!(!prefs.take_changed(), "once");
    }

    /// What is written is what is read back; a file from before the field
    /// existed reads as the default, on.
    #[test]
    fn it_round_trips_through_json_and_an_old_file_means_on() {
        let mut prefs = Prefs::default();
        prefs.set_direct_calls(false);
        let text = serde_json::to_string(&prefs).unwrap();
        let back: Prefs = serde_json::from_str(&text).unwrap();
        assert!(!back.direct_calls);
        assert!(!back.take_changed(), "fresh from disk is not a change");
        let old: Prefs = serde_json::from_str("{}").unwrap();
        assert!(old.direct_calls);
    }
}
