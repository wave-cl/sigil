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
//! keep a call alive, and its sessions listening, when it is closed to the
//! tray.
//! The host maps one onto the other.

use std::any::Any;
use std::rc::Rc;

use crate::Icon;

use crate::account::Account;
use crate::accounts::Accounts;
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

/// How hard to ask for attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// Once: a mention while away.
    Informational,
    /// Until answered: a call.
    Critical,
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
    /// Ask for the person's attention without taking it: the Dock icon
    /// bounces, the taskbar entry lights. For something worth noticing that
    /// is not worth interrupting for.
    Attention(Attention),
    /// End the process: close the window and do not come back to the tray.
    /// Raised by an update that has a new copy waiting to start. A plain
    /// close would put sigil in the tray, where an update never finishes.
    Quit,
    /// Go back to the opening screen to choose an identity.
    ///
    /// Not a switch in itself: the choosing happens on that screen, which is
    /// the one place that lists every identity in `~/.sqnr` and can ask for a
    /// passphrase. An app asking for this is asking to be *shown* it -- what
    /// is live stays live, including the identity being switched away from.
    ChooseIdentity,
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
    /// Every identity sigil is holding, and which is on screen. Shared,
    /// because voice and chat are the same person: two apps unlocking the same
    /// file separately would ask for the passphrase twice and disagree about
    /// who you are.
    ///
    /// **An app draws the active one and keeps sessions for all of them** —
    /// see [`crate::accounts`].
    pub accounts: &'a mut Accounts,
    /// True while the main window is **not in front** — behind another
    /// window, on another desktop, or closed to the tray. What decides
    /// whether something is said out loud rather than only drawn.
    ///
    /// **Not a reason to stop drawing or to stop passes.** A window behind
    /// another is still on screen, and a call in a hidden one still needs
    /// its passes; this once nearly gated a call's own repaint, which would
    /// have brought back a microphone that outlived its call. It is
    /// `!viewport().focused`, or the shell having hidden the window.
    pub unfocused: bool,
    /// True while nobody has touched this machine for a while -- five
    /// minutes, decided by the shell from the desktop's own idle time
    /// where it has one and from sigil's own input where it does not.
    /// What an app tells the exchange as *away*: connected, and nobody
    /// there. Not a reason to do less; a reason to say so.
    pub away: bool,
    /// Somewhere to say something out loud when sigil is not in front.
    ///
    /// Returns whether it went out, and a caller with nothing else to fall back
    /// on must not rely on it: notifications can be off at the desktop level
    /// with nothing here able to tell.
    pub notify: &'a dyn Notify,
    /// The connections this window holds, by identity and exchange.
    ///
    /// **One identity, one connection.** The chat session for an identity is
    /// what dials, holds and redials one; a call and the administrative console
    /// borrow it rather than dialling their own. That is not tidiness — an
    /// exchange writes a relayed datagram to every connection an identity
    /// holds, so a second one carries a duplicate of every audio frame for the
    /// length of every call, and costs a handshake at the moment somebody
    /// presses answer.
    ///
    /// Shared like `accounts` and for the same reason: voice, chat and the
    /// console are the same person talking to the same exchange.
    pub connections: &'a sigil_net::Connections,
}

impl AppContext<'_> {
    /// The account being shown. What a view draws.
    pub fn account(&self) -> &Account {
        self.accounts.active()
    }

    /// The account being shown, to change.
    pub fn account_mut(&mut self) -> &mut Account {
        self.accounts.active_mut()
    }

    /// Try a passphrase on the account being shown.
    ///
    /// Goes through the roster rather than the account so that opening one
    /// **bumps the generation** — unlocking is the moment an identity becomes
    /// usable, and an app that reconciled only on add would never start its
    /// session. Calling `account_mut().unlock(..)` would open it and tell
    /// nobody, which works right up until a second identity exists.
    pub fn unlock_active(&mut self, passphrase: &str) -> bool {
        let i = self.accounts.active_index();
        self.accounts.unlock(i, passphrase)
    }
}

/// Where a notification leads: the conversation it is about, at the
/// identity and exchange it belongs to. Pressing the notification opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub identity: sqnr_core::PubKey,
    pub exchange: String,
    pub channel: [u8; 32],
    /// The press was **Answer** on a ring, not an ordinary press.
    ///
    /// Opening the conversation is not answering it, and on a phone the two
    /// are pressed in the same place: a ring's Answer led into the window
    /// and stopped there, so the call went on ringing behind the
    /// conversation it had just opened. Answering still happens in the one
    /// place it happens from the Answer button, and this only says the
    /// person asked for it.
    pub answer: bool,
}

