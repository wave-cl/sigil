//! Desktop notifications.
//!
//! The reason this exists is the ring. Without it a call only reaches somebody
//! who happens to be looking at the window, which is not a telephone.

use crate::support::{Session, Support};

/// On macOS a notification is posted by a *bundle*, identified by its bundle
/// id. A binary run straight from `cargo` is not one, so it cannot notify at
/// all — and the failure is silent, which is the worst kind here.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "org.squic.sigil";

pub struct Notifier {
    support: Support,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier { support: probe() }
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
        if !self.support.is_yes() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            let _ = notify_rust::set_application(BUNDLE_ID);
        }
        notify_rust::Notification::new()
            .summary(summary)
            .body(body)
            .appname("sigil")
            .show()
            .is_ok()
    }
}

fn probe() -> Support {
    match Session::detect() {
        Session::Headless => {
            Support::no("there is no desktop session here, so nothing can be shown")
        }
        #[cfg(target_os = "macos")]
        Session::MacOs => {
            // `set_application` fails when the running binary is not inside a
            // bundle, which is exactly the `cargo run` case. Better to say so
            // than to post into nothing.
            match notify_rust::set_application(BUNDLE_ID) {
                Ok(()) => Support::Yes,
                Err(_) => Support::no(
                    "macOS only lets a bundled application notify; run the built \
                     sigil.app rather than the bare binary",
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
    fn post(&self, summary: &str, body: &str) -> bool {
        Notifier::post(self, summary, body)
    }
}
