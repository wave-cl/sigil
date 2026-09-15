//! The tray icon, where there is one.
//!
//! It carries the unread state and is how sigil stays reachable with its window
//! closed. Where it cannot exist, closing the window must not make the program
//! unreachable — see [`Tray::support`].

use std::sync::Mutex;

use crate::support::{Session, Support};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    // Dropping a TrayIcon removes it, so it is held even though nothing reads
    // it back.
    icon: Option<TrayIcon>,
    /// The menu's check item for do-not-disturb, kept so the tick can follow
    /// the setting when it is changed elsewhere.
    quiet_item: Option<CheckMenuItem>,
    /// Whether the icon currently carries the dot, so it is only redrawn
    /// when that changes.
    marked: bool,
    support: Support,
}

/// What somebody did to the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Bring the window up: the icon was pressed, or Open was chosen.
    Open,
    /// The do-not-disturb item was chosen: the setting flips, and the
    /// interface, which holds it, sets the tick back to match.
    QuietToggled,
    Quit,
}

const OPEN: &str = "sigil-open";
const QUIET: &str = "sigil-quiet";
const QUIT: &str = "sigil-quit";

/// What the desktop reported and nobody has collected yet. The tray's
/// callbacks run on the desktop's own thread, so they leave the event here
/// and ask the interface to look, and the interface drains it each pass.
static PENDING: Mutex<Vec<TrayAction>> = Mutex::new(Vec::new());

fn report(action: TrayAction) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(action);
    }
    crate::wake();
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
                quiet_item: None,
                marked: false,
                support: Support::No(why),
            },
            Support::Yes => {
                // The menu: the one way to the window on Linux, where the
                // library reports no press on the icon itself, and the way
                // to quit once closing the window only hides it.
                let open = MenuItem::with_id(OPEN, "Open Sigil", true, None);
                let quiet = CheckMenuItem::with_id(QUIET, "Do not disturb", true, false, None);
                let quit = MenuItem::with_id(QUIT, "Quit Sigil", true, None);
                let menu = Menu::new();
                let _ = menu.append_items(&[
                    &open,
                    &PredefinedMenuItem::separator(),
                    &quiet,
                    &PredefinedMenuItem::separator(),
                    &quit,
                ]);
                // Events arrive on the desktop's thread and are put down
                // for the interface to pick up; see `PENDING`. Set once for
                // the process, which is what the library allows.
                MenuEvent::set_event_handler(Some(|e: MenuEvent| match e.id.as_ref() {
                    OPEN => report(TrayAction::Open),
                    QUIET => report(TrayAction::QuietToggled),
                    QUIT => report(TrayAction::Quit),
                    _ => {}
                }));
                TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
                    // A press on the icon itself opens the window. macOS
                    // reports it; the menu is up on a right press there and
                    // on any press on Linux, where this never fires.
                    if let TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    } = e
                    {
                        report(TrayAction::Open);
                    }
                }));
                match TrayIconBuilder::new()
                    .with_tooltip("Sigil")
                    .with_icon(icon(false))
                    .with_menu(Box::new(menu))
                    // On macOS a left press is a press and the right one is
                    // the menu; elsewhere the menu is all there is.
                    .with_menu_on_left_click(!cfg!(target_os = "macos"))
                    // The menu bar tints a template to match itself -- black
                    // on a light bar, white on a dark one -- which is what
                    // every other mark up there does. Elsewhere the icon is
                    // the icon.
                    .with_icon_as_template(cfg!(target_os = "macos"))
                    .build()
                {
                    Ok(icon) => Tray {
                        icon: Some(icon),
                        quiet_item: Some(quiet),
                        marked: false,
                        support: Support::Yes,
                    },
                    // The probe can only guess; this is the answer.
                    Err(e) => Tray {
                        icon: None,
                        quiet_item: None,
                        marked: false,
                        support: Support::no(format!("{e}")),
                    },
                }
            }
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    /// What has happened at the tray since last asked.
    pub fn events(&self) -> Vec<TrayAction> {
        PENDING
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default()
    }

    /// Say how many things want attention, so the icon carries it: the
    /// number beside the mark, the dot on it, and the tooltip. Under
    /// do-not-disturb the number stays and the dot does not: the count is
    /// a fact, the dot is a nudge.
    pub fn set_unread(&mut self, unread: u32, quiet: bool) {
        let Some(icon) = &self.icon else { return };
        icon.set_title(tray_title(unread).as_deref());
        let _ = icon.set_tooltip(Some(tray_tooltip(unread)));
        let marked = unread > 0 && !quiet;
        if marked != self.marked {
            self.marked = marked;
            let _ =
                icon.set_icon_with_as_template(Some(self::icon(marked)), cfg!(target_os = "macos"));
        }
    }

    /// Show the do-not-disturb item ticked or not, when the setting was
    /// changed somewhere other than the menu.
    pub fn set_quiet(&self, quiet: bool) {
        if let Some(item) = &self.quiet_item
            && item.is_checked() != quiet
        {
            item.set_checked(quiet);
        }
    }
}