/// Whether a notification makes a sound, and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    #[default]
    None,
    /// The desktop's ordinary notification sound: a message.
    Default,
    /// Something more insistent: a call.
    Ring,
}

/// One notification: what it says, where it leads, what it sounds like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice<'a> {
    pub summary: &'a str,
    pub body: &'a str,
    pub target: Option<Target>,
    pub sound: Sound,
}

impl<'a> Notice<'a> {
    /// Words alone: no sound, nowhere to go.
    pub fn plain(summary: &'a str, body: &'a str) -> Notice<'a> {
        Notice {
            summary,
            body,
            target: None,
            sound: Sound::None,
        }
    }
}

/// Telling somebody something when they are not looking at sigil.
///
/// A trait so that `sigil` needs no dependency on the desktop crates, and so a
/// test can watch what would have been said without a notification daemon.
pub trait Notify {
    /// Post a notification. Returns whether it went out.
    fn notice(&self, notice: Notice<'_>) -> bool;

    /// Words alone.
    fn post(&self, summary: &str, body: &str) -> bool {
        self.notice(Notice::plain(summary, body))
    }

    /// The targets of notifications pressed since last asked.
    fn pressed(&self) -> Vec<Target> {
        Vec::new()
    }

    /// Take down a ring that is no longer ringing: answered, declined,
    /// cancelled, or simply missed.
    ///
    /// **A ring is the one notification that must be withdrawn.** It is
    /// posted ongoing, so the person cannot swipe it away, and it carries an
    /// Answer. Left up after the call has gone, it is an offer to answer
    /// something that no longer exists -- pressing it opens the conversation
    /// and nothing else happens, which is indistinguishable from a broken
    /// button. On the phone this was every ring ever posted: the platform
    /// had the call to withdraw one and nothing ever made it.
    ///
    /// Platforms whose notifications expire on their own may do nothing.
    fn withdraw(&self, _target: &Target) {}

    /// A call is up, with whoever is named; `None` when it is not.
    ///
    /// **For the platforms that have to be told a process is busy.** Android
    /// will stop an app that is not in front, microphone or no microphone,
    /// unless a foreground service says otherwise -- so a call there is a
    /// service with a notification, and without one the call ends when the
    /// screen does. `CallService.kt` was written for this and nothing ever
    /// started it, because there was nowhere for it to be started *from*.
    ///
    /// Told on change, not on the clock: the caller compares what is true
    /// now against what it last said, so a platform that turns this into a
    /// notification does not repost one every frame. A desktop, where a
    /// process stays alive because nobody is killing it, does nothing.
    fn calling(&self, _with: Option<&str>) {}
}

/// Says nothing, for tests and for a session with no desktop at all.
pub struct Silent;

impl Notify for Silent {
    fn notice(&self, _notice: Notice<'_>) -> bool {
        false
    }
}

/// One hosted application: voice, chat, and whatever follows.
///
/// Only [`render`](App::render) has no default. A single-view app that never
/// pushes a route implements exactly that one method.
pub trait App {
    /// Background work, run every pass for **every opened app** — including
    /// while the window is hidden. Never draws.
    fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {}

    /// What the background work wants the shell to do, if anything: taken
    /// once, after each [`update`](App::update). A ring asks to be
    /// presented; a mention while away asks for attention. Separate from
    /// `update`'s arguments so an app with nothing to ask changes nothing.
    fn asked(&mut self) -> Vec<AppAction> {
        Vec::new()
    }

    /// Somebody pressed a notification: show what it was about, if it is
    /// this app's. The shell has already brought the window up, and
    /// switches to the app that says yes.
    fn open(&mut self, _ctx: &mut AppContext<'_>, _target: &Target) -> bool {
        false
    }

    /// Act on a `sigil://` link, if it is this app's: a contact to write to,
    /// somebody to call, a room to join.
    ///
    /// **Somebody has already said yes.** The shell asks
    /// [`deeplink::confirmation`](crate::deeplink::confirmation) out loud and
    /// waits, because a link is a thing somebody else wrote and put where you
    /// would press it. By the time this is called that question has been
    /// answered, so this acts. The shell switches to the app that says yes,
    /// and tells whoever offered it when nothing did.
    fn follow(&mut self, _ctx: &mut AppContext<'_>, _link: &crate::Link) -> bool {
        false
    }

    /// Whether [`update`](App::update) should run before this app has ever
    /// been looked at.
    ///
    /// The default is no: an app nobody has opened costs nothing per pass.
    /// An app whose background work is the point -- one that watches for
    /// something and badges its tab when it happens -- says yes, or the
    /// badge could only appear after somebody had already gone looking.
    fn runs_unopened(&self) -> bool {
        false
    }

