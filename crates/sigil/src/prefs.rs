//! What this machine prefers, when it is not about any one conversation or
//! identity: how a call connects, and how much a notification says.
//!
//! Beside [`Quiet`](crate::quiet::Quiet) rather than inside it: a mute is
//! about what is said out loud, and this is not.

use std::cell::Cell;
use std::path::PathBuf;

/// How much a notification says, on a screen anybody standing nearby can
/// read.
///
/// **SIP-47 offers exactly these three** -- sender and text, sender only,
/// the fact of a message -- and leaves the default to the client. The words
/// arrived sealed and were opened on this machine; nothing here is about
/// the wire. What a notification carries is handed to the platform, which
/// draws it on a locked screen and may pass it to a watch, so it is the one
/// piece of a message whose audience the person chooses rather than the
/// sender.
///
/// It lives in the preferences rather than beside the composing because
/// **two paths compose notifications and both must obey it**: the running
/// client's, in `sigil_chat::announce`, and a phone's wake window, which
/// composes from its store with no app around it. The setting was built for
/// the second and the first did not know about it, so a phone told to say
/// only that something had arrived said who and what for as long as the
/// process was alive -- which, with the reachable service on, is most of
/// the time.
/// Spelt `snake_case` in the file because a phone has been writing it that
/// way since SIP-47's wake window was built, in a settings file of its own.
/// That file is the one the wake window reads with no app around it; the two
/// are one setting, and a different spelling here would read a phone's saved
/// choice as unparseable and quietly hand back the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privacy {
    /// Who wrote, and what they said.
    #[default]
    SenderAndText,
    /// Who wrote, and nothing of what.
    SenderOnly,
    /// That something arrived, and nothing else: no sender, no conversation,
    /// and one notice for everything rather than one per conversation --
    /// which would say how many are busy.
    FactOnly,
}

impl Privacy {
    /// Every choice, in the order a settings row offers them: most said
    /// first, because that is the default and a list reads from the top.
    pub const ALL: [Privacy; 3] = [
        Privacy::SenderAndText,
        Privacy::SenderOnly,
        Privacy::FactOnly,
    ];

    /// The state in two or three words, for a row that names it.
    ///
    /// **Shorter than [`describe`](Privacy::describe) because a row is not a
    /// dialog.** "Notifications say who wrote, and what they said" drew the
    /// settings pane 392 points wide inside a 360-point phone, and egui
    /// grows a ui to what is drawn in it -- so every row under it was laid
    /// out for a pane that wide. The long form belongs where there is room
    /// for it, beside the choice being made.
    pub fn word(self) -> &'static str {
        match self {
            Privacy::SenderAndText => "everything",
            Privacy::SenderOnly => "who, not what",
            Privacy::FactOnly => "that one arrived",
        }
    }

    /// The words for a choice in a dialog, where there is room to say what
    /// it means.
    pub fn describe(self) -> &'static str {
        match self {
            Privacy::SenderAndText => "who wrote, and what they said",
            Privacy::SenderOnly => "who wrote, and nothing of what",
            Privacy::FactOnly => "only that something arrived",
        }
    }

    /// Whether what was said may be quoted.
    pub fn quotes(self) -> bool {
        self == Privacy::SenderAndText
    }

    /// Whether anybody or any conversation may be named.
    pub fn names(self) -> bool {
        self != Privacy::FactOnly
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    /// A direct-message call asks the exchange for a SIP-25 introduction
    /// and goes straight between the two people when it can, relayed by
    /// the exchange when it cannot. On by default; off, every call is
    /// relayed. Off is a choice worth offering because an introduction
    /// discloses this machine's address to the other person.
    #[serde(default = "yes")]
    pub direct_calls: bool,
    /// How much a notification says (SIP-47). Everything, until somebody
    /// says otherwise.
    #[serde(default)]
    pub privacy: Privacy,
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
            privacy: Privacy::default(),
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

    pub fn set_privacy(&mut self, privacy: Privacy) {
        if self.privacy != privacy {
            self.privacy = privacy;
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

    /// **A choice that is not noticed is a choice that is lost.** The shell
    /// writes the preferences when `take_changed` says to, so a setter that
    /// forgot to raise it would hold for the session and be gone at the
    /// next start -- and on a phone the setting this is for is one somebody
    /// changes once and expects to stay changed.
    #[test]
    fn choosing_what_a_notification_says_is_noticed_and_kept() {
        let mut prefs = Prefs::default();
        assert_eq!(
            prefs.privacy,
            Privacy::SenderAndText,
            "everything, at first"
        );
        assert!(prefs.privacy.quotes() && prefs.privacy.names());

        prefs.set_privacy(Privacy::SenderAndText);
        assert!(!prefs.take_changed(), "nothing changed");
        prefs.set_privacy(Privacy::FactOnly);
        assert!(prefs.take_changed());
        assert!(!prefs.take_changed(), "once");
        assert!(!prefs.privacy.quotes() && !prefs.privacy.names());

        let text = serde_json::to_string(&prefs).unwrap();
        assert!(
            text.contains("fact_only"),
            "the spelling a phone's own settings file already uses: {text}"
        );
        let back: Prefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.privacy, Privacy::FactOnly, "kept across a restart");
        let old: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.privacy,
            Privacy::SenderAndText,
            "a file from before the field says everything, as it did"
        );
    }
}