/// The count beside the mark: nothing at nought, since a "0" beside the
/// icon all day is a badge reading nought.
pub fn tray_title(unread: u32) -> Option<String> {
    (unread > 0).then(|| unread.to_string())
}

fn tray_tooltip(unread: u32) -> String {
    match unread {
        0 => "Sigil".to_string(),
        1 => "Sigil — 1 waiting".to_string(),
        n => format!("Sigil — {n} waiting"),
    }
}

/// A plain disc in sigil's accent. Drawn rather than shipped as an asset: an
/// icon file is one more thing to lose between the build and the bundle.
/// The mark, as `crate::mark` draws it -- the same drawing as the app icon.
///
/// On macOS the emblem alone, in black, as a template the menu bar tints;
/// on the others the emblem on its rounded square, at a size the tray
/// scales from. Twice the nominal size so a Retina bar draws it sharp.
fn icon(marked: bool) -> Icon {
    const SIZE: u32 = 64;
    let rgba = match (cfg!(target_os = "macos"), marked) {
        (true, false) => crate::mark::glyph_rgba(SIZE, [0, 0, 0]),
        (true, true) => crate::mark::glyph_rgba_marked(SIZE, [0, 0, 0]),
        // Elsewhere the number beside the mark says it; the mark stays.
        (false, _) => crate::mark::icon_rgba(SIZE),
    };
    Icon::from_rgba(rgba, SIZE, SIZE).expect("a square rgba buffer is a valid icon")
}

fn probe() -> Support {
    // The menu bar, and the menu that goes in it, belong to the main
    // thread; asked from anywhere else the library panics rather than
    // refuses, so this refuses first.
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_none() {
        return Support::no("the tray can only be made on the main thread");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing beside the mark at nought; the number otherwise.
    #[test]
    fn the_count_beside_the_mark_is_absent_at_nought() {
        assert_eq!(tray_title(0), None);
        assert_eq!(tray_title(1).as_deref(), Some("1"));
        assert_eq!(tray_title(12).as_deref(), Some("12"));
        assert_eq!(tray_tooltip(0), "Sigil");
        assert_eq!(tray_tooltip(3), "Sigil — 3 waiting");
    }

    /// What the desktop reports is held until the interface asks, and
    /// then handed over once.
    #[test]
    fn tray_events_are_held_until_asked_and_handed_over_once() {
        let tray = Tray {
            icon: None,
            quiet_item: None,
            marked: false,
            support: Support::no("test"),
        };
        report(TrayAction::Open);
        report(TrayAction::QuietToggled);
        assert_eq!(
            tray.events(),
            vec![TrayAction::Open, TrayAction::QuietToggled]
        );
        assert_eq!(tray.events(), vec![]);
    }
}
