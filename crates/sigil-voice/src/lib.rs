//! Calls and rooms, as a sigil app.
//!
//! Everything that holds a call is elsewhere: `sqex_voice::engine` runs the
//! loop, `sigil_net` runs it on a task and reports what it is doing. This draws
//! the result and collects the two decisions a person makes — who to call, and
//! when to stop.
//!
//! Nothing here awaits anything. A frame that waits on the network is a frame
//! that is not drawn, and the whole arrangement exists to make that impossible
//! rather than merely unlikely.

use sigil::account::Account;
use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};
use sigil_net::{
    CallHandle, CallOpts, CallState, Phase, RoomId, discovery, spawn_call, spawn_room,
};
use sqnr::config::Config;
use sqnr_core::PubKey;

/// How many lines of narrative to keep. Enough to see what happened during a
/// call, bounded so a machine left running for a week does not accumulate one
/// allocation per second forever.
const LOG_LIMIT: usize = 500;

/// Where you are inside the voice app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Roster,
}

pub struct VoiceApp {
    /// The key typed into the call field, and why it was refused if it was.
    peer_input: String,
    peer_trouble: Option<String>,
    /// The room secret typed in, or one just minted and not yet joined.
    room_input: String,
    room_trouble: Option<String>,
    /// The passphrase field, when the identity is sealed. Held here rather than
    /// in `Account` because it is a transient piece of interface, not a
    /// property of the identity.
    passphrase: String,
    call: Option<CallHandle>,
    /// What the engine has said, newest last.
    log: Vec<String>,
    /// The exchange, read once at startup. Re-read when settings can change it.
    config: Config,
    /// A `sigil://` link somebody has already said yes to, waiting for a pass
    /// that has a `Ui` to act in.
    ///
    /// `App::follow` is handed neither the account's held connection nor an
    /// egui context to wake, and both are wanted; and starting a call from
    /// outside the draw would be a second way in beside the button, which is
    /// how two paths that refuse differently get written. So the link sets
    /// the field the button reads and `render` presses it.
    asked: Option<Asked>,
}

/// What a followed link asked for. See [`VoiceApp::asked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Call,
    Room,
}

impl Default for VoiceApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether what was typed names somebody at another exchange rather than a
/// key (SIP-38's `name@domain`).
///
/// By shape, and decided before the key parse: a key is base58 and base58
/// has no `@`, so the two cannot be confused. Anything that is neither is
/// still refused as a key, in the same words as before -- this only takes
/// the one shape that *was* refused and had no business being.
fn is_handle(typed: &str) -> bool {
    matches!(typed.rsplit_once('@'), Some((label, domain))
        if !label.is_empty() && !domain.is_empty())
        && typed.parse::<PubKey>().is_err()
}

/// What to say when there is neither a connection to borrow nor an exchange to
/// dial. One string, because both halves of this tab say it.
const NOWHERE: &str = "no exchange configured — set SQEX_SERVER or ~/.sqnr/config";

/// The connection this identity's chat session holds, if it holds one.
///
/// This asked for the **default** exchange by name and got it wrong: an
/// identity whose exchange is named explicitly in its account settings has no
/// default session, so the tab found nothing and dialled a second connection to
/// an exchange the identity was already on. `one_of` knows the rule — the
/// default when there is one, the only one when there is not, and nothing when
/// there are several to choose between.
fn borrowable(ctx: &AppContext<'_>) -> Option<sigil_net::Held> {
    let me = ctx.account().unlocked()?.me();
    ctx.connections.one_of(me)
}

impl VoiceApp {
    pub fn new() -> Self {
        Self {
            peer_input: String::new(),
            peer_trouble: None,
            room_input: String::new(),
            room_trouble: None,
            passphrase: String::new(),
            call: None,
            log: Vec::new(),
            config: Config::load(),
            asked: None,
        }
    }

    /// Point at an exchange without reading `~/.sqnr/config`, which a test must
    /// never depend on.
    #[doc(hidden)]
    pub fn set_exchange_for_test(&mut self, host: &str, key: &str) {
        self.config.server = Some(host.to_string());
        self.config.server_key = Some(key.to_string());
    }

