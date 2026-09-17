//! Desktop notifications.
//!
//! The reason this exists is the ring. Without it a call only reaches somebody
//! who happens to be looking at the window, which is not a telephone.
//!
//! **A notification pressed leads somewhere.** One that carries a
//! [`Target`] is watched, on a thread of its own, until it is pressed or
//! dismissed; a press puts the target down for the interface, which brings
//! the window up on that conversation. The watching is blocking on both
//! desktops -- there is no other shape the libraries offer -- so it is
//! bounded: past [`MOST_WATCHED`] notifications waiting at once, the rest
//! go out as plain notices that lead nowhere, rather than as threads that
//! may never end.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(not(target_os = "android"))]
use sigil::Sound;
use sigil::{Notice, Target};

#[cfg(not(target_os = "android"))]
use crate::support::Session;
use crate::support::Support;

/// On macOS a notification is posted by a *bundle*, identified by its bundle
/// id. A binary run straight from `cargo` is not one, so it cannot notify at
/// all — and the failure is silent, which is the worst kind here.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "org.squic.sigil";

/// How many notifications may be waiting to be pressed at once.
const MOST_WATCHED: usize = 32;

pub struct Notifier {
    support: Support,
    /// Targets of notifications that were pressed, waiting to be collected.
    pressed: Arc<Mutex<Vec<Target>>>,
    /// How many threads are waiting on a notification right now.
    watching: Arc<AtomicUsize>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "android"))]