    /// Draw. Called only for the app the user is looking at.
    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse;

    /// Draw into the window's title strip -- the band the close, minimise and
    /// zoom buttons sit in -- at its **right-hand end**.
    ///
    /// Called only for the app on screen, in a right-to-left layout, so the
    /// first thing drawn lands against the window's right edge. The strip is
    /// one small control tall and is the window's drag region, so what goes
    /// here should be small and rare: the thing an identity is looking *at*,
    /// not the things it can do. The default draws nothing.
    ///
    /// `token` is the history entry being drawn -- the same one `nav_title`
    /// and `render_nav` are given -- so a view that is its own nav entry can
    /// put its one action here. On a phone that is where it belongs: the
    /// shell draws Back and the view's name in the bar, and the corner is
    /// the rest of the bar.
    fn chrome_ui(&mut self, _ctx: &mut AppContext<'_>, _ui: &mut egui::Ui, _token: &Rc<dyn Any>) {}

    /// Draw the **head** of a phone's app bar: its left-hand end, in a
    /// left-to-right layout, before the title.
    ///
    /// Called only on a phone, only for the app on screen. Return `true`
    /// when what was drawn names the view -- a Back button and a
    /// conversation's name, say -- and the shell draws no title after it;
    /// `false` to have the title follow whatever was drawn (an identity's
    /// mark, or nothing). The default draws nothing and leaves the title to
    /// the shell.
    fn head_ui(&mut self, _ctx: &mut AppContext<'_>, _ui: &mut egui::Ui) -> bool {
        false
    }

    /// A phone's Back button, once no menu is open and the shell's own
    /// history has nothing to go back to: take one step back within the
    /// app -- a conversation closes for the list -- and say whether there
    /// was one. With `false` the shell treats the press as Escape, which
    /// is what closes a viewer or a dialog. The default has no step.
    fn back(&mut self, _ctx: &mut AppContext<'_>) -> bool {
        false
    }

    /// Whether [`notice_ui`](App::notice_ui) has something to draw.
    ///
    /// Asked every pass, for every opened app, before the band is laid out
    /// -- a band drawn for nobody would be an empty stripe across the window.
    fn has_notice(&self) -> bool {
        false
    }

    /// Draw into the **notice band**: one line across the whole window,
    /// under the title strip and above everything else, whichever app is
    /// on screen and whether or not an identity is open.
    ///
    /// For the one thing that must be seen without anybody going to look
    /// for it -- an update that is ready, say -- with the control to act on
    /// it at the right-hand end. Rare by nature: a band that is always
    /// there is a band nobody reads. Called only while
    /// [`has_notice`](App::has_notice) says so; the default draws nothing.
    fn notice_ui(&mut self, _ctx: &mut AppContext<'_>, _ui: &mut egui::Ui) {}

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

    /// The set of identities changed: reconcile.
    ///
    /// Called when [`Accounts::generation`](crate::Accounts::generation) moves
    /// — an account added, removed, unlocked, closed, or a different one put on
    /// screen. An app holding anything per identity must bring it into line
    /// here: start what is newly unlocked, **stop what is no longer held**.
    ///
    /// This exists because the failure without it is silent. Both apps used to
    /// guard startup with `if self.session.is_some() { return }` and then never
    /// look again, so switching identity left the previous key's session
    /// running — connected, succeeding, and the wrong person. Nothing errors,
    /// no test fails, and the only symptom is messages going out as somebody
    /// else.
    ///
    /// Reconcile idempotently: this is a *"something moved"* signal, not a
    /// diff, and it may be called when nothing an app cares about has changed.
    fn accounts_changed(&mut self, _ctx: &mut AppContext<'_>) {}

    /// What to badge this app's tab with. Also feeds the tray and dock.
    fn tab_notifications(&self) -> TabNotifications {
        TabNotifications::default()
    }

    /// A short name for the tab strip.
    fn title(&self) -> &str;

    /// The mark for it on the shell's rail.
    ///
    /// The rail is icons — a word there is a label on a column of labels, and
    /// the whole point of a rail is that it is narrow. [`title`](App::title) is
    /// still what the icon *says*: every one of these carries its app's name to
    /// the accessibility tree and to a tooltip, because an icon on its own is a
    /// convention somebody has to already know.
    ///
    /// Defaulted, so a new app draws something rather than nothing.
    fn icon(&self) -> Icon {
        Icon::Public
    }
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
            let mut accounts = Accounts::of(vec![Account::Missing {
                path: "nowhere".into(),
            }]);
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &Silent,
                connections: &Default::default(),
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