    /// Carry a call this app did not place, so a view of one can be drawn
    /// without an exchange to place it against.
    ///
    /// The same seam the chat app has, and for the same reason: what a call
    /// *looks like* -- a roster, a clock, the control that ends it -- is
    /// worth checking without a network, and every other way in needs one.
    #[doc(hidden)]
    pub fn hold_call_for_test(&mut self, handle: sigil_net::CallHandle) {
        self.call = Some(handle);
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            // Drain from the front in one go rather than one `remove(0)` per
            // line, which is quadratic and happens on the frame path.
            let excess = self.log.len() - LOG_LIMIT;
            self.log.drain(..excess);
        }
    }

    /// The current call's state, or a default one when there is no call.
    fn state(&self) -> CallState {
        self.call.as_ref().map(|c| c.state()).unwrap_or_default()
    }

    /// The exchange to dial, or why we cannot.
    ///
    /// Shared by calling and joining so that both refuse for the same reasons
    /// in the same words — two copies of this drifted apart in the CLI once,
    /// which is why resolution itself lives in one place.
    fn where_to(&self, account: &Account) -> Result<Vec<sigil_net::Layer>, String> {
        // The identity's own handle names its exchange (SIP-38), so an account
        // that is open needs nothing configured anywhere.
        let identity = account.unlocked().map(|u| u.path());
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, identity);
        if !discovery::any_configured(&layers) {
            return Err(NOWHERE.into());
        }
        Ok(layers)
    }

    fn place_call(
        &mut self,
        account: &Account,
        held: Option<sigil_net::Held>,
        egui_ctx: &egui::Context,
    ) {
        self.peer_trouble = None;
        let Some(unlocked) = account.unlocked() else {
            self.peer_trouble = Some("unlock your identity first".into());
            return;
        };
        let typed = self.peer_input.trim().to_string();
        // **SIP-39: a name at another exchange is not a bad key.** Typing
        // `ada@b.test` was refused with "that is not a key", which is true
        // and useless: a person whose account lives elsewhere is exactly who
        // you reach by name, and it is their home that turns the name into
        // one. The call is placed here and carried there.
        //
        // Decided before the key parse, and by the shape of what was typed:
        // a key is base58 and never has an `@` in it, so the two cannot be
        // confused. Anything else is still refused as a key, in the same
        // words as before.
        if is_handle(&typed) {
            let where_to = self.where_to(account);
            let Some(reach) =
                sigil_net::Dial::borrowed_or(held, where_to.clone().unwrap_or_default())
            else {
                self.peer_trouble = Some(where_to.err().unwrap_or_else(|| NOWHERE.into()));
                return;
            };
            let wake = egui_ctx.clone();
            self.call = Some(sigil_net::spawn_cross_call(
                reach,
                unlocked.signer(),
                typed,
                120,
                CallOpts::default(),
                move || wake.request_repaint(),
            ));
            self.log.clear();
            return;
        }
        let peer: PubKey = match typed.parse() {
            Ok(k) => k,
            Err(e) => {
                self.peer_trouble = Some(format!("that is not a key: {e}"));
                return;
            }
        };
        if peer == unlocked.me() {
            // Worth catching here rather than at the exchange, which would only
            // ever answer `Waiting`: a session needs two identities.
            self.peer_trouble = Some("that is you — a call needs somebody else".into());
            return;
        }
        // **On the connection this identity already holds**, when there is one.
        // A chat session for the same identity dials the same exchange — both
        // resolve `~/.sqnr/config` through the same layers — and a second
        // connection would cost a handshake now and a duplicate of every audio
        // frame afterwards. See `Dial::borrowed_or`.
        //
        // The exchange is only asked for when there is nothing to borrow, which
        // is why the trouble message comes second: an identity that is
        // connected can call without anything being configured here at all.
        let where_to = self.where_to(account);
        let Some(reach) = sigil_net::Dial::borrowed_or(held, where_to.clone().unwrap_or_default())
        else {
            self.peer_trouble = Some(where_to.err().unwrap_or_else(|| NOWHERE.into()));
            return;
        };

        let wake = egui_ctx.clone();
        self.call = Some(spawn_call(
            reach,
            unlocked.signer(),
            peer,
            120,
            CallOpts::default(),
            // The only thing that makes this interface redraw. Everything else
            // is idle, which is what lets a silent call cost nothing.
            move || wake.request_repaint(),
        ));
        self.log.clear();
    }

    fn join_room(
        &mut self,
        account: &Account,
        held: Option<sigil_net::Held>,
        egui_ctx: &egui::Context,
    ) {
        self.room_trouble = None;
        let Some(unlocked) = account.unlocked() else {
            self.room_trouble = Some("unlock your identity first".into());
            return;
        };
        let room: RoomId = match self.room_input.trim().parse() {
            Ok(r) => r,
            Err(e) => {
                self.room_trouble = Some(format!("that is not a room secret: {e}"));
                return;
            }
        };
        // The same rule as a call: the connection this identity already holds,
        // or the exchange it is configured for.
        let where_to = self.where_to(account);
        let Some(reach) = sigil_net::Dial::borrowed_or(held, where_to.clone().unwrap_or_default())
        else {
            self.room_trouble = Some(where_to.err().unwrap_or_else(|| NOWHERE.into()));
            return;
        };
        let wake = egui_ctx.clone();
        self.call = Some(spawn_room(
            reach,
            unlocked.signer(),
            room,
            CallOpts::default(),
            move || wake.request_repaint(),
        ));
        self.log.clear();
    }
}

