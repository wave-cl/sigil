//! The count on the application's own icon: the Dock tile on macOS, the
//! launcher entry on a Linux desktop that shows one.
//!
//! **Not the tray.** The tray is a mark sigil puts up; this is the icon the
//! desktop already shows for the running application, and the count on it
//! is the one somebody sees when they look for sigil rather than at it.
//!
//! macOS has an API for exactly this. Linux has no standard: what exists is
//! the signal Unity defined and KDE, Cinnamon and a few docks adopted, sent
//! on the session bus by the application about itself, naming its `.desktop`
//! file. GNOME ignores it. Either way the answer is a [`Support`] somebody
//! can read.

use crate::support::Support;

pub struct Badge {
    support: Support,
    #[cfg(all(unix, not(target_os = "macos")))]
    connection: Option<zbus::blocking::Connection>,
    /// What was last set, so the desktop is only told about a change.
    shown: Option<u32>,
}

impl Default for Badge {
    fn default() -> Self {
        Self::new()
    }
}

impl Badge {
    /// **Main thread**, on macOS: the Dock tile belongs to the application
    /// object, which only the main thread may touch.
    pub fn new() -> Badge {
        #[cfg(target_os = "macos")]
        {
            let support = match objc2::MainThreadMarker::new() {
                Some(_) => Support::Yes,
                None => Support::no("the Dock can only be reached from the main thread"),
            };
            Badge {
                support,
                shown: None,
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            match zbus::blocking::Connection::session() {
                Ok(connection) => Badge {
                    support: Support::Yes,
                    connection: Some(connection),
                    shown: None,
                },
                Err(e) => Badge {
                    support: Support::no(format!("no session bus to tell the launcher: {e}")),
                    connection: None,
                    shown: None,
                },
            }
        }
        #[cfg(not(unix))]
        {
            Badge {
                support: Support::no("no application badge on this desktop"),
                shown: None,
            }
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    /// Put `count` on the icon; nought takes it off.
    pub fn set_count(&mut self, count: u32) {
        if self.shown == Some(count) {
            return;
        }
        if self.set(count) {
            self.shown = Some(count);
        }
    }

    #[cfg(target_os = "macos")]
    fn set(&mut self, count: u32) -> bool {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        let label = badge_label(count).map(|s| NSString::from_str(&s));
        app.dockTile().setBadgeLabel(label.as_deref());
        true
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn set(&mut self, count: u32) -> bool {
        let Some(connection) = &self.connection else {
            return false;
        };
        let (uri, properties) = launcher_entry(count);
        connection
            .emit_signal(
                None::<zbus::names::BusName<'_>>,
                "/org/squic/sigil",
                "com.canonical.Unity.LauncherEntry",
                "Update",
                &(uri, properties),
            )
            .is_ok()
    }

    #[cfg(not(unix))]
    fn set(&mut self, _count: u32) -> bool {
        false
    }
}

/// What the Dock tile says: the number, or nothing at nought.
pub fn badge_label(count: u32) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

/// The launcher entry's update: which application, and the count with
/// whether to show it -- the count is left in place at nought and hidden,
/// which is what the docks expect.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn launcher_entry(
    count: u32,
) -> (
    String,
    std::collections::HashMap<String, zbus::zvariant::Value<'static>>,
) {
    use zbus::zvariant::Value;
    let mut properties = std::collections::HashMap::new();
    properties.insert("count".to_string(), Value::I64(i64::from(count)));
    properties.insert("count-visible".to_string(), Value::Bool(count > 0));
    ("application://sigil.desktop".to_string(), properties)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_is_the_number_and_nothing_at_nought() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(7).as_deref(), Some("7"));
    }

    /// The desktop is told once per change, not once per pass.
    #[test]
    fn a_count_already_shown_is_not_set_again() {
        let mut badge = Badge {
            support: Support::no("test"),
            #[cfg(all(unix, not(target_os = "macos")))]
            connection: None,
            shown: Some(3),
        };
        // With nothing to set on -- no bus, or not the main thread -- `set`
        // fails and `shown` stays, so the next pass tries again; the same
        // count short-circuits before that.
        badge.set_count(3);
        assert_eq!(badge.shown, Some(3));
        badge.set_count(4);
        assert_eq!(badge.shown, Some(3));
    }

    /// The launcher entry names sigil's own `.desktop` file, which is what
    /// the docks match against, and hides the count at nought.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_launcher_entry_names_the_desktop_file_and_hides_nought() {
        use zbus::zvariant::Value;
        let (uri, props) = launcher_entry(0);
        assert_eq!(uri, "application://sigil.desktop");
        assert_eq!(props["count-visible"], Value::Bool(false));
        let (_, props) = launcher_entry(5);
        assert_eq!(props["count"], Value::I64(5));
        assert_eq!(props["count-visible"], Value::Bool(true));
    }

    /// The Dock is reachable from the main thread and from nowhere else,
    /// and a badge made anywhere else says so rather than failing quietly
    /// on every pass. (A test runs off the main thread, so the tile itself
    /// is exercised by hand: the count appears on the Dock icon.)
    #[cfg(target_os = "macos")]
    #[test]
    fn off_the_main_thread_the_dock_is_reported_unreachable() {
        let badge = Badge::new();
        assert!(
            badge
                .support()
                .reason()
                .is_some_and(|why| why.contains("main thread")),
            "{:?}",
            badge.support()
        );
    }
}
