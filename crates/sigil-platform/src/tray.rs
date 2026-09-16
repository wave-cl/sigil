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
    /// The icon and its menu, on this thread. On Linux they live on the
    /// GTK thread instead, and this is `None`; see [`gtk_thread`].
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    built: Option<Built>,
    support: Support,
}

/// The icon and the menu that is built when the desktop has a tray:
/// what the state is kept on, wherever it lives.
struct Built {
    // Dropping a TrayIcon removes it, so it is held even though nothing reads
    // it back.
    icon: TrayIcon,
    /// The menu's check item for do-not-disturb, kept so the tick can follow
    /// the setting when it is changed elsewhere.
    quiet_item: CheckMenuItem,
    /// Whether the icon currently carries the dot, so it is only redrawn
    /// when that changes.
    marked: bool,
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

pub(crate) fn report(action: TrayAction) {
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

impl Built {
    /// The icon with its menu, and the handlers that report what is done
    /// to them. **On the thread that owns the desktop's tray**: the main
    /// thread on macOS, the GTK thread on Linux.
    fn build() -> Result<Built, String> {
        // The menu: the one way to the window on Linux, where the library
        // reports no press on the icon itself, and the way to quit once
        // closing the window only hides it.
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
        // Events arrive on the desktop's thread and are put down for the
        // interface to pick up; see `PENDING`. Set once for the process,
        // which is what the library allows.
        MenuEvent::set_event_handler(Some(|e: MenuEvent| match e.id.as_ref() {
            OPEN => report(TrayAction::Open),
            QUIET => report(TrayAction::QuietToggled),
            QUIT => report(TrayAction::Quit),
            _ => {}
        }));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
            // A press on the icon itself opens the window. macOS reports
            // it; the menu is up on a right press there and on any press on
            // Linux, where this never fires.
            if let TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = e
            {
                report(TrayAction::Open);
            }
        }));
        let icon = TrayIconBuilder::new()
            .with_tooltip("Sigil")
            .with_icon(icon(false))
            .with_menu(Box::new(menu))
            // On macOS a left press is a press and the right one is the
            // menu; elsewhere the menu is all there is.
            .with_menu_on_left_click(!cfg!(target_os = "macos"))
            // The menu bar tints a template to match itself -- black on a
            // light bar, white on a dark one -- which is what every other
            // mark up there does. Elsewhere the icon is the icon.
            .with_icon_as_template(cfg!(target_os = "macos"))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Built {
            icon,
            quiet_item: quiet,
            marked: false,
        })
    }

    fn set_unread(&mut self, unread: u32, quiet: bool) {
        self.icon.set_title(tray_title(unread).as_deref());
        let _ = self.icon.set_tooltip(Some(tray_tooltip(unread)));
        let marked = unread > 0 && !quiet;
        if marked != self.marked {
            self.marked = marked;
            let _ = self
                .icon
                .set_icon_with_as_template(Some(self::icon(marked)), cfg!(target_os = "macos"));
        }
    }

    fn set_quiet(&self, quiet: bool) {
        if self.quiet_item.is_checked() != quiet {
            self.quiet_item.set_checked(quiet);
        }
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
impl Tray {
    /// Build the tray icon.
    ///
    /// **Must be called on the main thread**, from inside eframe's creator:
    /// the menu bar is main-thread-only.
    pub fn new() -> Tray {
        match probe() {
            Support::No(why) => Tray {
                built: None,
                support: Support::No(why),
            },
            Support::Yes => match Built::build() {
                Ok(built) => Tray {
                    built: Some(built),
                    support: Support::Yes,
                },
                // The probe can only guess; this is the answer.
                Err(e) => Tray {
                    built: None,
                    support: Support::no(e),
                },
            },
        }
    }

    /// Say how many things want attention, so the icon carries it: the
    /// number beside the mark, the dot on it, and the tooltip. Under
    /// do-not-disturb the number stays and the dot does not: the count is
    /// a fact, the dot is a nudge.
    pub fn set_unread(&mut self, unread: u32, quiet: bool) {
        if let Some(built) = &mut self.built {
            built.set_unread(unread, quiet);
        }
    }

    /// Show the do-not-disturb item ticked or not, when the setting was
    /// changed somewhere other than the menu.
    pub fn set_quiet(&self, quiet: bool) {
        if let Some(built) = &self.built {
            built.set_quiet(quiet);
        }
    }
}

/// **On Linux the tray lives on a thread of its own, running GTK's loop.**
///
/// The indicator is GTK, and GTK wants to be initialised and to run its
/// own main loop on the thread that owns its objects -- and eframe's loop
/// is winit's, on the main thread. Without this the first menu built
/// panicked with "GTK has not been initialized", on every Linux desktop,
/// at launch. So a thread is started that initialises GTK, builds the
/// icon and menu, keeps them in a thread-local, and runs `gtk::main()`
/// for as long as the process does; everything that touches them from
/// the interface is handed to that loop as an idle callback. The desktop's
/// own callbacks already ran there and reported through [`PENDING`], so
/// nothing about events changes.
///
/// A desktop with no GTK to initialise -- no display -- is reported as
/// unavailable, with GTK's reason, rather than left to panic.
#[cfg(all(unix, not(target_os = "macos")))]
mod gtk_thread {
    use std::cell::RefCell;
    use std::sync::mpsc;

    use super::Built;

    thread_local! {
        /// The icon and menu, on the GTK thread and nowhere else.
        static BUILT: RefCell<Option<Built>> = const { RefCell::new(None) };
    }

    /// Start the thread, and wait for it to say whether the tray exists.
    pub(super) fn start() -> Result<(), String> {
        let (tell, told) = mpsc::channel::<Result<(), String>>();
        let spawned = std::thread::Builder::new()
            .name("sigil-tray".into())
            .spawn(move || {
                if let Err(e) = gtk::init() {
                    let _ = tell.send(Err(format!("GTK could not start: {e}")));
                    return;
                }
                match Built::build() {
                    Ok(built) => {
                        BUILT.with(|b| *b.borrow_mut() = Some(built));
                        let _ = tell.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = tell.send(Err(e));
                        return;
                    }
                }
                gtk::main();
            });
        if let Err(e) = spawned {
            return Err(format!("the tray thread could not start: {e}"));
        }
        told.recv()
            .unwrap_or_else(|_| Err("the tray thread ended before it answered".into()))
    }

    /// Run `f` against the icon on its own thread, at the loop's next
    /// idle moment.
    pub(super) fn on_tray(f: impl FnOnce(&mut Built) + Send + 'static) {
        gtk::glib::idle_add_once(move || {
            BUILT.with(|b| {
                if let Some(built) = &mut *b.borrow_mut() {
                    f(built);
                }
            });
        });
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl Tray {
    /// Build the tray icon, on its own thread; see [`gtk_thread`].
    pub fn new() -> Tray {
        let support = match probe() {
            Support::No(why) => Support::No(why),
            Support::Yes => match gtk_thread::start() {
                Ok(()) => Support::Yes,
                Err(e) => Support::no(e),
            },
        };
        Tray { support }
    }

    /// Say how many things want attention, so the icon carries it: the
    /// number beside the mark, the dot on it, and the tooltip. Under
    /// do-not-disturb the number stays and the dot does not: the count is
    /// a fact, the dot is a nudge.
    pub fn set_unread(&mut self, unread: u32, quiet: bool) {
        if self.support.is_yes() {
            gtk_thread::on_tray(move |b| b.set_unread(unread, quiet));
        }
    }

    /// Show the do-not-disturb item ticked or not, when the setting was
    /// changed somewhere other than the menu.
    pub fn set_quiet(&self, quiet: bool) {
        if self.support.is_yes() {
            gtk_thread::on_tray(move |b| b.set_quiet(quiet));
        }
    }
}

/// Held by every test that reports through [`PENDING`], which is one
/// queue for the process: two such tests running at once would read each
/// other's reports.
#[cfg(test)]
pub(crate) static SERIAL: Mutex<()> = Mutex::new(());

/// Everything reported and not yet collected, for a test of what reports.
#[cfg(test)]
pub(crate) fn drain_for_test() -> Vec<TrayAction> {
    PENDING
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

impl Tray {
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

    /// With nowhere to draw, the GTK thread says so and the tray is
    /// unavailable -- where it used to panic on the first menu built.
    /// Run where there is no display, which is what every test runner is.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn without_a_display_the_gtk_thread_refuses_rather_than_panics() {
        if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return; // a desktop: this is about not having one
        }
        let why = gtk_thread::start().expect_err("no display, no tray");
        assert!(why.contains("GTK could not start"), "{why}");
        let tray = Tray::new();
        assert!(!tray.support().is_yes());
    }

    /// What the desktop reports is held until the interface asks, and
    /// then handed over once.
    #[test]
    fn tray_events_are_held_until_asked_and_handed_over_once() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _ = drain_for_test();
        let tray = Tray {
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            built: None,
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