impl App for VoiceApp {
    /// **A `sigil://` link, once somebody has said yes to it.** Two of the
    /// three kinds are this app's: somebody to call, and a room to join. It
    /// fills the field the button reads and asks for the press; `render`
    /// makes it, on a pass that has a `Ui` and the account's held connection.
    /// See [`VoiceApp::asked`].
    fn follow(&mut self, _ctx: &mut AppContext<'_>, link: &sigil::Link) -> bool {
        match link {
            sigil::Link::Call(who) => {
                self.peer_input = who.to_string();
                self.peer_trouble = None;
                self.asked = Some(Asked::Call);
                true
            }
            sigil::Link::Room(secret) => {
                self.room_input = secret.clone();
                self.room_trouble = None;
                self.asked = Some(Asked::Room);
                true
            }
            // A contact is the chat app's: writing to somebody is not this
            // tab's errand, and a link that opened the Calls tab to show a
            // key would be a link that went to the wrong place.
            sigil::Link::Contact(_) => false,
        }
    }

    /// Runs every pass, for every opened app, and while the window is hidden.
    /// Draining here rather than in `render` is what keeps a call's history
    /// intact while you are reading messages in the other tab.
    fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
        if let Some(call) = self.call.as_mut() {
            for event in call.drain() {
                self.note(event.describe());
            }
        }
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.spacing_mut().item_spacing.y = tokens::SPACING_SM;

        if !ctx.account().is_unlocked() {
            self.identity_ui(ctx, ui, &theme);
            return AppResponse::default();
        }
        // A link somebody said yes to, acted on exactly as the button does:
        // same refusals, same words, and not while a call is already up --
        // a link that hung up a call in progress would be a link that could.
        // The phase first, and `take` second: with the take first, a link
        // followed while a call is up would be swallowed by the pass that
        // could not act on it. Held instead, and pressed when the call ends.
        if matches!(self.state().phase, Phase::Idle | Phase::Ended)
            && let Some(asked) = self.asked.take()
        {
            let held = borrowable(ctx);
            match asked {
                Asked::Call => self.place_call(ctx.account(), held, ui.ctx()),
                Asked::Room => self.join_room(ctx.account(), held, ui.ctx()),
            }
        }
        match self.state().phase {
            Phase::Idle | Phase::Ended => self.idle_ui(ctx, ui, &theme),
            Phase::Connecting | Phase::Waiting | Phase::Live => self.call_ui(ui, &theme),
        }
        AppResponse::default()
    }

    fn title(&self) -> &str {
        "Calls"
    }

    fn icon(&self) -> sigil::Icon {
        sigil::Icon::Call
    }
}

