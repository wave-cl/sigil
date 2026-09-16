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
pub mod badge;
pub mod deeplink;
pub mod hotkey;
pub mod idle;
pub mod instance;
pub mod mark;
pub mod notify;
#[cfg(target_os = "macos")]
pub mod reopen;
pub mod support;
pub mod tray;

use std::sync::OnceLock;

/// How to ask the interface to look when something happens off its thread
/// -- a press on the tray, a notification pressed: egui's `request_repaint`,
/// set once the interface exists. Without it the event would wait for the
/// next pass, which on an idle window is whenever something else happens.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Set what [`wake`] does. Once per process; a later call is ignored.
pub fn wake_with(f: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(f));
}

/// Ask the interface to look.
pub(crate) fn wake() {
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

pub use autostart::Autostart;
pub use badge::Badge;
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
    pub badge: Badge,
    pub hotkeys: Hotkeys,
    pub autostart: Autostart,
    /// Whether a press on the Dock icon, or the application being brought
    /// to the front, reaches the window (macOS; elsewhere the launcher has
    /// no such press to report, and the tray's Open is the way back).
    pub reopen: Support,
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
            badge: Badge::new(),
            hotkeys: Hotkeys::new(),
            autostart: Autostart::new(),
            #[cfg(target_os = "macos")]
            reopen: reopen::watch(),
            #[cfg(not(target_os = "macos"))]
            reopen: Support::no("only the Dock reports a press on the icon"),
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
                "Application badge",
                "puts the number waiting on sigil's own icon in the Dock or the launcher",
                self.badge.support().clone(),
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
