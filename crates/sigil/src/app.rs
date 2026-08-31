//! The contract between the shell and the things it hosts.
//!
//! Three layers, each ignorant of the one below it: a **host** owning
//! resources, a **shell** owning the app roster and navigation, and **apps**
//! owning their own domain and knowing nothing of each other.
//!
//! # Why `update` and `render` are separate
//!
//! A call must keep running while you are reading messages. So every *opened*
//! app gets [`App::update`] each pass, and only the visible one gets
//! [`App::render`]. Correctness never depends on being drawn.
//!
//! eframe 0.36 has the same split one layer down — `App::logic` runs even while
//! the window is hidden, with no egui pass at all — which is what lets sigil
//! keep a call alive and a ring listener running when it is closed to the tray.
//! The host maps one onto the other.

use std::any::Any;
use std::rc::Rc;

use crate::account::Account;
use crate::navigator::Navigator;

/// A badge on an app's tab, its tray entry, and the dock icon.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabNotifications {
    /// Zero means no badge, rather than a badge reading nought.
    pub count: u32,
}

impl TabNotifications {
    pub fn count(count: u32) -> Self {
        Self { count }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// What an app wants the shell to do about something the user just did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum AppAction {
    #[default]
    None,
    /// Show or hide the shell's chrome — the tab strip and side panel.
    ToggleChrome,
    /// Bring the window forward and focus it. Raised by a ring, so an incoming
    /// call reaches someone who is looking at something else.
    Present,
}

/// An app's answer to being rendered.
#[derive(Default)]
pub struct AppResponse {
    pub action: Option<AppAction>,
}

impl AppResponse {
    pub fn action(action: AppAction) -> Self {
        Self {
            action: Some(action),
        }
    }
}

/// Everything an app may touch, borrowed for one pass.
///
/// Rebuilt each pass from the host's own fields, which is what keeps it a
/// bundle of `&mut` borrows rather than a pile of `Rc<RefCell<_>>`. Apps get
/// exactly this and nothing else.
pub struct AppContext<'a> {
    /// Queue navigation here. Apps never touch the real stack; the shell
    /// drains this after render. See [`crate::navigator`].
    pub navigator: &'a mut Navigator,
    /// The identity sigil acts as. Shared, because voice and chat are the same
    /// person: two apps unlocking the same file separately would ask for the
    /// passphrase twice and disagree about who you are.
    pub account: &'a mut Account,
    /// True while the main window is hidden — closed to the tray. An app
    /// should keep working and stop doing anything only a viewer would want,
    /// like animating.
    pub hidden: bool,
}

/// One hosted application: voice, chat, and whatever follows.
///
/// Only [`render`](App::render) has no default. A single-view app that never
/// pushes a route implements exactly that one method.
pub trait App {
    /// Background work, run every pass for **every opened app** — including
    /// while the window is hidden. Never draws.
    fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {}

    /// Draw. Called only for the app the user is looking at.
    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse;

    /// Draw one entry of the shell's global history.
    ///
    /// `token` is exactly the `Rc<dyn Any>` this app pushed. Downcast it back
    /// to your own route type and draw *that* view. A token you do not
    /// recognise — another app's type, or the `()` of a plain tab switch — must
    /// fall back to [`render`](App::render) or a sensible default. **Never
    /// panic on one**: the shell cannot tell them apart and will hand you
    /// whatever it holds.
    ///
    /// The default ignores the token and draws the whole app, so an app that
    /// pushes no routes needs none of this.
    fn render_nav(
        &mut self,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        token: &Rc<dyn Any>,
    ) -> AppResponse {
        let _ = token;
        self.render(ctx, ui)
    }

    /// A name for one history entry, for the back-button's dropdown. `None`
    /// falls back to the app's own name.
    fn nav_title(&self, _token: &Rc<dyn Any>) -> Option<String> {
        None
    }

    /// Free whatever a discarded route owned.
    ///
    /// The shell hands back the token of a route that has become unreachable —
    /// see [`crate::nav::Discarded`]. **In sigil this may be a live SIP-12
    /// session**, so an app that opens one must close it here; the exchange
    /// will otherwise carry it until it expires and the peer will keep sending
    /// into it.
    ///
    /// Same token contract as [`render_nav`](App::render_nav): downcast, and do
    /// nothing with one you do not recognise.
    fn dispose(&mut self, _ctx: &mut AppContext<'_>, _token: &Rc<dyn Any>) {}

    /// What to badge this app's tab with. Also feeds the tray and dock.
    fn tab_notifications(&self) -> TabNotifications {
        TabNotifications::default()
    }

    /// A short name for the tab strip.
    fn title(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Minimal;
    impl App for Minimal {
        fn render(&mut self, _: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.label("minimal");
            AppResponse::default()
        }
        fn title(&self) -> &str {
            "minimal"
        }
    }

    /// The cost of being a trivial app is one method. If this ever stops
    /// compiling, a default has been taken away.
    #[test]
    fn an_app_needs_only_render_and_title() {
        let app = Minimal;
        assert_eq!(app.title(), "minimal");
        assert!(app.tab_notifications().is_empty());
        assert_eq!(app.nav_title(&(Rc::new(()) as Rc<dyn Any>)), None);
    }

    /// `render_nav` defaulting to `render` is what lets the shell hand any app
    /// any token without checking first.
    #[test]
    fn render_nav_falls_back_to_render_for_an_unknown_token() {
        let ctx = egui::Context::default();
        let mut app = Minimal;
        let token: Rc<dyn Any> = Rc::new(9u8);
        let mut drew = false;
        let output = ctx.run_ui(Default::default(), |ui| {
            let mut nav = Navigator::default();
            let mut account = Account::Missing {
                path: "nowhere".into(),
            };
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                account: &mut account,
                hidden: false,
            };
            // The point is that this does not panic on a token the app has
            // never seen -- it quietly draws the app instead.
            let _ = app.render_nav(&mut app_ctx, ui, &token);
            drew = true;
        });
        // egui insists a FullOutput's texture deltas are dealt with; nothing
        // here paints them, so say so rather than leaking a panic on drop.
        output.drop_without_applying_deltas();
        assert!(drew, "the fallback drew rather than refusing the token");
    }

    #[test]
    fn a_zero_badge_is_no_badge() {
        assert!(TabNotifications::count(0).is_empty());
        assert!(!TabNotifications::count(1).is_empty());
    }
}