/// A box in a row, at the width there is.
///
/// # Why not the number that was there
///
/// Every box here asked for a fixed width chosen against a 900-point window:
/// 420 for a key, 320 for a passphrase. A 360-point pane has neither, so the
/// box ran past the right edge and the button after it was off the screen
/// entirely -- and because egui grows a ui to whatever is drawn in it, the
/// prose above and below wrapped to that wider ui and was then clipped by the
/// pane. Both explanations on the Calls screen ended mid-word.
///
/// `want` is still what it should be where there is room; this only ever
/// reduces it, and below [`tokens::NARROW_WIDTH`] gives the box its own line
/// whole -- a base58 key is 44 characters, and sharing a row with two buttons
/// leaves nowhere near that.
fn box_width(ui: &egui::Ui, want: f32, after: f32) -> f32 {
    if ui.available_width() < tokens::NARROW_WIDTH {
        return (ui.available_width() - tokens::SPACING_SM).max(120.0);
    }
    want.min((ui.available_width() - after).max(120.0))
}

impl VoiceApp {
    /// Unlocking, without a terminal prompt anywhere in sight.
    fn identity_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Identity");
        ui.colored_label(theme.text_secondary, ctx.account().describe());
        ui.add_space(tokens::SPACING_SM);

        if let Account::Locked { .. } = ctx.account() {
            let field = sigil_ui::password_field(
                ui,
                &mut self.passphrase,
                "passphrase",
                box_width(ui, 320.0, 0.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || ui.button("Unlock").clicked()) && ctx.unlock_active(&self.passphrase) {
                // Only cleared on success. Making somebody retype a long
                // passphrase because the program threw it away is its own
                // small cruelty.
                self.passphrase.clear();
            }
        }
        if let Account::Missing { .. } | Account::Broken { .. } = ctx.account() {
            ui.colored_label(
                theme.text_muted,
                "Voice and chat act as an identity on the transport, so they need a \
                 software identity. A YubiKey signs but never releases a seed, and \
                 cannot be a transport key.",
            );
        }
    }

