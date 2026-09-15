//! What is not to be said out loud: do-not-disturb, and the conversations
//! that are muted.
//!
//! Kept beside the roster rather than in a conversation's own store: a
//! muted conversation is muted on this machine, by the person sitting at
//! it, whatever identity it belongs to -- and do-not-disturb is not about
//! any conversation at all.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Quiet {
    /// Nothing is said out loud, and nothing asks for attention. The counts
    /// stay: what is waiting is still waiting.
    #[serde(default)]
    pub dnd: bool,
    /// Conversations nothing is said about, and whose waiting messages are
    /// not counted on the icon -- their own row still says. Keyed by
    /// [`Quiet::key`].
    #[serde(default)]
    muted: BTreeSet<String>,
    /// Something changed since it was last written; the shell writes it.
    #[serde(skip)]
    changed: Cell<bool>,
}

impl Quiet {
    /// A conversation's key: the exchange it is at and its channel, since a
    /// channel id is only unique within one exchange.
    pub fn key(exchange: &str, channel: &[u8; 32]) -> String {
        format!("{exchange}:{}", bs58::encode(channel).into_string())
    }

    pub fn is_muted(&self, exchange: &str, channel: &[u8; 32]) -> bool {
        self.muted.contains(&Quiet::key(exchange, channel))
    }

    pub fn set_muted(&mut self, exchange: &str, channel: &[u8; 32], muted: bool) {
        let key = Quiet::key(exchange, channel);
        let moved = if muted {
            self.muted.insert(key)
        } else {
            self.muted.remove(&key)
        };
        if moved {
            self.changed.set(true);
        }
    }

    pub fn set_dnd(&mut self, dnd: bool) {
        if self.dnd != dnd {
            self.dnd = dnd;
            self.changed.set(true);
        }
    }

    /// Whether a conversation is to be left alone: muted, or everything is.
    pub fn silenced(&self, exchange: &str, channel: &[u8; 32]) -> bool {
        self.dnd || self.is_muted(exchange, channel)
    }

    /// Whether something changed since the last [`save`](Quiet::save), and
    /// forget that it did.
    pub fn take_changed(&self) -> bool {
        self.changed.replace(false)
    }

    /// Where this is remembered, beside the roster.
    pub fn remembered_at() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("sigil").join("quiet.json"))
    }

    /// What this machine remembers; nothing muted on a first run or a lost
    /// file.
    pub fn load() -> Quiet {
        Quiet::remembered_at()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Quiet::remembered_at() else {
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

    /// Muting is per conversation per exchange; do-not-disturb silences
    /// everything; each change is noticed once, so it is written once.
    #[test]
    fn muting_is_per_conversation_and_dnd_is_everything() {
        let mut quiet = Quiet::default();
        assert!(!quiet.take_changed());
        quiet.set_muted("trunk.exchange", &[1u8; 32], true);
        assert!(quiet.is_muted("trunk.exchange", &[1u8; 32]));
        assert!(
            !quiet.is_muted("squic.org", &[1u8; 32]),
            "the same id elsewhere"
        );
        assert!(!quiet.is_muted("trunk.exchange", &[2u8; 32]));
        assert!(quiet.take_changed());
        assert!(!quiet.take_changed(), "once");
        quiet.set_muted("trunk.exchange", &[1u8; 32], true);
        assert!(!quiet.take_changed(), "nothing changed");

        assert!(!quiet.silenced("squic.org", &[1u8; 32]));
        quiet.set_dnd(true);
        assert!(quiet.silenced("squic.org", &[1u8; 32]));
        assert!(quiet.take_changed());
    }

    /// What is written is what is read back, and a file that is not there
    /// is nothing muted.
    #[test]
    fn it_round_trips_through_json() {
        let mut quiet = Quiet::default();
        quiet.set_muted("", &[3u8; 32], true);
        quiet.set_dnd(true);
        let text = serde_json::to_string(&quiet).unwrap();
        let back: Quiet = serde_json::from_str(&text).unwrap();
        assert!(back.dnd);
        assert!(back.is_muted("", &[3u8; 32]));
        assert!(!back.take_changed(), "fresh from disk is not a change");
        let empty: Quiet = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Quiet::default());
    }
}
