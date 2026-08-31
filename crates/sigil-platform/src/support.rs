//! Whether this desktop can do a thing, and why not when it cannot.
//!
//! sigil runs on macOS, on X11 and on Wayland, and they differ in ways that are
//! not sigil's to fix. There is no Wayland equivalent of an X11 key grab, by
//! design. GNOME shows no tray icon without an extension. A macOS `.app` cannot
//! post a notification without a bundle identifier, so a `cargo run` build
//! never will.
//!
//! The rule this module exists to enforce: **nothing is ever silently inert.**
//! A control that cannot work here is disabled with the reason beside it, and
//! the reason is a sentence somebody can act on. A switch that does nothing
//! when you flip it is worse than no switch, because it costs somebody an
//! afternoon before they conclude the program is lying.

use std::fmt;

/// Whether a capability works on this machine, in this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// It works.
    Yes,
    /// It does not, and this is why — in words for a person, not a log.
    No(String),
}

impl Support {
    pub fn no(reason: impl Into<String>) -> Support {
        Support::No(reason.into())
    }

    pub fn is_yes(&self) -> bool {
        matches!(self, Support::Yes)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Support::Yes => None,
            Support::No(why) => Some(why),
        }
    }
}

impl fmt::Display for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Support::Yes => write!(f, "available"),
            Support::No(why) => write!(f, "unavailable — {why}"),
        }
    }
}

/// One row of the platform matrix, for the settings pane to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// What it is called.
    pub name: &'static str,
    /// What sigil uses it for — so somebody reading the row knows what they
    /// lose when it says unavailable.
    pub what: &'static str,
    pub support: Support,
}

impl Capability {
    pub fn new(name: &'static str, what: &'static str, support: Support) -> Capability {
        Capability {
            name,
            what,
            support,
        }
    }
}

/// Which windowing system is in use, where that decides what works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    MacOs,
    X11,
    Wayland,
    /// Neither variable is set, so this is a login shell, a container, or
    /// something else with no display at all.
    Headless,
}

impl Session {
    /// Work out what we are running under.
    ///
    /// `XDG_SESSION_TYPE` is the intended answer, but it is unset often enough
    /// — bare `startx`, some display managers — that `WAYLAND_DISPLAY` and
    /// `DISPLAY` are checked too. Wayland first: a Wayland session usually has
    /// `DISPLAY` set as well, for Xwayland, and believing that would report X11
    /// on every modern desktop.
    pub fn detect() -> Session {
        if cfg!(target_os = "macos") {
            return Session::MacOs;
        }
        let kind = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        if kind == "wayland" || std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Session::Wayland
        } else if kind == "x11" || std::env::var_os("DISPLAY").is_some() {
            Session::X11
        } else {
            Session::Headless
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Session::MacOs => "macOS",
            Session::X11 => "Linux (X11)",
            Session::Wayland => "Linux (Wayland)",
            Session::Headless => "no display",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_a_reason_somebody_can_act_on() {
        let s = Support::no("GNOME needs the AppIndicator extension for a tray icon");
        assert!(!s.is_yes());
        assert!(s.reason().unwrap().contains("AppIndicator"));
        assert!(s.to_string().starts_with("unavailable — "));
    }

    #[test]
    fn availability_says_so_plainly() {
        assert_eq!(Support::Yes.to_string(), "available");
        assert_eq!(Support::Yes.reason(), None);
    }

    /// A Wayland session usually has `DISPLAY` set too, for Xwayland. Reading
    /// that first would report X11 on every modern Linux desktop and then offer
    /// a global hotkey that cannot work.
    #[test]
    fn wayland_is_not_mistaken_for_x11() {
        // Cannot safely mutate the environment in a threaded test, so this
        // checks the ordering the detector documents rather than driving it.
        // The rule: Wayland is decided before X11, never after.
        let source = include_str!("support.rs");
        let detect = source
            .split("pub fn detect()")
            .nth(1)
            .expect("detect exists");
        let wayland_at = detect.find("WAYLAND_DISPLAY").expect("checks wayland");
        let display_at = detect.find("\"DISPLAY\"").expect("checks x11");
        assert!(
            wayland_at < display_at,
            "Wayland must be decided before X11, or Xwayland's DISPLAY wins"
        );
    }
}