    /// No call in progress: who would you like to call?
    fn idle_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Calls");
        if let Some(me) = ctx.account().unlocked().map(|u| u.me()) {
            // In full, and selectable, because a key is the only thing that
            // actually identifies somebody (SIP-21).
            // **Wrapped, not on one line.** A base58 key is 44 characters of
            // monospace, and beside the words "You are" that is wider than a
            // phone; a `horizontal` never wraps, so the key simply left the
            // pane and took the ui's width with it, which is what clipped
            // every explanation under it.
            //
            // And small, the size the Phone tab and the Devices pane show a
            // key at: at body size forty-four monospace characters do not fit
            // a phone's width even on a line of their own, and a key broken
            // across two lines is a key somebody reads out wrong.
            // A key is 44 characters and wraps on a phone, and every
            // wrapped line of a `horizontal_wrapped` is at least
            // `interact_size.y` — a thumb's worth, around small text that
            // nobody taps. Zeroed on the ui the row is made from, as
            // `sigil_ui::message::centred` does it.
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(theme.text_secondary, "You are");
                    ui.add(
                        egui::Label::new(egui::RichText::new(me.to_string()).monospace().small())
                            .selectable(true),
                    );
                });
            });
        }
        ui.add_space(tokens::SPACING_MD);

        let state = self.state();
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        if let Some(summary) = &state.final_stats {
            ui.colored_label(theme.text_secondary, format!("Last call — {summary}"));
        }
        ui.add_space(tokens::SPACING_SM);

        // The same row every other field in sigil is: the box given the
        // width there is and its one action beside it, as a mark. This was
        // a `horizontal_wrapped` of a box at a fixed width and a "Call"
        // button, which on a phone put the button on a row of its own.
        let (_, call) = sigil_ui::labelled_field(
            ui,
            "",
            &mut self.peer_input,
            "their key, or name@domain at another exchange",
            Some(sigil_ui::Action::Mark(sigil_ui::Icon::Call, "Call")),
        );
        if call {
            let held = borrowable(ctx);
            self.place_call(ctx.account(), held, ui.ctx());
        }
        if let Some(trouble) = &self.peer_trouble {
            ui.colored_label(theme.destructive, trouble);
        }

        ui.add_space(tokens::SPACING_XL);
        self.room_entry_ui(ctx, ui, theme);
        self.log_ui(ui, theme);
    }

    /// Minting or joining a room.
    fn room_entry_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Rooms");
        ui.colored_label(
            theme.text_secondary,
            "A room is named by a secret, and holding it is what being in the room \
             consists of.",
        );
        let (_, join) = sigil_ui::labelled_field(
            ui,
            "",
            &mut self.room_input,
            "room secret, base58",
            Some(sigil_ui::Action::Word("Join")),
        );
        if join {
            let held = borrowable(ctx);
            self.join_room(ctx.account(), held, ui.ctx());
        }
        // Minting is not acting on the box, so it is not the box's mark: a
        // word, under it, for what no picture says.
        if ui.button("New room").clicked() {
            self.room_input = RoomId::generate().to_base58();
            self.room_trouble = None;
        }
        if let Some(trouble) = &self.room_trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        if !self.room_input.is_empty() {
            // Said wherever a secret is on screen, because it is the whole
            // security model and it is not what people expect from a group
            // chat: there is no owner, nobody can be removed, and anyone you
            // give it to can pass it on. Excluding somebody means a new room.
            ui.colored_label(
                theme.warning,
                "Anyone you give this to is in the room, and can give it to anyone else. \
                 It cannot be taken back — to leave somebody out, mint a new room.",
            );
        }
    }

    /// A call or a room in progress.
    fn call_ui(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        let state = self.state();
        let in_room = state.room.is_some();
        ui.heading(match (state.phase, in_room) {
            (Phase::Connecting, _) => "Connecting…",
            (Phase::Waiting, _) => "Waiting for them to answer",
            (_, true) => "In a room",
            (_, false) => "On a call",
        });

        // **The way out is pinned, and the roster scrolls.** The roster was
        // drawn first and the control that ends the call last, so a room of a
        // dozen people -- two lines each on a phone -- put Leave at y 1344 of
        // an 804-point screen, with nothing to scroll. A call somebody cannot
        // end is a microphone that stays open, and the only way out of that
        // is force-stopping the app.
        //
        // So it goes in a panel at the foot, where a phone's call controls
        // live, and everything that can grow is above it in a scroll area.
        // A desktop sees the same arrangement and has always had room for
        // it; what changes is that the room is no longer assumed.
        let leave = if in_room { "Leave" } else { "Hang up" };
        let mut hung_up = false;
        egui::Panel::bottom("call_controls")
            .frame(
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(0, tokens::SPACING_SM as i8)),
            )
            .show(ui, |ui| {
                if ui.button(leave).clicked() {
                    hung_up = true;
                }
                if let Some(stats) = &state.stats {
                    ui.colored_label(theme.text_secondary, egui::RichText::new(stats).monospace());
                }
            });
        if hung_up && let Some(call) = &self.call {
            call.hang_up();
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.call_body(&state, ui, theme));
    }

    /// Everything above the call's own controls: who is there, how it is
    /// going, and the log. All of it can grow, so all of it scrolls.
    fn call_body(&self, state: &CallState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let in_room = state.room.is_some();
        if in_room {
            self.roster_ui(state, ui);
        }

        if let Some(peer) = state.peer {
            // The same: the peer's key wraps, and a wrapped line of text
            // nobody taps should not be a thumb tall.
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(theme.text_secondary, "with");
                    ui.add(
                        egui::Label::new(egui::RichText::new(peer.to_string()).monospace())
                            .selectable(true),
                    );
                });
            });
        }
        if state.phase == Phase::Waiting {
            // The wait is mutual consent, not a fault. Say so, or it reads as
            // a program that has hung.
            ui.colored_label(
                theme.text_muted,
                "A session opens only when both sides have named the other, so nothing \
                 happens until they call you back.",
            );
        }
        if state.deaf {
            ui.colored_label(
                theme.warning,
                "Nothing has arrived from them at all — see the log below.",
            );
        }
        ui.add_space(tokens::SPACING_MD);
        self.log_ui(ui, theme);
    }

    /// Who is in the room, and who is talking.
    ///
    /// The drawing is `sigil_ui::roster`, which takes plain data; this only
    /// maps the protocol's `PeerStatus` onto it. Keeping the widget free of the
    /// wire format is what lets it be tested against a five-person room without
    /// arranging one.
    fn roster_ui(&self, state: &CallState, ui: &mut egui::Ui) {
        ui.add_space(tokens::SPACING_SM);
        let rows: Vec<sigil_ui::Row> = state
            .present
            .iter()
            .map(|p| sigil_ui::Row {
                key: p.identity.to_string(),
                speaking: p.speaking,
                level: p.level,
                detail: format!(
                    "loss {:.0}% · conceal {} · buf {}",
                    p.loss_pct, p.concealed, p.buffered
                ),
            })
            .collect();
        sigil_ui::roster(ui, &rows, state.connecting);
    }

    fn log_ui(&self, ui: &mut egui::Ui, theme: &ColorTheme) {
        if self.log.is_empty() {
            return;
        }
        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(240.0)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.colored_label(theme.text_muted, egui::RichText::new(line).monospace());
                }
            });
    }
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    /// **SIP-39: a name at another exchange is not a bad key.**
    ///
    /// Typing `ada@b.test` into the call field was refused with "that is not
    /// a key" -- true, and useless: somebody whose account lives elsewhere is
    /// exactly who you reach by name, and it is their home that turns the
    /// name into one. This is the shape that decides, on its own, because
    /// the branch it guards dials an exchange and a test must not.
    #[test]
    fn a_name_at_a_domain_is_reached_by_name_and_a_key_is_not() {
        let key = sigil::Account::unlocked_for_test([3u8; 32])
            .unlocked()
            .expect("unlocked")
            .me()
            .to_string();
        assert!(is_handle("ada@b.test"));
        assert!(is_handle("ada@sub.b.test"));
        // A key is base58 and base58 has no `@`, so the two cannot collide.
        assert!(!is_handle(&key), "a real key was read as a handle: {key}");
        // Neither is anything that is only half of one: those are still
        // refused as keys, in the same words as before.
        for not in ["", "ada", "ada@", "@b.test", "@"] {
            assert!(!is_handle(not), "{not:?} was read as a handle");
        }
    }
}

