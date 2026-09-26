//! Conversations put away: out of the list, still there.
//!
//! Kept beside the roster rather than in a conversation's own store, and for
//! the same reason [`Quiet`](crate::quiet::Quiet) is: filing a conversation
//! away is a decision the person sitting at *this* machine made about their
//! own list. Nothing is sent, nobody else is told, and the other party cannot
//! tell — which is the difference between this and leaving.
//!
//! **Its own file rather than a field on `Quiet`.** That type is named for
//! what is not said out loud and is read by the wake window to decide whether
//! to make a noise; whether a conversation is on the list is not about noise
//! and is no business of a window with no list. Two small files, each about
//! one thing.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Filed {
    /// Conversations kept off the list. Keyed as
    /// [`Quiet::key`](crate::quiet::Quiet::key) keys a mute — the exchange
    /// and the channel, because a channel id is only unique within one
    /// exchange.
    #[serde(default)]
    away: BTreeSet<String>,
    /// Something changed since it was last written; the shell writes it.
    #[serde(skip)]
    changed: Cell<bool>,
}

impl Filed {
    pub fn is_filed(&self, exchange: &str, channel: &[u8; 32]) -> bool {
        self.away
            .contains(&crate::quiet::Quiet::key(exchange, channel))
    }

    pub fn set_filed(&mut self, exchange: &str, channel: &[u8; 32], away: bool) {
        let key = crate::quiet::Quiet::key(exchange, channel);
        let moved = if away {
            self.away.insert(key)
        } else {
            self.away.remove(&key)
        };
        if moved {
            self.changed.set(true);
        }
    }

    /// How many are put away, for the row that leads to them. Nothing draws
    /// that row when this is zero: a way to reach an empty list is furniture.
    pub fn count(&self) -> usize {
        self.away.len()
    }

    /// Whether a conversation is filed away **and should stay that way**.
    ///
    /// **Something waiting brings it back.** A conversation put away and then
    /// written in is not a conversation somebody is finished with, and a
    /// message nobody can find is worse than a list one row longer — this is
    /// what every messenger that has an archive does, and the one that did
    /// not lost people's mail. Filing it again is one press away.
    ///
    /// A *muted* one stays put: muting is the answer to "I do not want to
    /// hear from this again", and somebody who has said both means both.
    pub fn stays_away(
        &self,
        exchange: &str,
        channel: &[u8; 32],
        waiting: bool,
        muted: bool,
    ) -> bool {
        self.is_filed(exchange, channel) && (!waiting || muted)
    }

    /// Whether something changed since the last [`save`](Filed::save), and
    /// forget that it did.
    pub fn take_changed(&self) -> bool {
        self.changed.replace(false)
    }

    /// Where this is remembered, beside the roster and the mutes.
    pub fn remembered_at() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("sigil").join("filed.json"))
    }

    /// What this machine remembers; nothing filed away on a first run or a
    /// lost file, which is the right way to fail: a conversation that comes
    /// back is a list one row longer, and one that does not is mail nobody
    /// can find.
    pub fn load() -> Filed {
        Filed::remembered_at()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Filed::remembered_at() else {
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

    const HERE: &str = "trunk.exchange";

    #[test]
    fn filing_is_remembered_and_noticed_once() {
        let mut filed = Filed::default();
        let c = [4u8; 32];
        assert!(!filed.is_filed(HERE, &c));
        assert!(!filed.take_changed());

        filed.set_filed(HERE, &c, true);
        assert!(filed.is_filed(HERE, &c));
        assert!(filed.take_changed());
        assert!(!filed.take_changed(), "once");

        filed.set_filed(HERE, &c, true);
        assert!(!filed.take_changed(), "nothing changed");

        let text = serde_json::to_string(&filed).unwrap();
        let back: Filed = serde_json::from_str(&text).unwrap();
        assert!(back.is_filed(HERE, &c), "kept across a restart");
        assert!(!back.take_changed(), "fresh from disk is not a change");
    }

    /// **The same channel at two exchanges is two conversations.** A channel
    /// id is only unique within one exchange, so filing one away must not
    /// file the other — which is the whole reason the key carries both.
    #[test]
    fn the_exchange_is_part_of_the_key() {
        let mut filed = Filed::default();
        let c = [4u8; 32];
        filed.set_filed(HERE, &c, true);
        assert!(!filed.is_filed("elsewhere.example", &c));
        assert_eq!(filed.count(), 1);
    }

    /// **Something waiting brings it back, unless it was muted too.**
    ///
    /// A conversation put away and then written in is not one somebody is
    /// finished with. Muting is the answer to "I do not want to hear from
    /// this again", so somebody who said both meant both.
    #[test]
    fn a_message_brings_it_back_but_not_if_it_was_also_muted() {
        let mut filed = Filed::default();
        let c = [4u8; 32];
        filed.set_filed(HERE, &c, true);

        assert!(filed.stays_away(HERE, &c, false, false), "nothing waiting");
        assert!(
            !filed.stays_away(HERE, &c, true, false),
            "something arrived, so it comes back"
        );
        assert!(
            filed.stays_away(HERE, &c, true, true),
            "muted as well as filed: both were meant"
        );
        assert!(
            !filed.stays_away(HERE, &[9u8; 32], false, false),
            "one that was never filed is not away"
        );
    }
}