impl Notifier {
    pub fn new() -> Notifier {
        Notifier {
            support: probe(),
            pressed: Arc::new(Mutex::new(Vec::new())),
            watching: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    /// Post a notification.
    ///
    /// Returns whether it went out. A caller that has nothing else to fall back
    /// on — a ring, say — should be showing the window as well, not relying on
    /// this: notifications can be off at the desktop level with nothing here
    /// able to tell.
    pub fn post(&self, summary: &str, body: &str) -> bool {
        self.notice(Notice::plain(summary, body))
    }

    /// Post a notification, with where it leads and what it sounds like.
    pub fn notice(&self, notice: Notice<'_>) -> bool {
        if !self.support.is_yes() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            let _ = notify_rust::set_application(BUNDLE_ID);
        }
        let mut n = notify_rust::Notification::new();
        n.summary(notice.summary).body(notice.body).appname("sigil");
        if let Some(sound) = sound_name(notice.sound) {
            n.sound_name(sound);
        }
        // A target, and room to watch it: the notification is sent on its
        // own thread and watched there until pressed or dismissed.
        let watched = notice
            .target
            .filter(|_| self.watching.load(Ordering::Relaxed) < MOST_WATCHED);
        let Some(target) = watched else {
            return n.show().is_ok();
        };
        // On Linux a press on the notification itself is the "default"
        // action, which the daemon only reports for a notification that
        // declared it. On macOS a press is a press, and an action would be
        // a button.
        #[cfg(linux_desktop)]
        n.action("default", "Open");
        let pressed = Arc::clone(&self.pressed);
        let watching = Arc::clone(&self.watching);
        watching.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("sigil-notification".into())
            .spawn(move || {
                if let Ok(handle) = n.show() {
                    handle.wait_for_action(|action| {
                        if action == "default"
                            && let Ok(mut pressed) = pressed.lock()
                        {
                            pressed.push(target);
                            crate::wake();
                        }
                    });
                }
                watching.fetch_sub(1, Ordering::Relaxed);
            })
            .is_ok()
    }

    /// The targets of notifications pressed since last asked.
    pub fn pressed(&self) -> Vec<Target> {
        self.pressed
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default()
    }
}

/// The phone posts nothing from here. Its notifications are the platform's,
/// composed and shown by sigil-android through the system's own surface --
/// which is also where a press on one comes back from -- and the shell is
/// handed that notifier in place of this one. This one only says so, in
/// case somebody looks.
#[cfg(target_os = "android")]
impl Notifier {
    pub fn new() -> Notifier {
        Notifier {
            support: Support::no(
                "the phone posts its own notifications; sigil-android installs them",
            ),
            pressed: Arc::new(Mutex::new(Vec::new())),
            watching: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    pub fn post(&self, summary: &str, body: &str) -> bool {
        self.notice(Notice::plain(summary, body))
    }

    pub fn notice(&self, _notice: Notice<'_>) -> bool {
        false
    }

    pub fn pressed(&self) -> Vec<Target> {
        self.pressed
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default()
    }
}

/// The desktop's name for the sound, if any.
///
/// macOS takes the name of a system sound; the freedesktop sound naming
/// specification has words for both cases.
#[cfg(not(target_os = "android"))]
fn sound_name(sound: Sound) -> Option<&'static str> {
    match sound {
        Sound::None => None,
        #[cfg(target_os = "macos")]
        Sound::Default => Some("Tink"),
        #[cfg(target_os = "macos")]
        Sound::Ring => Some("Glass"),
        #[cfg(not(target_os = "macos"))]
        Sound::Default => Some("message-new-instant"),
        #[cfg(not(target_os = "macos"))]
        Sound::Ring => Some("phone-incoming-call"),
    }
}

#[cfg(not(target_os = "android"))]
fn probe() -> Support {
    match Session::detect() {
        Session::Headless => {
            Support::no("there is no desktop session here, so nothing can be shown")
        }
        #[cfg(target_os = "macos")]
        Session::MacOs => {
            // Binding the bundle id is the operative question — if this
            // succeeds, posting works — so it is what gets asked, rather than
            // whether the executable happens to sit inside a `.app`.
            //
            // Those are not the same question, and the difference surprised me
            // enough to be worth writing down. A bare `cargo run` binary fails
            // this on a machine that has never seen Sigil.app, and reports
            // unavailable, correctly. But once the bundle has been run *once*,
            // macOS knows `org.squic.sigil`, and from then on the bare binary
            // binds it happily and really can post. So this can say "available"
            // for an unbundled binary on a developer's machine and "unavailable"
            // for the same binary on somebody else's.
            //
            // That is the right behaviour — it answers "can this post here",
            // which is what a caller needs — but it means an unbundled build
            // must never be taken as evidence that shipping one would work.
            match notify_rust::set_application(BUNDLE_ID) {
                Ok(()) => Support::Yes,
                Err(_) => Support::no(
                    "macOS routes notifications by bundle identifier and this build \
                     has none registered; run the built Sigil.app rather than the \
                     bare binary",
                ),
            }
        }
        #[cfg(not(target_os = "macos"))]
        Session::MacOs => Support::Yes,
        // Both speak to the same D-Bus service, so the windowing system does
        // not come into it.
        Session::X11 | Session::Wayland => Support::Yes,
    }
}

/// So the shell can hand a `Notifier` to apps as the host's
/// [`sigil::Notify`]. Implemented here rather than in the shell, which may not
/// implement another crate's trait for another crate's type.
impl sigil::Notify for Notifier {
    fn notice(&self, notice: Notice<'_>) -> bool {
        Notifier::notice(self, notice)
    }

    fn pressed(&self) -> Vec<Target> {
        Notifier::pressed(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring sounds different from a message, and a plain notice is silent.
    #[cfg(not(target_os = "android"))]
    #[test]
    fn a_ring_and_a_message_sound_different_and_a_plain_notice_is_silent() {
        assert_eq!(sound_name(Sound::None), None);
        assert!(sound_name(Sound::Default).is_some());
        assert_ne!(sound_name(Sound::Default), sound_name(Sound::Ring));
    }

    /// What was pressed is held until asked, then handed over once.
    #[test]
    fn pressed_targets_are_handed_over_once() {
        let notifier = Notifier {
            support: Support::no("test"),
            pressed: Arc::new(Mutex::new(Vec::new())),
            watching: Arc::new(AtomicUsize::new(0)),
        };
        let target = Target {
            identity: sqnr_core::PubKey::new([1u8; 32]),
            exchange: "trunk.exchange".into(),
            channel: [2u8; 32],
        };
        notifier.pressed.lock().unwrap().push(target.clone());
        assert_eq!(notifier.pressed(), vec![target]);
        assert_eq!(notifier.pressed(), vec![]);
    }
}
