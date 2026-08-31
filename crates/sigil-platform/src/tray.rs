//! The tray icon, where there is one.
//!
//! It carries the unread state and is how sigil stays reachable with its window
//! closed. Where it cannot exist, closing the window must not make the program
//! unreachable — see [`Tray::support`].

use crate::support::{Session, Support};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct Tray {
    // Dropping a TrayIcon removes it, so it is held even though nothing reads
    // it back.
    icon: Option<TrayIcon>,
    support: Support,
}

impl Default for Tray {
    fn default() -> Self {
        Self::new()
    }
}

impl Tray {
    /// Build the tray icon.
    ///
    /// **Must be called on the main thread**, from inside eframe's creator: on
    /// macOS the menu bar is main-thread-only, and on Linux the GTK context the
    /// indicator uses is too.
    pub fn new() -> Tray {
        match probe() {
            Support::No(why) => Tray {
                icon: None,
                support: Support::No(why),
            },
            Support::Yes => match TrayIconBuilder::new()
                .with_tooltip("sigil")
                .with_icon(icon())
                .build()
            {
                Ok(icon) => Tray {
                    icon: Some(icon),
                    support: Support::Yes,
                },
                // The probe can only guess; this is the answer.
                Err(e) => Tray {
                    icon: None,
                    support: Support::no(format!("{e}")),
                },
            },
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    /// Say how many things want attention, so the icon carries it.
    pub fn set_unread(&self, unread: u32) {
        let Some(icon) = &self.icon else { return };
        let _ = icon.set_tooltip(Some(if unread == 0 {
            "sigil".to_string()
        } else {
            format!("sigil — {unread} waiting")
        }));
    }
}

/// A plain disc in sigil's accent. Drawn rather than shipped as an asset: an
/// icon file is one more thing to lose between the build and the bundle.
fn icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let centre = (SIZE as f32 - 1.0) / 2.0;
    let radius = centre - 1.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 - centre, y as f32 - centre);
            let inside = (dx * dx + dy * dy).sqrt() <= radius;
            if inside {
                rgba.extend_from_slice(&[0x6E, 0x8B, 0xFF, 0xFF]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("a square rgba buffer is a valid icon")
}

fn probe() -> Support {
    match Session::detect() {
        Session::Headless => Support::no("there is no desktop session here"),
        Session::MacOs => Support::Yes,
        Session::X11 | Session::Wayland => {
            // The tray is StatusNotifierItem over D-Bus, which KDE and most
            // desktops serve. GNOME does not without the AppIndicator
            // extension, and there is no way to ask from here that is cheaper
            // than trying — so this reports the likely cause and `new` reports
            // what actually happened.
            Support::Yes
        }
    }
}