#[cfg(test)]
mod borrow_tests {
    use super::*;
    use sigil::accounts::Accounts;
    use sigil::navigator::Navigator;

    /// The tab asks for a connection this identity is holding, whatever
    /// exchange it is at.
    ///
    /// It used to ask for the **default** one by name, and that was wrong for
    /// a real configuration: an identity whose exchange is named in its
    /// account settings has no default session, so the tab found nothing and
    /// dialled a second connection to an exchange the identity was already on.
    #[test]
    fn the_tab_asks_for_a_connection_the_shown_identity_holds() {
        let account = sigil::Account::unlocked_for_test([4u8; 32]);
        let me = account.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![account]);
        let mut nav = Navigator::default();

        let nothing = sigil_net::Connections::new();
        let ctx = AppContext {
            navigator: &mut nav,
            accounts: &mut accounts,
            unfocused: true,
            away: false,
            notify: &sigil::Silent,
            connections: &nothing,
        };
        assert!(
            borrowable(&ctx).is_none(),
            "nothing is lent, so there is nothing to borrow"
        );

        // An exchange named in the account settings, which is what several of
        // these identities actually have. No default session exists.
        let named = sigil_net::Connections::new();
        named.lend(me, "squic.org", sigil_net::Held::empty());
        let ctx = AppContext {
            navigator: &mut nav,
            accounts: &mut accounts,
            unfocused: true,
            away: false,
            notify: &sigil::Silent,
            connections: &named,
        };
        assert!(
            borrowable(&ctx).is_some(),
            "the tab should borrow the one connection this identity holds, \
             whatever the exchange is called"
        );

        // Somebody else's is not ours, and two of ours with nothing to say
        // which is not a choice to make on the reader's behalf.
        let others = sigil_net::Connections::new();
        others.lend(
            sqnr_core::PubKey::new([9u8; 32]),
            "",
            sigil_net::Held::empty(),
        );
        others.lend(me, "squic.org", sigil_net::Held::empty());
        others.lend(me, "indra.org", sigil_net::Held::empty());
        let ctx = AppContext {
            navigator: &mut nav,
            accounts: &mut accounts,
            unfocused: true,
            away: false,
            notify: &sigil::Silent,
            connections: &others,
        };
        assert!(
            borrowable(&ctx).is_none(),
            "with two exchanges and no default, the tab should dial what it is \
             configured for rather than pick one"
        );
    }
}
