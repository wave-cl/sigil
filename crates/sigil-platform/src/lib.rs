//! What this desktop can and cannot do, and the things sigil asks of it.
//!
//! # One rule
//!
//! **Nothing is ever silently inert.** macOS, X11 and Wayland differ in ways
//! sigil cannot fix — Wayland has no key grab by design, GNOME shows no tray
//! without an extension, an unbundled macOS binary cannot notify — and every
//! one of those differences reaches the interface as a
//! [`Support::No`](support::Support) carrying a sentence somebody can act on.
//!
//! A control that quietly does nothing is worse than a missing one. It costs
//! somebody an afternoon before they conclude the program is lying, and in at
//! least one case here it is dangerous: a mute key that silently fails leaves
//! somebody believing they are muted.

pub mod autostart;
pub mod deeplink;
pub mod hotkey;
pub mod instance;
pub mod notify;
pub mod support;
pub mod tray;

pub use autostart::Autostart;
pub use deeplink::Link;
pub use hotkey::Hotkeys;
pub use instance::Instance;
pub use notify::Notifier;
pub use support::{Capability, Session, Support};
pub use tray::Tray;

/// Everything sigil asks of the desktop, built once at startup.
///
/// **Construct on the main thread, inside eframe's creator.** The tray needs it
/// on macOS (the menu bar) and on Linux (GTK).
pub struct Platform {
    pub notifier: Notifier,
    pub tray: Tray,
    pub hotkeys: Hotkeys,
    pub autostart: Autostart,
    session: Session,
}

impl Default for Platform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform {
    pub fn new() -> Platform {
        Platform {
            notifier: Notifier::new(),
            tray: Tray::new(),
            hotkeys: Hotkeys::new(),
            autostart: Autostart::new(),
            session: Session::detect(),
        }
    }

    pub fn session(&self) -> Session {
        self.session
    }

    /// The matrix, for the settings pane to draw as a list.
    ///
    /// Each row says what sigil uses the capability *for*, so somebody reading
    /// an unavailable one knows what they are losing rather than only that
    /// something is missing.
    pub fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new(
                "Notifications",
                "tells you about a call or a message when sigil is not in front",
                self.notifier.support().clone(),
            ),
            Capability::new(
                "Tray icon",
                "keeps sigil reachable with its window closed",
                self.tray.support().clone(),
            ),
            Capability::new(
                "Global shortcuts",
                "mute and push-to-talk while sigil is not focused",
                self.hotkeys.support().clone(),
            ),
            Capability::new(
                "Start at login",
                "so calls can arrive without starting sigil first",
                self.autostart.support().clone(),
            ),
        ]
    }

    /// Whether a call can reach somebody who is not looking at the window.
    ///
    /// Worth asking as one question: with no notification *and* no tray, sigil
    /// is only a telephone while it is on screen, and it should say so rather
    /// than let somebody find out by missing a call.
    pub fn can_reach_you_when_away(&self) -> bool {
        self.notifier.support().is_yes() || self.tray.support().is_yes()
    }
}
