//! What a conversation looks like with messages in it.
//!
//! The data is fixed rather than fetched — see `ChatApp::show_state_for_test`.
//! Everything about *how* a message is drawn is the production path; only where
//! the messages came from is different, and `chat_session.rs` covers that
//! against a real exchange.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, theme, tokens};
use sigil_chat::{
    Attached, ChatApp, ChatState, Happened, Hit, Line, LinkState, Member, Person, Posted, Quoted,
    Receipt, Summary, Thumb, Trouble,
};
use sqnr_core::PubKey;

/// 2026-09-08 12:00:00 UTC. Pinned, because a day separator says "Today" and
/// a snapshot taken against the real clock passes until it does not.
const NOW: u64 = 1_788_004_800;
const DAY: u64 = 86_400;

/// Our own account key.
///
/// **Derived from the seed, not the seed bytes.** `unlocked_for_test` takes a
/// seed and expands it, so `PubKey::new([1u8; 32])` is a different key from the
/// account built on `[1u8; 32]` — and a fixture using both describes two people
/// while looking like it describes one.
fn me() -> PubKey {
    account().unlocked().expect("open").me()
}

fn account() -> Account {
    Account::unlocked_for_test([1u8; 32])
}
fn them() -> PubKey {
    PubKey::new([2u8; 32])
}

/// A reaction on a line: the emoji, who sent it as this conversation names
/// them, and whether one of them is us.
fn reacted(emoji: &str, who: &[&str], ours: bool) -> sigil_ui::Reaction {
    sigil_ui::Reaction {
        emoji: emoji.to_string(),
        who: who.iter().map(|n| n.to_string()).collect(),
        ours,
    }
}

fn a_conversation() -> ChatState {
    let channel = [9u8; 32];
    ChatState {
        me: Some(me()),
        prekeys: None,
        folds: 0,
        devices_known: true,
        join_trouble: None,
        not_admitted: None,
        devices_trouble: None,
        exchange: Some(PubKey::new([3u8; 32])),
        domain: Some("squic.org".into()),
        carried: None,
        link: LinkState::Up,
        trouble: None,
        posted: None,
        conversations: vec![
            Summary {
                channel,
                peer: Some(them()),
                label: "Ada".into(),
                unread: 2,
                mentioned: 0,
                avatar: None,
                waiting: false,
                preview: Some("the second one, then".into()),
                at: Some(NOW - 60),
                public: Some(false),
                group: false,
                typing: false,
            },
            Summary {
                channel: [8u8; 32],
                peer: None,
                label: "release check".into(),
                unread: 0,
                mentioned: 0,
                avatar: None,
                waiting: false,
                preview: Some("anybody may join this one".into()),
                at: Some(NOW - 2 * DAY),
                public: Some(true),
                group: true,
                typing: false,
            },
        ],
        open: Some(channel),
        copies: Vec::new(),
        stranded: Vec::new(),
        peer_home: None,
        lines: vec![
            Line {
                seq: 1,
                who: them(),
                name: Some("Ada".into()),
                mine: false,
                at: NOW - DAY - 3600,
                text: "yesterday's message, so there is a separator above today".into(),
                redacted: false,
                edited: false,
                said: None,
                via: None,
                reactions: Vec::new(),
                reply_to: None,
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
                earlier: false,
            },
            Line {
                seq: 2,
                who: me(),
                name: None,
                mine: true,
                at: NOW - 3600,
                text: "mine, on the other side".into(),
                redacted: false,
                edited: true,
                said: None,
                via: None,
                reactions: vec![reacted("\u{1f44d}", &["You", "Ada"], true)],
                reply_to: None,
                receipt: Some(Receipt::Read),
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
                earlier: false,
            },
            Line {
                seq: 3,
                who: them(),
                name: Some("Ada".into()),
                mine: false,
                at: NOW - 120,
                text: "one".into(),
                redacted: false,
                edited: false,
                said: None,
                via: None,
                reactions: Vec::new(),
                reply_to: None,
                receipt: None,
                attachments: vec![Attached {
                    // Not an image, so it draws as a named row rather than as
                    // a picture — and no bytes, because nothing has been
                    // fetched.
                    kind: 0x04,
                    described: "[notes.txt, 2.1 kB]".into(),
                    size: 2100,
                    preview: sigil_ui::attachment::no_preview().clone(),
                    bytes: None,
                    missing: false,
                    held: false,
                    duration_ms: None,
                    shape: None,
                    waveform: Default::default(),
                    id: "abc123".into(),
                }],
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
                earlier: false,
            },
            Line {
                seq: 4,
                who: them(),
                name: Some("Ada".into()),
                mine: false,
                at: NOW - 60,
                text: "the second one, then".into(),
                redacted: false,
                edited: false,
                said: None,
                via: None,
                reactions: Vec::new(),
                reply_to: Some(Quoted {
                    seq: 2,
                    who: "me".into(),
                    said: "mine, on the other side".into(),
                    preview: None,
                }),
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
                earlier: false,
            },
            Line {
                seq: 5,
                who: them(),
                name: Some("Ada".into()),
                mine: false,
                at: NOW - 30,
                text: "gone".into(),
                redacted: true,
                edited: false,
                said: None,
                via: None,
                reactions: Vec::new(),
                reply_to: None,
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
                earlier: false,
            },
        ],
        typing: false,
        // The fixture is a conversation the exchange has already answered
        // about: what this file is testing is a transcript, not a wait.
        loading: false,
        synced: true,
        trouble_with: Default::default(),
        people: [(
            them(),
            Person {
                name: Some("Ada".into()),
                // Self-declared and attested by nobody. It must not appear
                // beside the name in the transcript.
                title: Some("Exchange Administrator".into()),
                handle: Some("ada@squic.org".into()),
                picture: None,
            },
        )]
        .into_iter()
        .collect(),
        mine: Person {
            name: Some("me".into()),
            title: None,
            handle: Some("me@squic.org".into()),
            picture: None,
        },
        found: Vec::new(),
        searched: false,
        note: None,
        members: vec![
            Member {
                account: me(),
                admin: true,
                muted: false,
            },
            Member {
                account: them(),
                admin: false,
                muted: false,
            },
        ],
        i_am_admin: true,
        reports: Vec::new(),
        reports_pending: 0,
        backup: None,
        topic: String::new(),
        home: None,
        ringing: Vec::new(),
        cross_ring: None,
        succession: None,
        over: Vec::new(),
        arrivals: Vec::new(),
        unseen: Vec::new(),
        presence: std::collections::HashMap::new(),
        peers: Vec::new(),
        verified: std::collections::HashMap::new(),
        attested: std::collections::HashMap::new(),
        succeeded: std::collections::HashMap::new(),
        wake: None,
        moved_to: None,
        devices: Vec::new(),
        linked: None,
        credential: None,
        blocked: Vec::new(),
        hits: Vec::new(),
        searched_messages: false,
        divider: Some(3),
        unread_on_open: 2,
        // Between the two of today's messages, so the interleaving is what is
        // actually being drawn rather than an event tacked on the end.
        events: vec![Happened {
            seq: 3,
            at: NOW - 120,
            said: "Ada added Bram".into(),
            actor: them(),
            subject: PubKey::new([4u8; 32]),
            caveat: None,
            call: None,
        }],
        // The whole conversation, so the paging control is out of the way of
        // everything else here. `a_paged_conversation` is what covers it.
        earlier: 0,
        locked_out: None,
        mail: Vec::new(),
        // SIP-59: the fixture lives somewhere, so `me_card_phone` shows the
        // line that says where. With `None` it draws nothing and the screen
        // has no picture of it at all.
        my_home: Some((PubKey::new([7u8; 32]), "trunk.exchange".into())),
        home_moved: None,
    }
}

fn harness(dark: bool) -> Harness<'static> {
    harness_with(a_conversation(), dark)
}

/// A harness showing one of the app's inner routes, through `render_nav` —
/// `harness_at`, recording what the app asks of its session each pass.
fn harness_at_recording(
    state: ChatState,
    route: sigil_chat::Route,
    asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(route);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render_nav(&mut app_ctx, ui, &token);
            *asked.borrow_mut() = app.asked_for_test().to_vec();
        })
}

/// the same path the shell takes, so a route that draws nothing fails here.
fn harness_at(state: ChatState, route: sigil_chat::Route) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(route);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render_nav(&mut app_ctx, ui, &token);
        })
}

/// The same, with this machine's call-path preference set either way.
fn harness_at_prefs(state: ChatState, route: sigil_chat::Route, direct: bool) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    accounts.prefs.set_direct_calls(direct);
    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(route);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render_nav(&mut app_ctx, ui, &token);
        })
}

/// A harness whose identity is connected to more than one exchange.
/// A harness holding more than one identity.
fn harness_with_accounts(state: ChatState, accounts: Vec<Account>) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(accounts);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        })
}

/// A harness that keeps what the app asked the *navigator* for.
///
/// Every other harness here builds a fresh `Navigator` inside the closure, so
/// a route the app pushes is thrown away with it -- which makes "pressing this
/// goes there" untestable, and is why the header's controls had no test
/// covering where they lead.
fn harness_watching_routes(
    state: ChatState,
    routes: std::rc::Rc<std::cell::RefCell<Vec<sigil_chat::Route>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            // Drained here rather than by a shell: the token is opaque by
            // design, so it is downcast back to this app's own route type,
            // which is exactly what the shell hands back to `render_nav`.
            for request in nav.take() {
                let token = match request {
                    sigil::navigator::NavRequest::PushActive(e)
                    | sigil::navigator::NavRequest::ReplaceActive(e) => e.token,
                    sigil::navigator::NavRequest::Push(e)
                    | sigil::navigator::NavRequest::Replace(e) => e.token,
                    _ => continue,
                };
                if let Some(route) = token.downcast_ref::<sigil_chat::Route>() {
                    routes.borrow_mut().push(route.clone());
                }
            }
        })
}

/// A harness that keeps what the app asked the shell for, pass by pass.
///
/// Every other harness here drops the render's answer on the floor, which is
/// fine while the answer is always the default -- and blind the moment it is
/// not. What an app *returns* is the whole of how it reaches the shell.
fn harness_watching_asks(
    state: ChatState,
    asks: std::rc::Rc<std::cell::RefCell<Vec<sigil::app::AppAction>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            if let Some(action) = app.render(&mut app_ctx, ui).action {
                asks.borrow_mut().push(action);
            }
        })
}

/// The strip the window's own buttons sit in, as the shell draws it.
///
/// Where the exchange control lives now, so this harness draws it: a top
/// panel one small control tall, the app's corner of it laid out from the
/// right. What the shell does is the contract; see `Shell::ui`.
const STRIP: f32 = sigil::tokens::BUTTON_SM;

fn harness_at_exchanges(state: ChatState, extra: &[&str]) -> Harness<'static> {
    harness_showing_exchange(state, extra, None)
}

/// The same, looking at one of them: what the exchange control calls the
/// selected row, and the one whose key it shows in full.
fn harness_showing_exchange(
    state: ChatState,
    extra: &[&str],
    shown: Option<&str>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    for name in extra {
        assert!(accounts.add_exchange(0, name, None));
    }
    if let Some(name) = shown {
        accounts.show_exchange(me(), Some(name.to_string()));
    }
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            egui::Panel::top("chrome")
                .exact_size(STRIP)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let corner = ui
                        .max_rect()
                        .shrink2(egui::vec2(sigil::tokens::SPACING_SM, 0.0));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(corner)
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        |ui| {
                            let mut nav = Navigator::default();
                            let mut app_ctx = AppContext {
                                navigator: &mut nav,
                                accounts: &mut accounts,
                                unfocused: false,
                                away: false,
                                notify: &sigil::Silent,
                                connections: &Default::default(),
                            };
                            // A desktop's title strip: the app's root,
                            // which has no name of its own.
                            let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(());
                            app.chrome_ui(&mut app_ctx, ui, &token);
                        },
                    );
                });
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        })
}

/// Open the exchange control in the title strip.
fn open_exchanges(h: &mut Harness<'static>) {
    h.get_by_label("Exchange").click();
    h.run();
}

fn harness_with(state: ChatState, dark: bool) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        accounts: &mut accounts,
                        unfocused: false,
                        away: false,
                        notify: &sigil::Silent,
                        connections: &Default::default(),
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

/// A phone: 412 by 915 points, told what it is, with a status bar and a
/// gesture bar over it. The same app, drawn the way the shell draws it
/// there -- the route reached through `render_nav`, as the shell does.
/// The phone the app is checked on: a OnePlus NE2213, 1080×2412 pixels at
/// 3 points per pixel. Not a Pixel's 412: at 412 every phone test passed
/// while the phone overflowed.
const PHONE_WIDTH: f32 = 360.0;
const PHONE_HEIGHT: f32 = 804.0;
/// The pane the chat gets on it: the whole screen, there being no rail on
/// a phone. Kept as its own name because the tests that use it are about
/// the pane, not the screen.
const PHONE_PANE: f32 = PHONE_WIDTH;

fn harness_phone(state: ChatState, route: sigil_chat::Route) -> Harness<'static> {
    harness_phone_with(state, route).0
}

/// The same phone, in the light theme.
///
/// Every phone render was dark, so the light one had never been looked at
/// on a 360-point pane -- and the two themes are two sets of colours, not
/// one set inverted: a contrast that works on a dark ground can vanish on a
/// light one without anything else changing.
fn harness_phone_light(state: ChatState, route: sigil_chat::Route) -> Harness<'static> {
    harness_phone_themed(state, route, egui::Theme::Light).0
}

/// How wide the pane's contents actually came out, per route.
///
/// egui grows a ui to whatever is drawn in it, and that is the whole
/// mechanism behind every phone layout fault found so far: one row wider
/// than the pane does not merely stick out, it re-lays the rows after it for
/// a pane that wide -- so a member's action buttons were painted over the
/// member above, the reports below started off the left edge, and the
/// console's prose was clipped mid-word. The width of the ui after a pass
/// *is* that fault, in one number, and it needs no renderer to read.
type Drawn = std::rc::Rc<std::cell::Cell<f32>>;

/// The phone harness, and the app it drives, for a test that has to read
/// what a press sent.
fn harness_phone_with(
    state: ChatState,
    route: sigil_chat::Route,
) -> (Harness<'static>, std::rc::Rc<std::cell::RefCell<ChatApp>>) {
    let (h, app, _) = harness_phone_measured(state, route);
    (h, app)
}

#[allow(clippy::type_complexity)]
fn harness_phone_measured(
    state: ChatState,
    route: sigil_chat::Route,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::RefCell<ChatApp>>,
    Drawn,
) {
    harness_phone_themed(state, route, egui::Theme::Dark)
}

/// What the app asked the shell for, in the order it asked.
type Asks = std::rc::Rc<std::cell::RefCell<Vec<sigil::app::AppAction>>>;

/// The phone, for a test about what a press asks the *shell* to do -- which
/// on a phone includes Back, since the shell is what a Back key reaches.
fn harness_phone_asks(state: ChatState, route: sigil_chat::Route) -> (Harness<'static>, Asks) {
    let asks = Asks::default();
    let (h, _, _) = harness_phone_recording(
        state,
        route,
        egui::Theme::Dark,
        asks.clone(),
        Routes::default(),
    );
    (h, asks)
}

/// Where a press on this phone asked to go.
type Routes = std::rc::Rc<std::cell::RefCell<Vec<sigil_chat::Route>>>;

/// The phone, for a test about which card a press leads to.
fn harness_phone_routes(state: ChatState, route: sigil_chat::Route) -> (Harness<'static>, Routes) {
    let routes = Routes::default();
    let (h, _, _) = harness_phone_recording(
        state,
        route,
        egui::Theme::Dark,
        Asks::default(),
        routes.clone(),
    );
    (h, routes)
}

#[allow(clippy::type_complexity)]
fn harness_phone_themed(
    state: ChatState,
    route: sigil_chat::Route,
    theme_choice: egui::Theme,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::RefCell<ChatApp>>,
    Drawn,
) {
    harness_phone_recording(
        state,
        route,
        theme_choice,
        Asks::default(),
        Routes::default(),
    )
}

#[allow(clippy::type_complexity)]
fn harness_phone_recording(
    state: ChatState,
    route: sigil_chat::Route,
    theme_choice: egui::Theme,
    asks: Asks,
    routes: Routes,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::RefCell<ChatApp>>,
    Drawn,
) {
    harness_phone_beside(state, route, theme_choice, asks, routes, Vec::new())
}

/// The phone with the other apps beside this one, as the shell publishes
/// them.
///
/// Empty in every other harness, which is the honest answer for one app on
/// its own: there is nowhere else to go, and the Settings card draws no way
/// to nowhere.
#[allow(clippy::type_complexity)]
fn harness_phone_beside(
    state: ChatState,
    route: sigil_chat::Route,
    theme_choice: egui::Theme,
    asks: Asks,
    routes: Routes,
    siblings: Vec<sigil::Sibling>,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::RefCell<ChatApp>>,
    Drawn,
) {
    let drawn: Drawn = std::rc::Rc::new(std::cell::Cell::new(0.0));
    let width = drawn.clone();
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let app = std::rc::Rc::new(std::cell::RefCell::new(app));
    let shared = app.clone();
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(route);
    let h = Harness::builder()
        .with_size(egui::vec2(PHONE_WIDTH, PHONE_HEIGHT))
        // A tap is a press and a release within egui's click duration; at
        // the harness's default quarter-second per frame, a `run` between
        // the two can outlast it. See `with_inset` in the shell's tests.
        .with_step_dt(0.05)
        .build_ui(move |ui| {
            let mut app = app.borrow_mut();
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(theme_choice);
            let t = sigil::ColorTheme::current(&ctx);
            let mut nav = Navigator::default();
            nav.set_siblings(siblings.clone());
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            // The phone's app bar, as the shell composes it: the app's head
            // at the left, the title after it unless the head named the
            // view, and the app's corner from the right.
            egui::Panel::top("app_bar")
                .exact_size(sigil::tokens::BUTTON_LG)
                .frame(egui::Frame::NONE.fill(t.surface_secondary))
                .show(ui, |ui| {
                    let whole = ui.max_rect();
                    // **The corner first, and measured**, exactly as the
                    // shell does it: both used to be given the whole bar and
                    // drawn one over the other, so a long conversation name
                    // was painted under the call and More buttons.
                    let corner = whole.shrink2(egui::vec2(sigil::tokens::SPACING_SM, 0.0));
                    let drawn = ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(corner)
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        |ui| app.chrome_ui(&mut app_ctx, ui, &token),
                    );
                    let used = drawn.response.rect.width();
                    let mut left = whole.shrink2(egui::vec2(sigil::tokens::SPACING_MD, 0.0));
                    left.max.x = (left.max.x - used - sigil::tokens::SPACING_SM).max(left.min.x);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(left)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            // **As the shell composes it, not as this file
                            // used to.** The harness drew the app's head and
                            // fell back to the product's name, and the shell
                            // asks `nav_title` first -- so every phone
                            // snapshot showed "Sigil" over a pane whose real
                            // bar says "Devices", and the duplicate name and
                            // Back button in the pane below were invisible
                            // here for as long as they existed.
                            match app.nav_title(&token) {
                                Some(title) => {
                                    if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                                        app_ctx.navigator.back();
                                    }
                                    ui.label(egui::RichText::new(title).heading());
                                }
                                None => {
                                    if !app.head_ui(&mut app_ctx, ui) {
                                        ui.label(egui::RichText::new(sigil::NAME).heading());
                                    }
                                }
                            }
                        },
                    );
                });
            // The phone's Back, as the shell handles it: a menu closes; else
            // the app takes a step back; else it is Escape.
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack)) {
                if egui::Popup::is_any_open(&ctx) {
                    egui::Popup::close_all(&ctx);
                } else if !app.back(&mut app_ctx) {
                    ctx.input_mut(|i| {
                        i.events.push(egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                    });
                }
            }
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_MD as i8)),
                )
                .show(ui, |ui| {
                    if let Some(action) = app.render_nav(&mut app_ctx, ui, &token).action {
                        asks.borrow_mut().push(action);
                    }
                    // What the pane came out as, margins included: the
                    // number `no_phone_pane_is_wider_than_the_phone` reads.
                    width.set(ui.min_rect().width() + 2.0 * sigil::tokens::SPACING_MD);
                });
            // Where a press asked to go, downcast back to this app's own
            // routes, which is what the shell hands to `render_nav`.
            for request in nav.take() {
                let token = match request {
                    sigil::navigator::NavRequest::PushActive(e)
                    | sigil::navigator::NavRequest::ReplaceActive(e) => e.token,
                    sigil::navigator::NavRequest::Push(e)
                    | sigil::navigator::NavRequest::Replace(e) => e.token,
                    _ => continue,
                };
                if let Some(route) = token.downcast_ref::<sigil_chat::Route>() {
                    routes.borrow_mut().push(route.clone());
                }
            }
        });
    (h, shared, drawn)
}

/// A finger: the touch event egui-winit would forward, and the pointer it
/// emulates from it -- the harness feeds raw input, so both are supplied.
fn finger_down(h: &mut Harness<'static>, at: egui::Pos2) {
    let input = h.input_mut();
    input.events.push(egui::Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(1),
        phase: egui::TouchPhase::Start,
        pos: at,
        force: None,
    });
    input.events.push(egui::Event::PointerMoved(at));
    input.events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
}

fn finger_up(h: &mut Harness<'static>, at: egui::Pos2) {
    let input = h.input_mut();
    input.events.push(egui::Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(1),
        phase: egui::TouchPhase::End,
        pos: at,
        force: None,
    });
    input.events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    input.events.push(egui::Event::PointerGone);
}

/// The same conversation with everything in it as long as it can really be.
///
/// # Why the benign fixture is not enough
///
/// `a_conversation()` is "Ada", "release check", "notes.txt". Nothing in it
/// is longer than a phone, so a pane could pass the width check on it and
/// still be torn apart by the first person with a long display name, a room
/// with a sentence for a topic, or a photo straight off a camera. A width
/// check run only on data that fits is a check that agrees with itself.
///
/// Nothing here is invented for the test: these are the shapes real data
/// takes. A base58 key used as a name is what an unnamed contact reads as; a
/// camera's filename is what an attachment is called; a URL is a single word
/// with no break in it, which is the one thing wrapping cannot help.
fn a_long_conversation() -> ChatState {
    let mut state = a_conversation();
    const LONG_NAME: &str = "Alexandra Constantinopoulos-Whitmore";
    const LONG_TOPIC: &str =
        "everything about the release, including what we decided not to do and why";
    const URL: &str =
        "https://example.org/a/very/long/path/that/never/breaks?and=a&query=string&too=yes";

    state.conversations[0].label = LONG_NAME.into();
    state.conversations[0].preview = Some(URL.into());
    state.conversations[1].label = LONG_TOPIC.into();
    for line in &mut state.lines {
        if line.name.is_some() {
            line.name = Some(LONG_NAME.into());
        }
        for a in &mut line.attachments {
            a.described = "[PXL_20260921_184433912.MP.long-name-from-a-camera.jpg, 8.4 MB]".into();
        }
    }
    if let Some(first) = state.lines.first_mut() {
        first.text = URL.into();
    }
    for member in &mut state.members {
        state
            .people
            .entry(member.account)
            .or_default()
            .name
            .clone_from(&Some(LONG_NAME.to_string()));
    }
    state.found = vec![sigil_chat::Found {
        channel: [4u8; 32],
        instance: [2u8; 32],
        name: LONG_TOPIC.into(),
        topic: LONG_TOPIC.into(),
        members: 1,
        domain: "an-exchange-with-a-long-name.example.org".into(),
        here: false,
    }];
    state.searched = true;
    state.reports = vec![sigil_chat::Report {
        id: 7,
        reporter: them(),
        target: 3,
        reason: "harassment",
        at: NOW - 3600,
        note: LONG_TOPIC.into(),
    }];
    state
}

/// **Nothing on screen is drawn past the phone's edge** -- including the
/// things `no_phone_pane_is_wider_than_the_phone` cannot see.
///
/// That check reads the pane's own ui, which is the right instrument for the
/// pane and blind to everything on a layer of its own: a dialog, a menu, the
/// emoji picker, a message's action strip. This one asks the accessibility
/// tree where every widget actually is, so it covers whatever is up.
///
/// A widget a little past the edge is how a control becomes unreachable, and
/// on a phone there is no window to widen.
/// The routes the three phone-layout tests must cover, held to the enum.
///
/// All three listed five of the seven by hand — `no_phone_pane_is_wider_
/// than_the_phone`, `no_widget_on_any_route_is_drawn_off_the_screen` and
/// `nothing_on_a_phone_is_drawn_where_it_cannot_be_reached` — so Search and
/// Me were covered by none of them, which is exactly the shape those tests
/// exist to stop. This stops compiling if a `Route` is added and the lists
/// are not extended: the table is something the code has to satisfy rather
/// than something somebody remembers.
fn _every_route_is_measured(r: sigil_chat::Route) {
    match r {
        sigil_chat::Route::Conversations
        | sigil_chat::Route::Directory
        | sigil_chat::Route::Members
        | sigil_chat::Route::Settings
        | sigil_chat::Route::Devices
        | sigil_chat::Route::Search
        | sigil_chat::Route::Me
        | sigil_chat::Route::Call(_) => {}
    }
}

fn nothing_runs_off_the_edge(h: &Harness<'static>, what: &str) {
    // The accesskit node's own bounding box, not `Node::rect`: that one
    // `expect`s a rectangle and the root has none, so it panics -- a test
    // failing for a reason that has nothing to do with the layout. The box
    // is in physical pixels, hence the scale.
    fn walk(node: egui_kittest::Node<'_>, ppp: f64, out: &mut Vec<(String, (f64, f64))>) {
        let n = node.accesskit_node();
        let name = n
            .label()
            .map(|l| l.to_string())
            .or_else(|| n.value().map(|v| v.to_string()))
            .unwrap_or_else(|| format!("{:?}", n.role()));
        if let Some(b) = n.bounding_box() {
            out.push((name, (b.x0 / ppp, b.x1 / ppp)));
        }
        for c in node.children() {
            walk(c, ppp, out);
        }
    }
    let mut seen = Vec::new();
    walk(h.root(), h.ctx.pixels_per_point() as f64, &mut seen);
    assert!(
        seen.len() > 3,
        "{what}: only {} widgets on screen, so this proves nothing",
        seen.len()
    );
    let edge = PHONE_WIDTH as f64;
    let over: Vec<String> = seen
        .iter()
        .filter(|(_, (x0, x1))| x1 - x0 > 0.0 && (*x1 > edge + 1.0 || *x0 < -1.0))
        .map(|(name, (x0, x1))| format!("{name:?} at {x0:.0}..{x1:.0}"))
        .collect();
    assert!(
        over.is_empty(),
        "{what}: {} widget(s) are drawn outside a {PHONE_WIDTH}-point screen:\n  {}",
        over.len(),
        over.join("\n  ")
    );
}

/// **No route draws wider than the phone.**
///
/// The general form of every phone fault found here, and the one check that
/// would have caught all of them at once: Members overflowed by some 200
/// points, the operator console and the Calls app by more. Each was found by
/// rendering the screen and looking at it, which only happens when somebody
/// thinks to render that screen -- and three of these five routes had no
/// render at all until today.
///
/// It needs no renderer, so it runs in an ordinary `cargo test`, which is
/// where a layout regression should be caught rather than in the snapshot
/// job somebody runs before a release.
#[test]
fn no_phone_pane_is_wider_than_the_phone() {
    for (what, build) in [
        ("ordinary", a_conversation as fn() -> ChatState),
        ("long", a_long_conversation as fn() -> ChatState),
    ] {
        for route in [
            sigil_chat::Route::Conversations,
            sigil_chat::Route::Directory,
            sigil_chat::Route::Members,
            sigil_chat::Route::Settings,
            sigil_chat::Route::Devices,
            sigil_chat::Route::Search,
            sigil_chat::Route::Me,
            // **The fallback, not the card.** `Route::Call` draws a card
            // only while a call is held, and `CallHandle::for_test` wants a
            // tokio runtime these three do not have. With no call it
            // replaces itself with the conversations route, so what is
            // measured here is that the fallback fits and does not panic.
            // The card's own width is held by `call_card_ui` in sigil-ui and
            // by `tests/call_card.rs`, which has a runtime and a call.
            sigil_chat::Route::Call(me()),
        ] {
            let mut state = build();
            // A hit to find, a report to show: a pane that draws nothing cannot
            // overflow, and an empty pass would be a vacuous pass.
            state.found = vec![sigil_chat::Found {
                channel: [4u8; 32],
                instance: [2u8; 32],
                name: "elsewhere".into(),
                topic: "held at another exchange".into(),
                members: 1,
                domain: "trunk.exchange".into(),
                here: false,
            }];
            state.searched = true;
            state.reports = vec![sigil_chat::Report {
                id: 7,
                reporter: them(),
                target: 3,
                reason: "spam",
                at: NOW - 3600,
                note: "links".into(),
            }];
            let (mut h, _, drawn) = harness_phone_measured(state, route.clone());
            h.run();
            h.run();
            let width = drawn.get();
            assert!(
                width > 0.0,
                "{route:?} ({what}) drew nothing, so this proves nothing about it"
            );
            // A point of slack for the rounding egui does on a margin; the
            // faults this catches were tens of points, not fractions.
            assert!(
                width <= PHONE_WIDTH + 1.0,
                "{route:?} with {what} names draws {width} points wide in a \
             {PHONE_WIDTH}-point pane. egui grows a ui to what is drawn in it, \
             so every row after the one that overflowed is laid out for a pane \
             that wide -- which is how a roster's buttons end up painted over \
             the member above them."
            );
        }
    }
}

/// Every route, and every widget in it, inside the screen.
///
/// The companion to `no_phone_pane_is_wider_than_the_phone`. That one reads
/// the pane's own ui, which catches a ui grown wider than the pane and is
/// blind to anything on a layer of its own; this one asks where each widget
/// actually *is*, which catches one positioned outside and covers popups and
/// menus too. Neither subsumes the other, and both run without a renderer.
#[test]
fn no_widget_on_any_route_is_drawn_off_the_screen() {
    for (what, build) in [
        ("ordinary", a_conversation as fn() -> ChatState),
        ("long", a_long_conversation as fn() -> ChatState),
    ] {
        for route in [
            sigil_chat::Route::Conversations,
            sigil_chat::Route::Directory,
            sigil_chat::Route::Members,
            sigil_chat::Route::Settings,
            sigil_chat::Route::Devices,
            sigil_chat::Route::Search,
            sigil_chat::Route::Me,
            // **The fallback, not the card.** `Route::Call` draws a card
            // only while a call is held, and `CallHandle::for_test` wants a
            // tokio runtime these three do not have. With no call it
            // replaces itself with the conversations route, so what is
            // measured here is that the fallback fits and does not panic.
            // The card's own width is held by `call_card_ui` in sigil-ui and
            // by `tests/call_card.rs`, which has a runtime and a call.
            sigil_chat::Route::Call(me()),
        ] {
            let mut state = build();
            state.found = vec![sigil_chat::Found {
                channel: [4u8; 32],
                instance: [2u8; 32],
                name: "elsewhere".into(),
                topic: "held at another exchange".into(),
                members: 1,
                domain: "trunk.exchange".into(),
                here: false,
            }];
            state.searched = true;
            state.reports = vec![sigil_chat::Report {
                id: 7,
                reporter: them(),
                target: 3,
                reason: "spam",
                at: NOW - 3600,
                note: "links".into(),
            }];
            let mut h = harness_phone(state, route.clone());
            h.run();
            h.run();
            nothing_runs_off_the_edge(&h, &format!("{route:?} with {what} names"));
        }
    }
}

/// On a phone every field and its button fit the width: the key field in
/// Devices asked for 300 points and got them whether or not the pane had
/// them, and the button after it fell off the screen.
#[test]
fn on_a_phone_the_devices_pane_fits_its_width() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    let button = h.get_by_label("Write credential").rect();
    assert!(
        button.right() <= PHONE_WIDTH,
        "the button after the key field is off the screen: {button:?}"
    );
    assert!(button.left() > 0.0);
}

/// On a phone the conversation bar is the way back, the name, one More
/// button, the call, and the identity. Six controls beside the identity
/// were wider than the row, and a right-to-left row that overflows pushes
/// Back off the left edge and drags the transcript after it.
#[test]
fn on_a_phone_the_conversation_bar_fits_the_screen() {
    let mut state = a_conversation();
    for c in state.conversations.iter_mut() {
        c.label = "general".into();
    }
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    let width = PHONE_PANE;
    h.set_size(egui::vec2(width, PHONE_HEIGHT));
    h.run();
    h.run();
    let back = h.get_by_label("Back").rect();
    let more = h.get_by_label("More about this conversation").rect();
    assert!(back.left() >= 0.0, "Back is off the left edge: {back:?}");
    assert!(more.right() <= width, "{more:?}");
    assert!(
        (back.center().y - more.center().y).abs() < tokens::SPACING_SM,
        "the bar wrapped: back {back:?}, more {more:?}"
    );
    // The bar is the app bar: everything in it is in the top finger's
    // height, and nothing else heads the transcript.
    assert!(back.top() < sigil::tokens::BUTTON_LG, "{back:?}");
    assert!(
        h.query_by_label("Your identity").is_none(),
        "the identity is on the list's bar, not a conversation's"
    );
    let bubble = topmost(&h, "the second one, then");
    assert!(
        bubble.left() >= 0.0,
        "the transcript spilled left: {bubble:?}"
    );
    // The name has room to be read whole: a seven-letter name in the
    // heading size, not "ge…" beside a chevron that said what the mark
    // beside it says.
    let name = topmost(&h, "general");
    assert!(name.width() > 60.0, "the name is cut short: {name:?}");
    assert!(name.right() <= more.left(), "{name:?} runs into {more:?}");
    // The rest of the controls are behind More, and come out of it.
    assert!(
        h.query_by_label("Settings").is_none(),
        "Settings is in the bar"
    );
    h.get_by_label("More about this conversation").click();
    h.run();
    assert!(h.query_by_label("Settings").is_some());
    assert!(h.query_by_label("Devices").is_some());
}

/// On a phone the composer is the whole width and a little taller, with
/// the paperclip inside it at the right and no Send button: the keyboard's
/// own key sends.
#[test]
fn on_a_phone_the_composer_is_the_whole_width_with_the_paperclip_inside() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    let width = PHONE_PANE;
    h.set_size(egui::vec2(width, PHONE_HEIGHT));
    h.run();
    h.run();
    assert!(
        h.query_by_label("Send").is_none(),
        "a Send button on the phone"
    );
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
        .expect("the composer");
    let edge = width - 2.0 * sigil::tokens::SPACING_MD;
    assert!(field.right() >= edge, "the box stops short: {field:?}");
    assert!(field.height() >= sigil::tokens::FIELD_LG - 0.5, "{field:?}");
    let clip = h.get_by_label("Attach a file").rect();
    assert!(
        field.contains_rect(clip),
        "the paperclip is not in the box: {clip:?} vs {field:?}"
    );
    assert!(
        clip.right() > field.center().x,
        "the paperclip is at the right"
    );
}

/// On a phone the search box is the whole width, with the magnifier
/// inside it at the right. On its own card, which is where it now is.
#[test]
fn on_a_phone_the_search_box_is_the_whole_width_with_the_magnifier_inside() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Search);
    h.run();
    h.run();
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the search box");
    let edge = PHONE_WIDTH - 2.0 * sigil::tokens::SPACING_MD;
    assert!(field.right() >= edge, "the box stops short: {field:?}");
    // The card's own name is "Search" too, in the bar above: the one in
    // the box is the one inside the box.
    let glass = h
        .get_all_by_label("Search")
        .map(|n| n.rect())
        .find(|r| field.contains_rect(*r))
        .expect("the magnifier is not in the box");
    assert!(
        glass.right() > field.center().x,
        "the magnifier is at the right"
    );
}

/// No row is wider than the pane, whatever is in it. A file's row did
/// not wrap, so a long name pushed its bubble past the pane -- and egui
/// grows a ui to what is drawn in it, so every message after that one
/// was laid out for a pane 35 points wider than the phone: a message that
/// wrapped ran off the right edge, and one's own were right-aligned to an
/// edge past the screen.
#[test]
fn on_a_phone_no_row_is_wider_than_the_pane() {
    let mut state = a_conversation();
    let ada = state
        .lines
        .iter_mut()
        .find(|l| !l.mine && l.text == "the second one, then")
        .expect("Ada's line");
    ada.text = "posted at squic.org, ordered at trunk.exchange (SIP-43)".into();
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.set_size(egui::vec2(PHONE_PANE, PHONE_HEIGHT));
    h.run();
    h.run();
    let edge = PHONE_PANE - sigil::tokens::SPACING_MD;
    for label in [
        "posted at squic.org, ordered at trunk.exchange (SIP-43)",
        "mine, on the other side",
        "[notes.txt, 2.1 kB]",
        "Save",
    ] {
        let r = topmost(&h, label);
        assert!(r.right() <= edge, "{label:?} runs past the pane: {r:?}");
        assert!(r.left() >= 0.0, "{label:?} is off the left: {r:?}");
    }
    // One's own message sits against the pane's edge, not past it: its
    // furniture (the time, the receipt) ends inside the bubble.
    let mine = topmost(&h, "mine, on the other side");
    assert!(mine.right() < edge - sigil::tokens::SPACING_XL, "{mine:?}");
}

/// **A tap on the app bar is not a tap on the transcript**, even where the
/// transcript's topmost message is scrolled up behind it.
///
/// Found on a OnePlus NE2213: in a scrolled conversation, a tap anywhere
/// along the app bar -- the More button, the title, even the status bar
/// above it -- put an action strip on the topmost partly-visible message,
/// one nobody had touched. It outlived the menu that went up with it.
///
/// The cause is that the strip's hit test is `reach.contains(pointer)` on
/// the bubble's *layout* rect, and `Rect::contains` knows nothing of clip
/// rects or layers: a message scrolled half out of view still has a rect
/// reaching up behind the bar, so a point there is "in" it.
#[test]
fn a_tap_on_the_app_bar_does_not_reveal_a_message_scrolled_behind_it() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.set_size(egui::vec2(PHONE_PANE, PHONE_HEIGHT));
    h.run();
    h.run();
    // Scrolled, so the topmost message's rect runs up past the bar.
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run_steps(8);
    assert!(
        h.query_by_label("Reply").is_none(),
        "a strip is showing before anything was pressed"
    );
    // Down the whole bar, because which y lands inside a hidden bubble
    // depends on where the scroll stopped -- and none of them is the
    // transcript.
    let bar = sigil::tokens::BUTTON_LG;
    for step in 0..=8 {
        let at = egui::pos2(PHONE_PANE / 2.0, bar * (step as f32) / 8.0);
        finger_down(&mut h, at);
        h.run();
        finger_up(&mut h, at);
        h.run();
        h.run();
        assert!(
            h.query_by_label("Reply").is_none(),
            "a tap on the app bar at y={} revealed a message's action strip",
            at.y
        );
    }
}

/// **No strip at all** stays no strip when the conversation's menu opens.
///
/// `a_hidden_strip_does_not_come_back_when_another_menu_opens` is the case
/// next door: it reveals a strip, hides it, and checks it stays hidden. It
/// never asked what happens when there was never a strip, which is the
/// ordinary way somebody opens that menu, so that case had no test at all.
///
/// The phone's own version of this -- a tap on the app bar conjuring a strip
/// -- is `a_tap_on_the_app_bar_does_not_reveal_a_message_scrolled_behind_it`,
/// which is where that fault was finally caught: it needed the transcript
/// scrolled far enough that a bubble's rect ran up behind the bar, which
/// this sequence never does.
#[test]
fn a_menu_does_not_conjure_a_strip_that_was_never_shown() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.set_size(egui::vec2(PHONE_PANE, PHONE_HEIGHT));
    h.run();
    h.run();
    assert!(
        h.query_by_label("Reply").is_none(),
        "a strip is showing before anything was pressed"
    );
    // Scrolled, as a real transcript is: the phone showed the strip on the
    // topmost *visible* message, which is only a distinct thing when the
    // transcript is longer than the pane.
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    // `run_steps`, not `run`: a scroll asks for repaints while it settles,
    // and `run` refuses a ui that keeps repainting.
    h.run_steps(6);
    let more = h
        .get_by_label("More about this conversation")
        .rect()
        .center();
    // **Without `PointerGone`.** That is the difference between this harness
    // and the phone: `finger_up` pushes it, so the pointer is nowhere and
    // nothing can be hovered; Android leaves the last touch position in
    // place, so a rect containing it goes on reading as hovered after the
    // finger has lifted.
    {
        let input = h.input_mut();
        input.events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase: egui::TouchPhase::Start,
            pos: more,
            force: None,
        });
        input.events.push(egui::Event::PointerMoved(more));
        input.events.push(egui::Event::PointerButton {
            pos: more,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    {
        let input = h.input_mut();
        input.events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase: egui::TouchPhase::End,
            pos: more,
            force: None,
        });
        input.events.push(egui::Event::PointerButton {
            pos: more,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    h.run();
    assert!(
        h.query_by_label("Settings").is_some(),
        "the menu did not open, so this says nothing about the strip"
    );
    assert!(
        h.query_by_label("More").is_none(),
        "opening the conversation's menu put a message's action strip on the \
         transcript, on a message nobody had touched"
    );
}

/// A point on the transcript that is not on the strip.
///
/// The strip hangs off the bubble's top-outer corner and reaches back over
/// it, so a point "beside the bubble" is often on the strip -- and a tap
/// there is the strip's, not a tap away from it. This is below the pill and
/// out in the margin, and it asserts as much, so a test cannot quietly
/// start measuring nothing.
fn away_from_the_strip(h: &Harness<'static>) -> egui::Pos2 {
    // The last cell is More on either form: Reply is a cell of its own on a
    // wide pane and a row of the More menu on a phone.
    let pill = h
        .get_by_label(sigil_emoji::QUICK[0])
        .rect()
        .union(h.get_by_label("More").rect());
    let away = egui::pos2(6.0, pill.bottom() + 60.0);
    assert!(
        !pill.contains(away),
        "the point meant to be away from the strip is on it: {away:?} in {pill:?}"
    );
    away
}

/// A strip that has been put away stays away when some other menu opens.
/// The strip holds while *its* menu is open, by remembering which message
/// it was -- and it remembered after the strip had gone, so the phone's
/// title menu (or the identity's, anywhere) brought the last strip back.
#[test]
fn a_hidden_strip_does_not_come_back_when_another_menu_opens() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.set_size(egui::vec2(PHONE_PANE, PHONE_HEIGHT));
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    let on = bubble.center();
    finger_down(&mut h, on);
    h.run();
    finger_up(&mut h, on);
    h.run();
    h.run();
    assert!(h.query_by_label("More").is_some(), "the tap did not reveal");
    let away = away_from_the_strip(&h);
    finger_down(&mut h, away);
    h.run();
    finger_up(&mut h, away);
    h.run();
    h.run();
    assert!(
        h.query_by_label("More").is_none(),
        "the tap away did not hide"
    );
    // Some other menu: the conversation's More.
    let more = h
        .get_by_label("More about this conversation")
        .rect()
        .center();
    finger_down(&mut h, more);
    h.run();
    finger_up(&mut h, more);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Settings").is_some(),
        "the menu did not open"
    );
    assert!(
        h.query_by_label("More").is_none(),
        "the strip came back with the menu"
    );
}

/// On a phone the app bar is the identity's mark and the product's name
/// over the list, and Back and the conversation's name over a
/// conversation: one bar, not a bar under a bar.
#[test]
fn on_a_phone_the_app_bar_heads_the_list_with_the_identity_and_a_conversation_with_back() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let mark = h.get_by_label("Your identity").rect();
    assert!(mark.top() < sigil::tokens::BUTTON_LG, "{mark:?}");
    let name = topmost(&h, sigil::NAME);
    assert!(mark.right() <= name.left(), "the mark is left of the name");
    assert!(h.query_by_label("Back").is_none(), "nothing to go back to");

    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let back = h.get_by_label("Back").rect();
    assert!(back.top() < sigil::tokens::BUTTON_LG, "{back:?}");
    assert!(
        h.query_by_label(sigil::NAME).is_none(),
        "the bar is the conversation's"
    );
    assert!(h.query_by_label("Your identity").is_none());
}

/// A name in a bubble sits close to the words under it. The phone's theme
/// makes every row a finger tall, for buttons; inside a bubble that put a
/// finger's height between a name and the words.
#[test]
fn on_a_phone_a_name_sits_close_to_its_words() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let words = h.get_by_label_contains("yesterday's message").rect();
    let names: Vec<egui::Rect> = h
        .get_all_by_label("Ada")
        .map(|n| n.rect())
        .filter(|r| r.bottom() <= words.top())
        .collect();
    let name = names
        .iter()
        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
        .expect("Ada's name over her words");
    let gap = words.top() - name.bottom();
    assert!(
        gap <= sigil::tokens::SPACING_MD,
        "a finger's height between the name and the words: {gap}"
    );
}

/// The time is the furthest right thing on a message, whatever else is
/// beside it: "edited", a receipt, where it came from.
#[test]
fn the_time_is_furthest_right_on_a_message() {
    let mut h = harness(true);
    h.run();
    h.run();
    let edited = topmost(&h, "edited");
    let clock = sigil_ui::clock(NOW - 3600);
    let time = h
        .get_all_by_label(&clock)
        .map(|n| n.rect())
        .find(|r| (r.center().y - edited.center().y).abs() < 2.0)
        .expect("the time on the edited message's row");
    assert!(
        edited.right() <= time.left(),
        "edited {edited:?} is right of the time {time:?}"
    );
}

/// The phone's Back leaves a conversation for the list, the way the bar's
/// own Back does.
#[test]
fn on_a_phone_back_leaves_a_conversation_for_the_list() {
    let (mut h, app) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(h.query_by_label("Back").is_some(), "a conversation is open");
    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    // A fixed state has no session to close, so the ask is what shows.
    let sent = app.borrow().sent_for_test().to_vec();
    assert!(
        sent.iter().any(|c| c == "Close"),
        "Back did not ask to close the conversation: {sent:?}"
    );
}

/// **Back closes the viewer, and does not also leave the conversation.**
///
/// The phone's Back is a chain: a menu closes, else the app takes a step,
/// else it is Escape, which closes a viewer or a dialog. `App::back` returns
/// false while a picture is open *so that* the Escape arm is reached -- and
/// if it did not, one press would close the picture and leave the
/// conversation, two steps for one press, which reads as the app losing
/// its place.
///
/// Only the last link of that chain had a test.
#[test]
fn on_a_phone_back_closes_an_open_picture_and_stays_in_the_conversation() {
    let (mut h, app) = harness_phone_with(with_pictures(3), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let tile = h.get_by_label("[image 0, 4 KiB]").rect();
    press_at(&mut h, tile.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("of 3").is_some(),
        "the picture did not open, so this says nothing about closing it: {}",
        text_of(&h)
    );

    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("of 3").is_none(),
        "Back left the picture open: {}",
        text_of(&h)
    );
    let sent = app.borrow().sent_for_test().to_vec();
    assert!(
        !sent.iter().any(|c| c == "Close"),
        "one press closed the picture *and* left the conversation: {sent:?}"
    );

    // And the next press does leave it, which is the step after.
    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    let sent = app.borrow().sent_for_test().to_vec();
    assert!(
        sent.iter().any(|c| c == "Close"),
        "with the picture gone, Back leaves the conversation: {sent:?}"
    );
}

/// In the list, a conversation's name sits close to its last words. The
/// phone's theme makes every row a finger tall, for buttons; in a list
/// row that put a finger's height between the name and the preview.
#[test]
fn on_a_phone_a_conversations_name_sits_close_to_its_last_words() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let preview = h.get_by_label_contains("the second one, then").rect();
    let name = h
        .get_all_by_label("Ada")
        .map(|n| n.rect())
        .filter(|r| r.bottom() <= preview.top())
        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
        .expect("the conversation's name over its preview");
    let gap = preview.top() - name.bottom();
    assert!(
        gap <= sigil::tokens::SPACING_SM,
        "a finger's height between the name and the preview: {gap}"
    );
}

/// In the list, the name and the last words together are centred on the
/// mark beside them, not stacked at the top of the row.
#[test]
fn on_a_phone_a_conversations_lines_are_centred_on_its_mark() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.run();
    let preview = h.get_by_label_contains("the second one, then").rect();
    let name = h
        .get_all_by_label("Ada")
        .map(|n| n.rect())
        .filter(|r| r.bottom() <= preview.top())
        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
        .expect("the conversation's name over its preview");
    let lines = name.union(preview);
    // The mark is the presence on Ada's row: the one on the lines' row.
    let mark = h
        .get_all_by_label(sigil_ui::Presence::Offline.word())
        .map(|n| n.rect())
        .find(|r| r.top() <= lines.center().y && lines.center().y <= r.bottom() + 20.0)
        .expect("Ada's mark beside her lines");
    let off = (lines.center().y - mark.center().y).abs();
    assert!(
        off <= 2.0,
        "the lines are {off} off the mark's middle: lines {lines:?}, mark {mark:?}"
    );
}

/// A dialog on a phone is as wide as the screen has, not 360 points.
#[test]
fn on_a_phone_a_dialog_fits_the_screen() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    // Behind the heading's dots on a phone.
    h.get_by_label("More choices").click();
    h.run();
    h.get_by_label("Write to somebody").click();
    h.run();
    h.run();
    // The compose dialog's field takes the dialog's width; both must be
    // inside the screen with the margin the dialog keeps.
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .fold(None::<egui::Rect>, |acc, r| {
            Some(acc.map_or(r, |a| a.union(r)))
        })
        .expect("the dialog has a field");
    assert!(
        field.right() <= PHONE_WIDTH && field.left() >= 0.0,
        "{field:?}"
    );
}

/// Under a finger, a tap on a message reveals its actions and a tap
/// elsewhere puts them away; under a pointer that has never been a finger,
/// nothing is revealed by a click.
#[test]
fn a_tap_reveals_a_messages_actions_and_a_second_tap_hides_them() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    assert!(
        h.query_by_label("More").is_none(),
        "nothing is revealed before anybody touches"
    );
    // A tap: down, a frame, up.
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label("More").is_some(),
        "a tap on the message reveals its strip"
    );
    // A tap away -- below the strip and out in the margin, where there is
    // neither strip nor bubble -- puts it away.
    let away = away_from_the_strip(&h);
    finger_down(&mut h, away);
    h.run();
    finger_up(&mut h, away);
    h.run();
    h.run();
    assert!(
        h.query_by_label("More").is_none(),
        "a tap elsewhere puts the strip away"
    );
}

/// A press held still on a message opens a menu of its actions where the
/// finger is, once, and holding on does not open it again.
#[test]
fn a_long_press_on_a_message_opens_its_menu() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    assert!(h.query_by_label("Copy key").is_none());
    finger_down(&mut h, bubble.center());
    // Held: twenty steps of a twentieth of a second is a second, and a long
    // press is eight tenths.
    h.run_steps(20);
    assert!(
        h.query_by_label("Copy key").is_some(),
        "a held press opens the actions menu"
    );
    h.run_steps(10);
    assert_eq!(
        h.get_all_by_label("Copy key").count(),
        1,
        "the press still held opens no second menu"
    );
    finger_up(&mut h, bubble.center());
    h.run();
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_conversation() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_conversation");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_chats() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_chats");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_devices() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    h.snapshot("phone_devices");
}

/// Search results on a phone.
///
/// A hit is a conversation's name, who said it, a time, and the words around
/// the match -- four things on a 360-point row, one of them a fragment of a
/// sentence. It had only ever been drawn in a wide column.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_search() {
    let mut state = a_search();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Search);
    h.run();
    search_for(&mut h, "release");
    h.run();
    h.remove_cursor();
    h.run();
    h.snapshot("phone_search");
}

/// And it fits, with a long conversation name and a match deep in a long
/// line -- which is where a hit row is widest.
#[test]
fn the_search_results_fit_a_phone() {
    let mut state = a_search();
    state.open = None;
    for hit in &mut state.hits {
        hit.label = "Alexandra Constantinopoulos-Whitmore".into();
    }
    let (mut h, _, drawn) = harness_phone_measured(state, sigil_chat::Route::Search);
    h.run();
    search_for(&mut h, "release");
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("release is Thursday"),
        "no results are on screen, so this proves nothing about their width"
    );
    let width = drawn.get();
    assert!(
        width <= PHONE_WIDTH + 1.0,
        "the results draw {width} points wide in a {PHONE_WIDTH}-point pane"
    );
}

/// Type into the chat list's search box, as `search_dark` does: the leftmost
/// text field on screen. Typed rather than set, because what puts the
/// results on screen is the box being changed.
fn search_for(h: &mut Harness<'static>, what: &str) {
    let field = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the search box");
    field.focus();
    field.type_text(what);
}

/// A conversation with a search's worth of hits in it.
fn a_search() -> ChatState {
    let mut state = a_conversation();
    state.searched_messages = true;
    let long = format!(
        "{}so the release check moves to Thursday, bring the notes",
        "and another thing, ".repeat(6)
    );
    let deep = long.find("release").unwrap();
    state.hits = vec![
        Hit {
            channel: [9u8; 32],
            seq: 7,
            label: "release check".into(),
            who: "Ada".into(),
            text: "the release is Thursday".into(),
            found: 4..11,
            at: NOW - 120,
        },
        Hit {
            channel: [9u8; 32],
            seq: 3,
            label: "release check".into(),
            who: "You".into(),
            text: long,
            found: deep..deep + 7,
            at: NOW - 86_400,
        },
        Hit {
            channel: [8u8; 32],
            seq: 40,
            label: "Grace".into(),
            who: "Grace".into(),
            text: "no release without the notes\nand the notes are late".into(),
            found: 3..10,
            at: NOW - 3 * 86_400,
        },
    ];
    state
}

/// The chat list on a phone in the light theme.
///
/// Every phone render was dark. The two themes are two sets of colours, not
/// one inverted, so a contrast that reads on a dark ground can go to nothing
/// on a light one -- muted text and a disabled control most of all.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_chats_light() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone_light(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_chats_light");
}

/// And the transcript, where the two bubble grounds and the words on them
/// are the contrast that matters most.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_conversation_light() {
    let mut h = harness_phone_light(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_conversation_light");
}

/// The conversation on a phone with everything in it as long as it gets: a
/// display name, a filename from a camera, a URL that cannot be broken.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_long_conversation() {
    let mut h = harness_phone(a_long_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_long_conversation");
}

/// The same, with the list showing rather than a conversation.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_long_chats() {
    let mut state = a_long_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_long_chats");
}

/// SIP-48's backup on a phone, with the 24 words showing.
///
/// The devices pane is long and this is the foot of it, so no phone render
/// had ever reached it: the words are a six-column grid of `"NN. word"` in
/// monospace, which is a shape chosen for a window.
fn a_backup() -> ChatState {
    let mut state = a_conversation();
    // BIP39 words, and the longest ones there are: a grid that fits
    // "ab" and fails on "wrestle" fits nothing worth writing down.
    const WORDS: [&str; 8] = [
        "abandon", "wrestle", "youthful", "zebra", "vacuum", "universe", "tornado", "squirrel",
    ];
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: None,
        used: 4096,
        quota: 1_048_576,
        words: Some((0..24).map(|i| WORDS[i % 8].to_string()).collect()),
    });
    state
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_backup() {
    let mut h = harness_phone(a_backup(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    // Down to the foot of the pane, which is where the words are -- the
    // devices view is long and a snapshot of its top says nothing about
    // them. The pointer has to be in the pane: the wheel goes to whatever
    // is under it, and without this the snapshot was of the top with the
    // scroll silently going nowhere.
    for _ in 0..12 {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                PHONE_WIDTH / 2.0,
                PHONE_HEIGHT / 2.0,
            )));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    // The words really are on screen, or this is a picture of the pane's
    // middle labelled as its backup.
    assert!(
        h.get_all_by_label_contains("wrestle")
            .any(|n| n.rect().bottom() <= PHONE_HEIGHT && n.rect().top() >= 0.0),
        "the backup words are not on screen, so this snapshot is not of them"
    );
    h.remove_cursor();
    // The scroll is still settling, and `run` refuses a ui that keeps
    // asking to repaint.
    h.run_steps(4);
    h.snapshot("phone_backup");
}

/// And it fits, which is the part a picture cannot be trusted to show at a
/// glance: a grid that overflows takes the rest of the pane with it.
#[test]
fn the_backup_words_fit_a_phone() {
    let (mut h, _, drawn) = harness_phone_measured(a_backup(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("wrestle"),
        "the words are not on screen, so this proves nothing about them"
    );
    let width = drawn.get();
    assert!(
        width <= PHONE_WIDTH + 1.0,
        "the backup words draw {width} points wide in a {PHONE_WIDTH}-point pane"
    );
    nothing_runs_off_the_edge(&h, "the backup words");
}

/// **The foot of a pane can be got to.**
///
/// Not "the content fits", which it need not: a pane taller than the screen
/// is ordinary, and scrolling is the answer. What is not ordinary is a pane
/// taller than the screen with **nothing to scroll**, and that is what
/// Devices was. With the backup's 24 words showing it reaches 1825 points on
/// an 804-point screen, and the words -- the whole of what opens the backup,
/// on the one screen somebody copies them from -- were below the fold with
/// no way down. So were Restore and Drop. Members and the directory have had
/// a scroll area all along; this pane was simply never given one.
///
/// Asked as reachability rather than as "is there a ScrollArea", because
/// reachability is the property and a widget's rect says it plainly.
#[test]
fn the_foot_of_the_devices_pane_can_be_reached_on_a_phone() {
    let mut h = harness_phone(a_backup(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    let last = "wrestle";
    let on_screen = |h: &Harness<'static>| {
        h.get_all_by_label_contains(last)
            .any(|n| n.rect().bottom() <= PHONE_HEIGHT && n.rect().top() >= 0.0)
    };
    assert!(
        h.get_all_by_label_contains(last).next().is_some(),
        "the words are not drawn at all, so this says nothing about reaching them"
    );
    assert!(
        !on_screen(&h),
        "the words are already on screen without scrolling, so this test is \
         not about a pane taller than its screen any more"
    );

    let before = h
        .get_all_by_label_contains(last)
        .next()
        .map(|n| n.rect().top())
        .unwrap_or(0.0);
    for _ in 0..12 {
        // The wheel goes to whatever is under the pointer, so the pointer
        // has to be in the pane.
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                PHONE_WIDTH / 2.0,
                PHONE_HEIGHT / 2.0,
            )));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    let after = h
        .get_all_by_label_contains(last)
        .next()
        .map(|n| n.rect().top())
        .unwrap_or(0.0);
    assert!(
        after < before,
        "nothing moved: the words were at {before} and are at {after}, so \
         this pane does not scroll at all"
    );
    assert!(
        on_screen(&h),
        "scrolling never brought the backup words into view: the foot of this \
         pane cannot be reached on a phone"
    );
}

/// **Settings fits the screen, and has nothing to scroll if it stops.**
///
/// Devices reached 1825 points on an 804-point screen with nothing to
/// scroll, and its backup words were simply unreachable. Settings is the
/// other pane with no scroll area, and today it fits -- 692 points with the
/// longest name and topic, as an admin, with the destroy confirmation open,
/// which is everything it can show at once.
///
/// So this is not a fix, it is the tripwire: the day it stops fitting, the
/// failure is silent and identical to Devices', and the answer is the same
/// scroll area. Measured against everything showing rather than the ordinary
/// case, because the ordinary case has a hundred points of slack and would
/// go quiet long before somebody with a long topic noticed.
/// The same tripwire, for a **room**.
///
/// `a_conversation()` is a direct message, so the whole `!dm` half of the
/// settings pane — the name, the topic, authorising a replica, and moving
/// where the conversation lives — was drawn by no test at all. That half is
/// the taller one, and the move is the newest thing in it.
#[test]
fn the_settings_pane_fits_a_phone_for_a_room_too() {
    let mut state = a_long_conversation();
    state.i_am_admin = true;
    // A room: no peer, so the admin half draws.
    if let Some(open) = state.open
        && let Some(s) = state.conversations.iter_mut().find(|c| c.channel == open)
    {
        s.peer = None;
        s.group = true;
        s.label = "the square".into();
    }
    let mut h = harness_phone(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    // The control this exists for: it is the newest row on the pane and the
    // one that can strand messages, so it must be on the screen to be read.
    let moved = h
        .get_all_by_label_contains("Move where this conversation lives")
        .next()
        .map(|b| b.rect());
    let moved = moved.expect("an admin in a room is offered the move");
    assert!(
        moved.bottom() <= PHONE_HEIGHT + 1.0 && moved.top() >= -1.0,
        "the move sits at y {:.0}..{:.0} of {PHONE_HEIGHT} — off the screen it \
         cannot be read before it is pressed",
        moved.top(),
        moved.bottom()
    );
}

#[test]
fn the_settings_pane_still_fits_a_phone_or_needs_what_devices_needed() {
    let mut state = a_long_conversation();
    state.i_am_admin = true;
    let mut h = harness_phone(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    // The destroy confirmation, which is the tallest this pane gets.
    let at = h
        .get_all_by_label_contains("Destroy")
        .next()
        .map(|b| b.rect().center());
    let at = at.expect("an admin is offered Destroy");
    press_at(&mut h, at);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Yes, destroy it"),
        "the confirmation did not open, so this is not the tallest the pane gets"
    );

    fn deepest(node: egui_kittest::Node<'_>, ppp: f64, out: &mut f64) {
        if let Some(b) = node.accesskit_node().bounding_box() {
            *out = out.max(b.y1 / ppp);
        }
        for c in node.children() {
            deepest(c, ppp, out);
        }
    }
    let mut bottom = 0.0;
    deepest(h.root(), h.ctx.pixels_per_point() as f64, &mut bottom);
    assert!(
        bottom <= PHONE_HEIGHT as f64,
        "Settings reaches {bottom:.0} of {PHONE_HEIGHT} and has no scroll \
         area, so the foot of it cannot be got to -- give it the one Devices \
         was given"
    );
}

/// **A dialog's own controls stay on the screen.**
///
/// The modal bounds its *width* -- "a phone is narrower than a dialog" --
/// and not its height, and a modal does not scroll. A dialog taller than the
/// screen is one somebody can neither finish nor leave: the way out is the
/// Cancel at the foot of it, and Escape only works if they know to try.
///
/// The Exchange dialog is the tall one -- three paragraphs, a field, a
/// checkbox and two buttons -- and it is reached from the directory, which
/// is where somebody adds the exchange a room they found lives at.
#[test]
fn a_dialogs_controls_stay_on_a_phones_screen() {
    let mut state = a_conversation();
    state.open = None;
    state.found = vec![sigil_chat::Found {
        channel: [4u8; 32],
        instance: [2u8; 32],
        name: "elsewhere".into(),
        topic: "held at another exchange".into(),
        members: 1,
        domain: "an-exchange-with-a-long-name.example.org".into(),
        here: false,
    }];
    state.searched = true;
    let mut h = harness_phone(state, sigil_chat::Route::Directory);
    h.run();
    h.run();
    let add = h
        .get_all_by_label_contains("Add exchange")
        .next()
        .expect("a room living elsewhere offers its exchange")
        .rect()
        .center();
    press_at(&mut h, add);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Add an exchange"),
        "the dialog did not open, so this says nothing about its controls"
    );

    for label in ["Add", "Cancel"] {
        let lowest = h
            .get_all_by_label(label)
            .map(|n| n.rect().bottom())
            .fold(f32::MIN, f32::max);
        assert!(
            lowest > f32::MIN,
            "{label} is not drawn, so this says nothing about reaching it"
        );
        assert!(
            lowest <= PHONE_HEIGHT,
            "the dialog's {label} sits at y {lowest:.0} of {PHONE_HEIGHT}: \
             upright, this dialog should need no scrolling at all"
        );
    }
}

/// **A dialog too tall for the screen can still be left.**
///
/// A rotated phone is about 360 points tall -- the app is not orientation
/// locked -- and the Verify dialog is a QR, six words, a key, a checkbox and
/// two answers, some 660 points. Lying down it is nearly twice the screen,
/// and it is exactly the dialog somebody opens while holding the phone next
/// to the person whose words they are reading.
///
/// # It did not fit, and now it does
///
/// This used to assert the opposite -- that the buttons at its foot were off
/// the bottom -- and stood as a record of a dialog that could only be
/// finished by turning the phone upright. The obvious answer, a scroll area
/// inside the modal, is worse and still is: in egui 0.36 a `ScrollArea`
/// inside a `Modal` makes every press inside the dialog dismiss it, which
/// two existing tests caught immediately. Tried with each `auto_shrink` and
/// inside a sensing scope of its own; all three dismiss. The answer was a
/// second column, which a short screen has the width for.
///
/// The floor stays whatever the layout does: Escape leaves it, which is what
/// the phone's Back sends, so a dialog is never a trap.
#[test]
fn a_dialog_too_tall_for_the_screen_can_still_be_left() {
    const WIDE: f32 = 804.0;
    const SHORT: f32 = 360.0;
    let (mut h, _, _) = harness_phone_measured(a_conversation(), sigil_chat::Route::Members);
    h.set_size(egui::vec2(WIDE, SHORT));
    h.run();
    h.run();
    let verify = h
        .get_all_by_label("Verify")
        .map(|n| n.rect())
        .next()
        .expect("a member who is not me may be verified")
        .center();
    press_at(&mut h, verify);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Read these six words"),
        "the dialog did not open, so this says nothing: {}",
        text_of(&h)
    );

    // **It fits now**, which is what `every_dialog_fits_a_phone_held_sideways`
    // is for and what this test used to assert the opposite of: the code and
    // the key went into a second column and the dialog came down from 482
    // points to under 360. Asserted here too, because this is the one that
    // presses the real button on the real pane rather than setting the
    // dialog directly.
    let out = h
        .get_all_by_label("Not yet")
        .map(|n| n.rect().bottom())
        .fold(f32::MIN, f32::max);
    assert!(
        out > f32::MIN,
        "the way out is not drawn at all, so this says nothing about it"
    );
    assert!(
        out <= SHORT,
        "the way out is at {out:.0} of {SHORT}: the dialog does not fit lying \
         down, and it cannot scroll"
    );

    // And the floor stands whatever happens to the layout: Back leaves it.
    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Read these six words"),
        "Back did not leave a dialog whose own controls are off the screen, \
         which would make it a trap"
    );
}

/// The directory on a phone: the search box, and a hit with what may be
/// done about it. Three of the five routes had no phone render at all,
/// which is three screens nobody had looked at on a 360-point pane.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_directory() {
    let mut state = a_conversation();
    state.open = None;
    state.found = vec![
        sigil_chat::Found {
            channel: [9u8; 32],
            instance: [1u8; 32],
            name: "the square".into(),
            topic: "anybody may join this one".into(),
            members: 3,
            domain: String::new(),
            here: true,
        },
        sigil_chat::Found {
            channel: [4u8; 32],
            instance: [2u8; 32],
            name: "elsewhere".into(),
            topic: "held at another exchange".into(),
            members: 1,
            domain: "trunk.exchange".into(),
            here: false,
        },
    ];
    state.searched = true;
    let mut h = harness_phone(state, sigil_chat::Route::Directory);
    h.run();
    h.run();
    h.snapshot("phone_directory");
}

/// Who is in the room, on a phone: a roster row is a mark, a name and the
/// admin's controls, and on 360 points that row is the one most likely to
/// run off the edge.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_members() {
    let mut state = a_conversation();
    state.reports = vec![sigil_chat::Report {
        id: 7,
        reporter: them(),
        target: 3,
        reason: "spam",
        at: NOW - 3600,
        note: "links".into(),
    }];
    let mut h = harness_phone(state, sigil_chat::Route::Members);
    h.run();
    h.run();
    h.snapshot("phone_members");
}

/// Compose, on a phone: the one dialog that had a render, at a width it
/// never had. A dialog is bounded by the pane it is in, and a phone is
/// narrower than a dialog was built for.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_compose() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    // Behind the heading's dots on a phone.
    h.get_by_label("More choices").click();
    h.run();
    h.get_by_label("Write to somebody").click();
    h.run();
    h.run();
    // The harness leaves the pointer where it clicked, and it lands on
    // the dialog.
    h.remove_cursor();
    h.run();
    nothing_runs_off_the_edge(&h, "the compose dialog");
    h.snapshot("phone_dialog_compose");
}

/// SIP-41's safety words, on a phone. Six words and two answers, and the
/// dialog somebody is most likely to open while standing next to the person
/// they are comparing them with -- which is to say, on a phone.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_verify() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Members);
    h.run();
    h.get_by_label("What may be done about them").click();
    h.run();
    h.get_by_label("Verify").click();
    h.run();
    h.run();
    // The harness leaves the pointer where it clicked, and it lands on
    // the dialog.
    h.remove_cursor();
    h.run();
    nothing_runs_off_the_edge(&h, "the verify dialog");
    h.snapshot("phone_dialog_verify");
}

/// SIP-56's report, on a phone: a reason to choose and a note to write,
/// and who it reaches.
///
/// The fixture is a **direct message**, where the control names one — a
/// room is what the public channel beside it is. It used to say "room"
/// whatever it was open on.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_report() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Members);
    h.run();
    h.get_by_label("Report this conversation…").click();
    h.run();
    h.run();
    // The harness leaves the pointer where it clicked, and it lands on
    // the dialog.
    h.remove_cursor();
    h.run();
    nothing_runs_off_the_edge(&h, "the report dialog");
    h.snapshot("phone_dialog_report");
}

/// The conversation's own settings on a phone: name, topic, retention.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_settings() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Settings);
    h.run();
    h.run();
    h.snapshot("phone_settings");
}

/// Put the conversation list away.
///
/// It is **on screen from the start**: signing in lands on the chats with the
/// newest one open. A test about the transcript alone hides it, both to give
/// the transcript the width and because a row's preview repeats the text of
/// the message it previews — which makes a query for that text ambiguous
/// rather than wrong, and an ambiguous query fails loudly.
fn hide_column(h: &mut Harness<'static>) {
    h.get_by_label("Hide the chats").click();
    h.run();
    // The pointer is left where it clicked, which draws the button hovered
    // and the cursor over it in anything captured afterwards. A picture of a
    // transcript should not have a mouse in it.
    h.remove_cursor();
    h.run();
}

/// Open the identity block's menu.
///
/// Your key, your exchanges and the other identities you hold moved here from
/// the head of the conversation column. **One gesture away is still
/// reachable**; replaced by a name would not be.
fn open_identity(h: &mut Harness<'static>) {
    h.get_by_label("Your identity").click();
    h.run();
}

fn text_of(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

/// Every label on screen, whole.
///
/// `text_of` joins them, which is right for "is this sentence anywhere" and
/// wrong for "is there a control saying exactly `2`" -- a substring search for
/// a bare number matches a timestamp, an unread pill and half the keys.
/// A long press, as egui sees one: it turns a long touch into a secondary
/// click, so the same gesture is a right-click on a desktop and a held
/// finger on a phone, and a case can send the click it becomes.
fn long_press(h: &mut Harness<'static>, at: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
}

fn labels(h: &Harness<'static>) -> Vec<String> {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        // Both, and as separate entries. A plain `Label` carries its text as
        // the node's *value* and has no label at all, so collecting only
        // labels finds no ordinary text on the screen -- and an assertion that
        // some text is absent then passes over an empty list.
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found
}

#[test]
fn a_deleted_message_leaves_a_tombstone_rather_than_vanishing() {
    // Redaction keeps the entry and empties the body: **the gap is the
    // record**. A client that removed the row would destroy the one thing a
    // redaction is for, and nobody could tell a deletion from a message that
    // was never sent.
    let mut h = harness(true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Deleted"), "{said}");
    assert!(
        !said.contains("gone"),
        "the body itself must not survive: {said}"
    );
}

#[test]
fn an_edited_message_says_so() {
    // Showing an edit as though it were the original hides that the text
    // changed after somebody read it.
    let mut h = harness(true);
    h.run();
    assert!(text_of(&h).contains("edited"));
}

#[test]
fn a_day_boundary_is_marked_and_today_is_named() {
    let mut h = harness(true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Today"), "{said}");
    assert!(said.contains("Yesterday"), "{said}");
}

#[test]
fn the_unread_divider_says_how_many() {
    let mut h = harness(true);
    h.run();
    assert!(text_of(&h).contains("2 new messages"));
}

/// The markers that belong to the whole conversation are centred.
///
/// The day, the unread mark and what happened to the channel were not said by
/// anybody. Pinned to the left they read as a message from whoever is on that
/// side; centred, they read as what they are. Asserted as geometry rather than
/// looked at in a snapshot, because "it drifted 200px left" and "it is fine"
/// are the same picture to a threshold.
#[test]
fn a_marker_that_belongs_to_nobody_is_centred() {
    let mut h = harness(true);
    h.run();
    // The bubbles say where the transcript is: the marker has to be centred
    // in *that*, not in the window, and the conversation list is half of one
    // and none of the other. One of ours is right-aligned and one of theirs
    // is left-aligned, so between them they span it.
    //
    // The rightmost thing in our bubble, which is no longer the words: the
    // time and the receipt follow them on the same row now, so the receipt is
    // the bubble's right-hand end. (Our text also appears twice -- once as
    // the bubble and once quoted in the reply below -- hence the fold.)
    let mine = h
        .get_all_by_label_contains("mine, on the other side")
        .map(|n| n.rect())
        .chain(std::iter::once(h.get_by_label("read").rect()))
        .fold(f32::MIN, |right, r| right.max(r.right()));
    let theirs = h.get_by_label_contains("yesterday's message").rect().left();
    let pane = egui::Rect::from_x_y_ranges(theirs..=mine, 0.0..=1.0);
    for marker in ["2 new messages", "Ada added Bram", "Today"] {
        let rect = h.get_by_label_contains(marker).rect();
        let off = (rect.center().x - pane.center().x).abs();
        assert!(
            off < 24.0,
            "{marker} sits {off:.0}px off the middle of the transcript              ({:?} in {:?})",
            rect.center(),
            pane
        );
    }
}

/// A group gaining and losing people shows in the conversation.
///
/// The exchange signs an entry for every membership and metadata change and
/// the fold used to discard all twelve kinds, so a channel could be created,
/// gain four people and lose one with nothing on screen to show for it.
#[test]
fn what_happened_to_the_channel_is_in_the_transcript() {
    let mut h = harness(true);
    h.run();
    assert!(
        text_of(&h).contains("Ada added Bram"),
        "the exchange's own record is not drawn: {}",
        text_of(&h)
    );
}

/// **A key is always reachable from a name.** These events name people, and a
/// name is an assertion attested by nobody.
#[test]
fn an_event_keeps_the_keys_it_names_within_reach() {
    let mut h = harness(true);
    h.run();
    h.get_by_label("Ada added Bram").hover();
    h.run();
    let said = text_of(&h);
    let subject = PubKey::new([4u8; 32]).to_string();
    assert!(
        said.contains(&subject),
        "the account it names is nowhere: {said}"
    );
    assert!(
        said.contains(&them().to_string()),
        "and neither is who did it: {said}"
    );
}

/// SIP-31 **requires** a fork be surfaced, and a fork is not a gap.
///
/// A gap is ordinary — pruning, a retention window, and joining a channel
/// without its history all make one. A fork is two entries signed by one
/// device at one chain position, which cannot happen without that device
/// signing twice or somebody replaying. Drawn alike, the client would cry wolf
/// on every channel that keeps anything for a fixed time, and the cry that
/// matters would be lost in it.
#[test]
fn a_fork_is_surfaced_and_says_it_is_not_an_ordinary_gap() {
    // On messages that are actually **on screen**: the transcript is
    // bottom-aligned and scrolled, and a widget scrolled out of view is still
    // in the accessibility tree but cannot be pointed at -- so hovering one
    // silently does nothing and the tooltip half of this test would be a
    // check of nothing.
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 2].standing = sigil_chat::Standing::Fork;
    state.lines[n - 1].standing = sigil_chat::Standing::Gap;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("forked"), "a fork is not named: {said}");
    assert!(said.contains("gap"), "and neither is a gap: {said}");

    // And the words are different things, not one word twice.
    h.get_by_label("forked").hover();
    // `step`, not `run`: hovering a message also brings up its controls, and
    // `run` waits for the frame to settle -- which an animation never does.
    h.step();
    h.step();
    let means = text_of(&h);
    assert!(
        means.contains("evidence"),
        "a fork does not say what it is: {means}"
    );
    let mut h = harness_with(
        {
            let mut s = a_conversation();
            let n = s.lines.len();
            s.lines[n - 1].standing = sigil_chat::Standing::Gap;
            s
        },
        true,
    );
    h.run();
    h.get_by_label("gap").hover();
    h.step();
    h.step();
    let means = text_of(&h);
    assert!(
        means.contains("not evidence"),
        "an ordinary gap reads as misconduct: {means}"
    );
}

/// An entry nobody signed for is not a message, and is not silence either.
#[test]
fn something_forged_is_counted_and_said_rather_than_dropped() {
    let mut state = a_conversation();
    state.trouble_with.forged = 2;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("2 entries") && said.contains("not signed"),
        "a forged entry disappears without a word: {said}"
    );
}

/// A direct message has two people in it and cannot have any other number.
///
/// So the count beside the members control says nothing about *this*
/// conversation, only about what kind it is -- and it sits in the header of
/// every one-to-one conversation being read as though it might change.
#[test]
fn a_direct_message_does_not_count_its_two_people() {
    // The unread pill on the list row also says "2", and it is not what this
    // is about. Cleared, so a pass cannot come from the wrong widget -- and so
    // a failure names the header.
    let mut dm = a_conversation();
    for c in &mut dm.conversations {
        c.unread = 0;
    }
    assert_eq!(dm.members.len(), 2, "the fixture has two people in it");
    let mut h = harness_with(dm.clone(), true);
    h.run();
    assert!(
        !labels(&h).iter().any(|l| l == "2"),
        "a direct message counts its two people: {:?}",
        labels(&h)
    );

    // And a group, which is the case the count is for, still has it.
    let mut group = dm;
    group.open = Some([8u8; 32]);
    group.members = (0..7)
        .map(|i| Member {
            account: PubKey::new([100 + i; 32]),
            admin: i == 0,
            muted: false,
        })
        .collect();
    let mut h = harness_with(group, true);
    h.run();
    assert!(
        labels(&h).iter().any(|l| l == "7"),
        "a group stops saying how many are in it: {:?}",
        labels(&h)
    );
}

/// The top of the transcript is a door, and says so.
///
/// A conversation opens on its last page rather than on all of it, so the
/// first message drawn is not the beginning. A reader who cannot tell those
/// apart believes the channel started where their screen does.
#[test]
fn a_conversation_opened_on_its_last_page_says_there_is_more() {
    let mut state = a_conversation();
    state.earlier = 12;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("12 earlier messages"),
        "nothing says the conversation goes further back: {said}"
    );
    // And it is a control, not a note.
    h.get_by_label("12 earlier messages").click();
    h.run();
}

/// Reaching the top asks for one page, not one per frame.
///
/// The control that asks for earlier messages asks by **being on screen**, so
/// it asks on every pass until the answer arrives -- sixty a second against a
/// session that answers every seven hundred milliseconds. Reaching the top
/// therefore ordered dozens of pages, the transcript grew by hundreds of
/// messages, and the reader was left somewhere around the middle of a
/// conversation they had scrolled two lines into. Being on screen is a state;
/// asking is an event.
#[test]
fn reaching_the_top_asks_for_one_page_and_not_one_a_frame() {
    // **Short enough that the control stays on screen.** With a longer one
    // it is only visible on the first pass -- and the first version of this
    // test used six messages, which passed with the guard taken out.
    let mut state = a_page(50, 52);
    state.earlier = 50;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    for _ in 0..12 {
        h.step();
    }
    let earlier = asked.borrow().iter().filter(|c| *c == "Earlier").count();
    assert_eq!(
        earlier,
        1,
        "reaching the top asked for {earlier} pages: {:?}",
        asked.borrow()
    );
}

/// A conversation is chosen by pressing its row, not by finding its name.
///
/// **This passed before the row was rebuilt around its own sense**, which is
/// worth writing down: the contents already filled the width, so there was no
/// dead ground to press. What was missing was any sign of it -- see
/// `a_hovered_row_is_drawn_as_the_one_that_would_be_chosen`. This stays as the
/// thing that would notice if the row ever narrowed to its words.
#[test]
fn a_conversation_is_chosen_by_pressing_anywhere_on_its_row() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(a_conversation(), asked.clone());
    h.run();

    // Beside the name of the other conversation, where there is nothing drawn.
    let name = h.get_by_label_contains("release check").rect();
    let empty = egui::pos2(name.right() + 20.0, name.center().y);
    h.event(egui::Event::PointerButton {
        pos: empty,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: empty,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();

    // The row that was pressed, not any `Show` at all: the app opens a
    // conversation by itself when none is open, and a looser assertion here
    // passes on that instead of on the press.
    let wanted = format!("Show({:?})", [8u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "pressing the row beside the name did not open it: {:?}",
        asked.borrow()
    );
}

/// And the pointer over it says it can be pressed.
///
/// egui gives a click-sensing widget the pointing hand on its own, so this
/// passed before the change too. It pins that the row senses a click at all,
/// which is the half of "clicking anywhere works" that a cursor can show.
#[test]
fn a_row_answers_to_the_pointer() {
    let mut h = harness_with(a_conversation(), true);
    h.run();
    let name = h.get_by_label_contains("release check").rect();
    h.hover_at(egui::pos2(name.right() + 20.0, name.center().y));
    h.step();
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "the row does not offer itself to the pointer"
    );
}

/// A picture the session has put down is dropped by the interface too.
///
/// egui keeps the encoded bytes of everything it is given until it is told to
/// forget them, and nothing in this tree ever told it. So the session's own
/// eviction would have freed one copy of three: its own would go, and egui's
/// bytes and the texture behind them would stay for the life of the process.
///
/// Asked of egui, not of our own bookkeeping: after the picture leaves the
/// state, the loader must no longer be able to produce it.
#[test]
fn a_picture_the_session_has_put_down_is_forgotten_by_the_interface() {
    let picture: std::sync::Arc<[u8]> = vec![3u8; 4096].into();
    let with = {
        let mut state = a_conversation();
        let n = state.lines.len();
        // Not the tombstone: a redacted message draws no attachments, which
        // would make this test about nothing at all.
        state.lines[n - 1].redacted = false;
        state.lines[n - 1].attachments = vec![Attached {
            kind: sigil_ui::attachment::IMAGE,
            described: "[image, 4 KiB]".into(),
            size: picture.len() as u64,
            preview: sigil_ui::attachment::no_preview().clone(),
            bytes: Some(picture.clone()),
            missing: false,
            held: false,
            duration_ms: None,
            shape: None,
            waveform: Default::default(),
            id: "putdown".into(),
        }];
        state
    };
    let without = {
        let mut state = with.clone();
        let n = state.lines.len();
        state.lines[n - 1].attachments[0].bytes = None;
        state
    };

    let shown = std::rc::Rc::new(std::cell::RefCell::new(with));
    let mut h = harness_of(shown.clone());
    h.run();
    h.run();
    // Asked of the **bytes** loader, which is what `include_bytes` fills and
    // `forget_image` empties: whether these particular bytes decode into a
    // picture is a different question, and not this one.
    let uri = "bytes://putdown";
    assert!(
        h.ctx.try_load_bytes(uri).is_ok(),
        "the picture never reached egui, so this cannot say whether it leaves"
    );

    // The session lets it go.
    *shown.borrow_mut() = without;
    h.run();

    assert!(
        h.ctx.try_load_bytes(uri).is_err(),
        "egui still holds a picture the session has put down"
    );
}

/// The row under the pointer is drawn as the one that would be chosen.
///
/// This is the change: a list whose rows do not answer to the pointer is a
/// list somebody has to try, one row at a time, to find out that all of them
/// were pressable all along.
///
/// **Read off the pixels**, because a fill is not in the accessibility tree
/// and nothing else can see it. Two bands are compared: one across the row
/// being hovered, which must change, and one across another row, which must
/// not -- or this would pass for any repaint at all, including the pointer
/// egui itself draws into the picture.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn a_hovered_row_is_drawn_as_the_one_that_would_be_chosen() {
    fn band(image: &image::RgbaImage, rect: egui::Rect) -> Vec<u8> {
        let mut out = Vec::new();
        for y in rect.top() as u32..rect.bottom() as u32 {
            for x in rect.left() as u32..rect.right() as u32 {
                if x < image.width() && y < image.height() {
                    out.extend_from_slice(&image.get_pixel(x, y).0);
                }
            }
        }
        assert!(!out.is_empty(), "no pixels in {rect:?}");
        out
    }

    let mut h = harness(true);
    h.run();
    let hovered = h.get_by_label_contains("release check").rect();
    // The other row, picked out by being *in the column*: its name is also
    // the name of the conversation on screen and the author of half of it, so
    // the name alone finds three of them.
    let other = h
        .get_all_by_label_contains("Ada")
        .map(|n| n.rect())
        .filter(|r| r.right() < hovered.right() + 200.0)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the other conversation in the list");
    // The left end of each row, away from where the pointer will be: egui
    // paints a cursor into the rendered image, and a band under it changes
    // whatever the row does.
    let across = |name: egui::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(name.left() - 40.0, name.top() - 4.0),
            egui::pos2(name.left() - 8.0, name.bottom() + 4.0),
        )
    };

    let before = h.render().expect("a renderer");
    let (row_before, other_before) = (band(&before, across(hovered)), band(&before, across(other)));

    h.hover_at(egui::pos2(hovered.right() + 20.0, hovered.center().y));
    h.run();
    let after = h.render().expect("a renderer");

    assert_ne!(
        band(&after, across(hovered)),
        row_before,
        "the row under the pointer is drawn exactly as it was"
    );
    assert_eq!(
        band(&after, across(other)),
        other_before,
        "a row nobody is pointing at changed too, so this measures a repaint \
         rather than a highlight"
    );
}

/// The next page is asked for before the reader reaches the end of this one.
///
/// Waiting until the control is *visible* means arriving at the top of the
/// transcript and stopping there while the exchange is asked -- which reads as
/// a wall rather than as more conversation. A screen early, so it is on its
/// way before anybody gets there.
#[test]
fn the_next_page_is_asked_for_a_screen_early() {
    let mut state = a_page(50, 80);
    state.earlier = 50;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    // Nothing about *the transcript*: a direct message does ask the
    // registry once whether the other party moved, which is not a page.
    assert!(
        !asked.borrow().iter().any(|c| c == "Earlier"),
        "a conversation opened at the bottom asked for more of it: {:?}",
        asked.borrow()
    );

    // Up towards the top, but not to it. Measured on this fixture: 2,376
    // pixels of transcript in a 421-pixel pane, so it opens at an offset of
    // 1,955 and sixteen notches of 120 leave it around 170 -- inside one
    // screen of the top, with the control itself, forty pixels tall at the
    // very start of the content, still off it.
    h.hover_at(egui::pos2(600.0, 300.0));
    for _ in 0..16 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    assert!(
        asked.borrow().iter().any(|c| c == "Earlier"),
        "scrolling towards the top asked for nothing: {:?}",
        asked.borrow()
    );
}

/// And it is anchored on the very pass that shows it.
///
/// Correcting the offset after the pass puts the right number in the right
/// place a frame too late -- and that frame is drawn. For one sixtieth of a
/// second the transcript was five thousand pixels from where it belonged and
/// then snapped back: arithmetically perfect, and visibly a jump.
///
/// **One step**, therefore, and not `run`: what is being asserted is what the
/// first pass showing the page looked like, not where things ended up.
#[test]
fn the_page_is_anchored_on_the_pass_that_shows_it() {
    let shown = std::rc::Rc::new(std::cell::RefCell::new(a_page(50, 60)));
    let mut h = harness_of(shown.clone());
    h.run();

    h.hover_at(egui::pos2(600.0, 300.0));
    for _ in 0..6 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    let before = h.get_by_label_contains("message 55").rect().top();

    *shown.borrow_mut() = a_page(0, 60);
    h.step();

    let after = h.get_by_label_contains("message 55").rect().top();
    assert!(
        (after - before).abs() < 24.0,
        "the pass that first showed the page drew it {:.0} pixels from where \
         the reader was, and corrected it afterwards",
        after - before
    );
}

/// And it holds while the page is still arriving.
///
/// A page lands over several passes -- the messages, then the pictures in them
/// finding their size -- and an anchor let go after the first of them leaves
/// the rest of the growth to push the reader backwards. That is the same
/// defect in a smaller helping, and it is what was left after the first fix:
/// scrolling up to the first picture landed somewhere near the middle of the
/// conversation.
#[test]
fn the_anchor_holds_while_the_page_is_still_arriving() {
    let shown = std::rc::Rc::new(std::cell::RefCell::new(a_page(50, 60)));
    let mut h = harness_of(shown.clone());
    h.run();

    h.hover_at(egui::pos2(600.0, 300.0));
    for _ in 0..6 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    let before = h.get_by_label_contains("message 55").rect().top();

    // In two helpings, which is how it actually arrives: the session
    // republishes on its own tick and the pictures settle after that.
    *shown.borrow_mut() = a_page(25, 60);
    h.run();
    *shown.borrow_mut() = a_page(0, 60);
    h.run();
    h.run();

    let after = h.get_by_label_contains("message 55").rect().top();
    assert!(
        (after - before).abs() < 24.0,
        "the message being read moved {:.0} pixels while the page arrived in \
         two parts",
        after - before
    );
}

/// A harness that keeps what the interface asked the session for.
/// The same, a phone's size and form: a touch screen, where the strip is
/// revealed by a tap rather than by hovering.
fn harness_recording_commands_phone(
    state: ChatState,
    asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(PHONE_WIDTH, PHONE_HEIGHT))
        .with_step_dt(0.05)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let token: std::rc::Rc<dyn std::any::Any> =
                std::rc::Rc::new(sigil_chat::Route::Conversations);
            let _ = app.render_nav(&mut app_ctx, ui, &token);
            *asked.borrow_mut() = app.asked_for_test().to_vec();
        })
}

/// The Settings card, recording what it asked the session for.
fn me_card_commands(
    state: ChatState,
    asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(PHONE_WIDTH, PHONE_HEIGHT))
        .with_step_dt(0.05)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(sigil_chat::Route::Me);
            let _ = app.render_nav(&mut app_ctx, ui, &token);
            *asked.borrow_mut() = app.asked_for_test().to_vec();
        })
}

fn harness_recording_commands(
    state: ChatState,
    asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            *asked.borrow_mut() = app.asked_for_test().to_vec();
        })
}

/// A page arriving above the reader does not take the reader with it.
///
/// Earlier messages are asked for the moment their control reaches the screen,
/// so this happens by scrolling and not by choosing. A scroll offset is
/// measured from the top of the content, so prepending a page moves everything
/// the reader was looking at down by the height of the page: on a real
/// conversation the content went from 5,762 pixels to 10,859 while the offset
/// stayed at 220, which put the reader five thousand pixels from where they
/// had been. It read as the transcript hopping about at random, which is
/// exactly what it was.
///
/// Measured on a message, not on the offset: what has to hold still is the
/// thing somebody is reading.
#[test]
fn earlier_messages_arriving_do_not_move_what_is_being_read() {
    let shown = std::rc::Rc::new(std::cell::RefCell::new(a_page(50, 60)));
    let mut h = harness_of(shown.clone());
    h.run();

    // Away from the bottom, or `stick_to_bottom` holds the last message in
    // place on its own and this measures nothing.
    h.hover_at(egui::pos2(600.0, 300.0));
    for _ in 0..6 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    let before = h.get_by_label_contains("message 55").rect().top();

    // The page arrives: fifty older messages above everything on screen, the
    // way the session publishes them after `Cmd::Earlier`.
    *shown.borrow_mut() = a_page(0, 60);
    h.run();
    h.run();

    let after = h.get_by_label_contains("message 55").rect().top();
    assert!(
        (after - before).abs() < 24.0,
        "the message being read moved {:.0} pixels when older ones arrived above it",
        after - before
    );
}

/// A transcript of `from..to`, each message named so a test can find one.
fn a_page(from: u32, to: u32) -> ChatState {
    let mut state = a_conversation();
    state.earlier = from as usize;
    state.lines = (from..to)
        .map(|i| Line {
            seq: i as u64 + 1,
            who: them(),
            name: Some("Ada".into()),
            mine: false,
            at: NOW - u64::from(to - i) * 60,
            text: format!("message {i}"),
            redacted: false,
            edited: false,
            said: None,
            via: None,
            reactions: Vec::new(),
            reply_to: None,
            receipt: None,
            attachments: Vec::new(),
            standing: Default::default(),
            mentions: Vec::new(),
            me_mentioned: false,
            earlier: false,
        })
        .collect();
    state.events.clear();
    state
}

/// A harness whose state the test can replace between passes, the way the
/// session republishes it.
fn harness_of(state: std::rc::Rc<std::cell::RefCell<ChatState>>) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            app.show_state_for_test(state.borrow().clone());
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        })
}

/// One is one.
#[test]
fn one_earlier_message_is_not_one_earlier_messages() {
    let mut state = a_conversation();
    state.earlier = 1;
    let mut h = harness_with(state, true);
    h.run();
    assert!(text_of(&h).contains("1 earlier message"), "{}", text_of(&h));
}

/// A whole conversation offers nothing, because there is nothing to offer.
#[test]
fn a_whole_conversation_has_no_door_at_the_top_of_it() {
    let mut h = harness(true);
    h.run();
    assert!(
        !text_of(&h).contains("earlier message"),
        "a control that cannot do anything: {}",
        text_of(&h)
    );
}

/// The conversation column can be put away, and brought back.
///
/// **The control that brings it back is not inside it.** A toggle that hides
/// the thing it lives in is a toggle nobody can reach the second time, so it
/// moves to the conversation's own bar while the column is away.
#[test]
fn the_conversation_column_can_be_put_away_and_found_again() {
    let mut h = harness(true);
    h.run();
    // There from the start, without being asked for.
    assert!(text_of(&h).contains("Chats"), "{}", text_of(&h));

    h.get_by_label("Hide the chats").click();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("release check"),
        "the column is still there: {said}"
    );
    assert!(
        said.contains("Show the chats"),
        "and there is no way back to it: {said}"
    );

    h.get_by_label("Show the chats").click();
    h.run();
    assert!(
        text_of(&h).contains("release check"),
        "it did not come back: {}",
        text_of(&h)
    );
}

/// A dialog outlives the column it was opened from.
///
/// The dialogs hung off the conversation list, and the list is not drawn at
/// all once the window is narrow enough to show one pane at a time. So
/// narrowing the window with a dialog up left it *open in the state and absent
/// from the screen* — and it came back the next time the column did, over
/// whatever was there by then.
///
/// Narrowing is the way in, because the modal's own backdrop blocks the
/// column's hide control: while a dialog is up that click dismisses it, which
/// is what a modal is for.
#[test]
fn a_dialog_outlives_the_column_it_was_opened_from() {
    let mut h = harness(true);
    h.run();
    h.get_by_label("New conversation").click();
    h.run();
    assert!(text_of(&h).contains("Write to"), "{}", text_of(&h));

    // Narrow enough for one pane. `sigil::layout` decides this at runtime from
    // the width actually available, so this is the real path and not a flag.
    h.set_size(egui::vec2(420.0, 620.0));
    h.run();
    assert!(
        text_of(&h).contains("Write to"),
        "the dialog went with the column: {}",
        text_of(&h)
    );
}

/// One bar over the conversation: its name, its controls and the identity
/// share one row no taller than the avatar, and nothing sits above it.
///
/// There were two -- the session's, then the conversation's -- and a narrow
/// window put Back on a third.
#[test]
fn the_conversation_has_one_bar() {
    let mut h = harness_with(a_conversation(), true);
    h.run();
    let band = tokens::AVATAR_MD + tokens::SPACING_XS;
    // The conversation open is the direct message with Ada, so the heading
    // says "Ada" -- as do her bubbles, some of them scrolled above the
    // window, and her row in the list. The topmost node on screen with
    // each label is the bar's.
    let rects: Vec<(&str, egui::Rect)> = ["Ada", "Settings", "Members", "Your identity", "active"]
        .into_iter()
        .map(|l| (l, topmost(&h, l)))
        .collect();
    let top = rects.iter().map(|(_, r)| r.top()).fold(f32::MAX, f32::min);
    let bottom = rects
        .iter()
        .map(|(_, r)| r.bottom())
        .fold(f32::MIN, f32::max);
    assert!(
        bottom - top <= band,
        "the bar is more than one row ({top}..{bottom}): {rects:?}"
    );
    // Nothing above it but the window's margin: the bar is the first thing.
    assert!(
        top < 2.0 * tokens::SPACING_LG + band,
        "something sits above the bar: {top}"
    );
    // And the name is left of the controls, which are left of the identity.
    let x = |l: &str| rects.iter().find(|(n, _)| *n == l).unwrap().1.left();
    assert!(x("Ada") < x("Members") && x("Members") < x("Your identity"));
}

/// In a narrow window Back is the first thing in the bar, not a row above
/// it.
#[test]
fn in_a_narrow_window_back_is_in_the_bar() {
    let mut h = harness_with(a_conversation(), true);
    h.set_size(egui::vec2(420.0, 620.0));
    h.run();
    h.run();
    let back = h.get_by_label("Back").rect();
    let identity = h.get_by_label("Your identity").rect();
    assert!(
        (back.center().y - identity.center().y).abs() < tokens::SPACING_SM,
        "Back is on a row of its own: back {back:?}, identity {identity:?}"
    );
    assert!(back.left() < identity.left(), "Back is the leftmost");
    // The name is beside it, on the same row.
    let name = topmost(&h, "Ada");
    assert!((name.center().y - back.center().y).abs() < tokens::SPACING_SM);
}

/// With nothing open in a narrow window the identity is in the Chats row,
/// rather than on a bar with nothing else on it.
#[test]
fn with_nothing_open_in_a_narrow_window_the_identity_is_in_the_chats_row() {
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    let mut h = harness_with(state, true);
    h.set_size(egui::vec2(420.0, 620.0));
    h.run();
    h.run();
    let chats = h.get_by_label("Chats").rect();
    let identity = h.get_by_label("Your identity").rect();
    assert!(
        (chats.center().y - identity.center().y).abs() < tokens::SPACING_SM,
        "the identity is not level with the Chats heading: {chats:?} {identity:?}"
    );
}

/// Your presence is on your avatar: a small disc on its corner, and the
/// word on the same node -- filled green and "active" with the link up
/// and somebody here, hollow and the link's own word when it is down.
#[test]
fn the_connection_is_a_dot_on_the_avatar() {
    let colours = theme::dark();
    for (link, word, filled, colour) in [
        (
            sigil_chat::session::LinkState::Up,
            "active",
            true,
            colours.link_up,
        ),
        (
            sigil_chat::session::LinkState::Gone,
            "offline",
            false,
            colours.text_muted,
        ),
        (
            sigil_chat::session::LinkState::Retrying,
            "reconnecting…",
            false,
            colours.link_retrying,
        ),
    ] {
        let mut state = a_conversation();
        state.link = link;
        let mut h = harness_with(state, true);
        h.run();
        // The word is on the avatar -- and, when the link is down, on the
        // coloured word beside it as well; the avatar is the square one.
        let mark = h
            .get_all_by_label(word)
            .map(|n| n.rect())
            .find(|r| (r.width() - tokens::AVATAR_MD).abs() < 1.0)
            .unwrap_or_else(|| panic!("{word} is not on the avatar"));
        let corner = mark.right_bottom() - egui::vec2(tokens::SPACING_XS, tokens::SPACING_XS);
        // In the link's colour: the surface-coloured ring behind the dot is
        // a circle on the corner too, and is not the dot.
        assert_eq!(
            small_disc_at(&h, corner, colour),
            Some(filled),
            "{word}: no disc in the link's colour on the avatar's corner"
        );
    }
}

/// The topmost node on screen with this label.
fn topmost(h: &Harness<'static>, label: &str) -> egui::Rect {
    h.get_all_by_label(label)
        .map(|n| n.rect())
        .filter(|r| r.top() >= 0.0)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("{label} is not on screen"))
}

/// Whether a small circle in `colour` is painted over `at`, and whether it
/// is filled (`Some(true)`) or a ring (`Some(false)`); `None` if absent.
fn small_disc_at(h: &Harness<'static>, at: egui::Pos2, colour: egui::Color32) -> Option<bool> {
    fn walk(shape: &egui::Shape, at: egui::Pos2, colour: egui::Color32) -> Option<bool> {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().rev().find_map(|s| walk(s, at, colour)),
            egui::Shape::Circle(c)
                if c.radius < tokens::SPACING_MD && c.center.distance(at) <= c.radius + 1.0 =>
            {
                if c.fill == colour {
                    Some(true)
                } else if c.stroke.color == colour {
                    Some(false)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    h.output()
        .shapes
        .iter()
        .rev()
        .find_map(|c| walk(&c.shape, at, colour))
}

/// Nobody calls a public channel.
///
/// Anybody may join one, so the ring would go to a membership nobody chose,
/// and the room secret is a bearer capability (SIP-36) — whoever turns up next
/// holds it. There is nothing to fix at the point somebody presses it, so the
/// control is not there to press.
#[test]
fn a_public_channel_offers_no_way_to_call_it() {
    let mut state = a_conversation();
    // The public one, which the fixture's second row is.
    state.open = Some([8u8; 32]);
    let mut h = harness_with(state, true);
    h.run();
    assert!(
        !text_of(&h).contains("Call"),
        "a public channel offers a call: {}",
        text_of(&h)
    );

    // And a private one still does, or this would pass by drawing no header.
    let mut h = harness(true);
    h.run();
    assert!(
        text_of(&h).contains("Call"),
        "and a conversation with people in it lost its call: {}",
        text_of(&h)
    );
}

#[test]
fn a_public_channel_is_marked_in_the_list() {
    // Anybody may join it and nothing in it is encrypted. That is the whole
    // difference that matters about it, and it has to be visible before
    // somebody types into one.
    let mut h = harness(true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("release check"), "{said}");
    assert!(
        said.contains("public") || said.contains('#'),
        "a public channel is marked, not merely listed: {said}"
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn transcript_dark() {
    let mut h = harness(true);
    h.run();
    hide_column(&mut h);
    h.snapshot("transcript_dark");
}

/// The conversation list, which nothing else renders.
///
/// It is behind a modal in `chat_dialog_dark` and hidden by the transcript
/// pictures, so the rows themselves — the marks, the marker on a channel, the
/// unread pill — had no picture anybody could look at.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn list_dark() {
    // Ada is away, so the row's mark carries the amber dot.
    let mut state = a_conversation();
    state.presence.insert(
        them(),
        sigil_chat::presence::Presence {
            seen: sigil_chat::presence::Seen::Away,
            last_seen: NOW - 600,
            read_at: NOW,
        },
    );
    let mut h = harness_with(state, true);
    h.run();
    h.snapshot("list_dark");
}

/// The Members view: each mark with its presence -- ours active, Ada
/// away -- and the whole keys under the names.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn members_dark() {
    let mut state = a_conversation();
    state.presence.insert(
        them(),
        sigil_chat::presence::Presence {
            seen: sigil_chat::presence::Seen::Away,
            last_seen: NOW - 600,
            read_at: NOW,
        },
    );
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    h.snapshot("members_dark");
}

/// The column while a search stands: the count, and the results in place
/// of the list, one of them chosen.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn search_dark() {
    let mut state = a_conversation();
    state.searched_messages = true;
    let long = format!(
        "{}so the release check moves to Thursday, bring the notes",
        "and another thing, ".repeat(6)
    );
    let deep = long.find("release").unwrap();
    state.hits = vec![
        Hit {
            channel: [9u8; 32],
            seq: 7,
            label: "release check".into(),
            who: "Ada".into(),
            text: "the release is Thursday".into(),
            found: 4..11,
            at: NOW - 120,
        },
        Hit {
            channel: [9u8; 32],
            seq: 3,
            label: "release check".into(),
            who: "You".into(),
            text: long,
            found: deep..deep + 7,
            at: NOW - 86_400,
        },
        Hit {
            channel: [8u8; 32],
            seq: 40,
            label: "Grace".into(),
            who: "Grace".into(),
            text: "no release without the notes\nand the notes are late".into(),
            found: 3..10,
            at: NOW - 3 * 86_400,
        },
    ];
    let mut h = harness_with(state, true);
    h.run();
    let field = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the search box");
    field.focus();
    field.type_text("release");
    h.run();
    let second = h.get_by_label_contains("You: …").rect();
    press_at(&mut h, second.center());
    h.event(egui::Event::PointerMoved(egui::pos2(900.0, 600.0)));
    // Steps: the transcript went to the message and is washing it, which
    // repaints until the wash has faded.
    for _ in 0..10 {
        h.step();
    }
    h.snapshot("search_dark");
}

/// The same list with the pointer on a row.
///
/// The fill under the pointer is the whole of what says a row can be pressed,
/// and it is a colour: nothing but looking at it will do.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn list_hovered_dark() {
    let mut h = harness(true);
    h.run();
    let name = h.get_by_label_contains("release check").rect();
    h.hover_at(egui::pos2(name.right() + 20.0, name.center().y));
    h.run();
    h.snapshot("list_hovered_dark");
}

/// One's own bubble, with the two things that are written *about* a message
/// rather than in it: the reply it answers, and the file it carries.
///
/// A picture of its own because both were unreadable and both were correct in
/// every other sense -- present, positioned, and grey on blue. Nothing but
/// looking at it, or a contrast figure, can catch that.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn mine_dark() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].mine = true;
    state.lines[n - 1].name = None;
    state.lines[n - 1].who = me();
    state.lines[n - 1].text = "sent by me, with something attached".into();
    state.lines[n - 1].reply_to = Some(Quoted {
        seq: 1,
        who: "Ada".into(),
        said: "the second one, then".into(),
        preview: None,
    });
    state.lines[n - 1].attachments = vec![Attached {
        kind: 0x04,
        described: "[notes.txt, 2.1 kB]".into(),
        size: 2100,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: false,
        duration_ms: None,
        shape: None,
        waveform: Default::default(),
        id: "mine123".into(),
    }];
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);
    h.snapshot("mine_dark");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn transcript_light() {
    let mut h = harness(false);
    h.run();
    hide_column(&mut h);
    h.snapshot("transcript_light");
}

/// A self-declared title must not be drawn beside the name.
///
/// SIP-21 makes this a MUST, and the reason is in the fixture: a title
/// asserting "Exchange Administrator" does the social engineering by itself.
/// Nobody attests it, so it must never appear where a reader looks for
/// authority — not as a badge, not in channel-role styling, not next to a
/// verification mark. It belongs where somebody goes looking for it, beside
/// the key.
#[test]
fn a_title_is_never_rendered_beside_the_name() {
    let mut h = harness(true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Ada"), "the display name is shown: {said}");
    assert!(
        !said.contains("Exchange Administrator"),
        "a self-declared title must not be on the message: {said}"
    );
}

/// The name is shown, and the key is what it leads back to.
#[test]
fn a_name_never_appears_without_its_key_reachable() {
    let mut h = harness(true);
    h.run();
    // What matters here is that the interface never *replaces* the key with a
    // name it was handed: a name is an assertion (SIP-21) and the key is the
    // only identity. It is no longer on the hover -- see
    // `a_peers_key_is_in_full_in_members` for where it went, and
    // `hovering_a_conversation_offers_no_key` for why.
    let said = text_of(&h);
    assert!(said.contains("Ada"));
    // Our own key in full, one gesture from the name it sits under.
    open_identity(&mut h);
    let said = text_of(&h);
    let key = me().to_string();
    assert!(
        said.contains(&key),
        "your own key is shown in full, not abbreviated away: {said}"
    );
}

/// A mention is marked in the words themselves -- the name, by this
/// client's spelling of it, once -- with the mark, the name and the whole
/// key on a card when it is hovered (SIP-21). A mention whose name is not
/// in the words is a chip under them, so it is never invisible. A line
/// that mentions nobody draws neither.
#[test]
fn a_mention_is_marked_in_the_words_and_its_key_is_a_hover_away() {
    let mut state = a_conversation();
    // On lines at the foot, where the bottom-aligned transcript shows them
    // and a hover lands on something.
    let one = state.lines.iter().position(|l| l.text == "one").unwrap();
    state.lines[one].text = "hi @Ada hi".into();
    state.lines[one].mentions = vec![sigil_chat::session::Mentioned {
        key: them(),
        label: "Ada".into(),
    }];
    let second = state
        .lines
        .iter()
        .position(|l| l.text == "the second one, then")
        .unwrap();
    // Written as one name over a part that names somebody else.
    state.lines[second].text = "hey @Eve".into();
    state.lines[second].mentions = vec![sigil_chat::session::Mentioned {
        key: me(),
        label: "me".into(),
    }];
    state.lines[second].me_mentioned = true;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("hi @Ada hi"), "{said}");
    assert!(
        h.query_by_label("@Ada").is_none(),
        "the name is in the words, not printed a second time: {said}"
    );
    assert!(
        h.query_by_label("@me").is_some(),
        "a name the words do not carry is a chip: {said}"
    );
    assert!(
        !said.contains(&them().to_string()),
        "the key waits to be asked for: {said}"
    );

    // Hovering the name in the words: the card, with the whole key.
    let words = h.get_by_label("hi @Ada hi").rect();
    h.hover_at(words.center());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains(&them().to_string()),
        "hovering the name shows the key: {said}"
    );

    // A line with no mention has no chip and no mark.
    let mut h = harness_with(a_conversation(), true);
    h.run();
    let said = text_of(&h);
    assert!(!said.contains("@Ada") && !said.contains("@me"), "{said}");
}

/// A press at a point: down and up, as the pointer does it.
fn press_at(h: &mut Harness<'static>, at: egui::Pos2) {
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
}

/// Pressing a mentioned name opens a card with things to do about them:
/// a direct message, mentioning them in the box, copying the key. Each
/// does what it says, and the card is not there until the press.
#[test]
fn pressing_a_mentioned_name_offers_a_direct_message_and_more() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let one = state.lines.iter().position(|l| l.text == "one").unwrap();
    state.lines[one].text = "hi @Ada hi".into();
    state.lines[one].mentions = vec![sigil_chat::session::Mentioned {
        key: them(),
        label: "Ada".into(),
    }];
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state.clone(), asked.clone());
    h.run();
    assert!(
        h.query_by_label("Direct message").is_none(),
        "not before the press"
    );
    let words = h.get_by_label("hi @Ada hi").rect();
    h.hover_at(words.center());
    h.run();
    press_at(&mut h, words.center());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Direct message"),
        "the press opens the card: {said}"
    );
    assert!(
        said.contains("Mention them here") && said.contains("Copy key"),
        "{said}"
    );
    assert!(
        said.contains(&them().to_string()),
        "with the whole key: {said}"
    );
    h.get_by_label("Direct message").click();
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains(&format!("OpenDm({:?})", them())),
        "Direct message opens the conversation with them: {sent}"
    );

    // Mention them here: into the box, and the send carries the key.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    let words = h.get_by_label("hi @Ada hi").rect();
    h.hover_at(words.center());
    h.run();
    press_at(&mut h, words.center());
    h.run();
    h.run();
    h.get_by_label("Mention them here").click();
    h.run();
    h.run();
    assert_eq!(composed(&h), "@Ada ", "{}", text_of(&h));
    composer(&h).focus();
    composer(&h).type_text("yes");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Post(") && sent.contains(&format!("{:?}", them())),
        "{sent}"
    );
}

/// The same, looked at: the chips, and the outline on the line that
/// mentions the reader, which the tree cannot see.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn transcript_mention_dark() {
    let mut state = a_conversation();
    // On the lines at the foot, where the bottom-aligned transcript shows
    // them: Ada's "one" mentions me; her "the second one, then" mentions
    // Bram, a third person.
    let one = state.lines.iter().position(|l| l.text == "one").unwrap();
    state.lines[one].text = "one for @me, then".into();
    state.lines[one].mentions = vec![sigil_chat::session::Mentioned {
        key: me(),
        label: "me".into(),
    }];
    state.lines[one].me_mentioned = true;
    let second = state
        .lines
        .iter()
        .position(|l| l.text == "the second one, then")
        .unwrap();
    state.lines[second].mentions = vec![sigil_chat::session::Mentioned {
        key: PubKey::new([5u8; 32]),
        label: "Bram".into(),
    }];
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);
    h.snapshot("transcript_mention_dark");
}

/// A conversation with a mention of the reader waiting in it is marked in
/// the list, in the accent, beside the count.
#[test]
fn a_conversation_that_mentions_you_is_marked_in_the_list() {
    let mut state = a_conversation();
    state.conversations[1].unread = 3;
    state.conversations[1].mentioned = 1;
    let mut h = harness_with(state, true);
    h.run();
    let mark = h.query_by_label("@");
    assert!(mark.is_some(), "no mark: {}", text_of(&h));
    // And not on one with unread but no mention.
    let mut state = a_conversation();
    state.conversations[1].unread = 3;
    state.conversations[1].mentioned = 0;
    let mut h = harness_with(state, true);
    h.run();
    assert!(h.query_by_label("@").is_none(), "{}", text_of(&h));
}

/// The way to another identity is a door, not a roster.
///
/// **Moved here from the shell's rail**, and then cut down to one item. The
/// menu listed every identity sigil happened to be holding, which put a second
/// and shorter list beside the opening screen's — shorter because it could
/// only name the ones already in the roster, so an identity sitting in
/// `~/.sqnr` that sigil had never opened was unreachable from here. One item
/// that goes back to the screen which lists them all, draws each one's mark,
/// says what is wrong with a file and can ask for a passphrase.
#[test]
fn the_identity_menu_offers_one_way_out_and_not_a_list() {
    let one = Account::unlocked_for_test([1u8; 32]);
    let two = Account::unlocked_for_test([2u8; 32]);
    let other = two.unlocked().unwrap().me().to_string();

    let mut h = harness_with_accounts(a_conversation(), vec![one, two]);
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    assert!(
        said.contains("Switch identity"),
        "no way to be anybody else: {said}"
    );
    assert!(
        !said.contains(&other[..10]),
        "the other identity is listed here as well as on the screen that lists them: {said}"
    );

    // And with one identity held, where the old list drew nothing at all: the
    // opening screen has every identity in `~/.sqnr`, so there is somewhere to
    // go even when sigil is holding a single one.
    let mut h = harness_with_accounts(
        a_conversation(),
        vec![Account::unlocked_for_test([1u8; 32])],
    );
    h.run();
    open_identity(&mut h);
    assert!(
        text_of(&h).contains("Switch identity"),
        "holding one identity is not the same as there being one: {}",
        text_of(&h)
    );
}

/// Asking for the opening screen is said to the shell **once**.
///
/// The item sets a flag and `render` returns it, because an app reaches the
/// shell by what it returns. A flag that is read rather than taken keeps
/// returning it, and the shell would then draw the opening screen on every
/// pass for ever -- a screen nobody can leave, from an interface that looks
/// entirely correct in a screenshot.
#[test]
fn asking_to_switch_identity_is_said_once() {
    let asks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_watching_asks(a_conversation(), asks.clone());
    h.run();
    open_identity(&mut h);
    assert!(
        asks.borrow().is_empty(),
        "the app asked for something nobody pressed: {:?}",
        asks.borrow()
    );

    h.get_by_label("Switch identity").click();
    h.run();
    h.run();
    let said: Vec<_> = asks.borrow().clone();
    assert_eq!(
        said,
        vec![sigil::app::AppAction::ChooseIdentity],
        "the ask was dropped, or kept being made"
    );
}

/// A conversation nobody has answered about yet says it is still asking.
///
/// "Nothing here yet" is a claim about the conversation, and it was being made
/// about every one of them for as long as the exchange took to answer --
/// including the one sigil opens for you on the way in. What this machine
/// holds is drawn at once now, so an empty transcript *and* a fetch still out
/// is the only case left, and it says so.
#[test]
fn a_conversation_still_being_fetched_says_so_rather_than_that_it_is_empty() {
    let mut state = a_conversation();
    state.lines.clear();
    state.events.clear();
    state.loading = true;
    let mut h = harness_with(state, true);
    // Stepped: the loading mark asks for the next step of itself.
    h.run_steps(3);
    let said = text_of(&h);
    assert!(
        said.contains("Loading this conversation"),
        "an unanswered conversation is drawn as an empty one: {said}"
    );
    assert!(!said.contains("Nothing here yet"), "{said}");

    // Answered, and there really is nothing in it.
    let mut state = a_conversation();
    state.lines.clear();
    state.events.clear();
    state.loading = false;
    let mut h = harness_with(state, true);
    h.run();
    assert!(
        text_of(&h).contains("Nothing here yet"),
        "an answered, empty conversation says nothing at all: {}",
        text_of(&h)
    );
}

/// Having no name here is a control, not a note.
///
/// "You have no name at this exchange" is only useful beside the way to get
/// one. It is one word now, too: the sentence it replaced wrapped onto a
/// second line in a corner block and pushed it down rather than out. It is
/// the head of the identity menu, where the name and handle went when the
/// bar became one row.
#[test]
fn having_no_name_offers_the_way_to_claim_one() {
    let mut state = a_conversation();
    state.mine.handle = None;
    let mut h = harness_with(state, true);
    h.run();
    assert!(
        !text_of(&h).contains("unregistered"),
        "the bar is one row and carries no second line: {}",
        text_of(&h)
    );
    open_identity(&mut h);
    assert!(
        text_of(&h).contains("unregistered"),
        "nothing says the exchange knows no name here: {}",
        text_of(&h)
    );

    h.get_by_label("unregistered").click();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Claim a name"), "{said}");
    // And it says which kind of name it is. A profile name is what somebody
    // says about themselves; this one is bound at the exchange.
    assert!(
        said.contains("Bound at this exchange"),
        "the two things called a name are not told apart: {said}"
    );
}

/// An exchange can be taken back off.
///
/// There was a control to add one and none to remove one, so a name added by
/// mistake — or one that turned out to be the default under another spelling,
/// which is how this was found — could only be undone by editing the roster
/// file by hand.
#[test]
fn an_added_exchange_can_be_removed_again() {
    let mut h = harness_at_exchanges(a_conversation(), &["indra.org"]);
    h.run();
    open_exchanges(&mut h);
    assert!(text_of(&h).contains("indra.org"), "{}", text_of(&h));

    h.get_by_label("Remove").click();
    h.run();
    open_exchanges(&mut h);
    assert!(
        !text_of(&h).contains("indra.org"),
        "the exchange is still there: {}",
        text_of(&h)
    );
}

/// A message's strip hangs off its top-outer corner: the right corner of
/// somebody else's message, the left of one's own.
///
/// The controls have been under the bubble (everything below moved as the
/// pointer went by), then beside it (a column of the pane reserved on both
/// sides). Off the corner, on their own layer, they take no room in the row
/// and move nothing — so this asserts the corner, from both sides.
#[test]
fn the_strip_hangs_off_the_top_outer_corner() {
    for mine in [false, true] {
        let mut state = a_conversation();
        state.lines.truncate(1);
        state.lines[0].text = "the first line\nand a second one\nand a third".into();
        state.lines[0].mine = mine;
        let mut h = harness_with(state, true);
        h.run();
        hide_column(&mut h);
        h.run();
        let words = h.get_by_label_contains("and a third").rect();
        h.get_by_label_contains("and a third").hover();
        h.step();
        h.step();
        h.step();
        let reply = h.get_by_label("Reply").rect();
        let quick = h.get_by_label(sigil_emoji::QUICK[0]).rect();
        let strip = reply.union(quick);
        let whose = if mine { "your own" } else { "theirs" };
        // The bubble, as the tree can see it: the author line over the words
        // on theirs, the words alone on one's own. The frame is not in the
        // tree, and it begins a padding above whichever is first.
        let span = if mine {
            words
        } else {
            let author = h
                .get_all_by_label("Ada")
                .map(|n| n.rect())
                .filter(|r| (r.left() - words.left()).abs() < 4.0)
                .min_by(|a, b| a.top().total_cmp(&b.top()))
                .expect("the author line over the words");
            author.union(words)
        };
        // Off the top: the strip straddles the bubble's top edge — over the
        // first line of the bubble and above it — rather than sitting level
        // with the middle of the message or under it.
        assert!(
            strip.top() < span.top() && strip.bottom() > span.top(),
            "the strip on {whose} message does not hang off its top: bubble \
             {span:?}, strip {strip:?}"
        );
        // Off the outer corner: reaching past the bubble on the side it has
        // room on, and no further back over it than a corner.
        if mine {
            assert!(
                strip.left() < words.left() && strip.right() < words.right(),
                "your own message's strip should hang off its left corner: words \
                 {words:?}, strip {strip:?}"
            );
        } else {
            assert!(
                strip.right() > words.right() && strip.left() > words.left(),
                "their message's strip should hang off its right corner: words \
                 {words:?}, strip {strip:?}"
            );
        }
    }
}

/// One's own messages sit on the other side.
///
/// The layout note in `message.rs` records three shapes that left them on the
/// left and looked *almost* right, which is why they survived several passes —
/// and there was no test, so the fourth rewrite of that layout had nothing
/// watching it either.
#[test]
fn ones_own_messages_sit_on_the_other_side() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].mine = true;
    state.lines[n - 1].text = "sent by me".into();
    state.lines[n - 2].mine = false;
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);

    let mine = h.get_by_label_contains("sent by me").rect();
    let theirs = h.get_by_label_contains("the second one, then").rect();
    assert!(
        mine.left() > theirs.right(),
        "one's own message is on the same side as everybody else's: \
         {mine:?} against {theirs:?}"
    );
}

/// One's own message stops before the scrollbar rather than under it.
///
/// egui's scroll bars **float** by default: they allocate no width and are
/// painted over the last `bar_width` pixels of the content. A right-aligned
/// bubble is what is there, so one's own messages ran under the bar and the
/// bar sat on top of the text.
///
/// # What is being measured, and what a short message would measure instead
///
/// The accessibility tree carries the **text**, not the frame around it, and a
/// short message does not reach its own bubble's edge -- so the first version
/// of this passed with the margin taken away, by 917 against a limit of 962.
/// The message here is long enough to wrap, which makes the text exactly as
/// wide as the bubble allows and puts its right edge one padding in from the
/// frame's.
///
/// The pane's right edge comes from the identity block, which is right-aligned
/// in the same pane -- rather than from the window, which would be measuring
/// this harness's own frame.
#[test]
fn ones_own_messages_stop_before_the_scrollbar() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].mine = true;
    state.lines[n - 1].text = format!(
        "sent by me, {}",
        "and it goes on for long enough to wrap, ".repeat(6)
    );
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);

    let bar = h.ctx.style_of(egui::Theme::Dark).spacing.scroll.bar_width;
    let pad = sigil::tokens::SPACING_LG;
    let edge = h.get_by_label("Your identity").rect().right();
    let mine = h.get_by_label_contains("sent by me").rect();
    assert!(
        mine.right() + pad <= edge - bar,
        "one's own message runs under the scrollbar: its text ends at {}, \
         the bubble a padding of {pad} past that, the pane at {edge}, \
         and the bar is {bar} wide",
        mine.right()
    );
}

/// And their controls are on the other side too.
#[test]
fn ones_own_message_keeps_its_controls_on_the_left() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].mine = true;
    state.lines[n - 1].text = "sent by me".into();
    let mut h = harness_with(state, true);
    h.run();

    let bubble = h.get_by_label_contains("sent by me").rect();
    h.get_by_label_contains("sent by me").hover();
    h.step();
    h.step();
    h.step();
    let reply = h.get_by_label("Reply").rect();
    assert!(
        reply.center().x < bubble.left(),
        "one's own message keeps its strip off its left corner, the bubble being \
         on the right: {reply:?} against {bubble:?}"
    );
}

/// A message that arrives under the pointer while moving gets no strip
/// until it has held still; a strip already up follows its message.
///
/// A scroll carries message after message under a pointer that has not
/// moved, and each one under it got a strip for a frame: a flicker of pills
/// up the pane, and a new foreground area every frame, which is a sizing
/// pass and a repaint every frame for as long as the scroll lasted. But a
/// strip that hid whenever its own message moved cost a sizing pass every
/// time the layout shifted under it — the composer growing pushed the
/// transcript, and a test that pressed Edit ran out of frames.
#[test]
fn a_moving_message_gets_no_strip_until_it_holds_still_and_an_up_strip_follows() {
    // Enough of a transcript to scroll.
    let mut state = a_conversation();
    let base = state.lines[1].clone();
    for i in 0..40 {
        let mut line = base.clone();
        line.seq = 1000 + i;
        line.text = format!("filler {i}");
        line.redacted = false;
        state.lines.push(line);
    }
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);
    h.run();
    h.run();
    let wheel = |h: &mut Harness<'static>, points: f32| {
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, points),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
    };

    // The pointer rests on a message; a wheel carries the transcript under
    // it. On the frame another message arrives under the pointer, moving,
    // that message is new there and gets no strip.
    let pointer = h.get_by_label("filler 38").rect().center();
    h.get_by_label("filler 38").hover();
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Reply"),
        "at rest, the strip: {}",
        text_of(&h)
    );
    let under = |h: &Harness<'static>| -> Option<(String, f32)> {
        h.get_all_by_label_contains("filler")
            .map(|n| {
                let a = n.accesskit_node();
                let said = a.label().or_else(|| a.value()).unwrap_or_default();
                (said.to_string(), n.rect())
            })
            .find(|(_, r)| r.contains(pointer))
            .map(|(l, r)| (l, r.top()))
    };
    let before_top = h.get_by_label("filler 30").rect().top();
    wheel(&mut h, 240.0);
    let mut arrived = None;
    let mut last = under(&h);
    for _ in 0..12 {
        h.step();
        let now = under(&h);
        if let Some((label, top)) = &now
            && label != "filler 38"
            && last.as_ref().is_none_or(|(l, t)| l != label || t != top)
        {
            arrived = Some(label.clone());
            break;
        }
        last = now;
    }
    assert!(
        h.get_by_label("filler 30").rect().top() != before_top,
        "the wheel scrolled nothing, so this tests nothing"
    );
    let arrived = arrived.expect("no other message came under the pointer, so this tests nothing");
    assert!(
        !text_of(&h).contains("Reply"),
        "a strip appeared on {arrived}, which arrived under the pointer moving: {}",
        text_of(&h)
    );
    // Once it has come to rest, the strip.
    h.run();
    h.get_by_label(&arrived).hover();
    h.step();
    h.step();
    h.step();
    assert!(
        text_of(&h).contains("Reply"),
        "and never appeared once it held still: {}",
        text_of(&h)
    );

    // The strip is up. The message moves under it — a slow scroll, a
    // little each frame — and the strip goes with it rather than hiding.
    let reply = h.get_by_label("Reply").rect();
    let words = h.get_by_label(&arrived).rect();
    let at = words.center();
    // Little enough, in all, that the pointer is still on the bubble.
    for _ in 0..3 {
        wheel(&mut h, -5.0);
        h.step();
    }
    let moved = h.get_by_label(&arrived).rect();
    assert!(
        moved.expand(8.0).contains(at),
        "the pointer left the message, so this tests nothing: {moved:?} {at:?}"
    );
    assert_ne!(
        words.top(),
        moved.top(),
        "the transcript did not move, so this tests nothing"
    );
    let followed = h.get_by_label("Reply").rect();
    assert_ne!(
        reply.top(),
        followed.top(),
        "the strip did not follow its message: {reply:?}"
    );
}

/// Opening the reaction picker does not take it away again.
///
/// It is drawn below its button, which is outside the region that reveals the
/// controls — so moving the pointer down into the picker left that region, the
/// controls stopped being drawn, and the picker went with them. Visible and
/// unreachable, which is the same defect the controls themselves had when they
/// were under the bubble.
#[test]
fn a_picker_survives_the_pointer_leaving_the_message() {
    let mut h = harness(true);
    h.run();
    hide_column(&mut h);
    h.get_by_label_contains("the second one, then").hover();
    h.step();
    h.step();
    h.step();
    h.get_by_label("More emoji").click();
    h.step();
    // An emoji the picker offers on its first page and **neither the strip
    // nor this conversation already carries**: the fixture has a `👍 2` chip
    // on another message, and the strip has its own five, so looking for one
    // of those finds it whether or not the picker ever opened.
    let picker = '\u{1f600}';
    assert!(
        text_of(&h).contains(picker),
        "the picker did not open: {}",
        text_of(&h)
    );

    // The pointer moves off the message. The controls have to still be there,
    // or the picker goes with them — it is drawn by the same pass.
    h.get_by_label_contains("Yesterday").hover();
    h.step();
    h.step();
    assert!(
        text_of(&h).contains(picker),
        "the picker vanished as the pointer left the message: {}",
        text_of(&h)
    );
}

/// A name you hold can be given up.
///
/// Claiming one has been offered since there was a route for it; letting go
/// had no control at all, so a name taken by mistake was taken for good.
#[test]
fn a_name_you_hold_can_be_given_up() {
    let mut h = harness(true);
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    // The fixture's identity holds one, so the control is there.
    assert!(said.contains("me@squic.org"), "{said}");
    assert!(
        said.contains("Give it up"),
        "a name can be taken and not returned: {said}"
    );
}

/// And there is nothing to give up when the exchange knows no name.
#[test]
fn having_no_name_offers_nothing_to_give_up() {
    let mut state = a_conversation();
    state.mine.handle = None;
    let mut h = harness_with(state, true);
    h.run();
    open_identity(&mut h);
    assert!(
        !text_of(&h).contains("Give it up"),
        "a control that cannot do anything: {}",
        text_of(&h)
    );
}

/// A picture that could not be fetched says so, and offers another try.
///
/// A blob past its retention window is gone, so the fetch is not retried on
/// every tick — which left a picture that had *failed* and one that had not
/// been reached yet drawing the same bare filename, with nothing to say which
/// or what to do about it.
#[test]
fn a_picture_the_exchange_refused_says_so() {
    let mut state = a_conversation();
    let file = state.lines[2]
        .attachments
        .get_mut(0)
        .expect("the fixture's file");
    file.kind = 0x01;
    file.described = "[image, 28 KiB]".into();
    file.missing = true;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("could not be fetched"),
        "a refused picture is a bare filename: {said}"
    );
    assert!(said.contains("Try again"), "and offers nothing: {said}");
}

/// One still on its way says *that*, which is a different thing.
#[test]
fn a_picture_still_coming_says_it_is_coming() {
    let mut state = a_conversation();
    let file = state.lines[2]
        .attachments
        .get_mut(0)
        .expect("the fixture's file");
    file.kind = 0x01;
    file.described = "[image, 28 KiB]".into();
    file.missing = false;
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("fetching"), "{said}");
    assert!(
        !said.contains("could not be fetched"),
        "a picture on its way is reported as lost: {said}"
    );
}

/// A ring shows the caller's key in full, and does not dress it as proven.
///
/// Carried over from the voice app, which used to own ringing. The rule did
/// not change with the mechanism: a name is an assertion (SIP-21), and this is
/// the one screen where acting on the wrong one puts somebody in a call with a
/// stranger who chose a confusable name. The key stays on the ring.
/// A call of ours that has been answered shows no "Calling… Ringing…"
/// card: the call's own banner says what is happening from then on, and
/// on a phone the two sat one under the other for the whole call.
#[test]
fn an_answered_call_of_ours_is_no_longer_shown_as_ringing() {
    let ring = |answered: bool| sigil_chat::Ring {
        channel: [9u8; 32],
        seq: 7,
        from: me(),
        mine: true,
        secret: [3u8; 32],
        answered,
        label: "Ada".into(),
        direct: false,
        peer: Some(them()),
    };
    let mut state = a_conversation();
    state.ringing = vec![ring(false)];
    let mut h = harness_with(state, true);
    h.run();
    assert!(text_of(&h).contains("Ringing"), "unanswered, it rings");

    let mut state = a_conversation();
    state.ringing = vec![ring(true)];
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Ringing"),
        "answered, and still ringing: {said}"
    );
}

#[test]
fn a_ring_shows_the_callers_key_in_full() {
    let mut state = a_conversation();
    state.ringing = vec![sigil_chat::Ring {
        channel: [9u8; 32],
        seq: 7,
        from: them(),
        mine: false,
        secret: [3u8; 32],
        answered: false,
        label: "Ada".into(),
        direct: false,
        peer: None,
    }];
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("is calling"), "{said}");
    assert!(
        said.contains(&them().to_string()),
        "the caller's key is on the ring, in full: {said}"
    );
    assert!(
        said.contains("Answer") && said.contains("Decline"),
        "{said}"
    );
}

/// Every texture a screen painted with, other than the font atlas.
///
/// **Walked from the paint list rather than asked of the tree**, because a
/// picture has no label: what is being checked is that the bytes reached a
/// texture and the texture reached the screen, and the accessibility tree
/// can say neither.
fn painted_with(h: &mut Harness<'static>) -> Vec<egui::TextureId> {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::TextureId>) {
        match shape {
            egui::Shape::Mesh(mesh) => out.push(mesh.texture_id),
            egui::Shape::Rect(rect) => {
                if let Some(brush) = &rect.brush {
                    out.push(brush.fill_texture_id);
                }
            }
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    // Twice: a picture is decoded on the pass that first wants it and painted
    // on the next, which is the whole point of caching it.
    h.run();
    h.run();
    let mut found = Vec::new();
    for shape in &h.output().shapes {
        walk(&shape.shape, &mut found);
    }
    // The font atlas is what every glyph on the screen is drawn from and says
    // nothing about a picture.
    found.retain(|id| *id != egui::TextureId::default());
    found
}

/// A picture for somebody, as the store hands one over.
fn a_published_face() -> sigil_chat::Face {
    sigil_chat::Face {
        hash: 7,
        bytes: std::sync::Arc::new(a_png().to_vec()),
    }
}

/// **The people in the transcript have faces.**
///
/// Every bubble drew the identicon, so the main screen of a messenger was the
/// one place nobody had one -- while the conversation list beside it, the
/// call roster, the call card and the ring all showed published pictures.
/// SIP-21 pictures were reaching the screen everywhere except the screen
/// people read.
///
/// Both halves, because either alone proves nothing: a transcript that
/// ignored the picture and always drew the mark would pass the first, and one
/// that drew a blank square would pass the second.
#[test]
fn the_transcript_draws_the_faces_of_the_people_in_it() {
    let bare = {
        let mut h = harness_with(a_conversation(), true);
        painted_with(&mut h)
    };
    assert!(
        bare.is_empty(),
        "a transcript where nobody published a picture painted with {bare:?} \
         anyway, so the other half of this case would pass whatever the code did"
    );

    let mut state = a_conversation();
    let who = state.lines.iter().find(|l| !l.mine).map(|l| l.who);
    let who = who.expect("the fixture has a message from somebody else");
    state.people.entry(who).or_insert(sigil_chat::Person {
        name: Some("Ada".into()),
        title: None,
        handle: None,
        picture: None,
    });
    state.people.get_mut(&who).expect("just inserted").picture = Some(a_published_face());
    let mut h = harness_with(state, true);
    let drawn = painted_with(&mut h);
    assert!(
        !drawn.is_empty(),
        "somebody in the conversation published a picture and every bubble \
         still drew the identicon"
    );
}

/// **The caller's own face on the ring, where there is one.**
///
/// This screen drew the identicon outright rather than `avatar`, which
/// prefers a published picture and falls back to the mark -- so the one
/// screen in sigil where knowing who is calling matters most was the only
/// one that could not show it, while reading the caller's *name* out of the
/// very same `Person` two lines further down.
///
/// Walked from the paint list rather than asked of the tree, because a
/// picture has no label: what is being checked is that the bytes reached a
/// texture and the texture reached the screen, and the accessibility tree
/// can say neither.
///
/// Both halves, because either alone proves nothing: a ring that ignored the
/// picture and always drew the mark would pass the first, and one that drew
/// a blank square would pass the second.
#[test]
fn a_ring_draws_the_callers_picture_when_there_is_one() {
    fn ringing(with_a_face: bool) -> Vec<egui::TextureId> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::TextureId>) {
            match shape {
                egui::Shape::Mesh(mesh) => out.push(mesh.texture_id),
                egui::Shape::Rect(rect) => {
                    if let Some(brush) = &rect.brush {
                        out.push(brush.fill_texture_id);
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }

        let mut state = a_conversation();
        state.ringing = vec![sigil_chat::Ring {
            channel: [9u8; 32],
            seq: 7,
            from: them(),
            mine: false,
            secret: [3u8; 32],
            answered: false,
            label: "Ada".into(),
            direct: false,
            peer: None,
        }];
        if with_a_face {
            let person = state.people.entry(them()).or_insert(sigil_chat::Person {
                name: Some("Ada".into()),
                title: None,
                handle: None,
                picture: None,
            });
            person.picture = Some(sigil_chat::Face {
                hash: 7,
                bytes: std::sync::Arc::new(a_png().to_vec()),
            });
        }
        let mut h = harness_with(state, true);
        // Twice: the picture is decoded on the pass that first wants it and
        // painted on the next, which is the whole point of caching it.
        h.run();
        h.run();

        let mut found = Vec::new();
        for shape in &h.output().shapes {
            walk(&shape.shape, &mut found);
        }
        // The font atlas is what every glyph on the screen is drawn from and
        // says nothing about a picture.
        found.retain(|id| *id != egui::TextureId::default());
        found
    }

    let without = ringing(false);
    assert!(
        without.is_empty(),
        "a ring from somebody with no picture painted with {without:?} anyway, \
         so the other half of this case would pass whatever the code did"
    );
    let with = ringing(true);
    assert!(
        !with.is_empty(),
        "the caller published a picture and the ring drew the identicon"
    );
}

/// A call we placed is not a ring, and is not offered an Answer button.
#[test]
fn our_own_call_is_shown_as_ringing_out_not_as_an_incoming_ring() {
    let mut state = a_conversation();
    state.ringing = vec![sigil_chat::Ring {
        channel: [9u8; 32],
        seq: 7,
        from: me(),
        mine: true,
        secret: [3u8; 32],
        answered: false,
        label: "Ada".into(),
        direct: false,
        peer: None,
    }];
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Ringing"), "{said}");
    assert!(
        !said.contains("Answer"),
        "answering a call you are placing is nonsense: {said}"
    );
    assert!(said.contains("Cancel"), "{said}");
}

/// Cancel is centred against the words it belongs to, like Decline is.
///
/// A call being placed is drawn in the same banner as a call arriving, so this
/// asks the same thing of it that the incoming ring already gets: the control
/// sits against the middle of the block it acts on, not against one line of it.
///
/// **Measured, because this was wrong by six pixels when it was a bare row.**
/// `ui.horizontal` starts a row at `interact_size.y` — 18px — and centres each
/// item against the height known *when that item is placed*, so words added
/// before a taller button stay where they were put while the button grows the
/// row past them. Three pixels is the bound here: half a line's leading, and
/// the same order as the incoming ring's own arrangement, which this is
/// deliberately a copy of.
#[test]
fn cancel_is_centred_against_the_call_it_would_stop() {
    let mut state = a_conversation();
    state.ringing = vec![sigil_chat::Ring {
        channel: [9u8; 32],
        seq: 7,
        from: me(),
        mine: true,
        secret: [3u8; 32],
        answered: false,
        label: "Ada".into(),
        direct: false,
        peer: None,
    }];
    let mut h = harness_with(state, true);
    h.run();

    // The two lines the button acts on, taken together: what is being called,
    // and that it is ringing.
    let calling = h.get_by_label_contains("Calling").rect();
    let ringing = h.get_by_label_contains("Ringing").rect();
    let block = calling.union(ringing);
    let button = h.get_by_label_contains("Cancel").rect();
    let apart = (block.center().y - button.center().y).abs();
    assert!(
        apart <= 3.0,
        "the button's centre is {apart:.1}px from the middle of the call it \
         would stop: block {block:?}, button {button:?}"
    );
}

/// The receipt is in the bubble, on the time's row.
///
/// It was under the bubble for a while, on the argument that a receipt is
/// what happened to a message rather than part of it. True, and it cost every
/// message a row of its own height plus a mark floating below with nothing to
/// belong to. Where it sits now is where a reader of any other messenger looks
/// for it.
///
/// Measured against the **time**: on the same row their centres agree.
#[test]
fn the_receipt_is_on_the_time_row_inside_the_bubble() {
    let state = a_conversation();
    let mut h = harness_with(state, true);
    h.run();

    // "read", from `Receipt::word` — the fixture's own message carries it.
    let mark = h.get_by_label("read").rect();
    // The nearest time to it, which is the one on its row.
    let time = h
        .get_all_by_label_contains(":")
        .map(|n| n.rect())
        .filter(|r| (r.center().x - mark.center().x).abs() < 300.0)
        .min_by(|a, b| {
            (a.center().y - mark.center().y)
                .abs()
                .partial_cmp(&(b.center().y - mark.center().y).abs())
                .unwrap()
        })
        .expect("a time on the message");
    assert!(
        (mark.center().y - time.center().y).abs() < 4.0,
        "the receipt is not on the time's row: receipt {mark:?}, time {time:?}"
    );
    // And after it, not before: words, time, receipt is the order everywhere.
    // The time is the furthest right; the receipt sits to its left.
    assert!(
        mark.right() <= time.left(),
        "the receipt is after the time: receipt {mark:?}, time {time:?}"
    );
}

/// A short message is one line: the words, the time and the receipt together.
///
/// From a real transcript: "Give it a week." took a bubble two rows tall, the
/// words on one and "13:30" on the other, with the receipt floating under the
/// whole thing. Three rows of screen for four words. Every other messenger
/// puts the time after the words when they fit, and now so does this.
#[test]
fn a_short_message_is_one_line_with_its_time_and_receipt() {
    let mut h = harness_with(a_conversation(), true);
    h.run();

    // Our own text appears twice: as the bubble, and quoted in the reply
    // below it. The bubble is the one in body text, so it is the taller.
    let words = h
        .get_all_by_label_contains("mine, on the other side")
        .map(|n| n.rect())
        .max_by(|a, b| a.height().total_cmp(&b.height()))
        .expect("the message is drawn");
    let mark = h.get_by_label("read").rect();
    let time = h
        .get_all_by_label_contains(":")
        .map(|n| n.rect())
        .min_by(|a, b| {
            (a.center().y - words.center().y)
                .abs()
                .partial_cmp(&(b.center().y - words.center().y).abs())
                .unwrap()
        })
        .expect("a time near the words");

    for (what, r) in [("time", time), ("receipt", mark)] {
        assert!(
            r.top() >= words.top() - 2.0 && r.bottom() <= words.bottom() + 2.0,
            "the {what} is not on the words' line: {what} {r:?}, words {words:?}"
        );
        assert!(
            r.left() >= words.right(),
            "the {what} is not after the words: {what} {r:?}, words {words:?}"
        );
    }
}

/// A long message wraps, and its furniture goes on one row beneath.
///
/// The other half of the rule. `meta_row` draws into a top-down ui and stacked
/// its pieces vertically the first time round: the receipt sat under the time
/// under the text, which is three rows of furniture for one message.
#[test]
fn a_long_message_keeps_its_time_and_receipt_on_one_row_beneath() {
    let mut state = a_conversation();
    state.lines[1].text = "a message long enough that it has to wrap onto a second \
                           line and then a third, so that the time cannot possibly \
                           sit beside it and has to go underneath instead"
        .into();
    let mut h = harness_with(state, true);
    h.run();

    let words = h.get_by_label_contains("a message long enough").rect();
    let mark = h.get_by_label("read").rect();
    let time = h
        .get_all_by_label_contains(":")
        .map(|n| n.rect())
        .filter(|r| (r.center().x - mark.center().x).abs() < 300.0)
        .min_by(|a, b| {
            (a.center().y - mark.center().y)
                .abs()
                .partial_cmp(&(b.center().y - mark.center().y).abs())
                .unwrap()
        })
        .expect("a time on the message");

    assert!(
        time.top() >= words.bottom() - 2.0,
        "the time is not beneath the words: time {time:?}, words {words:?}"
    );
    assert!(
        (mark.center().y - time.center().y).abs() < 4.0,
        "the receipt is not on the time's row: receipt {mark:?}, time {time:?}"
    );
}

/// A bubble is never wider than three quarters of its pane.
///
/// Measured on the words rather than the frame, because a frame is not a node
/// in the tree; a long message's words wrap to the frame's inner width, so
/// they are the frame less its padding, and a rule about the frame holds for
/// them a fortiori.
#[test]
fn a_bubble_is_at_most_three_quarters_of_the_pane() {
    let mut state = a_conversation();
    state.lines[1].text = "x ".repeat(400);
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);

    let words = h.get_by_label_contains("x x x").rect();
    // The pane, with the column hidden: the window less the margin either
    // side. Wider than the transcript actually is, so this is the generous
    // bound -- a bubble over three quarters of the window is over three
    // quarters of anything inside it.
    let pane = 1000.0 - 2.0 * sigil::tokens::SPACING_LG;
    assert!(
        words.width() <= pane * 0.75,
        "the bubble is {} wide in a pane of {pane}: more than three quarters",
        words.width()
    );
}
/// A reserved row keeps its promise: nothing moves between frames.
///
/// The transcript reserves the height of a message nobody can see rather than
/// drawing it — 51 µs each, and a 60 Hz frame's whole budget by four hundred.
/// **The promise is the height.** If a reserved row would have drawn taller or
/// shorter, the content above the reader changes size and the transcript jumps
/// under them, which is the exact fault the anchoring in `transcript_ui` exists
/// to prevent.
///
/// No seam and no flag: the first frame has no remembered heights and so draws
/// everything, and the second reserves. If the two agree on where every visible
/// message sits, the promise held.
#[test]
fn reserving_a_row_does_not_move_anything() {
    let mut state = a_conversation();
    let seed = state.lines[0].clone();
    state.lines = (0..200)
        .map(|i| {
            let mut l = seed.clone();
            l.seq = i as u64 + 1;
            l.text = format!("message number {i}");
            l.reactions = Vec::new();
            l.attachments = Vec::new();
            l.reply_to = None;
            l
        })
        .collect();
    let mut h = harness_with(state, true);

    // Everything drawn, and where.
    h.run();
    let drew = seen_messages(&h);
    assert!(
        drew.len() > 3,
        "the fixture should put several messages on screen: {drew:?}"
    );

    // And again, with heights remembered and the far ones reserved.
    h.step();
    let reserved = seen_messages(&h);

    for (text, rect) in &drew {
        if let Some((_, again)) = reserved.iter().find(|(t, _)| t == text) {
            assert!(
                (rect.top() - again.top()).abs() < 0.5,
                "{text:?} moved from {rect:?} to {again:?} when the rows above \
                 it were reserved rather than drawn"
            );
        }
    }
}

/// The reserving actually happens — otherwise the test above passes by drawing
/// everything twice, which is what it is here to rule out.
///
/// Counted in the app rather than off the screen: egui culls what is outside
/// the clip rect from the accessibility tree already, so sixteen messages are
/// "on screen" whether two hundred were laid out or twenty were.
#[test]
fn the_rows_nobody_can_see_stop_being_drawn() {
    let mut state = a_conversation();
    let seed = state.lines[0].clone();
    state.lines = (0..200)
        .map(|i| {
            let mut l = seed.clone();
            l.seq = i as u64 + 1;
            l.text = format!("message number {i}");
            l.reactions = Vec::new();
            l.attachments = Vec::new();
            l.reply_to = None;
            l
        })
        .collect();
    let mut h = harness_with(state, true);
    h.run();

    // The first frame has no remembered heights, so it draws all of them.
    sigil_chat::reset_drawn();
    h.step();
    let with_heights = sigil_chat::drawn_so_far();
    assert!(
        with_heights < 60,
        "{with_heights} of 200 messages were drawn on a frame that had every \
         height remembered; the far ones are not being reserved"
    );
    assert!(
        with_heights > 0,
        "nothing was drawn at all, which is not virtualisation but a blank \
         transcript"
    );
}

/// Which messages are on screen, and where. Keyed by their text, which is what
/// the fixtures make unique.
fn seen_messages(h: &Harness<'static>) -> Vec<(String, egui::Rect)> {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<(String, egui::Rect)>) {
        let n = node.accesskit_node();
        for said in [n.label(), n.value()] {
            if let Some(said) = said
                && said.starts_with("message number ")
            {
                out.push((said.to_string(), node.rect()));
            }
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(h.root(), &mut out);
    out
}

/// What a long transcript costs to draw, per frame.
///
/// **Ignored, because it is a measurement and not an assertion.** Run it with
/// `cargo test -p sigil-chat --test transcript_ui -- --ignored --nocapture
/// how_much`. It exists so the number can be taken again rather than
/// remembered wrongly.
///
/// On this machine, headless, with plain messages — no pictures, no reactions,
/// no replies, which is the cheap case:
///
/// ```text
///                 laid out in full   reserving what is off screen
///  10 messages:        1.4 ms                1.4 ms
///  50 messages:        3.3 ms                1.6 ms
/// 200 messages:       10.7 ms                2.0 ms
/// 500 messages:       26.1 ms                3.0 ms
/// ```
///
/// The left column is what this cost before rows nobody can see were reserved
/// rather than drawn: linear, about 51 µs a message, past a 60 Hz frame's whole
/// budget by four hundred of them — which `general` reaches. `wanted` was only
/// five per cent of that (measured by stubbing it to a constant: 26.7 ms became
/// 25.3), because egui's galley cache already covers the text; the rest was the
/// bubbles themselves.
///
/// The right column is now, and what growth is left in it is the reservation
/// loop itself — a few microseconds a message to decide not to draw one.
#[test]
#[ignore]
fn how_much_does_a_long_transcript_cost() {
    for n in [10usize, 50, 200, 500] {
        let mut state = a_conversation();
        let seed = state.lines[0].clone();
        state.lines = (0..n)
            .map(|i| {
                let mut l = seed.clone();
                l.seq = i as u64 + 1;
                l.text = format!("message number {i}, of no particular length at all");
                l.reactions = Vec::new();
                l.attachments = Vec::new();
                l.reply_to = None;
                l
            })
            .collect();
        let mut h = harness_with(state, true);
        h.run();
        // Timed after the first frame, so nothing is being warmed up.
        let began = std::time::Instant::now();
        const FRAMES: u32 = 20;
        for _ in 0..FRAMES {
            h.step();
        }
        eprintln!("{n} messages: {:?} per frame", began.elapsed() / FRAMES);
    }
}

/// An account with nothing else linked is told what that costs.
///
/// This is the one warning in the client that is about **permanent** loss. An
/// epoch key arrives sealed against a one-time prekey and opening it spends
/// that prekey, so the exchange will hand over the same envelope tomorrow and
/// it will not open. The copy on this disk is the only one that will ever
/// exist, and a second linked device is the only backup there can be — losing
/// the store with nothing linked loses those conversations for everybody in
/// them, not only for the person who lost the machine.
#[test]
fn an_account_with_no_second_device_is_told_the_store_is_the_only_copy() {
    let mut state = a_conversation();
    state.devices = vec![sigil_chat::Linked {
        device: me(),
        added: NOW - DAY,
        not_after: NOW + 90 * DAY,
        is_this_one: true,
    }];
    let mut h = harness_at(state.clone(), sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("only copy"),
        "the warning has to say the store is unrecoverable: {said}"
    );
    assert!(
        said.contains("Back it up below, or link a second device"),
        "and what to do about it: {said}"
    );
    // SIP-48: backed up, the warning is withdrawn.
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: Some(sigil_chat::HeldBackup {
            generation: 3,
            written: NOW - DAY,
            device: me(),
        }),
        used: 1234,
        quota: 1 << 20,
        words: None,
    });
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("only copy"),
        "backed up, and still warned: {said}"
    );
}

/// SIP-48 in the Devices view: nothing backed up says so and offers to make
/// a key, with Back up now disabled; a key shown is 24 words under the
/// caveat; a backup held says its generation and offers Drop.
#[test]
fn the_backup_section_says_what_the_exchange_holds_and_shows_the_words_once() {
    let mut state = a_conversation();
    state.backup = Some(sigil_chat::Backup {
        has_key: false,
        held: None,
        used: 0,
        quota: 1 << 20,
        words: None,
    });
    let mut h = harness_at(state.clone(), sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Nothing backed up at this exchange."),
        "{said}"
    );
    assert!(said.contains("Make a backup key"), "{said}");
    assert!(said.contains("Back up now"), "{said}");
    assert!(!said.contains("Drop backup"), "nothing to drop: {said}");

    let words: Vec<String> = (1..=24).map(|i| format!("word{i}")).collect();
    state.backup.as_mut().unwrap().has_key = true;
    state.backup.as_mut().unwrap().words = Some(words);
    let mut h = harness_at(state.clone(), sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Write these words down"), "{said}");
    assert!(said.contains("word1") && said.contains("word24"), "{said}");
    assert!(said.contains("Hide"), "{said}");

    state.backup.as_mut().unwrap().words = None;
    state.backup.as_mut().unwrap().held = Some(sigil_chat::HeldBackup {
        generation: 2,
        written: NOW - DAY,
        device: me(),
    });
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Backed up: generation 2"), "{said}");
    assert!(said.contains("Show backup key"), "{said}");
    assert!(said.contains("Drop backup"), "{said}");
}

/// A revoked device says so, rather than looking broken.
///
/// Otherwise it is learned only by being refused as a stranger to every
/// conversation it can see, which reads as everything being broken rather than
/// as this one fact about this one machine.
#[test]
fn a_revoked_device_says_it_has_been_revoked() {
    let mut state = a_conversation();
    state.linked = Some(false);
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    assert!(text_of(&h).contains("revoked"), "{}", text_of(&h));
}

/// The exchange a conversation list belongs to can be **taken**, from the
/// control that says which exchange it is.
///
/// It is the key a receipt verifies under, and the one a client pins
/// independently of whatever it is connected to, so a name for it is not
/// enough. It used to be drawn in full — on the identity menu, under the
/// reader's own key, where it read as a second key of theirs — and the
/// user's call (2026-09-23) is that a copy is what it is for: forty-four
/// characters of base58 in a menu is a wall, and what anybody does with
/// this key is paste it.
#[test]
fn the_exchange_control_offers_the_exchange_key() {
    let mut h = harness_showing_exchange(a_conversation(), &["indra.org"], Some("indra.org"));
    h.run();
    open_exchanges(&mut h);
    h.get_by_label("Copy the exchange's key");
}

/// The control is there with one exchange too: it says which, and it is the
/// way to add another.
#[test]
fn a_single_exchange_is_still_named_and_offers_to_add_one() {
    let mut h = harness_at_exchanges(a_conversation(), &[]);
    h.run();
    open_exchanges(&mut h);
    let said = text_of(&h);
    assert!(!said.contains("indra.org"), "{said}");
    assert!(
        said.contains("Add a domain…"),
        "adding one is always offered: {said}"
    );
}

/// A second exchange is offered as somewhere to switch to, and choosing it
/// switches.
#[test]
fn a_second_exchange_appears_as_somewhere_to_switch_to() {
    let mut h = harness_at_exchanges(a_conversation(), &["indra.org"]);
    h.run();
    open_exchanges(&mut h);
    let said = text_of(&h);
    assert!(
        said.contains("indra.org"),
        "the added exchange is offered: {said}"
    );

    h.get_by_label("indra.org").click();
    h.run();
    h.run();
    // The control itself now says so: what it names is what is being looked
    // at, read back from the app rather than from a fixture.
    let control = h.get_by_label("Exchange").rect();
    let named = h
        .get_all_by_label_contains("indra.org")
        .map(|n| n.rect())
        .any(|r| control.contains(r.center()));
    assert!(
        named,
        "choosing an exchange did not switch to it: {}",
        text_of(&h)
    );
}

/// The exchange control sits in the window's title strip, against its right
/// edge -- the band the close, minimise and zoom buttons live in.
///
/// It was a list in the identity menu, two clicks behind a chevron. Which
/// exchange an identity is looking at changes the whole conversation list, so
/// it belongs where it can be seen at all times.
#[test]
fn the_exchange_control_is_in_the_title_strip_at_the_right() {
    let mut h = harness_at_exchanges(a_conversation(), &["indra.org"]);
    h.run();
    let control = h.get_by_label("Exchange").rect();
    // Above everything the app draws for itself: the identity block is the
    // top-right of the app's own header, and the strip is above that.
    let header = h.get_by_label("Your identity").rect();
    assert!(
        control.bottom() <= header.top(),
        "the control is not in the strip above the app: {control:?} against the \
         header at {header:?}"
    );
    assert!(
        control.height() <= STRIP + 1.0,
        "the control is taller than the strip: {control:?}, strip {STRIP} tall"
    );
    // Against the right edge, allowing the harness's own margin round the
    // window and the strip's inset from the edge.
    assert!(
        control.right() >= 1000.0 - 3.0 * sigil::tokens::SPACING_SM,
        "the control is not against the right edge: {control:?} in 1000"
    );
}

/// "Add a domain…" in the control opens the dialog that adds one.
#[test]
fn the_exchange_control_offers_to_add_a_domain() {
    let mut h = harness_at_exchanges(a_conversation(), &[]);
    h.run();
    open_exchanges(&mut h);
    h.get_by_label("Add a domain…").click();
    h.run();
    assert!(
        text_of(&h).contains("Add an exchange"),
        "the dialog did not open: {}",
        text_of(&h)
    );
}

/// The dialog offers what this exchange federates with (SIP-39 §The peer directory): a peer
/// with a domain, one press away; not one known by key alone, not one
/// already held, and not the exchange itself. Pressing one puts its domain
/// in the box, and Add takes it through the same path as one typed.
#[test]
fn the_dialog_offers_the_exchanges_this_one_federates_with() {
    let mut state = a_conversation();
    state.peers = vec![
        (PubKey::new([5u8; 32]), "trunk.exchange".into()),
        (PubKey::new([6u8; 32]), String::new()),
        (PubKey::new([7u8; 32]), "indra.org".into()),
        (PubKey::new([8u8; 32]), "squic.org".into()),
    ];
    let mut h = harness_at_exchanges(state, &["indra.org"]);
    h.run();
    open_exchanges(&mut h);
    h.get_by_label("Add a domain…").click();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("federates with"), "{said}");
    assert!(
        h.query_by_label("trunk.exchange").is_some(),
        "offered: {said}"
    );
    assert!(
        h.query_by_label("indra.org").is_none(),
        "already held, not offered again: {said}"
    );
    assert!(
        !said.contains(&sigil_ui::message::short(
            &PubKey::new([6u8; 32]).to_string()
        )),
        "a peer without a domain cannot be dialled and is not offered: {said}"
    );
    // The current exchange is squic.org; it is not offered to itself.
    let offers = h.query_all_by_label("squic.org").count();
    assert_eq!(offers, 0, "{said}");

    h.get_by_label("trunk.exchange").click();
    h.run();
    h.get_by_label("Add").click();
    h.run();
    assert!(
        !text_of(&h).contains("Add an exchange"),
        "the dialog stays open, so the add did not go through: {}",
        text_of(&h)
    );
}

/// And the identity menu no longer lists them: one place, not two.
#[test]
fn the_identity_menu_no_longer_lists_exchanges() {
    let mut h = harness_at_exchanges(a_conversation(), &["indra.org"]);
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    assert!(
        !said.contains("indra.org") && !said.contains("Add an exchange"),
        "the identity menu still offers the exchange switcher: {said}"
    );
    // The key of the one being talked to stays, labelled.
    assert!(said.contains("at"), "{said}");
}

/// Replying to a message with a dash in it must not take the application down.
///
/// `short` cut `&text[..8]` and panicked whenever the eighth **byte** fell
/// inside a character. An em dash is three bytes, so "one — and" put one
/// there. It killed the reply bar and the search results, which is to say two
/// of the most ordinary things anybody does.
#[test]
fn a_message_that_is_not_ascii_can_be_replied_to() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "one — and then a café, 日本語, 👍".into();
    let mut h = harness_with(state, true);
    h.run();
    h.get_by_label_contains("one — and").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    // The panic was here, drawing the quote above the composer.
    h.run();
    h.get_by_label_contains("Ada: one — and");
}

/// A reply being written is headed by the quote the reply will carry -- who
/// said it and what, as a reply bubble draws them -- with the way out in its
/// corner, and not by a "Replying to" status line.
#[test]
fn a_reply_is_previewed_as_the_quote_it_will_carry() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "shall we?".into();
    let mut h = harness_with(state, true);
    h.run();
    h.get_by_label_contains("shall we?").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    h.run();
    h.run();
    let quote = h.get_by_label("Ada: shall we?").rect();
    let out = h.get_by_label("Cancel reply").rect();
    let seen = text_of(&h);
    assert!(!seen.contains("Replying to"), "{seen}");
    assert!(
        out.left() > quote.right() && (out.top() - quote.top()).abs() < 12.0,
        "the × is in the corner, right of the quote and level with it: \
         quote {quote:?}, × {out:?}"
    );
    h.get_by_label("Cancel reply").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Ada: shall we?").is_none(),
        "the quote stays after the reply is cancelled: {}",
        text_of(&h)
    );
    assert!(h.query_by_label("Cancel reply").is_none());
}

/// Replying to a picture previews the picture: the thumbnail takes its room
/// before the words, and the words say what is quoted the way the sent
/// reply will.
#[test]
fn a_reply_to_a_picture_is_previewed_with_the_picture() {
    let words_at = |state: ChatState| {
        let mut h = harness_with(state, true);
        h.run();
        h.run();
        h.get_by_label("look").hover();
        h.run();
        h.run();
        h.get_by_label("Reply").click();
        h.run();
        h.run();
        h.get_by_label("Ada: look").rect().left()
    };
    let mut plain = with_pictures(1);
    let last = plain.lines.len() - 1;
    plain.lines[last].attachments.clear();
    let without = words_at(plain);
    let with = words_at(with_pictures(1));
    assert!(
        with > without + 20.0,
        "the thumbnail makes room before the words: {without} -> {with}"
    );

    // Only a picture, and the quote says so, in the words a sent reply uses.
    let mut wordless = with_pictures(1);
    wordless.lines[last].text.clear();
    let mut h = harness_with(wordless, true);
    h.run();
    h.run();
    h.get_by_label("[image 0, 4 KiB]").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    h.run();
    h.run();
    h.get_by_label("Ada: a picture");
}

/// Replying and reacting are on the message, not behind a right-click.
///
/// They lived only in a context menu, which is a control nobody finds: there
/// is nothing on screen to suggest the gesture, and these are the two most
/// common things anybody does to a message. Hovering is how a desktop offers
/// a per-item control, so hovering is what this asserts.
#[test]
fn hovering_a_message_offers_replying_and_reacting() {
    let mut h = harness(true);
    h.run();
    // At rest the transcript is a transcript, not a field of buttons.
    assert!(
        !text_of(&h).contains("Reply"),
        "the controls are not on every message all the time: {}",
        text_of(&h)
    );

    h.get_by_label("one").hover();
    h.run();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Reply"), "hovering offers a reply: {said}");
    for quick in sigil_emoji::QUICK {
        assert!(said.contains(quick), "and a quick reaction {quick}: {said}");
    }
    assert!(said.contains("More emoji"), "and the rest of them: {said}");
    assert!(
        said.contains("More"),
        "and the rest, behind one more control: {said}"
    );
}

/// A quick reaction from the strip is sent, and counted as this person's.
///
/// The count is what the picker's "Frequently used" row is drawn from, so
/// the row is asserted through the app's own reading of it rather than by
/// finding it on screen — the strip carries the same five, and a label found
/// there proves nothing about the row.
#[test]
fn a_quick_reaction_is_sent_and_remembered() {
    let state = a_conversation();
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    h.get_by_label("one").hover();
    h.run();
    h.run();
    h.get_by_label(sigil_emoji::QUICK[2]).click();
    h.run();
    let sent = asked.borrow().join("\n");
    assert!(
        sent.contains("React") && sent.contains(sigil_emoji::QUICK[2]),
        "the strip did not send the reaction: {sent}"
    );
}

/// The picker lists every group under its heading, narrows to a search, and
/// puts the person's own most-used first.
#[test]
fn the_picker_has_groups_a_search_and_the_persons_own_row() {
    let mut h = harness(true);
    h.run();
    hide_column(&mut h);
    h.get_by_label_contains("the second one, then").hover();
    h.step();
    h.step();
    h.step();
    h.get_by_label("More emoji").click();
    h.step();
    h.step();
    let said = text_of(&h);
    // Only the rows on screen are laid out — that is the point of the row
    // view — so the first heading and the first smiley are in the tree and
    // the last group is not until it is scrolled to.
    assert!(
        said.contains(sigil_emoji::Group::Smileys.label()),
        "no first heading: {said}"
    );
    assert!(
        said.contains("\u{1f600}"),
        "the first smiley is offered: {said}"
    );
    assert!(
        !said.contains(sigil_emoji::Group::Flags.label()),
        "every row was laid out at once: {said}"
    );
    assert!(
        !said.contains("Frequently used"),
        "a row of favourites for somebody who has sent none: {said}"
    );

    // The search box has the focus when the picker opens, so that is how it
    // is found: it is the field somebody is typing into.
    let search = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .find(|n| n.is_focused())
        .expect("the picker's search box, focused");
    search.type_text("party popper");
    h.step();
    h.step();
    let said = text_of(&h);
    assert!(
        said.contains("\u{1f389}"),
        "the search did not find it: {said}"
    );
    assert!(
        !said.contains("\u{1f600}"),
        "the search did not narrow the list: {said}"
    );
    assert!(
        !said.contains("Smileys & emotion"),
        "a search result is a flat list, not groups: {said}"
    );
    h.get_by_label("\u{1f389}").click();
    h.step();
    h.step();
    assert!(
        !text_of(&h).contains("Nothing by that name") && !text_of(&h).contains("Flags"),
        "choosing did not close the picker: {}",
        text_of(&h)
    );
}

/// A message in a group offers a direct message with whoever sent it, and
/// pressing it opens the conversation with them -- the same one, whether or
/// not it exists yet, because a direct message's channel is the pair's.
/// Not on your own messages, and not inside the direct message itself,
/// where it would open the conversation already open.
#[test]
fn a_message_in_a_group_offers_a_direct_message_with_its_sender() {
    // The group, open, with the same lines.
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    h.get_by_label("one").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    assert!(text_of(&h).contains("Direct message"), "{}", text_of(&h));
    h.get_by_label("Direct message").click();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains(&format!("OpenDm({:?})", them())),
        "pressing it should open the conversation with the sender: {sent}"
    );

    // Your own message: nobody to message. One of ours at the foot of the
    // transcript, where the pointer can reach it -- the fixture's own sits
    // under the top edge of the pane.
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let mut last = state.lines[1].clone();
    last.seq = 99;
    last.text = "and one more of mine".into();
    last.reply_to = None;
    last.reactions.clear();
    state.lines.push(last);
    let mut h = harness_with(state, true);
    h.run();
    h.get_by_label("and one more of mine").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Delete"), "the menu is open: {said}");
    assert!(!said.contains("Direct message"), "{said}");

    // Inside the direct message with them: already here.
    let mut h = harness(true);
    h.run();
    h.get_by_label("one").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Delete"), "the menu is open: {said}");
    assert!(!said.contains("Direct message"), "{said}");
}

// ---------------------------------------------------------------------------
// Names: what is drawn where one goes, and what it does when pressed.
// ---------------------------------------------------------------------------

/// A key as a reader should see it where a whole one will not fit: its first
/// four characters, three dots, and its last four.
///
/// Deliberately **not** `sigil_ui::short`: a test that shortens the key with
/// the same function the app does asserts that one function agrees with
/// itself. This is the rule stated on its own, and the tests below hold the
/// app to it.
fn short_form(key: &PubKey) -> String {
    let chars: Vec<char> = key.to_string().chars().collect();
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

/// Your own name is a control, because setting it is the thing to do about it.
///
/// It is the head of the identity menu -- one press behind the chevron, on
/// the one row the bar is now -- and pressing it opens the profile.
#[test]
fn clicking_your_own_name_opens_your_profile() {
    let mut h = harness_with(a_conversation(), true);
    h.run();
    assert!(
        !text_of(&h).contains("Your profile"),
        "the profile is open before anybody asked for it"
    );
    assert!(
        h.query_by_label("me").is_none(),
        "the name is on the bar, which is one row and has no room for it"
    );

    open_identity(&mut h);
    h.get_by_label("me").click();
    h.run();
    assert!(
        text_of(&h).contains("Your profile"),
        "pressing your own name did nothing: {}",
        text_of(&h)
    );
}

/// It opens on what is published, not on an empty box.
///
/// An empty box over a name that exists reads as "you have no name", and
/// **publishes** that the moment somebody presses the button -- which is why
/// there is one path in rather than two that seed it two ways.
#[test]
fn your_profile_opens_on_the_name_you_have() {
    let mut h = harness_with(a_conversation(), true);
    h.run();
    open_identity(&mut h);
    h.get_by_label("me").click();
    h.run();
    // The field carries the published name as its value.
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput"))
            .value("me"),
    );
    assert!(
        field.rect().height() > 0.0,
        "the profile opened without the name it is meant to be editing: {}",
        text_of(&h)
    );
}

/// With no name at all, the header shows the start of your key -- and that is
/// a control too, which is the whole point of it.
///
/// It used to be the **whole** key: forty-four characters of base58 in a
/// block clamped to 220 pixels, truncated into something that said nothing and
/// did nothing.
#[test]
fn with_no_name_the_header_is_a_short_key_that_opens_the_profile() {
    let mut state = a_conversation();
    state.mine = Person::default();
    let mut h = harness_with(state, true);
    h.run();
    open_identity(&mut h);

    let said = text_of(&h);
    assert!(
        said.contains(&short_form(&me())),
        "nothing on screen names this identity at all: {said}"
    );
    // The whole key is in the menu -- under "You", where it belongs -- and
    // not where the name goes, which comes first.
    assert!(
        said.find(&short_form(&me())) < said.find(&me().to_string()),
        "the whole key is drawn where a name goes: {said}"
    );

    h.get_by_label_contains(&short_form(&me())).click();
    h.run();
    assert!(
        text_of(&h).contains("Your profile"),
        "pressing the key that stands in for your name did nothing: {}",
        text_of(&h)
    );
}

/// Running the pointer down the conversation list offers no keys.
///
/// Every row used to answer a hover with the other person's whole key, and a
/// pointer crosses rows on its way anywhere -- so the list popped forty-four
/// characters of base58 over the row below, once per row, for a question
/// nobody had asked. The key is still reachable, by a gesture somebody chooses:
/// open the conversation and press Members.
#[test]
fn hovering_a_conversation_offers_no_key() {
    // Nothing open, so "Ada" is the row and not also the author of every
    // message in the transcript -- `get_by_label_contains` refuses an
    // ambiguous match, and a hover on the wrong one of two would test the
    // bubble rather than the row.
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    let mut h = harness_with(state, true);
    h.run();
    h.get_by_label_contains("Ada").hover();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains(&them().to_string()),
        "hovering a conversation put the other person's key on screen: {said}"
    );
    // The row is genuinely under the pointer, or this asserts nothing at all:
    // a hover that missed would pass with the tooltip fully restored.
    assert!(
        said.contains("Ada"),
        "the row being hovered is not on screen: {said}"
    );
}

/// Taking the keys off the hovers did not put a key out of reach.
///
/// This is the other half of `hovering_a_conversation_offers_no_key`: a key
/// stopped being something a pointer trips over on its way somewhere, and it
/// has to still be somewhere a person can *choose* to go. That place is
/// Members, off the conversation's own header, where it is in full, in
/// monospace and selectable -- because it is the only thing that identifies
/// somebody, and everything drawn above it is a claim.
#[test]
fn a_peers_key_is_in_full_in_members() {
    let mut h = harness_at(a_conversation(), sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains(&them().to_string()),
        "the one place a peer's whole key is offered does not have it: {said}"
    );
}

/// A member nobody can name is their key, once -- not a prefix of themselves
/// above themselves.
///
/// `Person::label` falls back to the start of the key, and in this view the
/// whole key is on the very next line, so drawing the label unconditionally
/// puts `3Kj9mNpQ…` directly over `3Kj9mNpQrs…`.
#[test]
fn an_unnamed_member_is_not_drawn_twice() {
    let mut state = a_conversation();
    state.people.clear();
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    let whole = them().to_string();
    assert!(
        said.contains(&whole),
        "the key is still shown in full: {said}"
    );
    // The short form has three dots in it and the whole key has none, so the
    // one cannot be found inside the other: if the short form is on screen at
    // all, it was drawn as its own label.
    assert!(
        !said.contains(&short_form(&them())),
        "an unnamed member is drawn as a short form of themselves, above \
         themselves in full: {said}"
    );
}

/// And pressing the **words** chooses it too.
///
/// `a_conversation_is_chosen_by_pressing_anywhere_on_its_row` presses the
/// empty ground beside the name, which is the half that always worked. The
/// name itself did not: egui makes labels selectable text by default, so every
/// word in the row -- the name, the time, the preview, the unread pill --
/// handled the press for its own text selection and the row underneath never
/// heard it. The row is the largest and most obvious target in the column and
/// most of it was dead.
#[test]
fn a_conversation_is_chosen_by_pressing_its_name() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(a_conversation(), asked.clone());
    h.run();

    // On the name of the other conversation, not beside it.
    let on = h.get_by_label_contains("release check").rect().center();
    h.event(egui::Event::PointerButton {
        pos: on,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: on,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();

    let wanted = format!("Show({:?})", [8u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "pressing the conversation's own name did not open it: {:?}",
        asked.borrow()
    );
}

/// A search result is chosen by pressing it, words included.
///
/// The same defect as `a_conversation_is_chosen_by_pressing_its_name`, one
/// list over: selectable labels take the press for their own text selection
/// and the row underneath never hears it. A search result is **entirely**
/// words, so there was no ground beside them to hit and the whole row was
/// dead, with a pointing hand over it saying otherwise.
///
/// It does not matter that this row claims its rectangle with
/// `Response::interact` *after* its children while the conversation row
/// declares its sense *before* them. That difference was my first explanation
/// for why this one looked fine, and it was wrong -- what actually made it
/// look fine was the assertion below.
#[test]
fn a_search_result_is_chosen_by_pressing_its_words() {
    let mut state = a_conversation();
    // Nothing open, so the search box is the only field on screen -- with a
    // conversation open the composer is a second one and the query is
    // ambiguous.
    state.open = None;
    state.lines = Vec::new();
    state.searched_messages = true;
    state.hits = vec![Hit {
        channel: [8u8; 32],
        seq: 3,
        label: "release check".into(),
        who: "Ada".into(),
        text: "the thing that was said".into(),
        found: 19..23,
        at: NOW - 60,
    }];
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();

    // Results replace the list only while there is something in the box.
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    );
    field.focus();
    field.type_text("said");
    h.run();

    let on = h
        .get_by_label_contains("the thing that was said")
        .rect()
        .center();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: on,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();

    // **The hit's own channel and message, not any `Show` at all.** With
    // nothing open the app opens the latest conversation by itself on the
    // first pass, so `starts_with("Show")` passed with the click six hundred
    // pixels off the row -- a test of the fixture rather than of the press.
    // And `ShowAt`, not `Show`: the result goes to the message, not to the
    // bottom of the conversation it is in.
    let wanted = format!("ShowAt {{ channel: {:?}, seq: 3 }}", [8u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "pressing a search result did not go to it: {:?}",
        asked.borrow()
    );
    // And the row says it was the one chosen: with the pointer gone from
    // it -- hovering fills a row too -- the chosen row is still filled.
    h.event(egui::Event::PointerMoved(egui::pos2(900.0, 600.0)));
    h.run();
    let words = h.get_by_label_contains("the thing that was said").rect();
    assert!(
        wide_fill_at(&h, words.center(), false),
        "the chosen result is not drawn as chosen"
    );
}

/// Whether the last pass painted a wide, row-tall filled rectangle over
/// `at` -- a row's ground, or a message's wash; not a glyph or a panel -- and, with `translucent`,
/// only one that can be seen through, which a bubble's own fill cannot.
fn wide_fill_at(h: &Harness<'static>, at: egui::Pos2, translucent: bool) -> bool {
    fn walk(shape: &egui::Shape, at: egui::Pos2, translucent: bool) -> bool {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().any(|s| walk(s, at, translucent)),
            egui::Shape::Rect(r) => {
                r.rect.contains(at)
                    && r.fill.a() > 0
                    && (!translucent || r.fill.a() < 255)
                    && r.rect.width() > 100.0
                    // A row or a bubble, not a panel's ground.
                    && r.rect.height() < 100.0
            }
            _ => false,
        }
    }
    h.output()
        .shapes
        .iter()
        .any(|c| walk(&c.shape, at, translucent))
}

/// The search box answers to the keyboard: Enter goes to the newest result
/// as pressing it would, and Escape clears the search and brings the list
/// back.
#[test]
fn enter_chooses_the_newest_result_and_escape_clears_the_search() {
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    state.searched_messages = true;
    let hit = |seq: u64, text: &str, at: u64| Hit {
        channel: [8u8; 32],
        seq,
        label: "release check".into(),
        who: "Ada".into(),
        text: text.into(),
        found: 0..4,
        at,
    };
    // Newest first, as the session sorts them.
    state.hits = vec![
        hit(7, "said last", NOW - 60),
        hit(3, "said first", NOW - 600),
    ];
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    );
    field.focus();
    field.type_text("said");
    h.run();
    h.get_by_label_contains("said last");
    asked.borrow_mut().clear();

    h.key_press(egui::Key::Enter);
    h.run();
    let wanted = format!("ShowAt {{ channel: {:?}, seq: 7 }}", [8u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "Enter did not go to the newest result: {:?}",
        asked.borrow()
    );
    assert!(
        !asked.borrow().iter().any(|c| c.contains("seq: 3")),
        "Enter went to the older one as well: {:?}",
        asked.borrow()
    );

    // Escape: the box empties, the search is withdrawn, the list is back.
    h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    )
    .focus();
    h.run();
    asked.borrow_mut().clear();
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(
        asked.borrow().contains(&"Search(\"\")".to_string()),
        "Escape did not withdraw the search: {:?}",
        asked.borrow()
    );
    assert!(
        h.query_by_label_contains("said last").is_none(),
        "the results are still up after Escape: {}",
        text_of(&h)
    );
}

/// Arriving at the message a search result named, the transcript washes it
/// in the accent for a moment, and the wash is gone a few seconds later.
/// Sixty messages and a result naming the fifth, which is off screen.
#[test]
fn the_message_a_result_goes_to_is_washed_and_the_wash_fades() {
    let mut state = a_page(0, 60);
    state.searched_messages = true;
    state.hits = vec![Hit {
        channel: state.open.unwrap(),
        seq: 5,
        label: "release check".into(),
        who: "Ada".into(),
        text: "message 4".into(),
        found: 0..7,
        at: NOW - 600,
    }];
    let mut h = harness_with(state, true);
    h.run();
    h.run();
    // The box is the first text field; the composer is the other.
    let field = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the search box");
    field.focus();
    field.type_text("message 4");
    h.run();
    // The result, in the column: the bubble's own words are off screen, so
    // by position -- the lowest node with those words is the bubble, if it
    // is laid out at all; the result is the one inside the column.
    let result = h
        .get_all_by_label_contains("message 4")
        .map(|n| n.rect())
        .filter(|r| r.right() < 300.0)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the result row");
    press_at(&mut h, result.center());
    // Steps, not runs: the wash asks to be repainted until it has faded,
    // and a step here is a quarter of a second.
    for _ in 0..3 {
        h.step();
    }
    // The bubble is on screen, and washed.
    let bubble = h
        .get_all_by_label("message 4")
        .map(|n| n.rect())
        .find(|r| r.left() > 300.0 && r.top() >= 0.0 && r.bottom() <= 620.0)
        .unwrap_or_else(|| panic!("the message was not brought on screen: {}", text_of(&h)));
    assert!(
        wide_fill_at(&h, bubble.center(), true),
        "the message the result went to is not marked"
    );
    // Three seconds on, it is not.
    for _ in 0..12 {
        h.step();
    }
    assert!(
        !wide_fill_at(&h, bubble.center(), true),
        "the wash did not fade"
    );
}

/// And a direct message's row, which is the one somebody presses most.
///
/// `a_conversation_is_chosen_by_pressing_its_name` presses a public channel.
/// A direct message's row is drawn by the same function with one fewer marker
/// in it, so this should not be able to differ -- which is exactly the reason
/// to check rather than reason about it, since the row somebody actually uses
/// all day is the one it would be worst to get wrong.
#[test]
fn a_direct_message_is_chosen_by_pressing_the_persons_name() {
    let mut state = a_conversation();
    // The *other* conversation is open, so "Ada" is the row in the list and
    // not also the author of every message in the transcript.
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();

    // Two nodes carry the name: the row itself, whose accessible name is
    // built from what is inside it, and the label drawing it. The **label**
    // is the one to press -- pressing the row would test that the row senses
    // a click, which nobody doubted, rather than that the words do.
    let on = h
        .get_all_by_label_contains("Ada")
        .map(|n| n.rect())
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the name is drawn somewhere")
        .center();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: on,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();

    let wanted = format!("Show({:?})", [9u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "pressing somebody's name did not open the conversation with them: {:?}",
        asked.borrow()
    );
}

// ---------------------------------------------------------------------------
// Renaming a group from its own name.
// ---------------------------------------------------------------------------

/// A group's name is the way to change its name.
///
/// It was a caption, and renaming lived behind the settings icon at the far
/// end of the header -- a long way from the thing being renamed, and nothing
/// said it was there.
#[test]
fn an_admin_opens_settings_by_pressing_the_group_name() {
    let mut state = a_conversation();
    // The public channel, so the header names a channel rather than a person.
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    state.i_am_admin = true;
    let routes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_watching_routes(state, routes.clone());
    h.run();
    routes.borrow_mut().clear();

    press_the_heading(&mut h, "release check");

    assert!(
        routes.borrow().contains(&sigil_chat::Route::Settings),
        "pressing the name did not offer to change it: {:?}",
        routes.borrow()
    );
}

/// Press the largest node carrying this text -- the header's heading, rather
/// than the row in the list, which carries the same name in a smaller one.
fn press_the_heading(h: &mut Harness<'static>, text: &str) {
    let on = h
        .get_all_by_label_contains(text)
        .map(|n| n.rect())
        .max_by(|a, b| a.height().total_cmp(&b.height()))
        .expect("the name is drawn somewhere");
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: on.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
}

/// And it opens on the name it has, not on an empty box.
///
/// **The dangerous half.** Neither field was ever seeded, so the pane opened
/// two empty boxes over a channel that had a name and a topic -- which reads
/// as "this has no name" -- and `Set` beside an empty box publishes the empty
/// string. The way in offered to erase what it was showing.
#[test]
fn channel_settings_open_on_the_name_it_already_has() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    state.i_am_admin = true;
    state.topic = "what it is for".into();
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();

    let said = text_of(&h);
    assert!(
        said.contains("Conversation settings"),
        "the settings pane is not open: {said}"
    );
    assert!(
        said.contains("release check"),
        "the name box is empty over a channel that has a name, and Set would \
         publish that: {said}"
    );
    assert!(
        said.contains("what it is for"),
        "and the topic box with it: {said}"
    );
}

/// Somebody who may not rename it is not invited to try.
#[test]
fn a_member_who_is_not_an_admin_is_not_offered_the_rename() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    state.i_am_admin = false;
    let routes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_watching_routes(state, routes.clone());
    h.run();
    routes.borrow_mut().clear();

    press_the_heading(&mut h, "release check");

    assert!(
        !routes.borrow().contains(&sigil_chat::Route::Settings),
        "pressing the name opened settings for somebody who cannot change \
         anything in them: {:?}",
        routes.borrow()
    );
}

/// And neither is a direct message, whose "name" is a person.
///
/// Both members of a direct message are admins of the channel that carries it
/// -- that is how the store records it -- so an `i_am_admin` check on its own
/// offers to rename somebody.
#[test]
fn a_direct_message_is_not_offered_a_rename() {
    let mut state = a_conversation();
    state.open = Some([9u8; 32]);
    state.lines = Vec::new();
    state.i_am_admin = true;
    let routes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_watching_routes(state, routes.clone());
    h.run();
    routes.borrow_mut().clear();

    press_the_heading(&mut h, "Ada");

    assert!(
        !routes.borrow().contains(&sigil_chat::Route::Settings),
        "pressing somebody's name offered to rename them: {:?}",
        routes.borrow()
    );
}

/// A long conversation name does not run over the time beside it.
///
/// From a real list: a group called "Right? Wrrrrooooonggggg!" was drawn
/// straight through its own timestamp, so both were unreadable where they
/// crossed. The name was laid out first and given no width to fit into, so it
/// took the whole row and the right-hand block -- the time and the unread
/// count -- was drawn on top of it.
///
/// The same lesson the conversation *header* already learned: lay the fixed
/// things out from the right first, and give the name what is left.
#[test]
fn a_long_name_does_not_run_over_the_time() {
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    state.conversations[0].label =
        "Right? Wrrrrooooonggggg! And rather longer than that, even".into();
    state.conversations[0].at = Some(NOW - 60);
    let mut h = harness_with(state, true);
    h.run();

    let name = h
        .get_all_by_label_contains("Wrrrrooooonggggg")
        .map(|n| n.rect())
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the name is drawn");
    // The row's time, as the row draws it: through `brief` rather than as a
    // literal, because a literal is the fixed clock in *one* time zone -- this
    // read "12:59" on a machine an hour east of the runner that saw "11:59".
    let time = h
        .get_by_label_contains(&sigil_ui::brief(NOW - 60, NOW))
        .rect();

    assert!(
        name.right() <= time.left() + 0.5,
        "the name is drawn over the time: name ends at {}, time starts at {}",
        name.right(),
        time.left()
    );
}

/// And a long preview stays on one line.
///
/// Not the same fault as the name's, which is why it is worth asking rather
/// than assuming. The preview does not run *over* anything -- it **wraps**,
/// and the row grows a second line, so one long message makes one row taller
/// than every other row in the column. `one_line` already flattens newlines
/// out of it; what it does not do is make it short.
///
/// Measured against the height of the time beside it, which is set in the same
/// small style: one line of it is a row that fits, two is the fault.
#[test]
fn a_long_preview_stays_on_one_line() {
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    state.conversations[0].preview = Some(
        "a preview considerably longer than the column it has to sit in, going on \
         and on well past the point where anybody would still be reading it"
            .into(),
    );
    let mut h = harness_with(state, true);
    h.run();

    let preview = h
        .get_all_by_label_contains("considerably longer")
        .map(|n| n.rect())
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the preview is drawn");
    // A line of the same small style, from the same row. Through `brief`, not
    // a literal: see `a_long_name_does_not_run_over_the_time`.
    let line = h
        .get_by_label_contains(&sigil_ui::brief(NOW - 60, NOW))
        .rect()
        .height();

    assert!(
        preview.height() < line * 1.5,
        "the preview wrapped, so this row is taller than the rest of the \
         column: {} against a line of {line}",
        preview.height()
    );
}

/// The time and the receipt sit against the bubble's right edge, always.
///
/// They were drawn straight after the words, which is flush right only when
/// the bubble is exactly as wide as the words -- and a bubble has a minimum
/// width, and a quoted reply above the words is usually wider than they are.
/// "ok" had its time in the middle of the bubble with fill to the right of it,
/// and a reply's time ended where the words did, short of the quote above.
///
/// Two of our own bubbles, one short and one long: both sit against the pane's
/// right edge, so if the furniture is against each bubble's right edge the two
/// receipts end at the same x. No proxy for the frame needed.
#[test]
fn the_time_and_receipt_are_against_the_bubble_edge() {
    let mut state = a_conversation();
    state.lines.push(Line {
        seq: 9,
        who: me(),
        name: None,
        mine: true,
        at: NOW - 60,
        text: "ok".into(),
        redacted: false,
        edited: false,
        said: None,
        via: None,
        reactions: vec![],
        reply_to: None,
        receipt: Some(Receipt::Read),
        attachments: Vec::new(),
        standing: Default::default(),
        mentions: Vec::new(),
        me_mentioned: false,
        earlier: false,
    });
    state.lines.push(Line {
        seq: 10,
        who: me(),
        name: None,
        mine: true,
        at: NOW - 30,
        text: "a message long enough that it has to wrap onto a second line and \
               then a third, so the time has to go underneath it"
            .into(),
        redacted: false,
        edited: false,
        said: None,
        via: None,
        reactions: vec![],
        reply_to: None,
        receipt: Some(Receipt::Read),
        attachments: Vec::new(),
        standing: Default::default(),
        mentions: Vec::new(),
        me_mentioned: false,
        earlier: false,
    });
    let mut h = harness_with(state, true);
    h.run();

    let marks: Vec<egui::Rect> = h.get_all_by_label("read").map(|n| n.rect()).collect();
    assert!(
        marks.len() >= 3,
        "three of our messages carry a receipt: {marks:?}"
    );
    // The time is the furthest right, against the bubble's edge, on every
    // one of them; the receipt sits to its left on the same row.
    let times: Vec<egui::Rect> = h.get_all_by_label_contains(":").map(|n| n.rect()).collect();
    let mut edges = Vec::new();
    for m in &marks {
        let time = times
            .iter()
            .filter(|t| (t.center().y - m.center().y).abs() < 4.0 && t.left() >= m.right())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap_or_else(|| panic!("no time to the right of the receipt {m:?}"));
        edges.push(time.right());
    }
    let rightmost = edges.iter().copied().fold(f32::MIN, f32::max);
    for e in &edges {
        assert!(
            (e - rightmost).abs() < 1.0,
            "a time is short of the bubble's right edge: {e}, against {rightmost} for the others"
        );
    }
}

/// A picture carries no caption saying it is a picture.
///
/// "[image, 262 KiB]" under every photograph is a line nobody reads, and the
/// size is what a save dialog is for. The words still exist -- on the picture
/// itself, in the accessibility tree, for anything that reads rather than
/// looks -- so this asks that the description belongs to the **image** and to
/// nothing else. A caption drawn as text would be a second node with the same
/// words, and the query below refuses two.
#[test]
fn a_picture_is_not_captioned_with_its_own_size() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    let picture: std::sync::Arc<[u8]> = vec![3u8; 4096].into();
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::IMAGE,
        described: "[image, 4 KiB]".into(),
        size: picture.len() as u64,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: Some(picture),
        missing: false,
        held: false,
        duration_ms: None,
        shape: None,
        waveform: Default::default(),
        id: "captioned".into(),
    }];
    let mut h = harness_with(state, true);
    h.run();

    let only = h.get_by_label_contains("[image, 4 KiB]");
    let role = format!("{:?}", only.accesskit_node().role());
    assert_eq!(
        role, "Image",
        "the description is on something other than the picture"
    );
}

// ---------------------------------------------------------------------------
// Going to the message a reply quotes.
// ---------------------------------------------------------------------------

/// Pressing a quote goes to the message it quotes.
///
/// A quote was a caption. What anybody wants from it is to see the message
/// in full and in context, and that message can be anywhere above -- so the
/// quote is a control, and pressing it scrolls the transcript until the
/// quoted message is in view.
///
/// Sixty messages, the last of which replies to the fifth: far enough up that
/// it is not on screen and, with the transcript reserving rows it cannot see,
/// not even laid out as a node until something scrolls to it.
#[test]
fn pressing_a_quote_scrolls_to_the_message_it_quotes() {
    let mut state = a_page(0, 60);
    let n = state.lines.len();
    state.lines[n - 1].reply_to = Some(Quoted {
        seq: 5,
        who: "Ada".into(),
        said: "message 4".into(),
        preview: None,
    });
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);
    // Settle: the first frame draws everything to learn heights, the next
    // reserves what is off screen.
    h.run();
    h.run();

    // The bubble's own words, by exact label: the quote says "Ada: message 4"
    // and is on screen from the start, so a `contains` would find that.
    let on_screen = |h: &Harness<'static>| {
        h.query_all_by_label("message 4")
            .map(|n| n.rect())
            .any(|r| r.top() >= 0.0 && r.bottom() <= 620.0)
    };
    assert!(
        !on_screen(&h),
        "the quoted message is already on screen, so this tests nothing"
    );

    // The quote, which reads "Ada: message 4". Steps rather than runs:
    // the message arrived at is washed, and the wash repaints until it
    // has faded.
    h.get_by_label_contains("Ada: message 4").click();
    for _ in 0..12 {
        h.step();
    }

    assert!(
        on_screen(&h),
        "pressing the quote did not bring the quoted message on screen: {}",
        text_of(&h)
    );
}

/// A quote of a message on a page not yet fetched asks for the page, and
/// goes there when it arrives.
///
/// A conversation opens on its last page, and a reply can point at anything
/// before it. So the ask is not answered on the pass it is made: the previous
/// page is asked for, the ask is kept, and when the page lands the message is
/// laid out and scrolled to. The state is replaced by hand here, the way the
/// session republishes it when a page arrives.
#[test]
fn a_quote_of_an_unfetched_message_fetches_its_page_and_then_goes_there() {
    // Messages 30..60 loaded, thirty earlier ones not, and the last replies
    // to the fifth.
    let mut state = a_page(30, 60);
    let n = state.lines.len();
    state.lines[n - 1].reply_to = Some(Quoted {
        seq: 5,
        who: "Ada".into(),
        said: "message 4".into(),
        preview: None,
    });
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    hide_column(&mut h);
    h.run();
    asked.borrow_mut().clear();

    h.get_by_label_contains("Ada: message 4").click();
    h.run();
    h.run();

    assert!(
        asked.borrow().iter().any(|c| c == "Earlier"),
        "the quoted message is on a page nobody has fetched, and pressing the \
         quote did not ask for it: {:?}",
        asked.borrow()
    );
}

/// The second half: the page lands, and the transcript goes to the message.
#[test]
fn a_kept_ask_is_answered_when_the_page_arrives() {
    let mut state = a_page(30, 60);
    let n = state.lines.len();
    state.lines[n - 1].reply_to = Some(Quoted {
        seq: 5,
        who: "Ada".into(),
        said: "message 4".into(),
        preview: None,
    });
    let shown = std::rc::Rc::new(std::cell::RefCell::new(state));
    let mut h = harness_of(shown.clone());
    h.run();
    hide_column(&mut h);
    h.run();

    h.get_by_label_contains("Ada: message 4").click();
    h.run();
    h.run();
    assert!(
        h.query_all_by_label("message 4").next().is_none(),
        "the quoted message cannot be here yet: its page has not arrived"
    );

    // The page arrives, as the session would publish it.
    let mut whole = a_page(0, 60);
    let n = whole.lines.len();
    whole.lines[n - 1].reply_to = Some(Quoted {
        seq: 5,
        who: "Ada".into(),
        said: "message 4".into(),
        preview: None,
    });
    *shown.borrow_mut() = whole;
    for _ in 0..12 {
        h.step();
    }

    let on_screen = h
        .query_all_by_label("message 4")
        .map(|n| n.rect())
        .any(|r| r.top() >= 0.0 && r.bottom() <= 620.0);
    assert!(
        on_screen,
        "the page arrived and the transcript did not go to the message: {}",
        text_of(&h)
    );
}

/// "Edit your profile", "Your devices" and "Switch identity" are one run of
/// rows in the same shape.
///
/// Editing was a plain button on its own, between your key and the exchange --
/// among the facts about the identity rather than the things done to it.
/// Now the things done to it are together at the foot, drawn the same way:
/// full width, with an icon, and the way out last.
#[test]
fn editing_your_profile_is_beside_switching_identity_and_shaped_like_it() {
    let mut h = harness(true);
    h.run();
    open_identity(&mut h);

    let rows = ["Edit your profile", "Your devices", "Switch identity"]
        .map(|label| h.get_by_label(label).rect());
    for pair in rows.windows(2) {
        let (above, below) = (pair[0], pair[1]);
        assert!(
            (above.left() - below.left()).abs() < 1.0
                && (above.width() - below.width()).abs() < 1.0,
            "the two are not the same shape: {above:?}, {below:?}"
        );
        assert!(
            above.bottom() <= below.top() && below.top() - above.bottom() < 12.0,
            "the two are not beside each other: {above:?}, {below:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Taking down what will never open.
// ---------------------------------------------------------------------------

/// The notice about unreadable messages offers to delete the ones that are
/// yours, and pressing it asks for each.
///
/// A message that will never open is not always waiting for a key: two
/// pictures with previews over SIP-18's cap sat in a public channel as two
/// unreadable messages for everybody, and the only thing to do with one is
/// take it down. Nothing draws it as a bubble, so nothing else can offer the
/// control.
#[test]
fn the_unreadable_notice_offers_to_delete_what_is_yours() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    state.trouble_with = Trouble {
        unreadable: 3,
        redactable: vec![41, 43],
        ..Default::default()
    };
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();

    let said = text_of(&h);
    assert!(
        said.contains("could not be read by this version"),
        "a public channel's unreadable messages are described as waiting for a \
         key, which no public channel has: {said}"
    );
    // Two of three are mine, so the offer says so.
    h.get_by_label("Delete the 2 of them that are yours")
        .click();
    h.step();

    let wanted: Vec<String> = [41u64, 43].iter().map(|s| format!("Redact({s})")).collect();
    for w in &wanted {
        assert!(
            asked.borrow().contains(w),
            "pressing delete did not ask for {w}: {:?}",
            asked.borrow()
        );
    }
    assert!(
        !asked.borrow().iter().any(|c| c == "Redact(42)"),
        "somebody else's message was asked to be deleted: {:?}",
        asked.borrow()
    );
}

/// With nothing of yours among them, nothing is offered.
#[test]
fn the_unreadable_notice_offers_nothing_when_none_are_yours() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    state.lines = Vec::new();
    state.trouble_with = Trouble {
        unreadable: 2,
        redactable: Vec::new(),
        ..Default::default()
    };
    let mut h = harness_with(state, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("2 messages here"), "{said}");
    assert!(
        !said.contains("Delete"),
        "a delete control is offered for messages that are not yours: {said}"
    );
}

/// In a private channel the words are about the key, because that is
/// usually what it is.
#[test]
fn a_private_channels_unreadable_notice_still_speaks_of_the_key() {
    let mut state = a_conversation();
    // The direct message, which is private.
    state.open = Some([9u8; 32]);
    state.lines = Vec::new();
    state.trouble_with = Trouble {
        unreadable: 1,
        ..Default::default()
    };
    let mut h = harness_with(state, true);
    h.run();
    assert!(
        text_of(&h).contains("its key may still arrive"),
        "{}",
        text_of(&h)
    );
}

/// A video in the transcript is its thumbnail with the length on it, and
/// pressing it asks the session for the file so that it can be played.
///
/// The fixture's video has not been fetched -- a forty-megabyte file is over
/// what is fetched unasked -- so the press is the fetch. What happens when
/// the bytes arrive is the player's business, covered in `sigil-video`.
#[test]
fn a_video_is_fetched_when_its_play_mark_is_pressed() {
    let mut state = a_conversation();
    let n = state.lines.len();
    // Not the tombstone: a redacted message draws no attachments.
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::VIDEO,
        described: "[video 449s, 46.1 MiB]".into(),
        size: 48_308_476,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: true,
        duration_ms: Some(449_344),
        shape: Some((1280, 720)),
        waveform: Default::default(),
        id: "clip".into(),
    }];
    let seq = state.lines[n - 1].seq;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    let video = h.get_by_label("[video 449s, 46.1 MiB]");
    let rect = video.rect();
    // Sixteen by nine at the bubble's width: a video is drawn in its own
    // shape, not as a file row.
    assert!(
        (rect.width() / rect.height() - 16.0 / 9.0).abs() < 0.05,
        "drawn {}x{}",
        rect.width(),
        rect.height()
    );
    assert!(
        !asked.borrow().iter().any(|c| c.starts_with("Fetch")),
        "fetched before anybody asked: {:?}",
        asked.borrow()
    );
    video.click();
    h.run();
    let wanted = format!("Fetch {{ seq: {seq}, index: 0 }}");
    assert!(
        asked.borrow().contains(&wanted),
        "pressing the video should ask for it: {:?}",
        asked.borrow()
    );
}

/// A portrait video is drawn tall and narrow, and its bubble is no wider
/// than it: a tall video used to sit in a bubble the width of a landscape
/// picture, with a field of the bubble's colour beside it.
#[test]
fn a_portrait_video_gets_a_bubble_its_own_width() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = String::new();
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::VIDEO,
        described: "[video 20s, 3.1 MiB]".into(),
        size: 3_250_000,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: true,
        duration_ms: Some(20_000),
        shape: Some((720, 1280)),
        waveform: Default::default(),
        id: "tall".into(),
    }];
    // The bubble's time label, in whatever zone the test runs in.
    let stamp = sigil_ui::clock(state.lines[n - 1].at);
    let mut h = harness_with(state, true);
    h.run();
    let video = h.get_by_label("[video 20s, 3.1 MiB]").rect();
    assert!(
        (video.height() / video.width() - 16.0 / 9.0).abs() < 0.05,
        "drawn {}x{}",
        video.width(),
        video.height()
    );
    // The bubble around it: the widest thing it holds is the video, so it
    // is the video plus its own padding and no more.
    let bubble_right = h
        .get_all_by_label(&stamp)
        .map(|n| n.rect().right())
        .fold(0.0f32, f32::max);
    assert!(
        bubble_right - video.right() < 40.0,
        "the bubble runs {} past the video's right edge",
        bubble_right - video.right()
    );
}

// ---------------------------------------------------------------------------
// Mentions: `@` in the composer.
// ---------------------------------------------------------------------------

/// The composer's box: the lowest text field on the screen, under the
/// search box in the column.
fn composer<'a>(h: &'a Harness<'static>) -> egui_kittest::Node<'a> {
    h.get_all(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    )
    .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
    .expect("a composer")
}

/// What the composer holds, as the tree reports it.
fn composed(h: &Harness<'static>) -> String {
    composer(h)
        .accesskit_node()
        .value()
        .map(|v| v.to_string())
        .unwrap_or_default()
}

/// The group, open, with Ada in it -- where a mention has somebody to mean.
fn the_room() -> ChatState {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    state
}

/// Typing `@` and some of a name offers the room's members by name, with
/// the key; Enter completes the name into the box; and the send carries the
/// key for it -- unless the name was deleted from the box first, in which
/// case there is nothing to carry.
#[test]
fn typing_at_offers_the_room_and_the_send_carries_the_key() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(the_room(), asked.clone());
    h.run();
    assert!(!text_of(&h).contains("@Ada"), "nothing offered before an @");

    let field = composer(&h);
    field.focus();
    field.type_text("@A");
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("@Ada"),
        "the room's member is offered: {said}"
    );
    assert!(
        said.contains(&short_form(&them())),
        "with the key beside the name: {said}"
    );
    assert!(!said.contains("@me"), "not ourselves: {said}");

    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert_eq!(
        composed(&h),
        "@Ada ",
        "Enter completes the name, and does not send"
    );
    assert!(
        !asked.borrow().iter().any(|c| c.starts_with("Post(")),
        "{:?}",
        asked.borrow()
    );

    let field = composer(&h);
    field.focus();
    field.type_text("look");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("Post("), "{sent}");
    assert!(
        sent.contains(&format!("{:?}", them())),
        "the mention's key goes with the message: {sent}"
    );
}

/// The name deleted from the box is a mention not sent.
#[test]
fn a_name_deleted_from_the_box_is_not_a_mention() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(the_room(), asked.clone());
    h.run();
    let field = composer(&h);
    field.focus();
    field.type_text("@A");
    h.run();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert_eq!(composed(&h), "@Ada ");
    // Five backspaces take "@Ada " away; then a word, then send.
    composer(&h).focus();
    for _ in 0..5 {
        h.key_press(egui::Key::Backspace);
    }
    h.run();
    composer(&h).type_text("hi");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("Post("), "{sent}");
    assert!(
        sent.contains("mentions: []"),
        "a deleted name is not a mention: {sent}"
    );
}

/// Escape puts the list away for this `@`, and Enter then sends what is in
/// the box as it is. Typing again brings the list back.
#[test]
fn escape_closes_the_list_and_enter_then_sends() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(the_room(), asked.clone());
    h.run();
    let field = composer(&h);
    field.focus();
    field.type_text("hi @A");
    h.run();
    h.run();
    assert!(text_of(&h).contains("@Ada"));
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("@Ada"),
        "Escape closed it: {}",
        text_of(&h)
    );
    // Typing brings it back.
    composer(&h).focus();
    composer(&h).type_text("d");
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("@Ada"),
        "typing reopened it: {}",
        text_of(&h)
    );
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Post("),
        "Enter with the list closed sends: {sent}"
    );
    assert!(sent.contains("hi @Ad"), "{sent}");
    assert!(sent.contains("mentions: []"), "nothing was chosen: {sent}");
}

// ---------------------------------------------------------------------------
// Files in the composer.
// ---------------------------------------------------------------------------

/// Files picked wait in the composer, each shown with a way to take it
/// back out, and go with the next message in the order they were staged
/// -- less any taken out. No more than the wire's four; the rest are
/// refused and said so.
#[test]
fn staged_files_are_shown_removable_and_sent_with_the_words() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<std::path::PathBuf> = ["a.png", "b.png", "c.mp4", "d.png", "e.png"]
        .iter()
        .map(|n| {
            let p = dir.path().join(n);
            std::fs::write(&p, b"not really").unwrap();
            p
        })
        .collect();
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(the_room());
    app.stage_for_test(me(), "", paths.clone());
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let recorder = asked.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            *recorder.borrow_mut() = app.asked_for_test().to_vec();
        });
    h.run();
    h.run();
    let said = text_of(&h);
    for name in ["a.png", "b.png", "c.mp4", "d.png"] {
        assert!(
            h.query_by_label(&format!("Remove {name}")).is_some(),
            "{name}: {said}"
        );
    }
    assert!(
        h.query_by_label("Remove e.png").is_none(),
        "the fifth is refused: {said}"
    );
    assert!(said.contains("up to 4 files"), "and said: {said}");

    h.get_by_label("Remove b.png").click();
    h.run();
    assert!(h.query_by_label("Remove b.png").is_none());
    assert!(h.query_by_label("Remove a.png").is_some());

    composer(&h).focus();
    composer(&h).type_text("from the walk");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("Post("), "{sent}");
    assert!(sent.contains("from the walk"), "{sent}");
    for name in ["a.png", "c.mp4", "d.png"] {
        assert!(
            sent.contains(name),
            "{name} should go with the message: {sent}"
        );
    }
    assert!(
        !sent.contains("b.png"),
        "the one taken out does not: {sent}"
    );
    let a = sent.find("a.png").unwrap();
    let c = sent.find("c.mp4").unwrap();
    let d = sent.find("d.png").unwrap();
    assert!(a < c && c < d, "in the order staged: {sent}");
    // And the composer is empty again.
    h.run();
    assert!(
        h.query_by_label("Remove a.png").is_none(),
        "{}",
        text_of(&h)
    );
}

/// Files alone are a message: nothing typed, Send sends them.
#[test]
fn files_alone_are_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("only.png");
    std::fs::write(&p, b"x").unwrap();
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(the_room());
    app.stage_for_test(me(), "", vec![p]);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let recorder = asked.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            *recorder.borrow_mut() = app.asked_for_test().to_vec();
        });
    h.run();
    h.get_by_label("Send").click();
    // Steps, not runs: what is in flight draws a spinner, and a spinner
    // asks for the next frame for ever.
    h.run_steps(4);
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Post(") && sent.contains("only.png"),
        "{sent}"
    );
    assert!(
        sent.contains("text: \"\""),
        "no words, and none invented: {sent}"
    );
}

/// The staged files, looked at: two pictures decoded to thumbnails, a clip
/// with its play mark, and the way out on each.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn composer_files_dark() {
    let fixtures = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sigil-video/tests/fixtures"
    ));
    let paths = vec![
        fixtures.join("frames/10.png"),
        fixtures.join("frames/44.png"),
        fixtures.join("two_seconds.mp4"),
    ];
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(the_room());
    app.stage_for_test(me(), "", paths);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            // Without the loaders every thumbnail is egui's broken-picture
            // mark, which is what the first take of this snapshot showed.
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    // The thumbnails are made on threads; give them a moment to land.
    for _ in 0..40 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    hide_column(&mut h);
    h.snapshot("composer_files_dark");
}

// ---------------------------------------------------------------------------
// A gallery: several pictures in one message.
// ---------------------------------------------------------------------------

/// A one-pixel PNG, so a picture loads rather than draws as a broken mark.
fn a_png() -> std::sync::Arc<[u8]> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([90, 120, 255, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner().into()
}

/// A picture the shape a camera gives, and bigger than a phone's screen.
///
/// `a_png` is four pixels across, which is right for the tests that only
/// need a picture to *load* -- but the viewer draws a small picture at its
/// own size on purpose ("a big one fills the window and a small one does not
/// grow"), so at four pixels the viewer's own snapshot was a blue dot. The
/// path a phone actually takes is the other one: a photograph larger than
/// the screen, scaled down to fit it.
///
/// Two bands rather than a flat colour, so the picture has a top and a
/// bottom and a snapshot shows whether it was fitted or stretched.
fn a_photo() -> std::sync::Arc<[u8]> {
    let (w, h) = (900u32, 1200u32);
    let img = image::RgbaImage::from_fn(w, h, |_, y| {
        if y < h / 2 {
            image::Rgba([90, 120, 255, 255])
        } else {
            image::Rgba([40, 60, 140, 255])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner().into()
}

/// The room with one message at the foot carrying `n` pictures.
fn with_pictures(n: usize) -> ChatState {
    let mut state = the_room();
    let last = state.lines.len() - 1;
    state.lines[last].redacted = false;
    state.lines[last].text = "look".into();
    state.lines[last].attachments = (0..n)
        .map(|i| Attached {
            kind: sigil_ui::attachment::IMAGE,
            described: format!("[image {i}, 4 KiB]"),
            size: 4096,
            preview: a_png(),
            bytes: Some(a_png()),
            missing: false,
            held: false,
            duration_ms: None,
            shape: Some((4, 4)),
            waveform: Default::default(),
            id: format!("pic{i}"),
        })
        .collect();
    state
}

/// Several pictures in one message are a gallery -- tiles two across --
/// where one picture is its own row; pressing a tile opens the viewer on
/// that picture, and the viewer moves through the message's pictures.
#[test]
fn several_pictures_are_a_gallery_and_the_viewer_moves_through_them() {
    let mut h = harness_with(with_pictures(3), true);
    h.run();
    h.run();
    let tiles: Vec<egui::Rect> = (0..3)
        .map(|i| h.get_by_label(&format!("[image {i}, 4 KiB]")).rect())
        .collect();
    assert!(
        (tiles[0].top() - tiles[1].top()).abs() < 1.0,
        "the first two share a row: {tiles:?}"
    );
    assert!(
        tiles[2].top() > tiles[0].bottom(),
        "the third is under them: {tiles:?}"
    );
    assert!(
        (tiles[0].width() - tiles[0].height()).abs() < 1.0,
        "a tile is square: {tiles:?}"
    );

    // The second tile opens the viewer on the second picture.
    press_at(&mut h, tiles[1].center());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("2 of 3"), "{said}");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.run();
    assert!(text_of(&h).contains("3 of 3"), "{}", text_of(&h));
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("1 of 3"),
        "round to the first: {}",
        text_of(&h)
    );
    h.get_by_label("Previous").click();
    h.run();
    h.run();
    assert!(text_of(&h).contains("3 of 3"), "{}", text_of(&h));

    // One picture is not a gallery: no place among others.
    let mut h = harness_with(with_pictures(1), true);
    h.run();
    h.run();
    let tile = h.get_by_label("[image 0, 4 KiB]").rect();
    assert!(
        tile.width() > tile.height() * 1.4,
        "a lone picture is drawn wide, not as a tile: {tile:?}"
    );
    press_at(&mut h, tile.center());
    h.run();
    h.run();
    assert!(!text_of(&h).contains(" of 1"), "{}", text_of(&h));
}

/// The gallery and the viewer on a phone.
///
/// Pictures are the one thing on this screen whose size is not the theme's,
/// and the viewer is the one surface that covers the whole of it -- so both
/// are worth a look at 360 points, which neither had ever had.
fn phone_pictures(n: usize) -> Harness<'static> {
    let mut state = with_pictures(n);
    // A photograph rather than the four-pixel stand-in: the viewer draws a
    // small picture at its own size, so at four pixels this snapshot showed
    // a dot and said nothing about fitting one to a phone.
    for a in &mut state.lines.last_mut().expect("a message").attachments {
        a.bytes = Some(a_photo());
        a.preview = a_photo();
        a.shape = Some((900, 1200));
        a.size = 900 * 1200 * 4;
        a.described = a.described.replace("4 KiB", "4.1 MB");
    }
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let margin = sigil::tokens::SPACING_MD;
    Harness::builder()
        .with_size(egui::vec2(PHONE_WIDTH, PHONE_HEIGHT))
        .with_step_dt(0.05)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(margin as i8)),
                )
                .show(ui, |ui| {
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

/// Wait for egui's loader thread to decode the tiles, and **say so if it
/// did not**.
///
/// A fixed number of passes is a guess at how fast a machine is, and a
/// snapshot taken before the pictures arrive is not a failure -- it is a
/// picture of an empty tile, recorded as correct. So this stops as soon as
/// the texture is there, and fails by name if it never is: a blank where a
/// photograph should be is exactly the thing a snapshot cannot tell you
/// about, because it looks like a snapshot.
fn let_pictures_arrive(h: &mut Harness<'static>) {
    let uri = "bytes://pic0";
    for _ in 0..80 {
        h.run();
        let ready = matches!(
            h.ctx.try_load_texture(
                uri,
                egui::TextureOptions::default(),
                egui::SizeHint::Scale(1.0.into()),
            ),
            Ok(egui::load::TexturePoll::Ready { .. })
        );
        if ready {
            // One more pass, so what was decoded is what gets drawn.
            h.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    panic!(
        "{uri} never decoded in two and a half seconds, so the snapshot would \
         be of an empty tile -- which looks exactly like a snapshot"
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_gallery() {
    let mut h = phone_pictures(3);
    let_pictures_arrive(&mut h);
    h.snapshot("phone_gallery");
}

/// One of them opened: the viewer takes the whole screen, and a phone's
/// whole screen is what it has to fit.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_viewer() {
    let mut h = phone_pictures(3);
    let_pictures_arrive(&mut h);
    let tile = h.get_by_label("[image 0, 4.1 MB]").rect();
    press_at(&mut h, tile.center());
    h.run();
    h.run();
    h.remove_cursor();
    h.run();
    h.snapshot("phone_viewer");
}

/// The gallery, looked at: three tiles two across, cropped to fill.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn gallery_dark() {
    let mut state = with_pictures(3);
    let last = state.lines.len() - 1;
    // A clip among them, with its play mark.
    state.lines[last].attachments[2].kind = sigil_ui::attachment::VIDEO;
    state.lines[last].attachments[2].described = "[video 2s, 1.2 MiB]".into();
    state.lines[last].attachments[2].bytes = None;
    // With the image loaders, which the ordinary harness leaves out: the
    // tiles are pictures, and this is the snapshot that looks at them.
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    // The pictures are decoded on egui's loader thread. Through the same
    // wait as the phone's, which stops when the texture is actually there
    // and fails by name when it never is -- a snapshot of an empty tile
    // looks exactly like a snapshot.
    let_pictures_arrive(&mut h);
    hide_column(&mut h);
    h.snapshot("gallery_dark");
}

/// The reply being written, looked at: the quote of the picture in a bubble
/// above the box, with the × in its corner.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn reply_preview_dark() {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(with_pictures(1));
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    for _ in 0..10 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    hide_column(&mut h);
    h.snapshot("reply_preview_dark");
}

/// A rewrite in progress, looked at: the "Rewriting" caption over the
/// quote, "Cancel rewrite" in its corner, and the message's own picture
/// staged as a tile with its ×.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn rewrite_preview_dark() {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(mine_with_pictures("look at this one", 1));
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    h.get_by_label("look at this one").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    h.get_by_label("Edit").click();
    for _ in 0..10 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    hide_column(&mut h);
    h.snapshot("rewrite_preview_dark");
}

/// Reactions, looked at: hung off the bubble's bottom edge, half over it,
/// on both sides of the conversation.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn reactions_dark() {
    let mut state = with_mine("and one of mine", 60);
    let n = state.lines.len();
    state.lines[n - 1].reactions = vec![
        reacted("\u{1f389}", &["You", "Ada", "Bo"], true),
        reacted("\u{1f44d}", &["Ada"], false),
    ];
    state.lines[n - 2].reactions = vec![reacted("\u{2764}", &["You"], true)];
    state.lines[n - 2].redacted = false;
    state.lines[n - 2].text = "theirs, reacted to".into();
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    hide_column(&mut h);
    h.snapshot("reactions_dark");
}

/// A reply to a picture quotes the picture: the thumbnail sits before the
/// words in the quote, and takes its room, so the words start further in
/// than they do in a quote of words alone.
#[test]
fn a_reply_to_a_picture_quotes_the_picture() {
    let quoted = |preview: Option<Thumb>| {
        let mut state = the_room();
        let n = state.lines.len();
        state.lines[n - 1].redacted = false;
        state.lines[n - 1].text = "lovely".into();
        state.lines[n - 1].reply_to = Some(Quoted {
            seq: 1,
            who: "Ada".into(),
            said: "a picture".into(),
            preview,
        });
        state
    };
    let words_at = |state: ChatState| {
        let mut h = harness_with(state, true);
        h.run();
        h.run();
        h.get_by_label("Ada: a picture").rect().left()
    };
    let without = words_at(quoted(None));
    let with = words_at(quoted(Some(Thumb {
        id: "pic0".into(),
        bytes: a_png(),
    })));
    assert!(
        with > without + 20.0,
        "the thumbnail makes room before the words: {without} -> {with}"
    );
}

/// The same, looked at.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn reply_to_picture_dark() {
    let mut state = the_room();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "lovely, where was this?".into();
    state.lines[n - 1].reply_to = Some(Quoted {
        seq: 1,
        who: "Ada".into(),
        said: "a picture".into(),
        preview: Some(Thumb {
            id: "pic0".into(),
            bytes: a_png(),
        }),
    });
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    hide_column(&mut h);
    h.snapshot("reply_to_picture_dark");
}

/// A rewrite is a whole post, so the mentions the message made come into
/// the composer with its words: a name left in the text keeps its key on
/// the rewrite, and a name taken out loses it -- the same rule a fresh
/// message follows. Without this every rewrite silently un-mentioned
/// everybody.
#[test]
fn rewriting_a_message_keeps_the_mentions_its_words_still_make() {
    let mine = |text: &str| {
        let mut state = the_room();
        let mut last = state.lines[1].clone();
        last.seq = 99;
        last.text = text.into();
        last.reply_to = None;
        last.reactions.clear();
        last.mentions = vec![sigil_chat::session::Mentioned {
            key: them(),
            label: "Ada".into(),
        }];
        state.lines.push(last);
        state
    };
    let rewriting = |text: &str| {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut h = harness_recording_commands(mine(text), asked.clone());
        h.run();
        h.get_by_label_contains("thanks").hover();
        h.run();
        h.run();
        h.get_by_label("More").click();
        h.run();
        h.get_by_label("Edit").click();
        h.run();
        h.run();
        assert_eq!(composed(&h), text, "the words come into the box");
        (h, asked)
    };

    // The name kept: so is the key.
    let (mut h, asked) = rewriting("@Ada thanks");
    composer(&h).focus();
    composer(&h).type_text("!");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("edit: Some(99)"), "{sent}");
    assert!(
        sent.contains(&format!("{:?}", them())),
        "the mention's key goes with the rewrite: {sent}"
    );

    // The name taken out: the key goes with it.
    let (mut h, asked) = rewriting("thanks @Ada");
    composer(&h).focus();
    for _ in 0..5 {
        h.key_press(egui::Key::Backspace);
    }
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("edit: Some(99)"), "{sent}");
    assert!(
        sent.contains("mentions: []"),
        "a name no longer in the words is no longer mentioned: {sent}"
    );
}

// ---------------------------------------------------------------------------
// A message the exchange refused comes back.
// ---------------------------------------------------------------------------

/// The recording harness, with a state that can be replaced between passes:
/// what the session would publish next.
fn harness_that_can_be_told(
    state: ChatState,
    asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    next: std::rc::Rc<std::cell::RefCell<Option<ChatState>>>,
    staged: Vec<std::path::PathBuf>,
) -> Harness<'static> {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    if !staged.is_empty() {
        app.stage_for_test(me(), "", staged);
    }
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            if let Some(state) = next.borrow_mut().take() {
                app.show_state_for_test(state);
            }
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            *asked.borrow_mut() = app.asked_for_test().to_vec();
        })
}

/// A message the exchange refused comes back into the box whole -- the
/// words, the file, and what it was replying to -- and one it took does
/// not. The composer used to empty itself on Send and never look back, so
/// every refused message was retyped by hand, which three comments said
/// could not happen.
#[test]
fn a_refused_message_comes_back_into_the_box() {
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("walk.png");
    std::fs::write(&picture, b"not really").unwrap();
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let next = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut h = harness_that_can_be_told(the_room(), asked.clone(), next.clone(), vec![picture]);
    h.run();
    h.get_by_label("one").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    h.run();
    composer(&h).focus();
    composer(&h).type_text("from the walk");
    h.run();
    h.key_press(egui::Key::Enter);
    // Steps: the picture on its way draws a spinner, and `run` waits for a
    // pass that asks for nothing.
    h.run_steps(4);
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("token: 1"), "{sent}");
    assert_eq!(composed(&h), "", "the box empties on Send");
    assert!(h.query_by_label("Cancel reply").is_none());
    assert!(h.query_by_label("Remove walk.png").is_none());

    // The exchange says no.
    let mut refused = the_room();
    refused.posted = Some(Posted {
        token: 1,
        trouble: Some("the exchange said no".into()),
    });
    *next.borrow_mut() = Some(refused);
    h.run();
    h.run();
    assert_eq!(composed(&h), "from the walk", "the words are back");
    assert!(
        h.query_by_label("Cancel reply").is_some(),
        "and what it replied to: {}",
        text_of(&h)
    );
    assert!(
        h.query_by_label("Remove walk.png").is_some(),
        "and the file: {}",
        text_of(&h)
    );
    assert!(!text_of(&h).contains("Put it back"), "{}", text_of(&h));

    // Sent again, and taken this time: nothing comes back. Steps, because
    // what is in flight draws a spinner.
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(composed(&h), "");
    let mut taken = the_room();
    taken.posted = Some(Posted {
        token: 2,
        trouble: None,
    });
    *next.borrow_mut() = Some(taken);
    h.run();
    h.run();
    assert_eq!(composed(&h), "", "a message that went stays gone");
    assert!(h.query_by_label("Remove walk.png").is_none());
}

/// A refused message does not write over the next one being typed: it is
/// offered under the box, to be put back or let go.
#[test]
fn a_refused_message_does_not_overwrite_the_next_one() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let next = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut h = harness_that_can_be_told(the_room(), asked.clone(), next.clone(), Vec::new());
    h.run();
    composer(&h).focus();
    composer(&h).type_text("the first");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    composer(&h).focus();
    composer(&h).type_text("the second, half");
    h.run();

    let mut refused = the_room();
    refused.posted = Some(Posted {
        token: 1,
        trouble: Some("the exchange said no".into()),
    });
    *next.borrow_mut() = Some(refused);
    h.run();
    h.run();
    assert_eq!(
        composed(&h),
        "the second, half",
        "what was being typed stays"
    );
    let said = text_of(&h);
    assert!(said.contains("Not sent"), "{said}");
    assert!(said.contains("the first"), "{said}");
    assert!(said.contains("the exchange said no"), "{said}");

    h.get_by_label("Put it back").click();
    h.run();
    h.run();
    assert_eq!(composed(&h), "the first");
    assert!(!text_of(&h).contains("Put it back"), "{}", text_of(&h));
}

// ---------------------------------------------------------------------------
// Rewrites: offered only when they can land, and one thing at a time.
// ---------------------------------------------------------------------------

/// The room with one more message of mine at its foot, `ago` seconds old.
fn with_mine(text: &str, ago: u64) -> ChatState {
    let mut state = the_room();
    let mut last = state.lines[1].clone();
    last.seq = 99;
    last.at = NOW - ago;
    last.text = text.into();
    last.reply_to = None;
    last.reactions.clear();
    state.lines.push(last);
    state
}

/// Edit is offered on a message of mine for a day (SIP-19's window) and
/// not after: past it every reader drops the rewrite, ours included, so
/// the button would do nothing and say nothing.
#[test]
fn edit_is_offered_only_inside_the_window() {
    let more_on = |state: ChatState| {
        let mut h = harness_with(state, true);
        h.run();
        h.get_by_label("still mine").hover();
        h.run();
        h.run();
        h.get_by_label("More").click();
        h.run();
        let said = text_of(&h);
        assert!(said.contains("Delete"), "the menu is open: {said}");
        said.contains("Edit")
    };
    assert!(more_on(with_mine("still mine", 3600)), "an hour old");
    assert!(
        !more_on(with_mine("still mine", 25 * 3600)),
        "a day and an hour old"
    );
}

/// Reply and Edit are not a pair: a rewrite that also picked up a reply
/// would re-thread the message, and only one of them is shown above the
/// box. Arming one disarms the other.
#[test]
fn reply_and_edit_disarm_each_other() {
    let arm = |first: &str, then: &str| {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut h = harness_recording_commands(with_mine("still mine", 60), asked.clone());
        h.run();
        for what in [first, then] {
            // The reply is to a message near the foot of the transcript:
            // once the rewrite's head is above the box, "one" sits under
            // the header, where a control beside it cannot be pressed.
            let (on, item) = match what {
                "reply" => ("the second one, then", "Reply"),
                _ => ("still mine", "Edit"),
            };
            // The lowest of that name: the column previews it too.
            h.get_all_by_label(on)
                .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
                .expect("the message")
                .hover();
            h.run();
            h.run();
            if item == "Edit" {
                h.get_by_label("More").click();
                h.run();
            }
            h.get_by_label(item).click();
            h.run();
            h.run();
        }
        let heads = ["Cancel reply", "Cancel rewrite"]
            .iter()
            .filter(|l| h.query_by_label(l).is_some())
            .count();
        assert_eq!(heads, 1, "one head above the box: {}", text_of(&h));
        composer(&h).focus();
        composer(&h).type_text(" now");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        h.run();
        asked.borrow().join(" | ")
    };
    let sent = arm("reply", "edit");
    assert!(
        sent.contains("reply: None, edit: Some(99)"),
        "Reply then Edit is a rewrite: {sent}"
    );
    let sent = arm("edit", "reply");
    assert!(
        sent.contains("reply: Some(4), edit: None"),
        "Edit then Reply is a reply: {sent}"
    );
}

/// Pressing Edit over a message half typed keeps it: the rewrite takes the
/// box, and what was there comes back when the rewrite is sent or dropped.
#[test]
fn a_rewrite_does_not_throw_away_what_was_being_typed() {
    let begin = || {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut h = harness_recording_commands(with_mine("still mine", 60), asked.clone());
        h.run();
        composer(&h).focus();
        composer(&h).type_text("half a th");
        h.run();
        h.get_by_label("still mine").hover();
        h.run();
        h.run();
        h.get_by_label("More").click();
        h.run();
        h.get_by_label("Edit").click();
        h.run();
        h.run();
        assert_eq!(composed(&h), "still mine", "the rewrite takes the box");
        (h, asked)
    };

    let (mut h, _) = begin();
    h.get_by_label("Cancel rewrite").click();
    h.run();
    h.run();
    assert_eq!(composed(&h), "half a th", "dropped: the words come back");

    let (mut h, asked) = begin();
    composer(&h).focus();
    composer(&h).type_text("!");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert!(asked.borrow().join(" | ").contains("edit: Some(99)"));
    assert_eq!(composed(&h), "half a th", "sent: the words come back");
}

// ---------------------------------------------------------------------------
// A rewrite is a whole post: its files, its mentions, and the way out.
// ---------------------------------------------------------------------------

/// A message of mine at the foot of the room, with `n` pictures on it.
fn mine_with_pictures(text: &str, n: usize) -> ChatState {
    let mut state = with_mine(text, 60);
    let last = state.lines.len() - 1;
    state.lines[last].attachments = with_pictures(n).lines.last().unwrap().attachments.clone();
    state
}

/// Rewriting a message shows its files as tiles beside any new ones, each
/// with its ×; the rewrite keeps the ones left and takes the others off.
/// And a picture's caption can be cleared: files alone are a message.
#[test]
fn a_rewrite_shows_the_files_it_carries_and_keeps_only_those_left() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(mine_with_pictures("look", 2), asked.clone());
    h.run();
    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Remove [image 0, 4 KiB]").is_some()
            && h.query_by_label("Remove [image 1, 4 KiB]").is_some(),
        "the original's pictures are tiles: {}",
        text_of(&h)
    );
    h.get_by_label("Remove [image 1, 4 KiB]").click();
    h.run();
    assert!(h.query_by_label("Remove [image 1, 4 KiB]").is_none());

    // The caption goes too; the picture left is enough to send.
    composer(&h).focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.key_press(egui::Key::Backspace);
    h.run();
    assert_eq!(composed(&h), "");
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("text: \"\", reply: None, edit: Some(99)"),
        "a wordless rewrite goes: {sent}"
    );
    assert!(
        sent.contains("keep: [\"pic0\"]"),
        "the picture left is kept, the one removed is not: {sent}"
    );
    assert!(
        h.query_by_label("Remove [image 0, 4 KiB]").is_none(),
        "sent: the tiles are gone"
    );
}

/// Dropping a rewrite drops the files staged for it, the original's
/// included; none of them belongs to the next message. And Escape drops a
/// reply or a rewrite as the × does, while the box has the keyboard.
#[test]
fn dropping_a_rewrite_drops_its_files_and_escape_drops_either() {
    let mut h = harness_with(mine_with_pictures("look", 1), true);
    h.run();
    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Remove [image 0, 4 KiB]").is_some());
    h.get_by_label("Cancel rewrite").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Remove [image 0, 4 KiB]").is_none(),
        "the tile went with the rewrite: {}",
        text_of(&h)
    );
    assert_eq!(composed(&h), "");

    // Escape, from the box.
    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.run();
    composer(&h).focus();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Cancel rewrite").is_none(),
        "Escape drops the rewrite: {}",
        text_of(&h)
    );
    assert_eq!(composed(&h), "", "and its words");

    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("Reply").click();
    h.run();
    composer(&h).focus();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Cancel reply").is_none(),
        "Escape drops the reply: {}",
        text_of(&h)
    );
}

/// A mention whose name has changed since the message was written is not
/// in the words any more, so no rewrite can find it there -- and no rewrite
/// typed it out, so it goes as it is rather than being dropped.
#[test]
fn a_rewrite_carries_a_mention_whose_name_has_changed() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut state = with_mine("thanks @Countess", 60);
    let last = state.lines.len() - 1;
    state.lines[last].mentions = vec![sigil_chat::session::Mentioned {
        key: them(),
        label: "Ada".into(),
    }];
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    h.get_by_label("thanks @Countess").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.run();
    composer(&h).focus();
    composer(&h).type_text("!");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("edit: Some(99)"), "{sent}");
    assert!(
        sent.contains(&format!("{:?}", them())),
        "the renamed mention still goes: {sent}"
    );
}

/// A reply to a message this reader does not hold -- from before it joined,
/// or pruned -- is quoted as what it is, "an earlier message", with no
/// author's colon in front of it, rather than drawn as no reply at all.
#[test]
fn a_reply_to_a_message_not_held_is_quoted_as_an_earlier_message() {
    let mut state = the_room();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "as I said".into();
    state.lines[n - 1].reply_to = Some(Quoted::unheld(1));
    let mut h = harness_with(state, true);
    h.run();
    h.run();
    h.get_by_label("an earlier message");
    assert!(
        h.query_by_label_contains(": an earlier message").is_none(),
        "{}",
        text_of(&h)
    );
}

// ---------------------------------------------------------------------------
// Pictures: the viewer waits, every file can be saved, staging is careful.
// ---------------------------------------------------------------------------

/// Next onto a picture whose bytes have not arrived keeps the viewer up on
/// its thumbnail and says what is happening, instead of shutting it. One
/// too big to come unasked offers to fetch it; one the exchange would not
/// give offers another try.
#[test]
fn the_viewer_waits_on_a_picture_not_yet_fetched() {
    let open_on_second = |state: ChatState, asked: std::rc::Rc<std::cell::RefCell<Vec<String>>>| {
        let mut h = harness_recording_commands(state, asked);
        h.run();
        h.run();
        let tile = h.get_by_label("[image 0, 4 KiB]").rect();
        press_at(&mut h, tile.center());
        h.run();
        h.run();
        assert!(text_of(&h).contains("1 of 3"), "{}", text_of(&h));
        h.get_by_label("Next").click();
        h.run();
        h.run();
        h
    };

    // Still on its way.
    let mut state = with_pictures(3);
    let last = state.lines.len() - 1;
    state.lines[last].attachments[1].bytes = None;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let h = open_on_second(state, asked);
    let said = text_of(&h);
    assert!(said.contains("2 of 3"), "the viewer stays up: {said}");
    assert!(said.contains("fetching the full image"), "{said}");
    assert!(
        h.query_by_label("Save…").is_none(),
        "nothing to save yet: {said}"
    );

    // Too big to come unasked: ask.
    let mut state = with_pictures(3);
    state.lines[last].attachments[1].bytes = None;
    state.lines[last].attachments[1].held = true;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = open_on_second(state, asked.clone());
    h.get_by_label("Fetch").click();
    h.run();
    assert!(
        asked
            .borrow()
            .iter()
            .any(|c| c == "Fetch { seq: 5, index: 1 }"),
        "{:?}",
        asked.borrow()
    );

    // Refused by the exchange: try again.
    let mut state = with_pictures(3);
    state.lines[last].attachments[1].bytes = None;
    state.lines[last].attachments[1].missing = true;
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = open_on_second(state, asked.clone());
    assert!(
        text_of(&h).contains("could not be fetched"),
        "{}",
        text_of(&h)
    );
    h.get_by_label("Try again").click();
    h.run();
    assert!(
        asked.borrow().iter().any(|c| c == "Refetch"),
        "{:?}",
        asked.borrow()
    );
}

/// A message with several files offers each of them to save and to
/// forward, by name; "Save file" on a gallery of three said nothing about
/// which, and always took the first.
#[test]
fn each_file_on_a_message_can_be_saved_and_forwarded() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(with_pictures(3), asked.clone());
    h.run();
    h.run();
    h.get_by_label("look").hover();
    h.run();
    h.run();
    h.get_by_label("More").click();
    h.run();
    let said = text_of(&h);
    for i in 0..3 {
        assert!(
            said.contains(&format!("Forward [image {i}, 4 KiB]")),
            "{said}"
        );
        assert!(said.contains(&format!("Save [image {i}, 4 KiB]")), "{said}");
    }
    assert!(!said.contains("Save file"), "{said}");
    h.get_by_label("Forward [image 1, 4 KiB]").click();
    h.run();
    h.run();
    // The destination list: the other conversation, lowest of the "Ada"s
    // on screen -- the list is above the composer, under the transcript.
    h.get_all_by_label("Ada")
        .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("the direct message")
        .click();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Forward { seq: 5, index: 1,"),
        "the second file, not the first: {sent}"
    );
}

/// Staging is careful: the same file twice is one tile, a path that is not
/// a file is refused with its name and does not fail the message later, and
/// "left out" goes away once room is made.
#[test]
fn staging_refuses_duplicates_and_non_files_and_forgets_a_stale_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let make = |n: &str| {
        let p = dir.path().join(n);
        std::fs::write(&p, b"not really").unwrap();
        p
    };
    let a = make("a.png");
    let paths = vec![
        a.clone(),
        a.clone(),
        dir.path().join("never-made.png"),
        make("b.png"),
        make("c.png"),
        make("d.png"),
        make("e.png"),
    ];
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let next = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut h = harness_that_can_be_told(the_room(), asked.clone(), next, paths);
    h.run();
    h.run();
    assert_eq!(
        h.get_all_by_label("Remove a.png").count(),
        1,
        "once: {}",
        text_of(&h)
    );
    let said = text_of(&h);
    assert!(said.contains("Not a file: never-made.png"), "{said}");
    assert!(
        said.contains("1 left out"),
        "a, b, c, d fit; e does not: {said}"
    );
    assert!(h.query_by_label("Remove e.png").is_none());

    h.get_by_label("Remove d.png").click();
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("left out"),
        "room was made: {}",
        text_of(&h)
    );
}

/// Reactions hang off the bubble: half over its bottom edge and half
/// below, not in a row of their own under it -- and the next message
/// starts clear of them, even the next in a run from the same person,
/// which follows closest.
#[test]
fn reactions_hang_off_the_bubbles_bottom_edge() {
    let mut state = with_mine("and the next", 60);
    let n = state.lines.len();
    state.lines[n - 2].redacted = false;
    state.lines[n - 2].text = "theirs, reacted to".into();
    state.lines[n - 2].reactions = vec![reacted("\u{2764}", &["You"], true)];
    // Theirs too, so it is grouped under the reacted one.
    state.lines[n - 1].who = them();
    state.lines[n - 1].mine = false;
    state.lines[n - 1].name = Some("Ada".into());
    state.lines[n - 1].receipt = None;
    let mut h = harness_with(state, true);
    h.run();
    h.run();
    // The bubble's bottom edge is the time's bottom plus the bubble's own
    // padding; the chip's middle should sit on it.
    let words = h.get_by_label("theirs, reacted to").rect();
    let time = h
        .get_all_by_label(&sigil_ui::clock(NOW - 30))
        .map(|t| t.rect())
        .find(|r| (r.center().y - words.center().y).abs() < 4.0)
        .expect("the time beside the words");
    let heart = h.get_by_label("\u{2764}").rect();
    let edge = time.bottom() + 12.0;
    assert!(
        (heart.center().y - edge).abs() <= 3.0,
        "the chip straddles the bubble's edge at {edge}: {heart:?}"
    );
    // The next bubble's top edge is its words' top less the padding; it
    // must not run under the chip.
    let next = h.get_by_label("and the next").rect();
    let next_edge = next.top() - 12.0;
    assert!(
        next_edge >= heart.bottom() + 2.0,
        "the next bubble starts clear of the chip: chip {heart:?}, next edge {next_edge}"
    );
}

// ---------------------------------------------------------------------------
// Muting a conversation.
// ---------------------------------------------------------------------------

/// The bell in the conversation's header mutes it: the control turns into
/// its opposite, the row shows the muted mark, and the channel settings say
/// the same thing; pressing again unmutes.
#[test]
fn the_bell_mutes_a_conversation_and_the_row_says_so() {
    let mut h = harness_with(the_room(), true);
    h.run();
    assert!(
        h.query_by_label("muted").is_none(),
        "not muted to begin with: {}",
        text_of(&h)
    );
    h.get_by_label("Mute this conversation").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("muted").is_some(),
        "the row carries the mark: {}",
        text_of(&h)
    );
    assert!(h.query_by_label("Unmute this conversation").is_some());
    assert!(h.query_by_label("Mute this conversation").is_none());

    // Pressed again: unmuted.
    h.get_by_label("Unmute this conversation").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("muted").is_none(),
        "unmuted: {}",
        text_of(&h)
    );
}

/// The channel settings carry the same switch, ticked when the
/// conversation is muted, and it is yours whatever your standing there.
#[test]
fn the_channel_settings_carry_the_mute() {
    let mut state = the_room();
    state.i_am_admin = false;
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    // **Read as a person reads it: the words change.** This asked
    // accesskit for a checkbox's `toggled` state -- which is exactly what
    // a switch reported as working while looking unchanged on screen,
    // once before today. The control is now the app's own and says what it
    // is rather than ticking a box.
    let said = text_of(&h);
    assert!(
        said.contains("Said out loud") && !said.contains("Muted"),
        "not muted to begin with: {said}"
    );
    h.get_by_label("Said out loud").click();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Muted") && !said.contains("Said out loud"),
        "it did not read as muted after the press: {said}"
    );
}

// ---------------------------------------------------------------------------
// Commands: `/` in the composer.
// ---------------------------------------------------------------------------

/// Typing `/` lists the commands with what each does; more of a word
/// narrows the list; Enter on one that takes nothing runs it, at once, and
/// the box is empty afterwards -- nothing was posted.
#[test]
fn typing_a_slash_offers_the_commands_and_enter_runs_one() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    // A private group: the one kind that can be called.
    let mut state = the_room();
    for c in &mut state.conversations {
        c.public = Some(false);
    }
    let mut h = harness_recording_commands(state, asked.clone());
    h.run();
    assert!(
        !text_of(&h).contains("/call"),
        "nothing offered before a slash"
    );

    let field = composer(&h);
    field.focus();
    field.type_text("/");
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("/call") && said.contains("/topic <text>"),
        "{said}"
    );
    assert!(
        said.contains("Ring this conversation"),
        "each says what it does: {said}"
    );

    composer(&h).type_text("ca");
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("/call"), "{said}");
    assert!(
        !said.contains("/topic"),
        "narrowed by what was typed: {said}"
    );

    asked.borrow_mut().clear();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Call {"),
        "Enter on /call did not call: {sent}"
    );
    assert!(
        !sent.contains("Post("),
        "a command was posted as a message: {sent}"
    );
    assert_eq!(composed(&h), "", "the box is emptied");
}

/// A command with an argument is completed by the list and run by Enter
/// once the argument is typed; a word that is no command stays in the box
/// and is said to be none; and `//` sends a message that begins with `/`.
#[test]
fn a_command_takes_its_argument_and_a_mistake_is_said() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(the_room(), asked.clone());
    h.run();

    let field = composer(&h);
    field.focus();
    field.type_text("/to");
    h.run();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert_eq!(composed(&h), "/topic ", "completed, with room for the text");
    assert!(
        !asked.borrow().iter().any(|c| c.starts_with("SetTopic")),
        "run before its argument was typed"
    );
    composer(&h).focus();
    composer(&h).type_text("release day");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("SetTopic(\"release day\")"), "{sent}");
    assert_eq!(composed(&h), "");

    // Not a command: kept, and said, and nothing asked of the session.
    composer(&h).focus();
    composer(&h).type_text("/frobnicate");
    h.run();
    let before = asked.borrow().len();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert_eq!(
        composed(&h),
        "/frobnicate",
        "the line is kept for correcting"
    );
    assert!(text_of(&h).contains("is not a command"), "{}", text_of(&h));
    assert_eq!(asked.borrow().len(), before, "{:?}", asked.borrow());

    // Two slashes: a message that starts with one.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(the_room(), asked.clone());
    h.run();
    composer(&h).focus();
    composer(&h).type_text("//etc/hosts is gone");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("Post(") && sent.contains("text: \"/etc/hosts is gone\""),
        "{sent}"
    );
}

// ---------------------------------------------------------------------------
// Presence: whether the people you talk to are there.
// ---------------------------------------------------------------------------

fn seen(seen: sigil_chat::presence::Seen, last_seen: u64) -> sigil_chat::presence::Presence {
    sigil_chat::presence::Presence {
        seen,
        last_seen,
        read_at: NOW,
    }
}

/// The other person's presence is on their mark in the list -- the dot in
/// their state's colour, filled or hollow, and the word on the node -- and
/// nothing is on a group's, which is many people.
#[test]
fn a_direct_messages_row_carries_the_other_persons_presence() {
    use sigil_chat::presence::Seen;
    let colours = theme::dark();
    for (state_of_them, word, filled, colour) in [
        (Seen::Active, "active", true, colours.link_up),
        (Seen::Away, "away", true, colours.warning),
        (Seen::Offline, "offline", false, colours.text_muted),
    ] {
        let mut state = a_conversation();
        // Nothing open, so the only mark on screen with the word is the row's.
        state.open = None;
        state.lines = Vec::new();
        state
            .presence
            .insert(them(), seen(state_of_them, NOW - 120));
        let mut h = harness_with(state, true);
        h.run();
        // The row's mark is in the column, at the left; ours is on the bar
        // at the right, and says "active" whatever Ada is.
        let mark = h
            .query_all_by_label(word)
            .map(|n| n.rect())
            .find(|r| (r.width() - tokens::AVATAR_MD).abs() < 1.0 && r.left() < 300.0)
            .unwrap_or_else(|| panic!("{word}: not on the row's mark: {}", text_of(&h)));
        let corner = mark.right_bottom() - egui::vec2(tokens::SPACING_XS, tokens::SPACING_XS);
        assert_eq!(
            small_disc_at(&h, corner, colour),
            Some(filled),
            "{word}: no disc in its colour on the row's mark"
        );
    }
    // The group's row has no dot -- a group is many people -- and a person
    // never read about is offline with nothing to say about when.
    let mut state = a_conversation();
    state.open = None;
    state.lines = Vec::new();
    let mut h = harness_with(state, true);
    h.run();
    let in_column: Vec<egui::Rect> = h
        .query_all_by_label("offline")
        .chain(h.query_all_by_label("away"))
        .chain(h.query_all_by_label("active"))
        .map(|n| n.rect())
        .filter(|r| r.left() < 300.0)
        .collect();
    assert_eq!(
        in_column.len(),
        1,
        "one direct message, one dot: {}",
        text_of(&h)
    );
}

/// In the Members view each member's mark carries their presence, ours from
/// this machine and everybody else's from their beacon, with when they
/// were last there for a pointer.
#[test]
fn members_carry_their_presence_and_when_they_were_last_there() {
    let mut state = a_conversation();
    state
        .presence
        .insert(them(), seen(sigil_chat::presence::Seen::Away, NOW - 3 * 60));
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("away"), "Ada is away: {said}");
    assert!(said.contains("active"), "we are active: {said}");
    // And when: on the mark's hover.
    let mark = h
        .get_all_by_label("away")
        .find(|n| (n.rect().width() - tokens::AVATAR_SM).abs() < 1.0)
        .expect("Ada's mark");
    mark.hover();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("last active"),
        "the hover does not say when: {said}"
    );
}

/// In a direct message the bar carries the other person's presence as a
/// dot before their name; a group's bar has none.
#[test]
fn the_bar_of_a_direct_message_says_whether_they_are_there() {
    let mut state = a_conversation();
    state
        .presence
        .insert(them(), seen(sigil_chat::presence::Seen::Away, NOW - 60));
    let mut h = harness_with(state, true);
    hide_column(&mut h);
    h.run();
    let name = topmost(&h, "Ada");
    let dot = topmost(&h, "away");
    assert!(
        (dot.center().y - name.center().y).abs() < tokens::SPACING_SM && dot.right() <= name.left(),
        "the dot is not before the name on the bar: dot {dot:?}, name {name:?}"
    );

    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let mut h = harness_with(state, true);
    hide_column(&mut h);
    h.run();
    let words = h
        .query_all_by_label("offline")
        .chain(h.query_all_by_label("away"))
        .count();
    assert_eq!(words, 0, "a group's bar carries no presence");
}

// ---------------------------------------------------------------------------
// Verified contacts (SIP-41).
// ---------------------------------------------------------------------------

/// Verify, from the Members view: the dialog shows the six words SIP-41
/// derives for the two keys and the code a camera reads, and "They match"
/// asks the session to keep the mark -- and, with the box ticked, to say so.
#[test]
fn verifying_shows_the_words_for_the_pair_and_they_match_keeps_the_mark() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_at_recording(a_conversation(), sigil_chat::Route::Members, asked.clone());
    h.run();
    h.get_by_label("Verify").click();
    h.run();
    let said = text_of(&h);
    let words = sqex_proto::safety::words_for(&me(), &them());
    for w in words {
        assert!(said.contains(w), "the word {w:?} is not shown: {said}");
    }
    assert!(
        said.contains(&sqex_proto::safety::code(&me(), &them())),
        "the code a camera reads is not on the picture: {said}"
    );
    assert!(
        said.contains(&them().to_string()),
        "their whole key: {said}"
    );
    // Not our own words with somebody else.
    let other = sqex_proto::safety::words_for(&me(), &PubKey::new([9u8; 32]));
    assert!(!said.contains(other[0]) || words.contains(&other[0]));

    h.get_by_label("They match").click();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains(&format!("Verify({:?})", them())), "{sent}");
    assert!(
        !sent.contains("Attest("),
        "nothing is said at the exchange unasked: {sent}"
    );

    // With the box ticked, the claim goes too.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_at_recording(a_conversation(), sigil_chat::Route::Members, asked.clone());
    h.run();
    h.get_by_label("Verify").click();
    h.run();
    h.get_by_label("Say at the exchange that we compared them")
        .click();
    h.run();
    h.get_by_label("They match").click();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains(&format!("Attest({:?})", them())), "{sent}");
}

/// A verified key wears the mark: on the row, on the bar of the direct
/// message, beside the name on their messages, and in Members -- and an
/// unverified one wears none.
#[test]
fn a_verified_key_wears_the_mark_everywhere_and_an_unverified_one_nowhere() {
    let mut state = a_conversation();
    let mut h = harness_with(state.clone(), true);
    h.run();
    assert!(
        h.query_by_label("verified").is_none(),
        "nothing is verified yet: {}",
        text_of(&h)
    );
    state.verified.insert(them(), NOW - 3600);
    let mut h = harness_with(state.clone(), true);
    h.run();
    let marks = h.query_all_by_label("verified").count();
    // The row, the bar, and Ada's bubbles that carry a name.
    assert!(
        marks >= 3,
        "the mark is not everywhere: {marks} of them: {}",
        text_of(&h)
    );
    let row = h
        .query_all_by_label("verified")
        .map(|n| n.rect())
        .filter(|r| r.left() < 300.0)
        .count();
    assert_eq!(row, 1, "one on Ada's row");
    let bar = topmost(&h, "verified");
    let name = topmost(&h, "Ada");
    assert!(
        (bar.center().y - name.center().y).abs() < tokens::SPACING_SM,
        "beside the name on the bar"
    );

    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    assert!(
        h.query_by_label("verified").is_some(),
        "in Members: {}",
        text_of(&h)
    );
    assert!(
        h.query_by_label("Verified").is_some(),
        "and the button says so"
    );
}

/// The verify dialog: the six words, the code, the key.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn verify_dark() {
    let mut h = harness_at(a_conversation(), sigil_chat::Route::Members);
    h.run();
    h.get_by_label("Verify").click();
    h.run();
    h.snapshot("verify_dark");
}

/// A verified contact: the mark on the row, on the bar and on the bubbles.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn verified_dark() {
    let mut state = a_conversation();
    state.verified.insert(them(), NOW - 3600);
    let mut h = harness_with(state, true);
    h.run();
    h.snapshot("verified_dark");
}

/// SIP-43: a conversation that lives at another exchange says so in the
/// bar; one that lives here says nothing.
#[test]
fn a_conversation_that_lives_elsewhere_says_where() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let mut h = harness_at(state.clone(), sigil_chat::Route::Conversations);
    h.run();
    assert!(
        !text_of(&h).contains("lives at"),
        "a conversation at this exchange claimed to live elsewhere"
    );

    state.home = Some((PubKey::new([7; 32]), "origin.example".into()));
    let mut h = harness_at(state.clone(), sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("lives at origin.example"), "{said}");

    // Known by key alone: the key's head, since there is nothing else.
    state.home = Some((PubKey::new([7; 32]), String::new()));
    let mut h = harness_at(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    let key = PubKey::new([7; 32]).to_string();
    assert!(said.contains(&format!("lives at {}", &key[..8])), "{said}");
}

/// A join the exchange refused is said in the directory pane, beside the
/// button that asked. It used to be said only in a conversation's bar,
/// which the directory is not, so a refused join was a button that did
/// nothing.
#[test]
fn a_refused_join_is_said_where_the_button_is() {
    let mut state = a_conversation();
    state.open = None;
    state.found = vec![sigil_chat::Found {
        channel: [9u8; 32],
        instance: [1u8; 32],
        name: "the square".into(),
        topic: String::new(),
        members: 3,
        domain: String::new(),
        here: true,
    }];
    state.searched = true;
    let mut h = harness_at(state.clone(), sigil_chat::Route::Directory);
    h.run();
    assert!(!text_of(&h).contains("cannot be reached"));

    // **The join's own field, not the session's last failure.** This pane
    // used to draw `trouble`, which any failing command sets and nothing
    // clears -- so a call that could not be placed hours earlier was drawn
    // in red over a directory that had just answered. A refused join sets
    // both; only this one reaches the pane.
    state.join_trouble = Some(
        "this conversation lives at another exchange, which cannot be reached right now; \
         nothing was sent"
            .into(),
    );
    let mut h = harness_at(state, sigil_chat::Route::Directory);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("cannot be reached"), "{said}");
}

/// SIP-43: a message the sender posted through another exchange says so
/// beside the time -- *via squic.org* -- and one they did not says nothing.
#[test]
fn a_message_sent_through_a_copy_says_via_where() {
    let mut state = a_conversation();
    let mut h = harness_at(state.clone(), sigil_chat::Route::Conversations);
    h.run();
    assert!(!text_of(&h).contains("via "), "{}", text_of(&h));

    state.lines[1].via = Some("squic.org".into());
    let mut h = harness_at(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("via squic.org"), "{said}");
    // The tree names a label twice (see "edited" beside it); what matters is
    // that no other message acquired one.
    assert!(
        !said.contains("via 2") && !said.contains("via A"),
        "only the one that was: {said}"
    );
}

/// SIP-16 §What a client does with a search row: a room the directory
/// lists from another exchange is not joinable here, and the pane says
/// where it lives and offers to add that exchange instead of Join.
#[test]
fn a_room_listed_from_elsewhere_offers_its_exchange_not_join() {
    let mut state = a_conversation();
    state.open = None;
    state.found = vec![
        sigil_chat::Found {
            channel: [11u8; 32],
            instance: [1u8; 32],
            name: "lounge@trunk.exchange".into(),
            topic: String::new(),
            members: 3,
            domain: "trunk.exchange".into(),
            here: false,
        },
        sigil_chat::Found {
            channel: [10u8; 32],
            instance: [1u8; 32],
            name: "here".into(),
            topic: String::new(),
            members: 1,
            domain: String::new(),
            here: true,
        },
    ];
    state.searched = true;
    let mut h = harness_at(state, sigil_chat::Route::Directory);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("lives at trunk.exchange"), "{said}");
    assert!(said.contains("Add trunk.exchange"), "{said}");
    // One Join, for the one held here.
    assert_eq!(said.matches("Join").count(), 1, "{said}");
}

/// SIP-56 in the members view: an admin can mute a member and sees who is
/// muted; a muted member's button reads Unmute; a member who is not an
/// admin gets neither, and everybody may report.
///
/// **The fixture is a room, and used to be a direct message.** It passed
/// there only because it made the other member a non-admin, which a real
/// direct message cannot: the exchange makes both parties admins on
/// joining, which is what lets either mint an epoch key. Muting is a
/// moderator's act and a direct message has no moderator, so it is not
/// offered in one — see `a_direct_message_offers_no_remove_and_no_demote`.
/// The direct-message wording of the report has its own test.
#[test]
fn an_admin_mutes_from_the_members_view_and_a_member_only_reports() {
    let mut state = a_conversation();
    for c in &mut state.conversations {
        c.peer = None;
    }
    let mut h = harness_at(state.clone(), sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Mute"), "{said}");
    assert!(!said.contains("Unmute"), "{said}");
    assert!(said.contains("Report this room"), "{said}");
    assert!(
        said.contains("No reports."),
        "an admin sees the reports section: {said}"
    );

    state.members[1].muted = true;
    state.reports = vec![sigil_chat::Report {
        id: 7,
        reporter: them(),
        target: 3,
        reason: "spam",
        at: 0,
        note: "links".into(),
    }];
    let mut h = harness_at(state.clone(), sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("muted"), "the roster's word: {said}");
    assert!(said.contains("Unmute"), "{said}");
    // The message is quoted, not numbered: "message 3" is this client's own
    // index into the channel and names nothing a reader has ever seen.
    assert!(
        said.contains("reported Ada's \u{201c}one\u{201d} as spam: links"),
        "{said}"
    );
    assert!(said.contains("Dismiss"), "{said}");

    state.i_am_admin = false;
    state.members[0].admin = false;
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Unmute") && !said.contains("Mute"),
        "not an admin's: {said}"
    );
    assert!(!said.contains("Dismiss"), "{said}");
    assert!(said.contains("Report this room"), "{said}");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_long_members() {
    let mut h = harness_phone(a_long_conversation(), sigil_chat::Route::Members);
    h.run();
    h.run();
    h.snapshot("phone_long_members");
}

/// **Nothing is drawn where a finger cannot get to it.**
///
/// The other half of `no_phone_pane_is_wider_than_the_phone`, and the class
/// of fault it does not see. Three panes drew content below a phone's screen
/// with nothing to scroll: the backup words in Devices at y=1274 on an
/// 804-point screen, the operator console's first operation at y=2392, and
/// the Calls pane's Leave button at y=1344 -- a call you cannot end is a
/// microphone that stays open. Each was found by asking, of one pane,
/// "where is this actually drawn?", which only happens when somebody thinks
/// to ask.
///
/// "Is it on screen" is the wrong question, because a pane taller than the
/// screen is ordinary and right: what matters is whether it can be *reached*.
/// So the check scrolls to the end and then asks. A widget still below the
/// bottom after that is in no scroll area, and there is no way to it.
///
/// Scrolling is a real wheel over the middle of the screen, which is what a
/// finger is: it finds whichever scroll area is actually under the pointer,
/// including none.
#[test]
fn nothing_on_a_phone_is_drawn_where_it_cannot_be_reached() {
    // Enough wheel to reach the end of anything this draws.
    //
    // **Down the screen, not only at its middle.** A wheel only ever turns
    // whatever is under the pointer, and a pane can nest -- a bounded box
    // inside the pane's own scroll area. A sweep at one height leaves every
    // other box untouched, and a widget inside an unscrolled box reports a
    // content position far below the screen, which reads exactly like one
    // that nothing reaches. The console's answer preview is such a box and
    // was reported as unreachable until this swept.
    fn to_the_end(h: &mut Harness<'static>) {
        for y in [100.0f32, 300.0, 500.0, 700.0] {
            for _ in 0..40 {
                h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, y));
                h.event(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::default(),
                });
                // Not `run`: a pane with a spinner in it repaints for ever,
                // and `run` calls that exceeding its step budget.
                h.run_steps(2);
            }
        }
    }

    // The control, and it has to be inside the test: if nothing is ever
    // below the screen to begin with, every route passes without the scroll
    // doing anything, and this says nothing at all.
    let mut was_below = Vec::new();
    let mut unreachable = Vec::new();
    for (what, build) in [
        ("ordinary", a_conversation as fn() -> ChatState),
        ("long", a_long_conversation as fn() -> ChatState),
    ] {
        for route in [
            sigil_chat::Route::Conversations,
            sigil_chat::Route::Directory,
            sigil_chat::Route::Members,
            sigil_chat::Route::Settings,
            sigil_chat::Route::Devices,
            sigil_chat::Route::Search,
            sigil_chat::Route::Me,
            // **The fallback, not the card.** `Route::Call` draws a card
            // only while a call is held, and `CallHandle::for_test` wants a
            // tokio runtime these three do not have. With no call it
            // replaces itself with the conversations route, so what is
            // measured here is that the fallback fits and does not panic.
            // The card's own width is held by `call_card_ui` in sigil-ui and
            // by `tests/call_card.rs`, which has a runtime and a call.
            sigil_chat::Route::Call(me()),
        ] {
            let mut state = build();
            state.reports = vec![sigil_chat::Report {
                id: 7,
                reporter: them(),
                target: 3,
                reason: "spam",
                at: NOW - 3600,
                note: "links".into(),
            }];
            let mut h = harness_phone(state, route.clone());
            h.run();
            h.run();
            let before = deepest(&h);
            if before.1 > PHONE_HEIGHT as f64 + 1.0 {
                was_below.push(format!("{route:?} with {what} names"));
            }
            to_the_end(&mut h);
            let after = deepest(&h);
            if after.1 > PHONE_HEIGHT as f64 + 1.0 {
                unreachable.push(format!(
                    "{route:?} with {what} names: {:?} still ends at y={:.0} on a \
                     {PHONE_HEIGHT}-point screen after scrolling to the end, so \
                     nothing reaches it",
                    after.0, after.1
                ));
            }
        }
    }
    assert!(
        !was_below.is_empty(),
        "no route drew anything below the screen even before scrolling, so \
         the scroll proves nothing and neither does this test"
    );
    assert!(
        unreachable.is_empty(),
        "{} pane(s) draw something no finger can get to:\n  {}",
        unreachable.len(),
        unreachable.join("\n  ")
    );
}

/// The deepest thing drawn, and what it is, in points.
///
/// The accesskit node's own bounding box, for the reason `walk` in
/// `nothing_runs_off_the_edge` gives: `Node::rect` panics on the root.
fn deepest(h: &Harness<'static>) -> (String, f64) {
    fn walk(node: egui_kittest::Node<'_>, ppp: f64, out: &mut Vec<(String, f64)>) {
        let n = node.accesskit_node();
        let name = n
            .label()
            .map(|l| l.to_string())
            .or_else(|| n.value().map(|v| v.to_string()))
            .unwrap_or_else(|| format!("{:?}", n.role()));
        if let Some(b) = n.bounding_box()
            && b.y1 > b.y0
        {
            out.push((name, b.y1 / ppp));
        }
        for c in node.children() {
            walk(c, ppp, out);
        }
    }
    let mut seen = Vec::new();
    walk(h.root(), h.ctx.pixels_per_point() as f64, &mut seen);
    seen.sort_by(|a, b| b.1.total_cmp(&a.1));
    seen.first().cloned().unwrap_or_default()
}

/// **A named view's name is in the bar, and only there.**
///
/// On a phone the shell draws Back and the view's name in the app bar, from
/// `nav_title` -- the same place a conversation's Back and name go. Each of
/// these four views also drew its own Back and its own heading in the pane
/// under it, so the name was on the screen twice, under two back arrows,
/// and a finger's height of an 804-point screen went on saying it again.
///
/// Nothing failed, because this file's harness composed the bar its own way
/// -- head, then the product's name -- and never asked `nav_title` at all.
/// It does now, which is the other half of this change.
#[test]
fn on_a_phone_a_named_view_says_its_name_once() {
    for (route, name) in [
        (sigil_chat::Route::Directory, "Public channels"),
        (sigil_chat::Route::Members, "Members"),
        (sigil_chat::Route::Settings, "Conversation settings"),
        (sigil_chat::Route::Devices, "Devices"),
    ] {
        let mut h = harness_phone(a_conversation(), route.clone());
        h.run();
        h.run();
        let said = h.get_all_by_label(name).count();
        assert_eq!(
            said, 1,
            "{route:?} draws {name:?} {said} times on a phone; the bar has it"
        );
        let backs = h.get_all_by_label("Back").count();
        assert_eq!(
            backs, 1,
            "{route:?} has {backs} back arrows on a phone: the bar draws one"
        );
        // And the bar's, not the pane's: it is above everything the view
        // draws.
        let back = h.get_by_label("Back").rect();
        let heading = h.get_by_label(name).rect();
        // The bar's Back, not a pane's: it is the topmost thing drawn.
        // A Back inside the view would have the bar's own contents above
        // it, which is exactly the shape this replaces.
        let above: Vec<String> = every_box(&h)
            .into_iter()
            .filter(|(_, r)| r.height() > 0.0 && r.bottom() <= back.top() + 1.0)
            .map(|(n, r)| format!("{n:?} at {r:?}"))
            .collect();
        assert!(
            above.is_empty(),
            "{route:?}: {} thing(s) are drawn above Back, so Back is in the \
             pane and not in the bar:\n  {}",
            above.len(),
            above.join("\n  ")
        );
        assert!(
            (back.center().y - heading.center().y).abs() < tokens::SPACING_SM,
            "{route:?}: Back and the name are not on one line: {back:?} / {heading:?}"
        );
    }
}

/// On a desktop the pane keeps its own head, because there is no bar over it.
///
/// The desktop has no app bar to hoist a name into: the view is one of two
/// columns, and taking its heading and its Back away would leave a pane
/// nobody could name or leave.
#[test]
fn on_a_desktop_a_view_keeps_its_own_head() {
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(a_conversation());
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(sigil_chat::Route::Devices);
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.render_nav(&mut app_ctx, ui, &token);
            });
        });
    h.run();
    h.run();
    assert_eq!(
        h.get_all_by_label("Devices").count(),
        1,
        "the desktop's Devices pane lost its own heading"
    );
    assert_eq!(
        h.get_all_by_label("Back").count(),
        1,
        "the desktop's Devices pane lost its own way back"
    );
}

/// Every widget's box and what it is called, for a test that needs to know
/// what is above what.
fn every_box(h: &Harness<'static>) -> Vec<(String, egui::Rect)> {
    fn walk(node: egui_kittest::Node<'_>, ppp: f32, out: &mut Vec<(String, egui::Rect)>) {
        let n = node.accesskit_node();
        let name = n
            .label()
            .map(|l| l.to_string())
            .or_else(|| n.value().map(|v| v.to_string()))
            .unwrap_or_else(|| format!("{:?}", n.role()));
        if let Some(b) = n.bounding_box() {
            out.push((
                name,
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32 / ppp, b.y0 as f32 / ppp),
                    egui::pos2(b.x1 as f32 / ppp, b.y1 as f32 / ppp),
                ),
            ));
        }
        for c in node.children() {
            walk(c, ppp, out);
        }
    }
    let mut seen = Vec::new();
    walk(h.root(), h.ctx.pixels_per_point(), &mut seen);
    seen
}

/// **Every dialog fits a phone's screen, because a dialog cannot scroll.**
///
/// A pane taller than the screen is ordinary: it scrolls, and
/// `nothing_on_a_phone_is_drawn_where_it_cannot_be_reached` checks that it
/// can be reached. A dialog has no such way out. egui 0.36 makes a
/// `ScrollArea` inside a `Modal` dismiss the dialog on any press inside it --
/// tried, with each `auto_shrink` and with a sensing scope, and every one of
/// them closes it -- so what does not fit is simply gone, with the buttons
/// the first thing over the edge.
///
/// Six dialogs, each opened the way its own control opens it, and each asked
/// where its deepest widget ended up. One of them was already checked by
/// hand (`a_dialogs_controls_stay_on_a_phones_screen`, for the Exchange
/// dialog, which is the tall one); the other five were not, and the one that
/// grows next is whichever somebody adds a paragraph to.
/// **A phone reads what attesting does, and never loses the way out.**
///
/// SIP-85's consequence was the one in this app that only a hover could reach,
/// and a phone cannot hover. It was left undrawn because the sentence pushed
/// `Not yet` past the bottom edge of a phone lying down.
///
/// Scrolling would have been the general answer and is not available: in egui
/// 0.36 a `ScrollArea` inside a `Modal` makes every press inside dismiss it
/// (`a_dialog_too_tall_for_the_screen_can_still_be_left` records the attempts;
/// `an_exchange_that_cannot_be_added_says_why` is what catches it). So the
/// sentence is drawn where there is room and withheld where there is not, and
/// both halves are asserted here -- the second is the one that stops this being
/// a fix that trades a tooltip for a dialog nobody can finish.
#[test]
fn a_phone_reads_what_attesting_does() {
    // Upright: the sentence is on the pane.
    let (mut h, app, _) = harness_phone_measured(a_conversation(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "verify", them());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Verify"),
        "the verify dialog did not open, so this says nothing about it: {said}"
    );
    assert!(
        said.contains("A signed statement others may read"),
        "a phone cannot hover, so the consequence of attesting has to be drawn \
         and is not on the pane upright: {said}"
    );

    // Lying down: it is withheld, and the way out is reachable.
    let (mut h, app, _) = harness_phone_measured(a_conversation(), sigil_chat::Route::Members);
    h.set_size(egui::vec2(PHONE_HEIGHT, PHONE_WIDTH));
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "verify", them());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Verify"),
        "the verify dialog did not open lying down, so the rest says nothing: {said}"
    );
    let out = h
        .get_all_by_label("Not yet")
        .map(|n| n.rect().bottom())
        .fold(f32::MIN, f32::max);
    assert!(
        out > f32::MIN,
        "the way out is not drawn at all, so this says nothing about reaching it"
    );
    assert!(
        out <= PHONE_WIDTH,
        "the way out is at {out:.0} of {PHONE_WIDTH} lying down: the dialog \
         cannot scroll, so anything added here has to fit or be withheld"
    );
}

#[test]
fn every_dialog_fits_a_phones_screen() {
    let mut over = Vec::new();
    // **Every dialog, from the enum rather than from a list here.** The six
    // written out on this line were the six that existed when it was, and
    // four have been added since -- the mailbox, both SIP-53 moves and the
    // notifications one -- none of which was ever measured on a phone.
    // `dialogs_for_test` is a match over `Dialog`, so a new one stops this
    // compiling rather than slipping quietly past the loop.
    let dialogs = sigil_chat::ChatApp::dialogs_for_test();
    assert!(!dialogs.is_empty(), "no dialogs to measure");
    for (which, named) in dialogs {
        let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
        h.run();
        app.borrow_mut()
            .open_dialog_for_test((me(), String::new()), which, them());
        h.run();
        h.run();
        // **Both edges.** A `Modal` is centred, so a dialog that outgrows the
        // screen goes off the *top* as much as the bottom: sixty extra lines
        // in the profile dialog put its buttons at y=1290 and its heading at
        // y=-487. A check on the bottom alone would name half of that.
        //
        // These have room, and it is worth saying how much: twenty extra
        // lines still fit -- the first control shown here was that, and it
        // passed because the dialog genuinely fitted, not because the
        // measurement was blind.
        // The instrument has to be looking at the dialog and not at the pane
        // behind it: every one of these says something of its own.
        let said = text_of(&h);
        assert!(
            said.contains(named),
            "the {which} dialog did not open ({named:?} is not on screen), so \
             this says nothing about it: {said}"
        );
        let boxes = every_box(&h);
        let low = boxes
            .iter()
            .filter(|(_, r)| r.height() > 0.0)
            .max_by(|a, b| a.1.bottom().total_cmp(&b.1.bottom()));
        let high = boxes
            .iter()
            .filter(|(_, r)| r.height() > 0.0)
            .min_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        if let Some((name, r)) = low
            && r.bottom() > PHONE_HEIGHT + 1.0
        {
            over.push(format!(
                "the {which} dialog ends at y={:.0} of {PHONE_HEIGHT} ({name:?}), \
                 and a dialog cannot scroll",
                r.bottom()
            ));
        }
        if let Some((name, r)) = high
            && r.top() < -1.0
        {
            over.push(format!(
                "the {which} dialog starts at y={:.0} ({name:?}): a Modal is \
                 centred, so what does not fit goes off the top as well",
                r.top()
            ));
        }
    }
    assert!(
        over.is_empty(),
        "{} dialog(s) run off the bottom of a phone:\n  {}",
        over.len(),
        over.join("\n  ")
    );
}

/// **And sideways.** A phone rotates, and a dialog cannot scroll.
///
/// Held sideways the screen is 360 points tall rather than 804, and five of
/// the six dialogs still fit -- they are short. Verify did not: six words, a
/// QR and a key stacked down the page came to 482 points, so its heading sat
/// 61 points above the top of the screen with no way to reach it. On a short
/// screen it is given a wider box and the code goes beside the words instead
/// of under them, which is what the width is for.
///
/// The pane behind a dialog is not the subject here: it scrolls, and on this
/// screen it is taller than the window either way. So this asks only about
/// what the dialog itself drew, which is everything above the top of the
/// screen -- a `Modal` is centred, so a dialog that does not fit shows there
/// first.
#[test]
fn every_dialog_fits_a_phone_held_sideways() {
    let mut over = Vec::new();
    let dialogs = sigil_chat::ChatApp::dialogs_for_test();
    assert!(!dialogs.is_empty(), "no dialogs to measure");
    for (which, named) in dialogs {
        let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
        // The same phone, turned: 804 across and 360 down.
        h.set_size(egui::vec2(PHONE_HEIGHT, PHONE_WIDTH));
        h.run();
        app.borrow_mut()
            .open_dialog_for_test((me(), String::new()), which, them());
        h.run();
        h.run();
        // **What this is pointed at.** There was no check that the dialog
        // opened at all: `every_box` would have measured the pane behind it
        // and found nothing above the screen, and the test would have passed
        // for a dialog that never drew.
        let said = text_of(&h);
        assert!(
            said.contains(named),
            "the {which} dialog did not open ({named:?} is not on screen), so \
             what is measured below is the pane behind it: {said}"
        );
        let top = every_box(&h)
            .into_iter()
            .filter(|(_, r)| r.height() > 0.0)
            .min_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        if let Some((name, r)) = top
            && r.top() < -1.0
        {
            over.push(format!(
                "the {which} dialog starts at y={:.0} ({name:?}) on a screen \
                 {PHONE_WIDTH} tall",
                r.top()
            ));
        }
    }
    assert!(
        over.is_empty(),
        "{} dialog(s) run off the top of a phone held sideways:\n  {}",
        over.len(),
        over.join("\n  ")
    );
}

/// The verify dialog on a phone held sideways, where it has a second column.
/// A picture, because "it fits" is not the same as "it reads".
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_verify_sideways() {
    let (mut h, app, _) = harness_phone_measured(a_conversation(), sigil_chat::Route::Members);
    h.set_size(egui::vec2(PHONE_HEIGHT, PHONE_WIDTH));
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "verify", them());
    h.run();
    h.run();
    h.remove_cursor();
    h.run();
    h.snapshot("phone_dialog_verify_sideways");
}

/// **SIP-39's ring is drawn like any other, and says where it is from.**
///
/// A call carried here from another exchange has no conversation to ring in,
/// so the incoming-call frame is the one place it can: who, their key in
/// full, and two answers. The key matters more here than anywhere -- a
/// caller from another exchange is one this exchange cannot vouch for -- and
/// the one thing about it that is different is said in words.
#[test]
fn a_call_from_another_exchange_rings_on_screen_with_a_way_to_answer_or_refuse() {
    let mut state = a_conversation();
    state.cross_ring = Some(sigil_chat::CrossRing {
        bridge: [7u8; 16],
        caller: them(),
    });
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("is calling"), "nothing is ringing: {said}");
    assert!(
        said.contains("from another exchange"),
        "the ring does not say where it is from: {said}"
    );
    assert!(
        said.contains(&them().to_string()),
        "the caller's key is not shown in full: {said}"
    );
    for button in ["Answer", "Decline"] {
        assert!(
            h.get_all_by_label(button).next().is_some(),
            "{button} is not offered"
        );
    }
    nothing_runs_off_the_edge(&h, "the cross-exchange ring");
}

/// **SIP-44 on the Devices pane, on a phone.** The section that was a
/// terminal's: a will, guardians, a vouch, the claim. Drawn for an account
/// with guardians lodged and a will just written, it fits the width, every
/// control can be reached, and it says what it has to -- and for a linked
/// device it says why the first two are not offered.
/// **The handover is asked for twice, and says what it costs in between.**
///
/// SIP-44 §The handover changes the key this account *still holds* — not
/// undoable, and the most consequential control in the app. So it follows
/// Destroy's shape: a button, then the consequence, then two answers. A phone
/// cannot hover, so the consequence is drawn.
///
/// What this holds is the guard itself. One press must not hand the account
/// over, Cancel must put it back, and the sentence between them has to name
/// the case nobody thinks of — that anything still holding the old key is a
/// stranger to the account afterwards.
#[test]
fn a_handover_is_asked_for_twice() {
    let mut state = a_conversation();
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        lodged: None,
        will: None,
        vouch: None,
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    h.run();

    // The instrument has to be pointed at the section, or the rest is noise.
    assert!(
        text_of(&h).contains("Change your key now"),
        "the handover section is not on the pane: {}",
        text_of(&h)
    );
    assert!(
        !text_of(&h).contains("cannot be undone"),
        "the consequence is showing before anything was pressed"
    );

    // **Scrolled to it first.** The Devices pane is long and this section is
    // at its foot: a kittest click on a widget that is scrolled out of view
    // does nothing at all, and the assertion below then fails for a reason
    // that has nothing to do with the guard. Measured the hard way.
    for y in [100.0f32, 300.0, 500.0, 700.0] {
        for _ in 0..40 {
            h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, y));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });
            h.run_steps(2);
        }
    }

    // One press asks, it does not act.
    h.get_by_label("Change this account's key").click();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("cannot be undone"),
        "pressing it did not say what it costs: {said}"
    );
    assert!(
        said.contains("is a stranger to this account"),
        "the consequence does not name what happens to anything still holding \
         the old key: {said}"
    );
    assert!(
        h.query_by_label("Yes, change it").is_some(),
        "no way to go on: {said}"
    );

    // And Cancel puts it back.
    h.get_by_label("Cancel").click();
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("cannot be undone"),
        "Cancel left the confirmation up: {}",
        text_of(&h)
    );
    assert!(
        h.query_by_label("Change this account's key").is_some(),
        "Cancel lost the control entirely: {}",
        text_of(&h)
    );
}

#[test]
fn the_succession_section_fits_a_phone_and_says_what_is_arranged() {
    let mut state = a_conversation();
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: None,
        used: 0,
        quota: 0,
        words: None,
    });
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        lodged: Some((
            2,
            vec![them(), PubKey::new([5u8; 32]), PubKey::new([6u8; 32])],
        )),
        will: Some(bs58::encode([7u8; 100]).into_string()),
        vouch: None,
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    nothing_runs_off_the_edge(&h, "the Devices pane with succession");
    // The whole pane, scrolled to the end: the section is at its foot.
    for y in [100.0f32, 300.0, 500.0, 700.0] {
        for _ in 0..40 {
            h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, y));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });
            h.run_steps(2);
        }
    }
    let said = text_of(&h);
    for want in [
        "If you lose your key",
        "any 2 of 3 guardians",
        "Write the will",
        "Add a guardian",
        "Vouch",
        "Take it",
        "Keep the will",
    ] {
        assert!(said.contains(want), "{want:?} is not on the pane: {said}");
    }
    let deepest = deepest(&h);
    assert!(
        deepest.1 <= PHONE_HEIGHT as f64 + 1.0,
        "{:?} still ends at y={:.0} after scrolling to the end",
        deepest.0,
        deepest.1
    );

    // A linked device is told why it cannot write a will, and still offered
    // the two things it can do.
    let mut state = a_conversation();
    state.succession = Some(sigil_chat::Succession {
        is_account: false,
        ..Default::default()
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("one of the account's devices, not the account"),
        "a linked device is not told why: {said}"
    );
    assert!(
        !said.contains("Write the will"),
        "a linked device is offered a will it cannot sign"
    );
    assert!(said.contains("Vouch") && said.contains("Take it"));
}

/// The succession section on a phone: a will just written, guardians
/// lodged. A picture, because a new screen is looked at before it ships.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_succession() {
    let mut state = a_conversation();
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: None,
        used: 0,
        quota: 0,
        words: None,
    });
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        lodged: Some((
            2,
            vec![them(), PubKey::new([5u8; 32]), PubKey::new([6u8; 32])],
        )),
        will: Some(bs58::encode([7u8; 100]).into_string()),
        vouch: None,
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    for _ in 0..40 {
        h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        h.run_steps(2);
    }
    // Back up from the very end, so the section's heading is in frame.
    for _ in 0..2 {
        h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 300.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        h.run_steps(2);
    }
    h.remove_cursor();
    h.run();
    h.snapshot("phone_succession");
}

// ---------------------------------------------------------------------------
// Devices, from the identity rather than from a conversation.
// ---------------------------------------------------------------------------

/// Your devices are reachable with nothing open.
///
/// The Devices pane -- linking a phone, the backup, the will and the
/// guardians of SIP-44 -- was reachable from an open conversation's header
/// and from nowhere else. On a phone the list is the whole screen, so a
/// person who wanted to link a phone first had to open a chat with somebody.
/// Devices are the identity's, and the identity menu is where they are.
#[test]
fn the_identity_menu_reaches_devices_with_no_conversation_open() {
    let mut state = a_conversation();
    state.open = None;
    let routes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_watching_routes(state, routes.clone());
    h.run();
    routes.borrow_mut().clear();

    open_identity(&mut h);
    h.get_by_label("Your devices").click();
    h.run();

    assert!(
        routes.borrow().contains(&sigil_chat::Route::Devices),
        "the menu did not lead to Devices: {:?}",
        routes.borrow()
    );
}

// ---------------------------------------------------------------------------
// A ring on a phone: the handsets beside the name, the key on its own row.
// ---------------------------------------------------------------------------

/// The handsets never lie over the caller's key.
///
/// Seen on the device, with a real call from another exchange: the ring was
/// a row of identicon, a text column carrying the 44-character key, and two
/// named buttons, and on a 360-point pane the buttons were painted over the
/// key and the words under the name. Both kinds of ring, since both drew
/// that row.
#[test]
fn a_rings_handsets_do_not_lie_over_its_key_on_a_phone() {
    let ordinary = {
        let mut state = a_conversation();
        state.open = None;
        state.ringing = vec![sigil_chat::Ring {
            channel: [9u8; 32],
            seq: 7,
            from: them(),
            mine: false,
            secret: [3u8; 32],
            answered: false,
            label: "Ada".into(),
            direct: false,
            peer: None,
        }];
        state
    };
    let cross = {
        let mut state = a_conversation();
        state.open = None;
        state.cross_ring = Some(sigil_chat::CrossRing {
            bridge: [7u8; 16],
            caller: them(),
        });
        state
    };
    for (what, state) in [
        ("an ordinary ring", ordinary),
        ("a ring from another exchange", cross),
    ] {
        let mut h = harness_phone(state, sigil_chat::Route::Conversations);
        h.run();
        h.run();
        let answer = h.get_by_label("Answer").rect();
        let decline = h.get_by_label("Decline").rect();
        let key = h
            .get_all_by_label_contains(&them().to_string())
            .map(|n| n.rect())
            .max_by(|a, b| a.width().total_cmp(&b.width()))
            .expect("the key is drawn");
        for (name, button) in [("Answer", answer), ("Decline", decline)] {
            assert!(
                button.width() > 0.0 && key.width() > 0.0,
                "{what}: a box is empty, so this checks nothing"
            );
            assert!(
                !button.intersects(key),
                "{what}: {name} at {button:?} lies over the key at {key:?}"
            );
            assert!(
                button.right() <= PHONE_WIDTH + 0.5,
                "{what}: {name} at {button:?} runs off a {PHONE_WIDTH}-point pane"
            );
        }
        // And the key is under the handsets, in full: a row of its own.
        assert!(
            key.top() >= answer.bottom() - 1.0,
            "{what}: the key at {key:?} is not under the handsets at {answer:?}"
        );
        nothing_runs_off_the_edge(&h, what);
    }
}

/// A ring from another exchange over the list, on a phone. A picture,
/// because the card is new and the old one was seen broken on the device.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_ring() {
    let mut state = a_conversation();
    state.open = None;
    state.cross_ring = Some(sigil_chat::CrossRing {
        bridge: [7u8; 16],
        caller: them(),
    });
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_ring");
}

/// On a phone the card's key is two even halves, not a line and an orphan.
///
/// Seen on the device: 43 characters on one line and "z" on the next. The
/// halves are one label, so the key is still there whole for anybody who
/// reads the tree; only the break moved.
#[test]
fn a_mentioned_names_card_halves_the_key_on_a_phone() {
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let one = state.lines.iter().position(|l| l.text == "one").unwrap();
    state.lines[one].text = "hi @Ada hi".into();
    state.lines[one].mentions = vec![sigil_chat::session::Mentioned {
        key: them(),
        label: "Ada".into(),
    }];
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let words = h.get_by_label("hi @Ada hi").rect();
    press_at(&mut h, words.center());
    h.run();
    h.run();
    let key = them().to_string();
    let (head, tail) = key.split_at(key.len().div_ceil(2));
    let halved = format!("{head}\n{tail}");
    assert!(
        h.query_by_label(&halved).is_some(),
        "the key is not in two halves: {}",
        text_of(&h)
    );
    // And on a desktop, where it fits, it is one line.
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let one = state.lines.iter().position(|l| l.text == "one").unwrap();
    state.lines[one].text = "hi @Ada hi".into();
    state.lines[one].mentions = vec![sigil_chat::session::Mentioned {
        key: them(),
        label: "Ada".into(),
    }];
    let mut h = harness_with(state, true);
    h.run();
    let words = h.get_by_label("hi @Ada hi").rect();
    h.hover_at(words.center());
    h.run();
    press_at(&mut h, words.center());
    h.run();
    h.run();
    assert!(h.query_by_label(&key).is_some(), "{}", text_of(&h));
    assert!(h.query_by_label(&halved).is_none());
}

/// A linked device's row fits a phone, with Revoke beside the short key
/// and the whole key under it.
///
/// No phone render had ever drawn a linked device: the fixture linked
/// nothing, so the pane said "Nothing else is linked" and the row that
/// carries a 44-character key beside a named button was never measured.
/// It had the ring card's fault.
#[test]
fn a_linked_devices_row_fits_a_phone_with_revoke_off_the_key() {
    let mut state = a_conversation();
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: false,
        },
    ];
    let mut h = harness_phone(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let revoke = h.get_by_label("Revoke").rect();
    let key = h
        .get_all_by_label_contains(&them().to_string())
        .map(|n| n.rect())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .expect("the key is drawn");
    assert!(revoke.width() > 0.0 && key.width() > 0.0, "a box is empty");
    assert!(
        !revoke.intersects(key),
        "Revoke at {revoke:?} lies over the key at {key:?}"
    );
    assert!(
        revoke.right() <= PHONE_WIDTH + 0.5,
        "Revoke at {revoke:?} runs off the pane"
    );
    assert!(
        key.top() >= revoke.bottom() - 1.0,
        "the key is not under the row"
    );
    nothing_runs_off_the_edge(&h, "the Devices pane with a linked device");
}

/// The Devices pane with a second device linked, on a phone. A picture,
/// because no render had ever drawn the row.
/// **A phone reads what giving up a name costs.**
///
/// The word says the act; what it does not say is that the name goes back to
/// the pool and is somebody else's to take, and that nothing of yours goes
/// with it. That was a tooltip, and a phone has no hover.
///
/// Drawn only where there is a name to give up, so the warning is never about
/// a button that is not on the screen — which is the other half of what this
/// holds.
#[test]
fn a_phone_reads_what_giving_up_a_name_costs() {
    let (mut h, app, _) = harness_phone_measured(a_conversation(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "name", them());
    h.run();
    h.run();

    let said = text_of(&h);
    assert!(
        said.contains("Give it up"),
        "the fixture holds no name, so this says nothing about the warning \
         beside giving one up: {said}"
    );
    assert!(
        said.contains("somebody else may take it afterwards"),
        "a phone cannot hover, so what giving up a name costs has to be drawn \
         and is not on the pane: {said}"
    );
}

/// **A phone can tell Revoke from Sign out.**
///
/// They are different acts with different costs: revoking is for a device you
/// have lost and it *keeps every key that device was already given*, so
/// anything it could read has to be rotated; signing out keeps nothing. Both
/// were drawn as a red glyph with the word in a tooltip, and a phone has no
/// hover — so the tint said "this takes something away" and nothing about
/// which one.
///
/// A line-based sweep of `on_hover_text` missed these: rustfmt puts the call
/// on one line and the string on the next, so matching the call's own line
/// matched an empty argument list. Read a window, not a line.
#[test]
fn a_phone_can_tell_revoke_from_sign_out() {
    let mut state = a_conversation();
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: false,
        },
    ];
    let mut h = harness_phone(state, sigil_chat::Route::Devices);
    h.run();
    h.run();

    // **Measured, not read off the tree.** `icon_button_as_named` puts the
    // word in the accessibility tree *and* draws only a glyph — which is the
    // whole defect — so `query_by_label` finds "Revoke" either way and an
    // assertion on the tree passes with nothing drawn. Caught by reverting
    // the fix and watching this test stay green.
    //
    // A glyph button is square; a button with a word in it is wider than it
    // is tall. That is the difference a phone can see.
    for word in ["Sign out", "Revoke"] {
        let r = h
            .get_all_by_label(word)
            .map(|n| n.rect())
            .max_by(|a, b| a.width().total_cmp(&b.width()))
            .unwrap_or_else(|| panic!("no control for {word:?}: {}", text_of(&h)));
        assert!(
            r.width() > r.height() * 1.5,
            "{word:?} is drawn {:.0}x{:.0}, which is a glyph and not a word — \
             a phone cannot hover to find out which act it is",
            r.width(),
            r.height()
        );
    }
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_devices_linked() {
    let mut state = a_conversation();
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: false,
        },
    ];
    let mut h = harness_phone(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    h.snapshot("phone_devices_linked");
}

/// Scroll a phone pane to its foot, where the succession section is.
fn scroll_to_the_foot(h: &mut Harness<'static>) {
    for _ in 0..40 {
        h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        h.run_steps(2);
    }
}

/// Type a key into the field on the Add-a-guardian row -- the text input
/// nearest it -- and press Add.
fn stage_a_guardian(h: &mut Harness<'static>, key: &PubKey) {
    let add = h.get_by_label("Add a guardian").rect();
    let field = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .min_by(|a, b| {
            (a.rect().center().y - add.center().y)
                .abs()
                .total_cmp(&(b.rect().center().y - add.center().y).abs())
        })
        .expect("the guardian field");
    field.focus();
    field.type_text(&key.to_string());
    h.run();
    h.get_by_label("Add a guardian").click();
    h.run();
    h.run();
}

/// A guardian added but not yet lodged is a mark, the key's stem and a
/// Remove, and none of it runs off a phone's edge.
///
/// **It was the whole key, wrapped**, which is what this test was named for.
/// The lodged list two sections up had gained a mark and a stem in the SIP-44
/// audit, and the staged list -- the same people one press earlier -- was
/// still a column of base58 that had to wrap to fit. The comment on the
/// lodged list claimed it was "the only place in sigil where somebody appears
/// without a mark" while this stood forty lines below it.
#[test]
fn a_staged_guardian_is_a_mark_and_a_stem() {
    let mut state = a_conversation();
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        ..Default::default()
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    scroll_to_the_foot(&mut h);
    stage_a_guardian(&mut h, &them());
    let said = text_of(&h);
    assert!(
        h.query_by_label("Remove").is_some(),
        "the guardian was not added: {said}"
    );
    assert!(
        said.contains(&short_form(&them())),
        "the stem is what is drawn: {said}"
    );
    nothing_runs_off_the_edge(&h, "the Devices pane with a pending guardian");
}

/// **Guardians staged, before they are lodged.** Two, so the marks are there
/// to compare. `phone_succession` renders the lodged side only, so this list
/// -- which the audit changed -- had no picture at all; three changes have
/// shipped that way already.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_guardians_staged() {
    let mut state = a_conversation();
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        ..Default::default()
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    scroll_to_the_foot(&mut h);
    stage_a_guardian(&mut h, &them());
    stage_a_guardian(&mut h, &PubKey::new([5u8; 32]));
    scroll_to_the_foot(&mut h);
    h.remove_cursor();
    // Not `run`: the guardian field still holds focus and its caret blinks,
    // so the context asks for a repaint for ever and `run`'s four steps are
    // never enough. `phone_succession` gets away with `run` because nothing
    // on that pass was focused.
    h.run_steps(2);
    h.snapshot("phone_guardians_staged");
}

// ---------------------------------------------------------------------------
// SIP-44: the other party's account has moved.
// ---------------------------------------------------------------------------

/// A direct message asks the registry once whether the other party was
/// succeeded, and when they were, says so and offers the new key.
///
/// A transcript says "X's account is now Y" only where the move was written
/// into a channel both are in; a contact who moved while nothing was said
/// read as merely silent. The registry knows, and `/account/succession` was
/// the one SIP-44 route nothing in the interface asked.
#[test]
fn a_direct_message_asks_once_whether_the_other_party_moved_and_says_where() {
    // Asked once, on opening, and not on every pass.
    // The fixture's open conversation is the direct message with Ada.
    let state = a_conversation();
    let peer = state
        .conversations
        .iter()
        .find(|c| Some(c.channel) == state.open)
        .and_then(|c| c.peer)
        .expect("the fixture's open conversation is a direct message");
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(state.clone(), asked.clone());
    h.run();
    h.run();
    h.run();
    let wanted = format!("SuccessionOf({peer:?})");
    let times = asked.borrow().iter().filter(|a| **a == wanted).count();
    assert_eq!(times, 1, "asked {times} times: {:?}", asked.borrow());
    assert!(
        !text_of(&h).contains("account is now"),
        "nothing is said before the registry answers"
    );

    // Not succeeded: nothing said.
    let mut not = state.clone();
    not.succeeded.insert(peer, None);
    let mut h = harness_with(not, true);
    h.run();
    assert!(!text_of(&h).contains("account is now"), "{}", text_of(&h));

    // Succeeded: said, and the new key offered.
    let successor = PubKey::new([0x44u8; 32]);
    let mut moved = state.clone();
    moved.succeeded.insert(peer, Some(successor));
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands(moved, asked.clone());
    h.run();
    let said = text_of(&h);
    assert!(said.contains("account is now"), "{said}");
    h.get_by_label("Write to them there").click();
    h.run();
    let all = asked.borrow().join(" | ");
    assert!(
        all.contains(&format!("OpenDm({successor:?})")),
        "the offer did not open a conversation with the successor: {all}"
    );
}

/// A direct message whose other party has moved, on a phone. A picture,
/// because the banner is new.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_moved() {
    let mut state = a_conversation();
    state
        .succeeded
        .insert(them(), Some(PubKey::new([0x44u8; 32])));
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_moved");
}

/// The ring over the list and the Devices pane, on a phone in the light
/// theme -- reachable now that the phone's own light or dark reaches sigil.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_ring_light() {
    let mut state = a_conversation();
    state.open = None;
    state.cross_ring = Some(sigil_chat::CrossRing {
        bridge: [7u8; 16],
        caller: them(),
    });
    let mut h = harness_phone_light(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_ring_light");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_devices_light() {
    let mut state = a_conversation();
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: false,
        },
    ];
    let mut h = harness_phone_light(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    h.snapshot("phone_devices_light");
}

// ---------------------------------------------------------------------------
// Back to the newest message.
// ---------------------------------------------------------------------------

/// A transcript scrolled up offers the way back, and pressing it takes it.
///
/// The transcript sticks to the bottom while the reader is at it and stays
/// put once they have scrolled -- which left a phone dragging a whole
/// history back to reach what was just said. Not offered at the foot, where
/// it would do nothing.
#[test]
fn a_scrolled_transcript_offers_the_way_back_to_the_newest() {
    let mut h = harness_phone(a_page(50, 120), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    // A transcript opens at its foot, where there is nothing to go back to.
    assert!(
        h.query_by_label_contains("Go to the latest").is_none(),
        "offered at the foot, where it does nothing: {}",
        text_of(&h)
    );

    // Up a few screens, as a finger does, with the pointer in the margin:
    // a message under it reveals its action strip, the strip is a
    // foreground layer, and a wheel over that layer is the strip's -- so
    // the middle of the pane scrolls nothing as soon as a layout change
    // puts a bubble under the pointer.
    h.hover_at(egui::pos2(26.0, 300.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 240.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    let up = h
        .query_by_label_contains("Go to the latest")
        .expect("the way back is offered once the reader has scrolled");
    // Over the foot of the transcript, not over the composer -- a control
    // over the box takes a press meant for it. The attach button is the
    // composer's row; a hint is not in the tree to ask about.
    let spot = up.rect();
    let composer = h.get_by_label_contains("Attach a file").rect();
    assert!(
        spot.bottom() <= composer.top() + 1.0,
        "the control at {spot:?} is over the composer at {composer:?}"
    );

    up.click();
    h.run();
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("Go to the latest").is_none(),
        "pressing it did not go back to the newest: {}",
        text_of(&h)
    );
}

/// And it says how many arrived while the reader was away.
#[test]
fn the_way_back_counts_what_arrived() {
    let mut state = a_page(50, 120);
    if let Some(open) = state.open
        && let Some(c) = state.conversations.iter_mut().find(|c| c.channel == open)
    {
        c.unread = 3;
    }
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, 300.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 240.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    let said = text_of(&h);
    assert!(
        said.contains("Go to the latest — 3 new"),
        "the control does not say what is waiting: {said}"
    );
}

/// The way back, over a scrolled transcript on a phone. A picture, because
/// the control is new and it floats over what it is about.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_way_back() {
    let mut h = harness_phone(a_page(50, 120), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.hover_at(egui::pos2(PHONE_WIDTH / 2.0, 300.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 240.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    h.remove_cursor();
    h.run();
    h.snapshot("phone_way_back");
}

/// The menu a long press opens, on a phone. A picture, because it was six
/// named buttons of six widths until it was looked at on the device.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_message_menu() {
    let mut state = a_conversation();
    // Somebody else's message: the menu that carries the most -- a reply,
    // their key, a direct message, the safety words, a report.
    state.open = Some([8u8; 32]);
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "the second one, then");
    finger_down(&mut h, bubble.center());
    h.run_steps(20);
    h.remove_cursor();
    h.run();
    h.snapshot("phone_message_menu");
}

/// A message's menu opens inside what the system leaves.
///
/// A popup is its own layer and no panel's inset reaches it, so a menu
/// opened near the foot of a phone grew straight down into the navigation
/// bar -- seen on the device, with "Report…" under the system's own row,
/// where the touch is the system's and the item cannot be pressed.
#[test]
fn a_messages_menu_stays_out_of_the_systems_own_row() {
    const BOTTOM: f32 = 48.0;
    let mut state = a_conversation();
    state.open = Some([8u8; 32]);
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    // As the phone reports it: a gesture bar at the foot.
    sigil::Insets::install(
        &h.ctx,
        sigil::Insets {
            bottom: BOTTOM,
            ..sigil::Insets::NONE
        },
    );
    h.run();
    h.run();
    // The lowest message there is, so the menu opens at the foot.
    let bubble = h
        .get_all_by_label_contains("the second one, then")
        .map(|n| n.rect())
        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
        .expect("a message at the foot");
    finger_down(&mut h, bubble.center());
    h.run_steps(20);
    h.run();
    let last = h.get_by_label_contains("Report").rect();
    assert!(
        last.bottom() <= PHONE_HEIGHT - BOTTOM + 1.0,
        "the menu's last row ends at {:.0}, inside the system's own {BOTTOM} points",
        last.bottom()
    );
}

/// **A finger on a quick reaction reacts.**
///
/// Seen on the phone: tapping a message reveals the strip, and tapping an
/// emoji on it put the strip away and did nothing else. The existing test
/// pressed the cell through the accessibility tree, which is a click
/// delivered straight to the widget -- it never went near the press and
/// release a finger makes, which is where this goes wrong.
#[test]
fn a_finger_on_a_quick_reaction_sends_it() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands_phone(a_conversation(), asked.clone());
    h.run();
    h.run();
    // A tap on the message reveals the strip, as a finger does.
    let bubble = topmost(&h, "mine, on the other side");
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();
    let quick = h
        .query_by_label(sigil_emoji::QUICK[0])
        .expect("the strip is revealed by a tap")
        .rect();

    // And a tap on the emoji sends it.
    finger_down(&mut h, quick.center());
    h.run();
    finger_up(&mut h, quick.center());
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("React"),
        "a finger on the quick reaction sent nothing: {sent}"
    );
}

/// **A message is on screen the moment it is sent.**
///
/// Sending hands the words to the session, which posts them and only then
/// publishes a transcript with them in it. On a slow link that is a round
/// trip during which the message is nowhere -- a press that did nothing.
/// What is in flight is drawn until the real one lands.
#[test]
fn a_sent_message_is_on_screen_before_the_exchange_answers() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands_phone(a_conversation(), asked.clone());
    h.run();
    h.run();
    assert!(!text_of(&h).contains("half a second later"));

    let field = h
        .get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("the composer");
    field.focus();
    field.type_text("half a second later");
    h.run();
    // The dart, which is what the phone's slot becomes with something to
    // send: no exchange answers here, so nothing will ever come back.
    h.get_by_label("Send").click();
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("half a second later"),
        "the message is nowhere until the exchange answers: {}",
        text_of(&h)
    );
    let sent = asked.borrow().join(" | ");
    assert!(sent.contains("Post"), "and it was sent: {sent}");
}

/// **The quick reactions are the size of a finger, and the strip fits.**
///
/// Nine cells across a 360-point pane are 28 points each with two points
/// between them: pressed on the phone, two presses in three landed on the
/// cell beside the one aimed at or between them, which is indistinguishable
/// from a reaction that does nothing. The pane is the budget, so the strip
/// spends it on fewer, larger cells -- Reply moved into the More menu -- and
/// this reads both halves of that: how big a cell came out, and that the row
/// still ends inside the pane.
#[test]
fn a_phones_quick_reactions_are_a_fingers_size_and_the_strip_fits_the_pane() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();

    let first = h
        .query_by_label(sigil_emoji::QUICK[0])
        .expect("a tap reveals the strip")
        .rect();
    let last = h
        .query_by_label(sigil_emoji::QUICK[sigil_emoji::QUICK.len() - 1])
        .expect("the last quick reaction")
        .rect();
    // A finger is about 9mm; 40 points on this phone is 7.5mm, which is as
    // much as a row of seven plus its own margins can be given. The number
    // asserted is the floor: below it the cells are the pointer's again.
    assert!(
        first.width() >= 36.0,
        "a quick reaction is {:.0} points across, which is a pointer's cell",
        first.width()
    );
    let strip = first
        .union(last)
        .union(h.get_by_label("More emoji").rect())
        .union(h.get_by_label("More").rect());
    assert!(
        strip.left() >= -1.0 && strip.right() <= PHONE_PANE + 1.0,
        "the strip runs from {:.0} to {:.0} on a {PHONE_PANE}-point pane",
        strip.left(),
        strip.right()
    );
}

/// And what the width bought is still reachable: Reply is the first row of
/// the More menu on a phone. Without this the fix above is a feature
/// removed rather than a control moved.
#[test]
fn a_phones_strip_offers_reply_inside_more() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label("Reply").is_none(),
        "Reply is still a cell of its own on the phone's strip"
    );
    h.get_by_label("More").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Reply").is_some(),
        "Reply is nowhere: {}",
        text_of(&h)
    );
}

/// **Back at the list of chats offers the identities.**
///
/// A phone's Back walked back through the views and then stopped: at the
/// list, with nothing open, it did nothing at all, which reads as a dead
/// key. The screen before the list is the one the identity was chosen on,
/// so that is where it goes -- and it is the only way back to it without
/// opening a conversation first.
#[test]
fn back_at_the_list_of_chats_asks_for_the_identities() {
    let mut state = a_conversation();
    state.open = None;
    let (mut h, asks) = harness_phone_asks(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        asks.borrow().is_empty(),
        "something was asked for before Back was pressed: {:?}",
        asks.borrow()
    );

    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    assert_eq!(
        asks.borrow().clone(),
        vec![sigil::app::AppAction::ChooseIdentity],
        "Back at the list asked for something else, or for nothing"
    );
}

/// With a conversation open Back still closes that and stays: it is one
/// step, and a key that skipped the conversation would drop somebody out of
/// what they were reading. The negative control for the test above -- same
/// key, same harness, one thing different about where it is pressed.
#[test]
fn back_in_a_conversation_does_not_leave_for_the_identities() {
    // `a_conversation` has one open, which is the difference.
    let (mut h, asks) = harness_phone_asks(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.key_press(egui::Key::BrowserBack);
    h.run();
    h.run();
    assert!(
        asks.borrow().is_empty(),
        "Back left the conversation for the opening screen: {:?}",
        asks.borrow()
    );
}

/// **The chat list's heading is the word, a magnifier and three dots.**
///
/// Four controls and a heading on a 360-point row left the word squeezed
/// and the search box a row of its own below it, on a screen whose whole
/// job is the list under them. The magnifier opens the box; everything
/// done *from* the list is behind the dots.
#[test]
fn the_chat_lists_heading_is_a_magnifier_and_a_menu() {
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        h.query_by_label("New conversation").is_none()
            && h.query_by_label("Public channel").is_none(),
        "the heading still carries its own compose and directory buttons"
    );
    // The box is not there until it is asked for.
    assert!(
        h.query_by_label("Search").is_none(),
        "the search box is drawn before the magnifier was pressed"
    );

    h.get_by_label("More choices").click();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Write to somebody") && said.contains("Public channels"),
        "the burger holds neither: {said}"
    );
}

/// And the magnifier goes to the search card, which is where the box is:
/// a search is something one goes off to do, and the list is not pushed
/// down by a row it carries about in case.
#[test]
fn the_magnifier_goes_to_the_search_card() {
    let mut state = a_conversation();
    state.open = None;
    let (mut h, routes) = harness_phone_routes(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Search").is_none(),
        "the box is on the list card: {}",
        text_of(&h)
    );
    h.get_by_label("Find a chat").click();
    h.run();
    h.run();
    assert_eq!(
        routes.borrow().clone(),
        vec![sigil_chat::Route::Search],
        "the magnifier went somewhere else, or nowhere"
    );
}

/// The card has the box, and it has the finger: a search card that opens
/// without the keyboard is a card you have to press twice.
#[test]
fn the_search_card_opens_with_the_box_ready() {
    let mut state = a_conversation();
    state.open = None;
    let (mut h, _) = harness_phone_routes(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.get_by_label("Find a chat").click();
    h.run();
    // The card, as the shell draws it after the push.
    let mut state = a_conversation();
    state.open = None;
    let mut card = harness_phone(state, sigil_chat::Route::Search);
    card.run();
    card.run();
    assert!(
        card.get_all_by_role(egui::accesskit::Role::TextInput)
            .count()
            == 1,
        "the card has no box, or more than one: {}",
        text_of(&card)
    );
}

/// **A press on a reaction says who sent it; it does not take it back.**
///
/// A chip is an emoji and a number, and the question everybody has of it is
/// who -- which nothing answered, while a finger that brushed one silently
/// undid one's own reaction. So it opens the list of who reacted with what,
/// and taking one's own back is a row in that list, said in words.
#[test]
fn a_press_on_a_reaction_names_who_sent_it_and_leaves_it_alone() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands_phone(a_conversation(), asked.clone());
    h.run();
    h.run();
    let chip = h
        .get_all(egui_kittest::kittest::by().label_contains("\u{1f44d}"))
        .map(|n| n.rect())
        .next()
        .expect("the reaction on the fixture's own message");
    finger_down(&mut h, chip.center());
    h.run();
    finger_up(&mut h, chip.center());
    h.run();
    h.run();

    let said = text_of(&h);
    assert!(
        said.contains("You, Ada"),
        "the press did not name who reacted: {said}"
    );
    let sent = asked.borrow().join(" | ");
    assert!(
        !sent.contains("React"),
        "the press took the reaction back: {sent}"
    );

    // The row is where it is taken back, now that the chip is a question.
    h.get_by_label_contains("You, Ada").click();
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("React"),
        "the row in the list sent nothing: {sent}"
    );
}

/// **The one you already sent is held down, and pressing it takes it back.**
///
/// The strip toggles: the same emoji again is a retraction. That is only
/// discoverable if the cell you already pressed looks unlike the five you
/// did not -- otherwise the way to take a reaction back is a thing you have
/// to be told. The fixture's own message carries a 👍 of ours.
#[test]
fn the_quick_reaction_already_sent_is_held_down_and_takes_itself_back() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_recording_commands_phone(a_conversation(), asked.clone());
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();

    // 👍 is the fixture's own reaction; the heart beside it is nobody's.
    let ours = h.get_by_label("\u{1f44d}");
    assert_eq!(
        ours.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "the reaction we have already sent is drawn like the ones we have not"
    );
    let theirs = h.get_by_label(sigil_emoji::QUICK[0]);
    assert_eq!(
        theirs.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False),
        "a reaction nobody sent is drawn as sent"
    );

    let at = ours.rect().center();
    finger_down(&mut h, at);
    h.run();
    finger_up(&mut h, at);
    h.run();
    h.run();
    let sent = asked.borrow().join(" | ");
    assert!(
        sent.contains("React"),
        "pressing the one already sent did not take it back: {sent}"
    );
}

/// A ring, for a test that only needs one to be happening.
fn a_ringing(open: bool) -> ChatState {
    let mut state = a_conversation();
    if !open {
        state.open = None;
    }
    state.ringing = vec![sigil_chat::Ring {
        channel: [9u8; 32],
        seq: 7,
        from: them(),
        mine: false,
        secret: [3u8; 32],
        answered: false,
        label: "Ada".into(),
        direct: false,
        peer: None,
    }];
    state
}

/// **A phone rings from the bottom of the screen.**
///
/// The card was drawn at the top of whatever was on screen, which on a
/// phone is the far end of the hand holding it: Answer and Decline were a
/// stretch away, over the thing being read. At the bottom they are under
/// the thumb, where a phone puts the two buttons of a call.
#[test]
fn a_phone_rings_from_the_bottom_of_the_screen() {
    for (what, state) in [
        ("with the list on screen", a_ringing(false)),
        ("in a conversation", a_ringing(true)),
    ] {
        let mut h = harness_phone(state, sigil_chat::Route::Conversations);
        h.run();
        h.run();
        let answer = h.get_by_label("Answer").rect();
        assert!(
            answer.center().y > PHONE_HEIGHT / 2.0,
            "{what}: Answer is at {:.0}, in the top half of a {PHONE_HEIGHT}-point screen",
            answer.center().y
        );
    }
}

/// A wide pane keeps it at the top, where a window's banners belong: the
/// negative control, and the reason the branch exists at all.
#[test]
fn a_window_still_rings_from_the_top() {
    let mut h = harness_with(a_ringing(true), true);
    h.run();
    h.run();
    let answer = h.get_by_label("Answer").rect();
    assert!(
        answer.center().y < 620.0 / 2.0,
        "the ring moved to the bottom of the window too: {:.0}",
        answer.center().y
    );
}

/// And the ring at the foot does not stop the transcript scrolling: the
/// card is as tall as a card, and everything below the pointer moved when
/// it appeared.
#[test]
fn a_ring_at_the_bottom_leaves_the_transcript_scrolling() {
    let mut state = a_page(50, 120);
    state.ringing = a_ringing(true).ringing;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("Go to the latest").is_none(),
        "a transcript opens at its foot"
    );
    // In the margin, not on a bubble: a message under the pointer reveals
    // its strip, and the strip is a foreground layer that takes the wheel.
    h.hover_at(egui::pos2(26.0, 300.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 240.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    assert!(
        h.query_by_label_contains("Go to the latest").is_some(),
        "the transcript did not scroll while a call was ringing"
    );
}

/// What is wrong with a conversation is said at the same end as the ring:
/// beside the box, not above the first message anybody has to scroll back
/// up to see.
#[test]
fn a_phone_says_what_is_wrong_beside_the_box() {
    let mut state = a_conversation();
    state.trouble_with.no_key = Some(4);
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let said = h
        .get_by_label_contains("You hold no key for this conversation")
        .rect();
    let box_row = h.get_by_label_contains("Attach a file").rect();
    assert!(
        said.center().y > PHONE_HEIGHT / 2.0,
        "the trouble is in the top half at {:.0}",
        said.center().y
    );
    assert!(
        said.bottom() <= box_row.top() + 1.0,
        "the trouble at {said:?} is over the box at {box_row:?}"
    );
}

/// The card opens on an empty box, and an empty box has not failed to find
/// anything: it says what a search here reaches, and says "Nothing here
/// matched." only once something has been looked for.
#[test]
fn an_empty_search_card_has_not_failed_to_find_anything() {
    let mut state = a_search();
    state.open = None;
    state.hits = Vec::new();
    let mut h = harness_phone(state, sigil_chat::Route::Search);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Nothing here matched"),
        "an empty box reports a failed search: {said}"
    );
    assert!(
        said.contains("Searches what this client has opened"),
        "it does not say what it can reach: {said}"
    );

    search_for(&mut h, "nothing like this");
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Nothing here matched"),
        "a search that found nothing says nothing else: {}",
        text_of(&h)
    );
}

/// **The identity menu's keys can be taken from it with a finger.**
///
/// Your own key is drawn in full and selectable, which is a pointer's
/// answer: on a phone nothing selects text, so the one thing anybody wants
/// to do with their key -- send it to somebody -- could not be done from
/// the only place that shows it. And the exchange's key, forty-four
/// characters of it, wrapped across the menu saying no more than its two
/// ends do.
#[test]
fn the_identity_menu_offers_its_keys_to_a_finger() {
    // The menu is the same one on either form, and the chevron that opens
    // it is the wide pane's; what is asserted here is what the menu holds.
    let mut h = harness_with(a_conversation(), true);
    h.run();
    h.run();
    open_identity(&mut h);
    assert!(
        h.query_by_label("Copy your key").is_some(),
        "no way to take your own key: {}",
        text_of(&h)
    );
    let said = text_of(&h);
    assert!(
        said.contains(&me().to_string()),
        "your own key is not in the menu in full: {said}"
    );
}

/// **The exchange's key is not one of yours**, and is not in this menu.
///
/// It sat under your own, which put a key you cannot act on beside the one
/// key here that is about you. It is copied where the exchange is chosen
/// -- see `the_exchange_control_offers_the_exchange_key`.
#[test]
fn the_identity_menu_does_not_carry_the_exchange_key() {
    let mut state = a_conversation();
    state.exchange = Some(them());
    let mut h = harness_with(state, true);
    h.run();
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    assert!(
        h.query_by_label("Copy your key").is_some(),
        "your own key went with it: {said}"
    );
    assert!(
        h.query_by_label("Copy the exchange's key").is_none(),
        "the exchange's key is still here: {said}"
    );
    assert!(
        !said.contains(&them().to_string()),
        "and drawn in full: {said}"
    );
}

/// **Picking a reaction puts the strip away.**
///
/// It was left up over the message it belongs to -- covering the mark it
/// had just made -- and the next tap on the transcript went to the strip
/// instead of where it was aimed.
#[test]
fn picking_a_quick_reaction_puts_the_strip_away() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let bubble = topmost(&h, "mine, on the other side");
    finger_down(&mut h, bubble.center());
    h.run();
    finger_up(&mut h, bubble.center());
    h.run();
    h.run();
    let quick = h
        .query_by_label(sigil_emoji::QUICK[0])
        .expect("a tap reveals the strip")
        .rect();

    finger_down(&mut h, quick.center());
    h.run();
    finger_up(&mut h, quick.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label("More").is_none(),
        "the strip is still up after a reaction was picked: {}",
        text_of(&h)
    );
}

/// **The viewer hands a picture on and saves it.**
///
/// Everything you can do with a picture was in the message's More menu,
/// behind a long press on a bubble whose picture is the whole of it -- so
/// the surface that shows the picture as large as the screen could only
/// close. Forwarding leaves the viewer for the list of where to send it,
/// which is drawn under the composer behind it.
#[test]
fn the_viewer_hands_a_picture_on() {
    let mut h = harness_with(with_pictures(3), true);
    h.run();
    h.run();
    let tile = h.get_by_label("[image 1, 4 KiB]").rect();
    press_at(&mut h, tile.center());
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("2 of 3"),
        "the tile did not open the viewer: {}",
        text_of(&h)
    );
    assert!(
        h.query_by_label("Save…").is_some(),
        "the viewer cannot save: {}",
        text_of(&h)
    );
    h.get_by_label("Forward it").click();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Forward to"),
        "forwarding did not offer anywhere to send it: {said}"
    );
}

/// **A tap on the picture leaves the viewer**, on a phone, where the
/// viewer is the whole screen and the tap that opened it is the gesture to
/// hand. A pointer still zooms with a click, which is what a pointer has
/// instead of pinching.
#[test]
fn a_tap_on_a_picture_leaves_the_phones_viewer() {
    let mut h = phone_pictures(1);
    h.run();
    h.run();
    let tile = h.get_by_label_contains("[image 0").rect();
    finger_down(&mut h, tile.center());
    h.run();
    finger_up(&mut h, tile.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label("Save…").is_some(),
        "the tile did not open the viewer: {}",
        text_of(&h)
    );

    let middle = egui::pos2(PHONE_WIDTH / 2.0, PHONE_HEIGHT / 2.0);
    finger_down(&mut h, middle);
    h.run();
    finger_up(&mut h, middle);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Save…").is_none(),
        "a tap on the picture did not leave the viewer: {}",
        text_of(&h)
    );
}

/// **A clip opens on the whole screen too**, with the same two things
/// beside the way out: hand it on, or keep it.
///
/// The phone's viewer used to be the window's dialog -- margins, rounded
/// corners and a small video in the middle of a 360-point screen -- and
/// the only way to save a clip was the message's More menu, behind a long
/// press on a bubble the clip fills.
#[test]
fn a_clip_opens_on_the_whole_screen_with_its_own_controls() {
    let mut state = with_pictures(1);
    let last = state.lines.len() - 1;
    state.lines[last].attachments[0].kind = sigil_ui::attachment::VIDEO;
    state.lines[last].attachments[0].described = "[video 2s, 1.2 MiB]".into();
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let tile = h.get_by_label_contains("[video 2s").rect();
    finger_down(&mut h, tile.center());
    h.run();
    finger_up(&mut h, tile.center());
    h.run();
    h.run();
    assert!(
        h.query_by_label("Save…").is_some() && h.query_by_label("Forward it").is_some(),
        "the clip's viewer offers neither: {}",
        text_of(&h)
    );
    h.get_by_label("Close").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Save…").is_none(),
        "the viewer would not close: {}",
        text_of(&h)
    );
}

/// **A file on its way is on screen while it goes.**
///
/// A message with nothing but a file in it had no echo at all: between the
/// press and the exchange's answer the transcript showed nothing whatever,
/// and on a phone's uplink with a twenty-megabyte clip that is a minute of
/// a press that did nothing.
#[test]
fn a_file_on_its_way_is_drawn_while_it_goes() {
    let (mut h, app) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let dir = tempfile::tempdir().expect("a directory");
    let file = dir.path().join("holiday.png");
    std::fs::write(&file, a_png()).expect("write it");
    app.borrow_mut()
        .stage_for_test(me(), "", vec![file.clone()]);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("holiday.png"),
        "the staged file is not in the composer: {}",
        text_of(&h)
    );

    h.get_by_label("Send").click();
    // Steps, not runs: what is in flight draws a spinner, and `run` waits
    // for a pass that asks for nothing.
    h.run_steps(4);
    // In the transcript, above the box -- not the staged row inside the
    // composer, which is what it looked like before the echo carried
    // files and is the thing this has to tell apart.
    let composer = h.get_by_label("Attach a file").rect();
    let above: Vec<egui::Rect> = h
        .get_all_by_label_contains("holiday.png")
        .map(|n| n.rect())
        .filter(|r| r.bottom() < composer.top())
        .collect();
    assert!(
        !above.is_empty(),
        "a file in flight is nowhere in the transcript: {}",
        text_of(&h)
    );
}

/// **A chain the exchange does not agree with is said in the conversation.**
///
/// SIP-43 §The heads by position: the exchange keeps what each device wrote
/// by position, and a client can ask. Two things writing under one device's
/// key -- a store rolled back by a restore, a copied file, the key on a
/// second machine -- otherwise shows up as nothing at all until somebody's
/// message is refused.
#[test]
fn a_chain_the_exchange_disagrees_with_is_said() {
    let mut state = a_conversation();
    state.trouble_with.chain_apart = true;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("does not match this device's own"),
        "nothing said about a chain the exchange disagrees with: {said}"
    );
}

/// And an ordinary conversation says nothing of the sort. The negative
/// control: a line drawn unconditionally would pass the test above.
#[test]
fn an_ordinary_conversation_says_nothing_about_chains() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("does not match this device's own"),
        "said of a conversation with nothing wrong"
    );
}

/// **A backup that keeps itself up to date says so**, where the generation
/// and the date are: it is a thing happening on somebody's behalf, and a
/// backup nobody is told about is a backup nobody can decide about.
#[test]
fn the_devices_card_says_the_backup_keeps_itself_up_to_date() {
    let mut h = harness_at(a_backup(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Written again by itself once a day"),
        "nothing said about the backup keeping itself: {}",
        text_of(&h)
    );
}

/// And with no key there is nothing being kept, so nothing is claimed. The
/// negative control: a line drawn unconditionally would pass the test above
/// and promise a backup to an account that has none.
fn a_backup_with_no_key() -> ChatState {
    let mut state = a_backup();
    state.backup = Some(sigil_chat::Backup {
        has_key: false,
        held: None,
        used: 0,
        quota: 1_048_576,
        words: None,
    });
    state
}

#[test]
fn with_no_backup_key_nothing_is_claimed_about_keeping_one() {
    let mut h = harness_at(a_backup_with_no_key(), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Written again by itself"),
        "promised a backup to an account with no key: {}",
        text_of(&h)
    );
}

/// **The directory arrives full.**
///
/// An empty box means "everything", which the hint says and nobody reads --
/// so the card opened on a sentence telling somebody to search a list it
/// could have shown them. It asks once, on arriving; a search of their own
/// replaces it.
#[test]
fn the_directory_asks_for_everything_on_arriving() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_at_recording(
        a_conversation(),
        sigil_chat::Route::Directory,
        asked.clone(),
    );
    h.run();
    h.run();
    let said = asked.borrow().join(" | ");
    assert!(
        said.contains("Find("),
        "the directory asked for nothing: {said}"
    );
}

/// And the conversation list does not: it has the conversations already,
/// and a directory search from there is a request nobody made. The negative
/// control for the test above.
#[test]
fn the_conversation_list_asks_the_directory_for_nothing() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = harness_at_recording(
        a_conversation(),
        sigil_chat::Route::Conversations,
        asked.clone(),
    );
    h.run();
    h.run();
    let said = asked.borrow().join(" | ");
    assert!(!said.contains("Find("), "it searched the directory: {said}");
}

/// SIP-18: a voice note carries its waveform and its length in the message
/// itself, "so it draws before any audio is fetched" — and here nothing is
/// fetched: `bytes` is `None` and the row still says how long it runs.
///
/// The bars themselves are counted in sigil-ui's own tests, off the shapes
/// the row paints. What this one is about is the *carriage*: that a length
/// the sender put in the meta survives the trip from the store, through the
/// session's state, to the bubble.
fn a_voice_note_saying(duration_ms: Option<u64>) -> String {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::VOICE,
        described: "[voice note 12s]".into(),
        size: 40_000,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: false,
        duration_ms,
        shape: None,
        waveform: std::sync::Arc::from(vec![0u8, 40, 120, 200, 255].into_boxed_slice()),
        id: "voice123".into(),
    }];
    let mut h = harness_with(state, true);
    h.run();
    text_of(&h)
}

#[test]
fn a_voice_note_says_its_length_before_any_audio_is_fetched() {
    let said = a_voice_note_saying(Some(12_000));
    assert!(said.contains("0:12"), "the length is on the row: {said}");
    // Drawn as a clock, read as a sentence: the description stays in the
    // tree for anything that reads rather than looks.
    assert!(said.contains("[voice note 12s]"), "{said}");
}

/// The negative control. A sender may send no length, and then the row has
/// nothing to count down — so it says the one thing it does know. A row that
/// printed "0:12" from anywhere but the meta would pass the test above and
/// fail this one.
#[test]
fn a_voice_note_with_no_length_says_how_big_it_is_instead() {
    let said = a_voice_note_saying(None);
    assert!(!said.contains("0:12"), "a length came from nowhere: {said}");
    assert!(said.contains("39 KiB"), "it says what it does know: {said}");
}

/// What a voice note looks like on a phone.
///
/// A picture, because the only question left about it is one no assertion
/// answers: whether a row of bars beside a clock reads as a voice note at
/// the width a phone's bubble actually has.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn voice_note_phone() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    // A shape with something in it: syllables. A smooth envelope draws a
    // hill and a hill is not what speech looks like -- what makes a voice
    // note recognisable at a glance is that it is lumpy.
    let levels: Vec<u8> = (0..48)
        .map(|i| {
            let t = i as f32 / 47.0;
            let envelope = (t * std::f32::consts::PI).sin().max(0.0);
            // A cheap deterministic wobble, so the picture is the same
            // every time it is taken.
            let wobble = ((i * 37 % 11) as f32 / 10.0) * 0.55 + 0.45;
            let loud = (envelope * wobble).clamp(0.05, 1.0);
            (255.0 - loud * 245.0) as u8
        })
        .collect();
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::VOICE,
        described: "[voice note 12s]".into(),
        size: 40_000,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: false,
        duration_ms: Some(12_000),
        shape: None,
        waveform: std::sync::Arc::from(levels.into_boxed_slice()),
        id: "voice123".into(),
    }];
    let (mut h, _app) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    h.remove_cursor();
    h.snapshot("voice_note_phone");
}

/// What a note *about to be sent* looks like, which is the picture the
/// assertions cannot take: the first draw laid the tile out vertically and
/// the clock overflowed the box by a line, landing on the background under
/// it. Nothing was off screen and nothing was unreachable, so only a
/// rendering says whether the two rows are inside the tile they belong to.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn composer_note_phone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("voice-2026-09-23-14-02-11.ogg");
    std::fs::write(&path, three_seconds_of_note()).unwrap();
    let (mut h, app) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    app.borrow_mut().stage_for_test(me(), "", vec![path]);
    // Decoded on a thread; the picture waits for it rather than catching
    // the tile halfway.
    for _ in 0..100 {
        h.run();
        if h.query_by_label("Voice note 0:03").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    h.remove_cursor();
    h.snapshot("composer_note_phone");
}

// ---------------------------------------------------------------------
// The Settings card: what is behind your own mark on a phone.
// ---------------------------------------------------------------------

/// The other things sigil does, as the shell publishes them.
fn three_apps() -> Vec<sigil::Sibling> {
    vec![
        sigil::Sibling {
            id: sigil::AppId(0),
            title: "Chat".into(),
            icon: sigil_ui::Icon::Compose,
            badge: 3,
            active: true,
        },
        sigil::Sibling {
            id: sigil::AppId(1),
            title: "Exchange".into(),
            icon: sigil_ui::Icon::Settings,
            badge: 0,
            active: false,
        },
        sigil::Sibling {
            id: sigil::AppId(2),
            title: "Phone".into(),
            icon: sigil_ui::Icon::Device,
            badge: 0,
            active: false,
        },
    ]
}

fn me_card(state: ChatState, siblings: Vec<sigil::Sibling>) -> (Harness<'static>, Routes) {
    let routes = Routes::default();
    let (h, _, _) = harness_phone_beside(
        state,
        sigil_chat::Route::Me,
        egui::Theme::Dark,
        Asks::default(),
        routes.clone(),
        siblings,
    );
    (h, routes)
}

/// A press on your own mark goes to the card, and not to a popup hanging
/// off it.
#[test]
fn your_mark_opens_the_settings_card_on_a_phone() {
    // The list, with nothing open: a phone's bar is the conversation's when
    // one is, and your mark is not on it.
    let mut state = a_conversation();
    state.open = None;
    let (mut h, routes) = harness_phone_routes(state, sigil_chat::Route::Conversations);
    h.run();
    assert!(
        routes.borrow().is_empty(),
        "something asked to go somewhere before anything was pressed"
    );
    h.get_by_label("Your identity").click();
    h.run();
    assert_eq!(
        routes.borrow().as_slice(),
        [sigil_chat::Route::Me],
        "the mark did not open the card"
    );
}

/// The head: the mark, the name and where you are reachable, and each of
/// the three is a control.
#[test]
fn the_card_puts_your_name_and_your_domain_at_its_head() {
    let mut state = a_conversation();
    state.mine.name = Some("Ada".into());
    state.mine.handle = Some("ada@squic.org".into());
    let (mut h, _) = me_card(state, three_apps());
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Ada"), "your name: {said}");
    assert!(said.contains("ada@squic.org"), "where you are: {said}");
    // The mark is a button, and what it does is what it says -- which is
    // also what the row further down says, so this counts rather than
    // fetching the one node.
    assert!(
        said.contains("Edit your profile"),
        "the mark is a control: {said}"
    );
}

/// With no name claimed the second line is the domain alone — the domain
/// the handle *would* have had, from the same field the handle is composed
/// from, so the two cannot disagree.
#[test]
fn with_no_name_claimed_the_card_says_the_domain_by_itself() {
    let mut state = a_conversation();
    state.mine.handle = None;
    state.domain = Some("squic.org".into());
    let (mut h, _) = me_card(state, three_apps());
    h.run();
    let said = text_of(&h);
    assert!(said.contains("@squic.org"), "the domain: {said}");
}

/// Keys are shown short. A whole base58 key is 41 to 44 characters and
/// nobody reads one: what anybody does with their own is send it, which is
/// the button beside it.
#[test]
fn the_card_shows_keys_short_with_a_way_to_copy_them() {
    let (mut h, _) = me_card(a_conversation(), three_apps());
    h.run();
    let said = text_of(&h);
    let whole = me().to_string();
    assert!(
        said.contains(&sigil_ui::short(&whole)),
        "the short form is on the card: {said}"
    );
    assert!(
        !said.contains(&whole),
        "the whole key was drawn where the short form belongs: {said}"
    );
    h.get_by_label("Copy your key");
    // And only yours: the exchange's key is copied where the exchange is
    // chosen, not from the card about you.
    assert!(
        h.query_by_label("Copy the exchange's key").is_none(),
        "{said}"
    );
}

/// The rail, for a screen with no room for one: the other things sigil
/// does, with the one you are on marked and its badge said.
#[test]
fn the_card_carries_the_way_to_the_other_apps() {
    let (mut h, _) = me_card(a_conversation(), three_apps());
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Chat (3)"), "the badge is said: {said}");
    assert!(said.contains("Exchange"), "{said}");
    assert!(said.contains("Phone"), "{said}");
}

/// The negative control for the row above: one app on its own has nowhere
/// else to go, and the card draws no way to nowhere. Without this, a test
/// that found "Exchange" anywhere on the card would pass on the word in
/// "Copy the exchange…".
#[test]
fn a_card_with_no_other_apps_draws_no_way_to_them() {
    let (mut h, _) = me_card(a_conversation(), Vec::new());
    h.run();
    let said = text_of(&h);
    assert!(!said.contains("Chat (3)"), "{said}");
    assert!(
        !said.contains("Phone"),
        "a way to an app that is not there: {said}"
    );
}

/// Everything that used to be behind the mark is still reachable, on a card
/// that has a name in the bar and a Back that means something.
#[test]
fn the_card_keeps_everything_the_menu_had() {
    let (mut h, _) = me_card(a_conversation(), three_apps());
    h.run();
    let said = text_of(&h);
    for row in [
        "Edit your profile",
        "Your devices",
        "Add an exchange",
        "Switch identity",
    ] {
        assert!(said.contains(row), "{row} is not on the card: {said}");
    }
}

/// Pressing Your devices from the card goes to Devices, the same as the
/// menu did.
#[test]
fn the_card_reaches_your_devices() {
    let (mut h, routes) = me_card(a_conversation(), three_apps());
    h.run();
    h.get_by_label("Your devices").click();
    h.run();
    assert_eq!(routes.borrow().as_slice(), [sigil_chat::Route::Devices]);
}

/// What the card looks like.
///
/// A picture because the head is the only part of sigil that is *centred*,
/// and nothing but looking at it says whether a mark, a name and a domain
/// stacked in the middle of a phone read as one thing or as three.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn me_card_phone() {
    let mut state = a_conversation();
    state.mine.name = Some("Ada Lovelace".into());
    state.mine.handle = Some("ada@squic.org".into());
    // A peer whose key sorts **above** this account's, so one direct message
    // is ordered here and the line that says so has a picture. The fixture's
    // own peer sorts below, so without this the count is zero and the row
    // draws nothing -- UI that no snapshot shows.
    if let Some(first) = state.conversations.first().cloned() {
        state.conversations.push(Summary {
            peer: Some(PubKey::new([0xffu8; 32])),
            label: "Zoë".into(),
            channel: [9u8; 32],
            ..first
        });
    }
    let (mut h, _) = me_card(state, three_apps());
    h.run();
    h.remove_cursor();
    h.snapshot("me_card_phone");
}

/// Giving a name up cannot be undone — it goes back to the pool and somebody
/// else may take it — and the line that shows it sits a thumb's width under
/// your own name. A press on it must reach the dialog, never the release.
#[test]
fn pressing_your_handle_does_not_give_the_name_up() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut state = a_conversation();
    state.mine.handle = Some("ada@squic.org".into());
    let mut h = me_card_commands(state, asked.clone());
    h.run();
    h.get_by_label("ada@squic.org").click();
    h.run();
    h.run();
    let said = format!("{:?}", asked.borrow());
    assert!(
        !said.contains("ReleaseName"),
        "one press gave the name up: {said}"
    );
    // It reached the dialog instead, which is where both claiming another
    // and giving this one up are deliberate.
    let on_screen = text_of(&h);
    assert!(on_screen.contains("Your name here"), "{on_screen}");
    assert!(on_screen.contains("Give it up"), "{on_screen}");
}

/// The negative control: the release still exists, and the dialog's own
/// button does it. Without this the test above would pass on a client that
/// had simply lost the ability to give a name up at all.
#[test]
fn the_dialog_can_still_give_the_name_up() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut state = a_conversation();
    state.mine.handle = Some("ada@squic.org".into());
    let mut h = me_card_commands(state, asked.clone());
    h.run();
    h.get_by_label("ada@squic.org").click();
    h.run();
    h.get_by_label("Give it up").click();
    h.run();
    h.run();
    let said = asked.borrow().clone();
    // The local part, without the domain: a release names it, and the
    // exchange it is released at is the one being talked to.
    assert!(said.iter().any(|c| c == "ReleaseName(\"ada\")"), "{said:?}");
}

/// A voice note is not fetched for being scrolled past — SIP-18 puts the
/// shape and the length in the message so it draws without the audio. The
/// press is what asks for it.
#[test]
fn pressing_play_on_a_voice_note_asks_the_exchange_for_it() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].attachments = vec![Attached {
        kind: sigil_ui::attachment::VOICE,
        described: "[voice note 12s]".into(),
        size: 40_000,
        preview: sigil_ui::attachment::no_preview().clone(),
        bytes: None,
        missing: false,
        held: false,
        duration_ms: Some(12_000),
        shape: None,
        waveform: std::sync::Arc::from(vec![0u8, 40, 120, 200, 255].into_boxed_slice()),
        id: "voice123".into(),
    }];
    let mut h = harness_recording_commands_phone(state, asked.clone());
    h.run();
    // Nothing asked for while it sits there: the waveform came free.
    assert!(
        !format!("{:?}", asked.borrow()).contains("Fetch"),
        "a note was fetched for being on screen: {:?}",
        asked.borrow()
    );
    // **Tapped as a finger taps it**, not clicked: the bubble's tap
    // behaviour is reached only once egui has seen a touch, and under a
    // plain pointer hover reveals the strip instead. A case that clicked
    // would exercise the desktop path and say nothing about the phone.
    let play = h.get_by_label("Play").rect().center();
    finger_down(&mut h, play);
    h.run();
    finger_up(&mut h, play);
    // `run_steps` after the press: the row turns into a spinner while the
    // fetch is out, and a spinner asks for the next frame forever.
    h.run_steps(3);
    // **Settled.** The strip is only drawn for a bubble that held still for
    // a frame, and pressing play changes the row (a spinner where the button
    // was), so a case that looked immediately after the press could pass
    // because the layout had moved rather than because the strip stayed
    // away. Several more passes with nothing changing.
    for _ in 0..4 {
        h.run_steps(2);
    }
    let said = format!("{:?}", asked.borrow());
    assert!(
        said.contains("Fetch"),
        "the press asked for nothing: {said}"
    );

    // **And the press was the button's, not the bubble's.**
    //
    // The strip is revealed by a tap inside the bubble, and a voice note's
    // Play button is inside the bubble -- so pressing play put the emoji
    // strip up, which is what somebody pressing play saw happen instead of
    // a note playing. Reported from the phone.
    let labels = labels(&h);
    assert!(
        !labels.iter().any(|l| l == "More emoji"),
        "pressing Play opened the reaction strip: {labels:?}"
    );
}

/// **Tapping the bubble itself still opens the strip.** The other half of
/// the case above: the fix asks egui whether a widget took the press, and a
/// fix that answered "yes" for every tap would leave a phone with no way to
/// react at all.
#[test]
fn tapping_a_bubble_still_opens_the_reaction_strip() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "something to react to".into();
    let mut h = harness_recording_commands_phone(state, asked.clone());
    h.run();
    let words = h.get_by_label("something to react to").rect().center();
    finger_down(&mut h, words);
    h.run();
    finger_up(&mut h, words);
    h.run_steps(3);
    let labels = labels(&h);
    assert!(
        labels.iter().any(|l| l == "More emoji"),
        "a tap on the words opened nothing: {labels:?}"
    );
}

/// The microphone is on the composer, beside the box.
#[test]
fn the_composer_offers_to_record_a_voice_note() {
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("Record a voice note");
}

/// It is not offered once there is something to send: the row is about to
/// commit words, and a microphone beside the dart is a second thing to
/// press by mistake.
///
/// The negative control for the test above as well: a client that drew the
/// microphone unconditionally would pass that one for the wrong reason.
#[test]
fn the_microphone_goes_away_once_there_is_something_to_send() {
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("Record a voice note");
    // Typed in, not set: what is being tested is what the row does with
    // something in the box, and the box is how something gets there.
    let field = composer(&h);
    field.focus();
    field.type_text("something");
    h.run();
    assert!(
        h.query_by_label("Record a voice note").is_none(),
        "the microphone stayed beside the dart"
    );
}

/// Pressing it turns the composer into the recording: throw it away, or
/// send it. Nothing to type into, because there is nothing to type.
#[test]
fn recording_replaces_the_composer_with_its_own_row() {
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("Record a voice note").click();
    h.run_steps(3);
    let said = text_of(&h);
    assert!(said.contains("Throw it away"), "{said}");
    assert!(
        said.contains("Send"),
        "there was no way to send what was recorded: {said}"
    );
    // Nothing to type into while talking: the box is gone, not disabled.
    assert!(
        h.query(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput"))
        )
        .is_none(),
        "the box was still there to type into: {said}"
    );
}

/// Thrown away, it is gone: the composer comes back and nothing was
/// staged. A recording somebody discarded must not turn up in the next
/// message.
#[test]
fn a_recording_thrown_away_leaves_nothing_behind() {
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("Record a voice note").click();
    h.run_steps(3);
    h.get_by_label("Throw it away").click();
    h.run_steps(3);
    let said = text_of(&h);
    assert!(!said.contains("Throw it away"), "{said}");
    // The box is back.
    composer(&h);
    // Nothing staged: no tile, and the microphone is on offer again.
    h.get_by_label("Record a voice note");
}

// ---------------------------------------------------------------------------
// A staged voice note draws as a note.
// ---------------------------------------------------------------------------

/// Three seconds of tone, encoded the way the recorder encodes what it
/// captures, so the tile is fed a real note and not a stub.
fn three_seconds_of_note() -> Vec<u8> {
    let samples: Vec<f32> = (0..sigil_video::note::RATE as usize * 3)
        .map(|i| {
            let t = i as f32 / sigil_video::note::RATE as f32;
            (t * 440.0 * std::f32::consts::TAU).sin() * 0.5
        })
        .collect();
    sigil_video::note::encode(&samples).expect("a note encodes")
}

/// **A note in the composer is a waveform and a length, not a filename.**
/// The recorder has to call the file something, and what it calls it is a
/// stamp -- `voice-2026-09-23-14-02.ogg` -- which is what the tile showed:
/// ten characters of that, truncated, where the thing just recorded should
/// be. The bars are measured at staging anyway, because they have to
/// travel with the message; they were being measured and thrown away.
#[test]
fn a_staged_voice_note_shows_its_waveform_and_its_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("voice-2026-09-23-14-02-11.ogg");
    std::fs::write(&path, three_seconds_of_note()).unwrap();

    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(the_room());
    app.stage_for_test(me(), "", vec![path]);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    // Decoded on a thread of its own; give it a moment to land.
    for _ in 0..100 {
        h.run();
        if h.query_by_label("Voice note 0:03").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let said = text_of(&h);
    assert!(
        h.query_by_label("Voice note 0:03").is_some(),
        "the tile did not say what it holds: {said}"
    );
    assert!(
        h.query_by_label("Remove Voice note 0:03").is_some(),
        "and the way out named a file: {said}"
    );
    assert!(
        !said.contains("voice-2026"),
        "the stamp the recorder had to write is on screen: {said}"
    );
    // The length is drawn beside the bars, not only spoken to the tree.
    assert!(said.contains("0:03"), "{said}");
}

// ---------------------------------------------------------------------------
// SIP-60: earlier copies of the same conversation.
// ---------------------------------------------------------------------------

/// One message of an earlier copy: whole, with no receipt, because the
/// channel it belongs to does not exist any more.
fn an_earlier_line(seq: u64, said: &str) -> Line {
    Line {
        seq,
        who: them(),
        name: Some("Ada".into()),
        mine: false,
        at: NOW - 7 * DAY,
        text: said.into(),
        redacted: false,
        edited: false,
        said: None,
        via: None,
        reactions: Vec::new(),
        reply_to: None,
        receipt: None,
        attachments: Vec::new(),
        standing: Default::default(),
        mentions: Vec::new(),
        me_mentioned: false,
        earlier: true,
    }
}

/// **An earlier copy is drawn, above, behind a separator.**
///
/// SIP-60 §The client keeps what it read: a direct message opened twice, or
/// one folded because it turned out to be a stray, leaves what this client
/// read of the earlier incarnation. sigil held it on the disc and drew none
/// of it -- it read `Chat::earlier` nowhere -- so the reader was told their
/// conversation had been destroyed while every word of it sat unread in
/// their own store.
#[test]
fn an_earlier_copy_is_drawn_above_the_conversation() {
    let mut state = a_conversation();
    state.copies = vec![vec![
        an_earlier_line(1, "from the copy that was folded"),
        an_earlier_line(2, "and the second thing said in it"),
    ]];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("from the copy that was folded"),
        "the earlier copy was not drawn: {said}"
    );
    assert!(said.contains("An earlier copy"), "and unlabelled: {said}");
    // And the boundary is said in both directions, or the live conversation
    // reads as a continuation of a channel that no longer exists.
    assert!(said.contains("This conversation"), "{said}");
    // Above, not merged: the copy's words come before the live ones.
    let copy = said.find("from the copy that was folded").expect("drawn");
    let live = said.find("mine, on the other side").expect("drawn");
    assert!(copy < live, "the copy was drawn below the conversation");
}

/// With one copy it is "an earlier copy"; with more than one they are
/// numbered, because "an earlier copy" twice on one screen says nothing
/// about which came first.
#[test]
fn several_copies_are_numbered_oldest_first() {
    let mut state = a_conversation();
    state.copies = vec![
        vec![an_earlier_line(1, "the oldest thing")],
        vec![an_earlier_line(1, "the middle thing")],
    ];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("An earlier copy (1 of 2)"), "{said}");
    assert!(said.contains("An earlier copy (2 of 2)"), "{said}");
    let first = said.find("the oldest thing").expect("drawn");
    let second = said.find("the middle thing").expect("drawn");
    assert!(first < second, "oldest first: {said}");
}

/// **Nothing may be done to a message in an earlier copy.** Its channel was
/// destroyed, so a Reply or a React offered on it is a button aimed at
/// nowhere. The live conversation's own messages still offer both, which is
/// what says the absence is about the copy and not about the harness.
#[test]
fn an_earlier_message_offers_nothing_to_do_to_it() {
    let mut state = a_conversation();
    // **Short enough that both are on the screen.** The transcript opens at
    // the bottom, and a hover over a row scrolled out of view exercises
    // nothing at all while reading exactly like a hover that did.
    state.lines.truncate(1);
    state.lines[0].text = "the one live message".into();
    state.events.clear();
    state.divider = None;
    state.copies = vec![vec![an_earlier_line(1, "nothing to be done about this")]];
    // A desktop, where Reply is a cell of the strip itself rather than a
    // row inside the More menu a phone puts it in.
    let mut h = harness_with(state, true);
    h.run();
    hide_column(&mut h);
    h.run();
    // The strip comes up under the pointer, and only once the message has
    // held still for a frame -- which is why each of these runs twice.
    h.get_by_label_contains("the one live message").hover();
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Reply"),
        "the live message offered nothing either, so this proves nothing: {}",
        text_of(&h)
    );
    h.get_by_label_contains("nothing to be done about this")
        .hover();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Reply"),
        "an earlier copy offered a reply: {said}"
    );
}

/// The restart notice tells the truth about which restart this was.
///
/// A channel destroyed and made again under the same name leaves nothing;
/// one folded leaves what was read, and it is on the screen above. Saying
/// "nothing above is related to what follows" over somebody's own
/// conversation is the worse of the two errors.
#[test]
fn a_restart_that_kept_a_copy_does_not_claim_the_conversation_was_destroyed() {
    let mut bare = a_conversation();
    bare.trouble_with.restarted = true;
    let (mut h, _) = harness_phone_with(bare, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("destroyed and started again"),
        "with nothing kept, the old words are right: {said}"
    );

    let mut kept = a_conversation();
    kept.trouble_with.restarted = true;
    kept.copies = vec![vec![an_earlier_line(1, "what was kept")]];
    let (mut h, _) = harness_phone_with(kept, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("destroyed"),
        "told the conversation was destroyed with it on the screen: {said}"
    );
    assert!(said.contains("kept as an earlier copy"), "{said}");
}

/// What an earlier copy looks like on a phone.
///
/// A picture, because the question the assertions cannot answer is whether
/// the boundary reads as one: above it is a conversation that no longer
/// exists, below it the one that does, and a reader who takes the two for
/// one conversation has been misled by the drawing and not by the words.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_earlier_copy() {
    let mut state = a_conversation();
    state.trouble_with.restarted = true;
    // **Short enough that the boundary is in the picture.** The transcript
    // opens at its foot, and the first rendering of this showed the live
    // conversation and none of the copy it is about.
    state.lines.truncate(1);
    state.events.clear();
    state.divider = None;
    state.copies = vec![vec![an_earlier_line(1, "said in the copy that was folded")]];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    h.remove_cursor();
    h.snapshot("phone_earlier_copy");
}

// ---------------------------------------------------------------------------
// SIP-53 §Posting again.
// ---------------------------------------------------------------------------

/// **A stranded post is offered, and only offered.**
///
/// "Each stranded post is offered to the person, in the order it was first
/// posted, and sent again only on their say: it is a new entry, and the
/// person may have said it since, or no longer mean it." sigil offered
/// nothing: a move took your own messages out of the conversation and the
/// only sign was that they were no longer there.
#[test]
fn a_stranded_post_is_offered_with_both_answers() {
    let mut state = a_conversation();
    state.stranded = vec![sigil_chat::Stranded {
        seq: 41,
        posted: NOW - DAY,
        text: "the thing the fork took".into(),
        files: 0,
    }];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("the thing the fork took"),
        "nothing said what was lost: {said}"
    );
    // Both answers, because a question with one answer is an instruction.
    h.get_by_label("Send again");
    h.get_by_label("Let it go");
}

/// One at a time, oldest first, and the rest counted.
#[test]
fn the_oldest_stranded_post_is_the_one_offered() {
    let mut state = a_conversation();
    state.stranded = vec![
        sigil_chat::Stranded {
            seq: 41,
            posted: NOW - 2 * DAY,
            text: "the older one".into(),
            files: 0,
        },
        sigil_chat::Stranded {
            seq: 42,
            posted: NOW - DAY,
            text: "the newer one".into(),
            files: 0,
        },
    ];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("the older one"), "{said}");
    assert!(
        !said.contains("the newer one"),
        "both were offered at once: {said}"
    );
    assert!(
        said.contains("2 of yours"),
        "and the rest uncounted: {said}"
    );
}

/// A stranded post with no words is still a thing somebody sent.
#[test]
fn a_stranded_post_of_files_alone_says_so() {
    let mut state = a_conversation();
    state.stranded = vec![sigil_chat::Stranded {
        seq: 41,
        posted: NOW - DAY,
        text: String::new(),
        files: 2,
    }];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("2 files"), "{said}");
}

/// **A message posted again reads at the time it was first said**, and says
/// that it was posted again.
///
/// SIP-53: "A reader that understands it shows the message at `said` and
/// marks it as posted again." Without the mark it is a message stamped a
/// week ago sitting between two from this morning, explaining nothing.
#[test]
fn a_post_sent_again_reads_at_the_time_it_was_first_said() {
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].text = "said before the fork, posted after it".into();
    state.lines[n - 1].at = NOW;
    state.lines[n - 1].said = Some(NOW - 3 * DAY - 1_500);
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("posted again"), "unmarked: {said}");
    // The clock is the first time, not the landing time.
    let first = sigil_ui::clock(NOW - 3 * DAY - 1_500);
    assert!(
        said.contains(&first),
        "the clock is not when it was first said ({first}): {said}"
    );
}

/// What the offer looks like on a phone, above the box.
///
/// A picture, because what the assertions cannot answer is whether a
/// question about something somebody said a week ago reads as a question
/// and not as an error — and whether the two answers are reachable with a
/// thumb at the width a phone actually has.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_stranded_post() {
    let mut state = a_conversation();
    state.stranded = vec![
        sigil_chat::Stranded {
            seq: 41,
            posted: NOW - DAY - 4200,
            text: "the one the fork took, which was a fairly long thing to say".into(),
            files: 0,
        },
        sigil_chat::Stranded {
            seq: 42,
            posted: NOW - DAY,
            text: "and another".into(),
            files: 0,
        },
    ];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    h.remove_cursor();
    h.snapshot("phone_stranded_post");
}

// ---------------------------------------------------------------------------
// A menu is as wide as a menu, not as wide as the window.
// ---------------------------------------------------------------------------

/// **A menu of two short phrases spanned the phone edge to edge.**
///
/// A menu's rows are `icon_item`s, and an icon row takes the width it is
/// given so that the whole row is the hit target — which is right inside a
/// card and absurd inside a popup, where the width it is given is the
/// window's. Nothing was off screen and nothing was unreachable, so the
/// existing phone-layout tests all passed.
#[test]
fn a_menu_is_no_wider_than_a_menu() {
    // The list, not a conversation: the menu hangs off the list's heading.
    let mut state = a_conversation();
    state.open = None;
    state.lines.clear();
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("More choices").click();
    h.run_steps(3);
    let row = h.get_by_label("Write to somebody").rect();
    assert!(
        row.width() <= sigil::tokens::MENU_MAX + 1.0,
        "the menu is {} wide on a {PHONE_WIDTH}-point phone",
        row.width()
    );
    // And still wide enough to be a target rather than the width of a word.
    assert!(
        row.width() >= sigil::tokens::MENU_MIN - 1.0,
        "{}",
        row.width()
    );
}

/// **Delete is offered only to somebody who may delete.**
///
/// SIP-19: a `Redact` "MUST be accepted only from the account of `target`,
/// or from an account the channel lists as an admin", and the exchange
/// enforces the same.
///
/// **This test used to say "in a direct message, where nobody is an
/// admin".** That is false: the exchange makes a direct message's parties
/// admins when they join — it is what lets either of them mint an epoch
/// key — and the members view shows the badge on both rows. So the admin
/// case below was asserting that one party may delete the other's
/// messages, which the exchange would have carried out. A moderation power
/// belongs to a room with a moderator in it; the last case keeps it there.
#[test]
fn delete_is_offered_on_ones_own_message_and_not_on_anybody_elses() {
    let strip_of = |text: &str, admin: bool, dm: bool| -> String {
        let mut state = a_conversation();
        state.i_am_admin = admin;
        if !dm {
            // A room is a conversation with no peer: the open one's
            // identifier stops standing for two accounts.
            for c in &mut state.conversations {
                c.peer = None;
            }
        }
        state.lines.truncate(1);
        state.lines[0].text = "theirs, in a direct message".into();
        state.lines[0].mine = false;
        state.lines[0].who = them();
        let mut mine = state.lines[0].clone();
        mine.seq = 2;
        mine.mine = true;
        mine.who = me();
        mine.text = "mine, in a direct message".into();
        state.lines.push(mine);
        state.events.clear();
        state.divider = None;
        let mut h = harness_with(state, true);
        h.run();
        hide_column(&mut h);
        h.run();
        h.get_by_label_contains(text).hover();
        h.run();
        h.run();
        // Reply is on the strip itself; Delete is inside the More menu.
        h.get_by_label("More").click();
        h.run_steps(3);
        text_of(&h)
    };

    let own = strip_of("mine, in a direct message", false, true);
    assert!(
        own.contains("Delete"),
        "one's own message offered no Delete, so this proves nothing: {own}"
    );
    let theirs = strip_of("theirs, in a direct message", false, true);
    assert!(
        !theirs.contains("Delete"),
        "Delete was offered on somebody else's message with no admin to back it: {theirs}"
    );
    // **Being an admin of a direct message buys nothing**, because both
    // parties are one. Otherwise the person you are talking to is offered a
    // control that reaches into what you said, and the exchange accepts it.
    let admin_of_a_dm = strip_of("theirs, in a direct message", true, true);
    assert!(
        !admin_of_a_dm.contains("Delete"),
        "one party of a direct message was offered Delete on the other's message: \
         {admin_of_a_dm}"
    );
    // And in a room, where an admin is somebody the others are not, the
    // power is real and is offered. The control for the case above: without
    // it, hiding Delete everywhere would pass.
    let admin_of_a_room = strip_of("theirs, in a direct message", true, false);
    assert!(
        admin_of_a_room.contains("Delete"),
        "an admin of a room was not offered Delete on a member's message: {admin_of_a_room}"
    );
}

// ---------------------------------------------------------------------------
// A direct message is not a channel with two people in it.
// ---------------------------------------------------------------------------

/// **A direct message has no name and no topic to set.**
///
/// The transcript's own heading has refused to rename one since it was
/// written, and this page offered both anyway — two fields with a tick
/// beside each, over the other person's name, on a conversation whose
/// label is that person. And a button to destroy "this channel".
#[test]
fn a_direct_message_is_not_offered_a_name_a_topic_or_a_channel_to_destroy() {
    // The public channel of the fixture: a real channel, where all of this
    // belongs. The control for the whole test.
    let mut channel = a_conversation();
    channel.open = Some([8u8; 32]);
    let (mut h, _) = harness_phone_with(channel, sigil_chat::Route::Settings);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Name"),
        "a channel lost its name field: {said}"
    );
    assert!(said.contains("Topic"), "{said}");
    assert!(said.contains("Destroy this channel"), "{said}");

    // The direct message.
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Settings);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Topic"),
        "a direct message was offered a topic: {said}"
    );
    assert!(
        !said.contains("Destroy this channel"),
        "a direct message is not a channel: {said}"
    );
    assert!(said.contains("Destroy this conversation"), "{said}");
    // What a direct message *does* have: it is still kept for a while, it
    // can still be left, and it is still yours to mute.
    assert!(said.contains("Keep messages for"), "{said}");
    assert!(said.contains("Leave"), "{said}");
    // Its own mute, whatever else a direct message lacks — named by what
    // it is now, as the app's other switches are.
    assert!(
        said.contains("Said out loud") || said.contains("Muted"),
        "{said}"
    );
}

// ---------------------------------------------------------------------------
// Stranded, and why.
// ---------------------------------------------------------------------------

/// **One line, with the count in it, and the cause beside it.**
///
/// A stranded member (SIP-17) got three messages for one fact: the
/// library's error in the banner, "an admin has to hand you one" in the
/// transcript, and under it "their key may still arrive" — which
/// contradicts the line above it. And when the reason was that the
/// conversation had *moved* (SIP-60: a direct message lives at the home of
/// the lower key, so one party moving moves the conversation), that was
/// said quietly up in the bar and nowhere near the trouble it caused.
#[test]
fn being_stranded_is_said_once_and_says_where_the_key_must_come_from() {
    let mut state = a_conversation();
    state.trouble_with.no_key = Some(1);
    state.trouble_with.unreadable = 156;
    let (mut h, _) = harness_phone_with(state.clone(), sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("none of its 156 messages can be read"),
        "the count belongs in the sentence: {said}"
    );
    assert!(
        !said.contains("may still arrive"),
        "still promising the key might turn up on its own: {said}"
    );
    // Not "an admin": a direct message has none.
    assert!(!said.contains("An admin"), "{said}");

    // And when it is ordered elsewhere, that is the cause, said here.
    let mut moved = state;
    moved.home = Some((them(), "trunk.exchange".into()));
    let (mut h, _) = harness_phone_with(moved, sigil_chat::Route::Conversations);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("ordered by trunk.exchange"),
        "the conversation moved and the trouble does not say so: {said}"
    );
}

/// **And something to press.** Being told a key has to reach you, with
/// nothing to do about it, is the whole of what somebody stranded could do
/// — which is nothing. SIP-17's own way out is to ask for an envelope, and
/// to mint the next epoch where none comes and this account may.
#[test]
fn being_stranded_offers_a_way_to_ask_for_the_key() {
    let mut state = a_conversation();
    state.trouble_with.no_key = Some(1);
    state.trouble_with.unreadable = 156;
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    h.get_by_label("Ask for the key");

    // And not offered where there is no key missing, or it reads as
    // something a healthy conversation needs.
    let (mut h, _) = harness_phone_with(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    assert!(
        h.query_by_label("Ask for the key").is_none(),
        "offered with nothing wrong: {}",
        text_of(&h)
    );
}

/// **An action's answer belongs on the page the action is on.**
///
/// Minting a key, setting a name, a topic, a retention window — all on the
/// settings page, and every one of their answers was drawn under the
/// *conversation's* title row, which is not the page somebody pressing
/// them is looking at. Pressing "Mint a new key" and being told nothing at
/// all is how this was found.
#[test]
fn what_was_just_done_is_said_on_the_page_it_was_done_on() {
    let mut state = a_conversation();
    state.note = Some(sigil_chat::Note {
        said: "New key minted (epoch 2).".into(),
        at: NOW,
    });
    let (mut h, _) = harness_phone_with(state.clone(), sigil_chat::Route::Settings);
    h.run();
    assert!(
        text_of(&h).contains("New key minted (epoch 2)."),
        "the settings page said nothing about what it had just done: {}",
        text_of(&h)
    );
    // And still on the conversation, which is where the other actions are.
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Conversations);
    h.run();
    assert!(
        text_of(&h).contains("New key minted (epoch 2)."),
        "{}",
        text_of(&h)
    );
}

/// **The direct-calls switch exists on a phone.**
///
/// `direct_allowed` reads `prefs.direct_calls` on every call, and the only
/// control for it was in the desktop's platform pane — which Android does
/// not have. So on a phone the preference decided every call and could
/// never be set.
#[test]
fn the_card_can_choose_whether_calls_connect_directly() {
    // **Tall enough to hold the whole page.** The Me page scrolls, and this
    // switch sits near its foot -- so on a 804-point viewport it fell below
    // the fold as the page grew, and a kittest click on a node that is not on
    // screen silently does nothing: the node is found, the press lands
    // nowhere, and the assertion fails as though the preference were not
    // wired. What is under test here is the switch reaching the preference,
    // not where the fold happens to fall; `phone_width.rs` and the snapshots
    // are what hold the layout.
    const TALL_ENOUGH: f32 = 1600.0;
    // A harness that keeps the accounts, since the switch is the person's
    // and not the conversation's.
    let accounts = std::rc::Rc::new(std::cell::RefCell::new(sigil::accounts::Accounts::of(
        vec![account()],
    )));
    let held = accounts.clone();
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(a_conversation());
    let siblings = three_apps();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE_WIDTH, TALL_ENOUGH))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = Navigator::default();
            nav.set_siblings(siblings.clone());
            let mut accounts = held.borrow_mut();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(sigil_chat::Route::Me);
            let _ = app.render_nav(&mut app_ctx, ui, &token);
        });
    h.run();
    assert!(
        accounts.borrow().prefs.direct_calls,
        "on by default, so pressing it below is turning it off"
    );
    // **The label follows the state**, so each press is found by what the
    // switch says it is *now* -- which is the whole point of the change:
    // it read the same in both states and so looked like it had not moved.
    h.get_by_label_contains("Calls connect directly").click();
    h.run();
    assert!(
        !accounts.borrow().prefs.direct_calls,
        "the switch did not reach the preference"
    );
    h.get_by_label_contains("Calls go through the exchange")
        .click();
    h.run();
    assert!(accounts.borrow().prefs.direct_calls, "and back on again");
}

/// **The device in your hand could not be signed out.**
///
/// Every other device on the card can be revoked; this one had no control
/// at all. Revoking is for a device you have *lost* — it keeps every key
/// it was already given — and is the wrong shape for the one you are
/// holding. SIP-22 has `sign_out_device` for that, and no client called it.
#[test]
fn this_device_can_be_signed_out_and_is_asked_twice() {
    let mut state = a_conversation();
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - 10 * DAY,
            not_after: NOW + 30 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - 5 * DAY,
            not_after: NOW + 30 * DAY,
            is_this_one: false,
        },
    ];
    let (mut h, _) = harness_phone_with(state, sigil_chat::Route::Devices);
    h.run();
    // The other device is revoked; this one is signed out. Neither offers
    // the other's control.
    h.get_by_label("Sign out");
    h.get_by_label("Revoke");

    // Asked twice, and the cost said before the second press.
    h.get_by_label("Sign out").click();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("cannot sign itself back in"),
        "signed out with no warning about what it costs: {said}"
    );
    h.get_by_label("Cancel").click();
    h.run();
    assert!(
        !text_of(&h).contains("cannot sign itself back in"),
        "cancelling left the warning up"
    );
}

// ---------------------------------------------------------------------------
// SIP-39: calling somebody at another exchange, from the conversation.
// ---------------------------------------------------------------------------

/// **The handset takes the SIP-39 path when the peer lives elsewhere.**
///
/// SIP-36's invitation carries a room secret and never says which exchange
/// the room is at, so two people at different exchanges each join a room at
/// their own — same secret, two rooms, and a call that rings, is answered
/// and carries nothing. sigil already *answers* a call another exchange
/// carried here; placing one was the missing half.
#[test]
fn a_peer_at_another_exchange_is_called_across_the_bridge() {
    // `lives` is where their account is (SIP-59); `bound` is the exchange
    // their name happens to be registered at. The two differ exactly in
    // the case that matters: somebody living elsewhere who holds a name
    // here as an alias.
    fn asked(lives: &str, bound: &str, ours: &str) -> Vec<String> {
        let mut state = a_conversation();
        state.domain = Some(ours.to_string());
        state.peer_home = Some((them(), lives.to_string()));
        state.people.insert(
            them(),
            sigil_chat::Person {
                name: Some("Ada".into()),
                title: None,
                handle: Some(format!("ada@{bound}")),
                picture: None,
            },
        );
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut h = harness_at_recording(state, sigil_chat::Route::Conversations, asked.clone());
        h.run();
        h.get_by_label("Call").click();
        h.run();
        asked.borrow().clone()
    }

    // Living here: the room path, which is a posted invitation.
    let same = asked("squic.org", "squic.org", "squic.org");
    assert!(
        same.iter().any(|c| c.contains("Call")),
        "a call at one exchange still posts its invitation: {same:?}"
    );

    // Living elsewhere: nothing is posted — the call is placed at our own
    // exchange for a handle at *their* home instead.
    let cross = asked("trunk.exchange", "trunk.exchange", "squic.org");
    assert!(
        !cross.iter().any(|c| c.contains("Call")),
        "a cross-exchange call posted a room invitation nobody can join: {cross:?}"
    );

    // **The case that caught the first version.** They live at trunk and
    // hold a name *here* as an alias, so their handle's domain is ours.
    // Asking about the handle says "same exchange" and takes the room
    // path; asking about the home says otherwise, and is right.
    let alias = asked("trunk.exchange", "squic.org", "squic.org");
    assert!(
        !alias.iter().any(|c| c.contains("Call")),
        "somebody living elsewhere who holds a name here was called through a room: \
         {alias:?}"
    );
}

/// Two devices, one of them this one, with whatever the exchange last said
/// about this device's one-time prekeys.
fn devices_with_prekeys(prekeys: Option<u16>) -> ChatState {
    let mut state = a_conversation();
    state.prekeys = prekeys;
    state.devices = vec![
        sigil_chat::Linked {
            device: me(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: true,
        },
        sigil_chat::Linked {
            device: them(),
            added: NOW - DAY,
            not_after: NOW + 90 * DAY,
            is_this_one: false,
        },
    ];
    state
}

/// **SIP-23: how many one-time keys this device has left is on its row.**
///
/// The exchange counts them per device and tells each one its own number on
/// every catch-up. That number arrived from the day catch-up existed and
/// went into a `tracing` line, so a pool that had run dry was invisible in
/// the one place somebody looks at their devices.
#[test]
fn this_devices_remaining_one_time_keys_are_on_its_row() {
    let mut h = harness_at(devices_with_prekeys(Some(42)), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("42 one-time keys"),
        "the count is not on the row: {}",
        text_of(&h)
    );
}

/// And a client nobody has told yet says nothing, rather than implying a
/// pool of nothing. The control: `None` and `Some(0)` are different facts,
/// and a row drawn unconditionally would make them the same one.
#[test]
fn a_device_nobody_has_counted_yet_claims_no_count() {
    let mut h = harness_at(devices_with_prekeys(None), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("one-time keys"),
        "a count was claimed before any exchange gave one: {}",
        text_of(&h)
    );
    assert!(
        !text_of(&h).contains("last-resort key"),
        "a drained pool was claimed before any exchange gave a count: {}",
        text_of(&h)
    );
}

/// **A drained pool says what it means.**
///
/// Not that the device is unreachable -- the exchange serves the fallback
/// prekey once the one-time pool runs dry, which is what stops a dry pool
/// becoming an unreachable device -- but that everything sealed to it now
/// shares one reused secret until the device republishes.
#[test]
fn a_drained_pool_says_what_falls_back() {
    let mut h = harness_at(devices_with_prekeys(Some(0)), sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("No one-time keys left here"),
        "a drained pool said nothing: {said}"
    );
    assert!(
        said.contains("last-resort key"),
        "it did not say what happens instead: {said}"
    );
}

/// And a pool with keys in it stays quiet about the fallback. The control
/// for the test above.
#[test]
fn a_full_pool_says_nothing_about_a_fallback() {
    let mut h = harness_at(devices_with_prekeys(Some(42)), sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("last-resort key"),
        "a fallback was announced for a pool that has keys: {}",
        text_of(&h)
    );
}

/// **An empty device list and an unanswered one are different facts.**
///
/// The card's "nothing else is linked" warning fired on `len() <= 1`, which
/// is as true of a fetch that failed as of an account with one device — so
/// an unreachable exchange told somebody their conversations could not be
/// recovered. Three states, three sentences, one test each way.
#[test]
fn a_device_list_nobody_has_answered_claims_nothing() {
    let mut state = devices_with_prekeys(None);
    state.devices = Vec::new();
    state.devices_known = false;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Asking the exchange which devices are linked"),
        "it did not say it was still asking: {said}"
    );
    assert!(
        !said.contains("Nothing else is linked"),
        "it claimed nothing was linked before anybody answered: {said}"
    );
}

/// And when the asking failed, it says so rather than asking for ever.
#[test]
fn a_device_list_that_could_not_be_fetched_says_why() {
    let mut state = devices_with_prekeys(None);
    state.devices = Vec::new();
    state.devices_known = false;
    state.devices_trouble = Some("the exchange is not answering".into());
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Could not ask the exchange which devices are linked"),
        "the failure was silent: {said}"
    );
    assert!(
        said.contains("the exchange is not answering"),
        "it did not say why: {said}"
    );
    // The *whole* sentence: the backup section a little further down this
    // same card says "Asking the exchange…" about itself, so the short
    // substring is true of a card that says nothing about devices at all.
    assert!(
        !said.contains("Asking the exchange which devices are linked"),
        "it is still claiming to be asking: {said}"
    );
    assert!(
        !said.contains("Nothing else is linked"),
        "a failed fetch was drawn as a fact about the account: {said}"
    );
}

/// The control: once the exchange has answered and the answer is one
/// device, the warning is exactly right and must still be drawn.
#[test]
fn one_device_that_the_exchange_confirmed_is_still_warned_about() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices.truncate(1);
    state.devices_known = true;
    state.backup = None;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Nothing else is linked"),
        "an account with one device was not warned: {said}"
    );
    // As above: the backup section says "Asking the exchange…" about
    // itself, and this card is not the only thing on it that asks.
    assert!(
        !said.contains("Asking the exchange which devices are linked")
            && !said.contains("Could not ask"),
        "it is still claiming not to know: {said}"
    );
}

/// **An empty list is not "nothing else".**
///
/// The warning takes for granted that *this* device is linked and says
/// nothing else is. When the exchange answers with no devices at all — as
/// trunk does for this account — that reading is wrong, and the card drew
/// no row either: it warned about linking a second device while saying
/// nothing whatever about the first.
#[test]
fn an_exchange_that_lists_no_devices_says_that_instead() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices = Vec::new();
    state.devices_known = true;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("The exchange lists no devices for this account"),
        "an empty list said nothing about being empty: {said}"
    );
    assert!(
        !said.contains("Nothing else is linked"),
        "it claimed this device is linked when the exchange lists none: {said}"
    );
}

/// And what this device holds is still said, because it is still true.
///
/// The count arrives on every catch-up and belongs on this device's row —
/// but there is no row when the list comes back empty, and a number we
/// have is not worth hiding behind a list we could not get.
#[test]
fn this_devices_keys_are_said_even_with_no_row_to_put_them_on() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices = Vec::new();
    state.devices_known = true;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("This device has 53 one-time keys at the exchange"),
        "the count we hold was hidden behind the list we did not get: {}",
        text_of(&h)
    );
}

/// The control: with no count yet, nothing is claimed about one.
#[test]
fn no_count_yet_claims_no_count_with_an_empty_list() {
    let mut state = devices_with_prekeys(None);
    state.devices = Vec::new();
    state.devices_known = true;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("one-time keys"),
        "a count was claimed before any exchange gave one: {}",
        text_of(&h)
    );
}

/// **A card cannot be asking a question it never managed to put.**
///
/// `Cmd::Devices` is sent once, when the menu item is pressed. A session
/// still connecting drops it and nothing asks again, so the card sat on
/// "asking the exchange…" for ever — and opening it a second later worked,
/// which is what made the bug look intermittent rather than ordered.
#[test]
fn a_device_list_with_no_link_says_it_is_waiting_for_one() {
    let mut state = devices_with_prekeys(None);
    state.devices = Vec::new();
    state.devices_known = false;
    state.link = sigil_chat::session::LinkState::Connecting;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Waiting for the exchange before asking"),
        "it claimed to be asking over a link it does not have: {said}"
    );
    assert!(
        !said.contains("Asking the exchange which devices are linked"),
        "it is asking without a link: {said}"
    );
}

/// And with a link and no answer yet, it is genuinely asking. The control:
/// the two sentences must not collapse into one.
#[test]
fn a_device_list_with_a_link_and_no_answer_is_asking() {
    let mut state = devices_with_prekeys(None);
    state.devices = Vec::new();
    state.devices_known = false;
    state.link = sigil_chat::session::LinkState::Up;
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Asking the exchange which devices are linked"),
        "it is not asking over a link it has: {said}"
    );
    assert!(
        !said.contains("Waiting for the exchange before asking"),
        "it is waiting for a link it has: {said}"
    );
}

/// **What an irreversible row costs is drawn on a phone, not hovered.**
///
/// "Mint a new key" acts on one press and cannot be pressed back: the old
/// epoch is superseded, and a device that does not get the new key reads
/// nothing sealed under it. That was explained only in hover text, which a
/// handset has no pointer to reach — so on a phone it was an unexplained
/// single tap, sitting beside a *destruction* that asks twice.
#[test]
fn on_a_phone_what_minting_a_key_costs_is_on_the_screen() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Settings);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Mint a new key"),
        "the row is not on screen at all: {said}"
    );
    assert!(
        said.contains("Everybody present is given a new key"),
        "a phone is told nothing about what the row does: {said}"
    );
    assert!(
        said.contains("You stop receiving this conversation"),
        "a phone is told nothing about what leaving does: {said}"
    );
}

/// And a pointer does not need it drawn: it hovers. The control — the same
/// sentences must not be duplicated into a desktop that already has them,
/// or every settings page grows a second copy of its own tooltips.
#[test]
fn with_a_pointer_the_same_words_stay_in_the_hover() {
    let mut h = harness_at(a_conversation(), sigil_chat::Route::Settings);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Mint a new key"),
        "the row is not on screen at all: {said}"
    );
    assert!(
        !said.contains("Everybody present is given a new key"),
        "the tooltip was drawn as well on a form that can hover it: {said}"
    );
}

/// **A report in a direct message goes to the person it is about.**
///
/// SIP-56 sends a report to the channel's admins. In a direct message both
/// parties *are* admins — the exchange makes a party one on joining, which
/// is what lets either mint an epoch key — so "the admins" is the other
/// side and nobody else, and no operator ever sees it. Somebody reporting
/// abuse here is handing it to the person they are reporting, with their
/// name on it. The only thing that hinted at any of it was a tooltip about
/// "the room's admins", which a phone cannot reach at all.
#[test]
fn reporting_a_direct_message_says_it_goes_to_the_other_party() {
    let mut h = harness_at(a_conversation(), sigil_chat::Route::Members);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Report this conversation"),
        "the control is not on screen: {said}"
    );
    assert!(
        said.contains("the only admins are the two of you"),
        "it does not say who the report reaches: {said}"
    );
    assert!(
        said.contains("No operator sees it"),
        "it implies somebody official is listening: {said}"
    );
}

/// The control: a room has admins who are not the person being reported, so
/// the direct-message sentence must not be shown there — it would be false,
/// and it is the sentence that makes the warning worth reading.
#[test]
fn reporting_a_room_does_not_claim_the_admins_are_the_two_of_you() {
    let mut state = a_conversation();
    // A room: the open conversation has no peer, which is what makes a
    // channel a channel rather than a direct message.
    for c in &mut state.conversations {
        c.peer = None;
    }
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("the only admins are the two of you"),
        "a room was described as a direct message: {said}"
    );
}

/// **A room's moderation is not offered inside a direct message.**
///
/// Both parties of a direct message are admins — the exchange makes them
/// one on joining, which is what lets either mint an epoch key (SIP-17) —
/// so every admin act was offered to each of them about the other.
/// "Remove" ejects the only other person and mints a key; "Demote" takes
/// away the admin-ness that lets them mint one at all. Both hover texts
/// call it "this room", which it is not.
#[test]
fn a_direct_message_offers_no_remove_and_no_demote() {
    let mut state = devices_with_prekeys(None);
    state.i_am_admin = true;
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    h.run();
    let said = text_of(&h);
    for act in ["Remove", "Demote", "Make admin"] {
        assert!(
            !said.contains(act),
            "a direct message offered {act:?} about the only other person: {said}"
        );
    }
    // What is not an admin's stays: blocking is the control somebody in a
    // conversation they want out of actually reaches for, and it is
    // personal rather than moderation.
    assert!(
        said.contains("Block") || said.contains("Unblock"),
        "blocking was taken away with the moderation: {said}"
    );
}

/// And a room still has them, or the test above would pass by hiding every
/// act everywhere. The control.
#[test]
fn a_room_still_offers_its_admin_their_acts() {
    let mut state = devices_with_prekeys(None);
    state.i_am_admin = true;
    for c in &mut state.conversations {
        c.peer = None;
    }
    let mut h = harness_at(state, sigil_chat::Route::Members);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Remove"),
        "an admin of a room was not offered Remove: {said}"
    );
}

/// **The public-channels pane shows its own refusal, not the last one.**
///
/// It drew `state.trouble`, which any failing command sets and nothing
/// clears — so a call that could not be placed hours earlier was still in
/// red above a directory listing that had just answered perfectly, where
/// it reads as "this pane is not connected". Found on the phone, with the
/// chat list showing a green dot at the same moment.
#[test]
fn the_directory_does_not_show_an_unrelated_failure() {
    let mut state = a_conversation();
    state.trouble = Some("not connected to the exchange".into());
    state.join_trouble = None;
    let mut h = harness_at(state, sigil_chat::Route::Directory);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("not connected to the exchange"),
        "a failure from somewhere else was drawn over the directory: {}",
        text_of(&h)
    );
}

/// And a join that really was refused still says so, where the button is.
/// The control: hiding everything would pass the test above.
#[test]
fn a_refused_join_still_says_so_in_the_directory() {
    let mut state = a_conversation();
    state.trouble = None;
    state.join_trouble = Some("this channel does not admit you".into());
    let mut h = harness_at(state, sigil_chat::Route::Directory);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("this channel does not admit you"),
        "a refused join said nothing where the button is: {}",
        text_of(&h)
    );
}

/// **SIP-42 had a command, a handler and a library call, and no control.**
///
/// The devices card states the problem — "an epoch key is sealed to a
/// device, so the other one has to hand them over before anything already
/// said can be read here" — and this is the handing over. Nothing anywhere
/// sent `Cmd::ResealToSiblings`.
#[test]
fn a_second_device_can_be_given_the_conversations_key() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices_known = true;
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    assert!(
        text_of(&h).contains("Give the key to your devices"),
        "an account with a second device was offered no way to hand it the key: {}",
        text_of(&h)
    );
}

/// And with nowhere to send it, it is not offered. The library seals
/// nothing and returns `Ok(0)`, which the session reports as "your other
/// devices already hold this conversation's key" — true of a device that
/// has them, and false of an account that has no other device at all.
#[test]
fn with_no_second_device_the_key_is_not_offered_anywhere() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices_known = true;
    state.devices.truncate(1);
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Give the key to your devices"),
        "offered to hand a key to devices that do not exist: {}",
        text_of(&h)
    );
}

/// And not having asked is not the same as having asked and been told
/// none: an unanswered device list claims nothing either way.
#[test]
fn an_unasked_device_list_offers_no_key_handover() {
    let mut state = devices_with_prekeys(Some(53));
    state.devices_known = false;
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Give the key to your devices"),
        "acted on a device list nobody had answered: {}",
        text_of(&h)
    );
}

/// **SIP-24 had a command, a handler, a library call — and nothing asked.**
///
/// An exchange running a whitelist refuses every gated route with
/// `NotWhitelisted`, so a session there can do nothing at all. The one
/// route it leaves open is `/admission/request`, which the exchange's own
/// comment calls "SIP-24's one way in, which answers everyone identically
/// so it is not an oracle". The refusal used to arrive as an ordinary
/// error string and retry for ever.
#[test]
fn an_exchange_that_does_not_admit_you_offers_the_way_in() {
    let mut state = a_conversation();
    state.not_admitted = Some("ex.squic.org".into());
    state.trouble = Some("ex.squic.org does not admit this account".into());
    let mut h = harness_at(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Ask to be let in"),
        "refused, and offered no way to ask: {said}"
    );
    // And it does not promise an answer, because the route deliberately
    // gives the same reply either way.
    assert!(
        said.contains("not back to you"),
        "it implied an answer would come back: {said}"
    );
}

/// The control: an exchange that admits you says nothing about admission.
#[test]
fn an_exchange_that_admits_you_says_nothing_about_being_let_in() {
    let mut state = a_conversation();
    state.not_admitted = None;
    let mut h = harness_at(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Ask to be let in"),
        "offered a way in to somebody already inside: {}",
        text_of(&h)
    );
}

/// **SIP-35 had a command, a handler and a library call, and nothing sent it.**
///
/// A channel's copies are deliberately entries rather than an arrangement
/// between two operators, so that the people in it can see them — and the
/// transcript already words both events, replication being one of the few
/// it refuses to treat as plumbing. The log is the readout; what was
/// missing was any way to author one.
#[test]
fn a_room_can_let_another_exchange_carry_a_copy() {
    let mut state = a_conversation();
    state.i_am_admin = true;
    for c in &mut state.conversations {
        c.peer = None;
    }
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Let another exchange carry a copy"),
        "a room's admin was offered no way to authorise a replica: {said}"
    );
    // Never "recall": SIP-35 is explicit that withdrawing ends a
    // subscription and takes nothing back.
    assert!(
        said.contains("What it already holds, it keeps"),
        "it implied a copy could be taken back: {said}"
    );
    assert!(
        !said.to_lowercase().contains("recall"),
        "SIP-35 forbids describing this as recalling anything: {said}"
    );
}

/// And not in a direct message: its identifier stands for two accounts,
/// and a third party holding a copy of one is not what SIP-35 is for. The
/// control for the test above.
#[test]
fn a_direct_message_is_not_offered_to_another_exchange() {
    let mut state = a_conversation();
    state.i_am_admin = true;
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Let another exchange carry a copy"),
        "offered to hand a direct message to a third operator: {}",
        text_of(&h)
    );
}

/// A room with a picture, drawn in the conversation list.
///
/// **SIP-16's avatar was carried and never drawn.** The bytes ride inline
/// on the attachment — a preview, the same ones a message thumbnail uses —
/// so there is nothing to fetch, and `sigil_ui::avatar` has always been "a
/// picture if there is one, and the identicon if there is not". Every
/// caller passed `None`, so every row was an identicon whatever the
/// channel carried.
///
/// A direct message is deliberately left with its identicon: it is drawn
/// as the person in it, whose picture is their profile's (SIP-21).
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_chats_with_a_picture() {
    let mut state = a_conversation();
    // A plain green square, big enough to see and small enough to be the
    // kind of preview an attachment really carries.
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(24, 24, image::Rgb([0x3d, 0xd6, 0x8c]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode the picture");
    let bytes = png.into_inner();
    for c in &mut state.conversations {
        // Only the room: the direct message keeps the person's mark.
        if c.peer.is_none() {
            c.avatar = Some(bytes.clone());
        }
    }
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_chats_with_a_picture");
}

/// **SIP-16's picture could be drawn and not set.**
///
/// `Cmd::SetChannelAvatar` uploads the file and publishes it as channel
/// metadata; nothing sent it. Offering it was pointless while nothing drew
/// a channel's picture — and that was the reason not to build it, until
/// the drawing existed.
#[test]
fn a_rooms_picture_can_be_set_and_taken_down() {
    let mut state = a_conversation();
    state.i_am_admin = true;
    for c in &mut state.conversations {
        c.peer = None;
    }
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Set a picture"),
        "a room's admin was offered no way to give it one: {said}"
    );
    assert!(
        said.contains("Remove it"),
        "a picture could be set and never taken down: {said}"
    );
}

/// And not in a direct message, which is drawn as the person in it — their
/// picture is their profile's, not the conversation's. The control.
#[test]
fn a_direct_message_has_no_picture_of_its_own() {
    let mut state = a_conversation();
    state.i_am_admin = true;
    let mut h = harness_at(state, sigil_chat::Route::Settings);
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("Set a picture"),
        "offered a direct message a picture that would compete with the person in it: {}",
        text_of(&h)
    );
}

/// **A switch says which it is, not what pressing it would do.**
///
/// This read "Connect calls directly" in both states and changed only a
/// thin border, so pressing it looked like nothing happening — reported as
/// "the button does not toggle" while it was toggling perfectly. The
/// switch beside it names a state ("Do not disturb" against
/// "Notifications") and changes its mark with it.
#[test]
fn the_call_path_switch_names_the_state_it_is_in() {
    for direct in [true, false] {
        let mut h = harness_at_prefs(a_conversation(), sigil_chat::Route::Me, direct);
        h.run();
        h.run();
        let said = text_of(&h);
        let (want, avoid) = if direct {
            ("Calls connect directly", "Calls go through the exchange")
        } else {
            ("Calls go through the exchange", "Calls connect directly")
        };
        assert!(
            said.contains(want),
            "direct={direct}: the switch does not say which it is: {said}"
        );
        assert!(
            !said.contains(avoid),
            "direct={direct}: it says both at once: {said}"
        );
        // And never the old wording, which named an action and so read as
        // a button that had done nothing.
        assert!(
            !said.contains("Connect calls directly"),
            "direct={direct}: back to naming the action: {said}"
        );
    }
}

/// **A one-line event takes one line's room.**
///
/// A system line is a space, a centred row and a space in a vertical
/// layout, and egui puts `item_spacing.y` between each of them and around
/// the lot — so the 12px of padding it asks for arrived as 50, and a
/// fourteen-pixel sentence stood 64 tall. Four calls in a row then ate a
/// quarter of a phone screen with nothing in it, which is what a
/// screenshot of a real conversation showed.
///
/// The bound is on the *stride* between consecutive events rather than on
/// any one line's height: the waste was all in the gaps, and a height
/// assertion would have passed throughout.
#[tokio::test(flavor = "multi_thread")]
async fn consecutive_events_do_not_each_take_a_paragraph() {
    let mut state = a_conversation();
    state.events = (0..4)
        .map(|i| Happened {
            seq: 3,
            at: NOW - 120,
            said: format!("Colin Lyons called, {i}s"),
            actor: them(),
            subject: them(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Was),
        })
        .collect();
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    // A call line carries its clock, so the label is the sentence and the
    // time -- computed here rather than matched loosely, which would pass
    // against the wrong row.
    let when = sigil_ui::clock(NOW - 120);
    let tops: Vec<f32> = (0..4)
        .map(|i| {
            h.get_by_label(&format!("Colin Lyons called, {i}s · {when}"))
                .rect()
                .top()
        })
        .collect();
    let strides: Vec<f32> = tops.windows(2).map(|w| w[1] - w[0]).collect();
    // 34 as this is written -- 14pt of small text, the 12 of padding
    // `system_line` asks for, and the parent's own gap. It was 64, all of
    // the difference being a row held open to `interact_size.y` because
    // the phone form sizes rows for a thumb. The bound is loose enough to
    // survive a pixel of font or layout drift and nowhere near 64.
    const ROOM: f32 = 40.0;
    for (i, stride) in strides.iter().enumerate() {
        assert!(
            *stride <= ROOM,
            "event {i} to {}: {stride}px apart, wanted at most {ROOM}. \
             tops={tops:?}",
            i + 1
        );
    }
}

/// **A call says when it was.**
///
/// Every message in the transcript carries a clock and a call carried
/// none, so a column of "Ada called, 31s" answered everything except the
/// question somebody actually has about a missed call — `Happened` held
/// the time the whole time. And a missed call read in exactly the muted
/// grey of "Ada added Bram", so in a run of call lines there was nothing
/// to catch the eye.
///
/// The membership line is the control: it is not a call and must carry no
/// clock. **The colouring is not asserted here** — the accessibility tree
/// carries text and not paint, so a name claiming it would be claiming
/// what this cannot see. `phone_call_log` holds that.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_carries_its_clock() {
    let mut state = a_conversation();
    state.events = vec![
        Happened {
            seq: 3,
            at: NOW - 120,
            said: "Ada called, 31s".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Was),
        },
        Happened {
            seq: 3,
            at: NOW - 60,
            said: "Missed call from Ada".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Missed),
        },
        // The control for the clock: a membership change is not a call and
        // wants no time beside it.
        Happened {
            seq: 3,
            at: NOW - 30,
            said: "Ada added Bram".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: None,
        },
    ];
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    let said = labels(&h);

    for (what, at) in [
        ("Ada called, 31s", NOW - 120),
        ("Missed call from Ada", NOW - 60),
    ] {
        let want = format!("{what} · {}", sigil_ui::clock(at));
        assert!(
            said.iter().any(|l| l == &want),
            "a call did not carry its clock, wanted {want:?}: {said:?}"
        );
    }
    assert!(
        said.iter().any(|l| l == "Ada added Bram"),
        "the membership line is missing: {said:?}"
    );
    assert!(
        !said.iter().any(|l| l.starts_with("Ada added Bram · ")),
        "a membership change was given a clock it did not ask for: {said:?}"
    );
}

/// **A call log on a phone: the clock on each, the colour on one.**
///
/// The pixels are the only place the colouring can be checked — the
/// accessibility tree carries text and not paint — so this is what holds
/// the claim that a missed call looks different from a call that
/// happened, and that a membership line is neither.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_call_log() {
    let mut state = a_conversation();
    state.events = vec![
        Happened {
            seq: 3,
            at: NOW - 3600,
            said: "You called, no answer".into(),
            actor: me(),
            subject: me(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Was),
        },
        Happened {
            seq: 3,
            at: NOW - 1800,
            said: "Missed call from Ada".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Missed),
        },
        Happened {
            seq: 3,
            at: NOW - 900,
            said: "Ada called, 31s".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: Some(sigil_chat::session::Call::Was),
        },
        Happened {
            seq: 3,
            at: NOW - 300,
            said: "Ada added Bram".into(),
            actor: them(),
            subject: them(),
            caveat: None,
            call: None,
        },
    ];
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();
    h.snapshot("phone_call_log");
}

/// **A home that could not be asked for is asked for again.**
///
/// SIP-59's `account_home` decides whether a call is placed at this
/// exchange or bridged to the peer's (SIP-39), and the ask can fail — a
/// reconnect, a slow exchange, an account mid-succession — with the
/// failure dropped. The guard was a `HashSet`, so one failure meant the
/// home stayed unknown for the life of the window; and unknown is read as
/// "they are here", so every later call rang, connected to a room at the
/// wrong exchange and carried nothing. Seen live, twice.
///
/// Both halves are asserted, and the first is the control: a question
/// asked from a draw is asked sixty times a second unless something stops
/// it, so "asks again" must not be bought with "asks always".
#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_home_is_asked_for_again_but_not_on_every_frame() {
    let mut state = a_conversation();
    state.peer_home = None;
    let (mut h, app) = harness_phone_with(state, sigil_chat::Route::Conversations);
    let asks = |app: &std::rc::Rc<std::cell::RefCell<ChatApp>>| {
        app.borrow()
            .sent_for_test()
            .iter()
            .filter(|c| c.contains("PeerHome"))
            .count()
    };

    // Many frames, one question: a draw runs sixty times a second.
    h.run();
    h.run();
    h.run();
    assert_eq!(
        asks(&app),
        1,
        "a draw asked more than once: {:?}",
        app.borrow().sent_for_test()
    );

    // Still unknown a moment later: still one question, not a loop. This is
    // the control — "asks again" must not be bought with "asks always".
    app.borrow_mut().set_now_for_test(NOW + 5);
    h.run();
    h.run();
    assert_eq!(asks(&app), 1, "it asked again far too soon");

    // And past the retry it asks again, rather than giving up for the life
    // of the window on one failure nobody saw.
    app.borrow_mut().set_now_for_test(NOW + 25);
    h.run();
    assert_eq!(asks(&app), 2, "a failed ask was never repeated");
}

/// **A call is not placed until it is known which exchange they are at.**
///
/// Which exchange a peer's account is at decides between an ordinary call
/// and a SIP-39 bridge, and an unanswered question was read as "here" — so
/// a call to somebody at another exchange rang, connected to a room at the
/// wrong exchange and carried nothing, with nothing on screen to say why.
/// Seen live, twice.
///
/// The second case is the one that makes a refusal safe, and the reason
/// this could not be decided until the state could tell them apart: a peer
/// whose exchange is reached by an address has **no domain** to be bridged
/// to, and the ordinary path is right for them. That is an answer, and is
/// recorded as an empty domain. Refusing it too would have broken calls
/// that work.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_waits_until_it_is_known_where_they_live() {
    for (what, home, placed) in [
        ("unknown", None, false),
        // Asked, and they live at an exchange with no name to bridge to.
        (
            "homeless",
            Some((PubKey::new([9u8; 32]), String::new())),
            true,
        ),
        // Asked, and they live here.
        (
            "here",
            Some((PubKey::new([9u8; 32]), "squic.org".to_string())),
            true,
        ),
    ] {
        let mut state = a_conversation();
        state.peer_home = home;
        state.domain = Some("squic.org".into());
        let (mut h, app) = harness_phone_with(state, sigil_chat::Route::Conversations);
        h.run();
        h.run();
        h.get_by_label("Call").click();
        h.run();

        let sent = app.borrow().sent_for_test().to_vec();
        let rang = sent.iter().any(|c| c.starts_with("Call"));
        assert_eq!(
            rang, placed,
            "{what}: placing the call was {rang}, wanted {placed}: {sent:?}"
        );
        if !placed {
            // And it says so, rather than doing nothing visible.
            let said = text_of(&h);
            assert!(
                said.contains("Still finding which exchange"),
                "{what}: the press was silent: {said}"
            );
            // And asks, so pressing again shortly after can work.
            assert!(
                sent.iter().any(|c| c.contains("PeerHome")),
                "{what}: it refused without asking: {sent:?}"
            );
        }
    }
}

/// **A call to somebody at another exchange is bridged, not rung here.**
///
/// A room exists only at the exchange it was made at, so a call placed
/// here for somebody who lives elsewhere rings, is answered, and carries
/// nothing — they joined a room of the same name at their own exchange.
/// That happened five times in one evening.
///
/// SIP-39's bridge is the mechanism, and it needs only a target: the far
/// exchange resolves the local part with `label.parse::<PubKey>()` before
/// it tries a name, so a peer with no name *here* is still reachable by
/// key. Insisting on a name was what fell back to the room.
///
/// The peer at this exchange is the control: nothing to bridge, and an
/// ordinary call is right.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_another_exchange_is_bridged_rather_than_rung_here() {
    for (what, peer_at, rings_here) in [
        ("peer elsewhere", "trunk.exchange", false),
        ("peer here", "squic.org", true),
    ] {
        let mut state = a_conversation();
        state.domain = Some("squic.org".into());
        state.peer_home = Some((PubKey::new([9u8; 32]), peer_at.to_string()));
        let (mut h, app) = harness_phone_with(state, sigil_chat::Route::Conversations);
        h.run();
        h.run();
        h.get_by_label("Call").click();
        h.run();

        let sent = app.borrow().sent_for_test().to_vec();
        let rang = sent.iter().any(|c| c.starts_with("Call"));
        assert_eq!(
            rang, rings_here,
            "{what}: ringing here was {rang}, wanted {rings_here}: {sent:?}"
        );
        if !rings_here {
            // The bridge was attempted. It cannot complete in a harness with
            // no live connection, and says so — which is the observable.
            let said = text_of(&h);
            assert!(
                said.contains("call to another exchange"),
                "{what}: it neither rang here nor tried to bridge: {said}"
            );
        }
    }
}

/// **The popup somebody picks a name from**, which nothing rendered until
/// now: the chip in a line has a snapshot, and the tree carries the name and
/// the key, so the marks beside the candidates went in against no picture at
/// all. Three of those have shipped this way. This is the picture.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn mention_picker_dark() {
    let mut h = harness_with(the_room(), true);
    h.run();
    hide_column(&mut h);
    let field = composer(&h);
    field.focus();
    field.type_text("@");
    h.run();
    h.run();
    h.snapshot("mention_picker_dark");
}

/// **The row names what will be said, and changes it** (SIP-47).
///
/// The setting is the person's, so it has to be reachable from the one
/// settings screen a phone has -- the desktop's platform pane is not one,
/// which is the mistake the call switch beside it was moved here to fix.
/// The row reads the preference back, so a choice that never reached the
/// preferences would leave the old words on screen.
#[test]
fn the_notifications_row_names_what_it_says_and_changes_it() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Me);
    h.run();
    scroll_to_the_foot(&mut h);
    h.get_by_label_contains("Notifications say everything")
        .click();
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("What notifications say"),
        "the dialog did not open: {said}"
    );
    h.get_by_label("only that something arrived").click();
    h.run();
    h.get_by_label("Done").click();
    h.run();
    h.run();
    let after = text_of(&h);
    assert!(
        after.contains("Notifications say that one arrived"),
        "the row reads the preference back: {after}"
    );
}

/// The three choices, looked at: a new dialog is a picture before it ships.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_notices_dialog() {
    let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "notices", them());
    h.run();
    h.run();
    h.remove_cursor();
    h.run();
    h.snapshot("phone_notices_dialog");
}

/// **The foot of the settings screen**: the two preferences that belong to
/// the person rather than to an identity, and the version under them.
///
/// A picture because the notifications row was added against none.
/// `me_card_phone` renders the head of this screen -- the mark, the name,
/// the domain -- and stops well above these.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_me_settings() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Me);
    h.run();
    scroll_to_the_foot(&mut h);
    h.remove_cursor();
    // Not `run`: this screen is still asking for repaints after the scroll,
    // and `run` gives up after four steps. The same four steps that caught
    // `phone_guardians_staged` on the way in.
    h.run_steps(2);
    h.snapshot("phone_me_settings");
}

/// Two things waiting, one opened: a stem with a mark, when it arrived, how
/// big it is, and the way to read or drop it.
fn a_mailbox() -> ChatState {
    let mut state = a_conversation();
    state.mail = vec![
        sigil_chat::session::MailItem {
            id: 1,
            from: them(),
            at: NOW - 3600,
            bytes: 4096,
            opened: None,
        },
        sigil_chat::session::MailItem {
            id: 2,
            from: PubKey::new([5u8; 32]),
            at: NOW - 7200,
            bytes: 99,
            opened: Some(sigil_chat::session::MailBody::Text(
                "left for you from the command line".into(),
            )),
        },
    ];
    state
}

/// **The mailbox, which nothing had ever rendered.** SIP-5's list was built
/// and checked by hand on a handset; no test drew it, so the one raw byte
/// count left in sigil sat there through a release.
#[test]
fn the_mailbox_says_a_size_not_a_byte_count() {
    let (mut h, app, _) = harness_phone_measured(a_mailbox(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "mail", them());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Messages left for you"), "{said}");
    assert!(
        said.contains("4 KiB"),
        "a size the way the rest of sigil writes one: {said}"
    );
    assert!(!said.contains("4096"), "and not a raw byte count: {said}");
    assert!(said.contains("99 B"), "{said}");
    assert!(
        said.contains("left for you from the command line"),
        "the opened one shows what it held: {said}"
    );
}

/// The picture of it, for the same reason: this screen had none.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_mailbox() {
    let (mut h, app, _) = harness_phone_measured(a_mailbox(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "mail", them());
    h.run();
    h.run();
    h.remove_cursor();
    h.run_steps(2);
    h.snapshot("phone_mailbox");
}

/// **A size, written the way the rest of sigil writes one.** The quota read
/// "4096 of 1048576 bytes used" — the only raw byte count left on a screen
/// here, while a file two panes away says "4 KiB" through the same function.
///
/// And nothing at all before the exchange has said what the quota is: both
/// numbers are zero then, and "0 B of 0 B used" answers nobody.
#[test]
fn the_backup_quota_is_a_size_not_a_byte_count() {
    let mut state = a_conversation();
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: None,
        used: 4096,
        quota: 1 << 20,
        words: None,
    });
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("4 KiB of 1.0 MiB used."), "{said}");
    assert!(!said.contains("1048576"), "a raw byte count: {said}");

    let mut state = a_conversation();
    state.backup = Some(sigil_chat::Backup {
        has_key: true,
        held: None,
        used: 0,
        quota: 0,
        words: None,
    });
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("0 B of"),
        "the quota line before the exchange has answered: {said}"
    );
}

/// Open one dialog on a phone and photograph it.
///
/// **Five of the ten had no picture at all**: the profile, adding an
/// exchange, claiming a name, and both SIP-53 moves — two of which were
/// added this month. `every_dialog_fits_a_phones_screen` measures all ten
/// now, but a dialog that fits is not a dialog that reads, and both defects
/// found by this month's audit were found by looking at one.
fn photograph_dialog(which: &str, name: &str) {
    let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), which, them());
    h.run();
    h.run();
    h.remove_cursor();
    // Not `run`: these carry text fields, and a focused caret repaints past
    // the four steps `run` allows.
    h.run_steps(2);
    h.snapshot(name);
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_profile() {
    photograph_dialog("profile", "phone_dialog_profile");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_exchange() {
    photograph_dialog("exchange", "phone_dialog_exchange");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_name() {
    photograph_dialog("name", "phone_dialog_name");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_rehome() {
    photograph_dialog("rehome", "phone_dialog_rehome");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn phone_dialog_movehome() {
    photograph_dialog("movehome", "phone_dialog_movehome");
}

/// **A caveat under a row of buttons says which button it is for**, and a
/// fallback label is not a place name. Both found by photographing dialogs
/// that had never been rendered.
#[test]
fn two_dialogs_say_which_thing_they_mean() {
    let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "name", them());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Giving it up stops you being reachable"),
        "the caveat names the button it belongs to: {said}"
    );
    assert!(
        !said.contains("Stop being reachable at this name."),
        "an instruction with no subject, under three buttons: {said}"
    );

    let (mut h, app, _) = harness_phone_measured(the_room(), sigil_chat::Route::Members);
    h.run();
    app.borrow_mut()
        .open_dialog_for_test((me(), String::new()), "rehome", them());
    h.run();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Ordered now by this identity's default exchange."),
        "{said}"
    );
    assert!(
        !said.contains("Ordered now by default."),
        "the bare word, where a place name belongs: {said}"
    );
}

/// **Both guardian lists name a guardian they know, and both keep the key.**
///
/// Written as one case over the two lists on purpose. They are the same
/// people either side of one press and they have now diverged twice: first a
/// bare key staged against a mark and a stem lodged, then a face and a name
/// lodged against a mark drawn from the key alone staged. A case that
/// checked one of them would have passed both times.
///
/// The key stays beside the name. Choosing who may take your account when
/// your key is gone is the last place to go on somebody's word for who they
/// are, and a name is a word (SIP-21) while a key is not.
#[test]
fn both_guardian_lists_name_somebody_they_know() {
    let called = "Grace Hopper";
    let mut state = a_conversation();
    state.people.insert(
        them(),
        sigil_chat::Person {
            name: Some(called.into()),
            title: None,
            handle: None,
            picture: None,
        },
    );
    state.succession = Some(sigil_chat::Succession {
        is_account: true,
        lodged: Some((1, vec![them()])),
        ..Default::default()
    });
    let (mut h, _, _) = harness_phone_measured(state, sigil_chat::Route::Devices);
    h.run();
    scroll_to_the_foot(&mut h);

    // Lodged: drawn from `succession.lodged`.
    let said = text_of(&h);
    assert!(
        said.contains(called),
        "a lodged guardian this window has a profile for is not named: {said}"
    );
    assert!(
        said.contains(&short_form(&them())),
        "and their key is gone from beside it: {said}"
    );
    // **Counted before, not assumed.** The accessibility tree repeats a
    // label for the widget and again for what contains it, so "appears
    // twice" is what *one* naming looks like. What the staged list adds is
    // the difference between this number and the next one.
    let lodged_only = said.matches(called).count();

    // Staged: the same person, one press earlier, from `pane.guardians`.
    stage_a_guardian(&mut h, &PubKey::new([5u8; 32]));
    stage_a_guardian(&mut h, &them());
    let said = text_of(&h);
    assert!(
        said.matches(called).count() > lodged_only,
        "the staged list does not name the same person the lodged list does \
         ({lodged_only} before staging, {} after): {said}",
        said.matches(called).count()
    );
    // And the one with no profile is still their key, which is the fallback
    // that has to keep working: a guardian is often somebody there is no
    // conversation with.
    assert!(
        said.contains(&short_form(&PubKey::new([5u8; 32]))),
        "a guardian with no profile lost their key too: {said}"
    );
    nothing_runs_off_the_edge(&h, "the Devices pane with named guardians");
}

/// **The successor's key, whole, on the card that says to write to it.**
///
/// The sentence shortens it, which is right in prose — and shortened was all
/// there was: no hover, no selection, no menu, nowhere on the card did the
/// whole key exist. Every other key in sigil is reachable in full, and this
/// is the one that most needs to be. The card says in its own words that the
/// exchange holds *their* word for who succeeded this account, so the only
/// way to know it is the right successor is to compare it against something
/// the person gave you somewhere else — and you cannot compare eleven
/// characters of it.
///
/// A phone cannot hover, so it has to be drawn rather than hidden behind
/// one.
#[test]
fn a_moved_account_shows_the_whole_successor_key() {
    let successor = PubKey::new([0x44u8; 32]);
    let mut state = a_conversation();
    state.succeeded.insert(them(), Some(successor));
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    h.run();

    let said = text_of(&h);
    assert!(
        said.contains("account is now"),
        "the moved card is not on screen, so this says nothing: {said}"
    );
    assert!(
        said.contains(&successor.to_string()),
        "the card names a successor to write to and never says which, in \
         full: {said}"
    );
    nothing_runs_off_the_edge(&h, "the conversation with a moved account");
}

/// **A long press on a conversation offers what can be done to it.**
///
/// Everything here was reachable only from inside the conversation, which is
/// the wrong place for "I do not want to look at this". egui turns a long
/// touch into a secondary click, so the same menu is a right-click on a
/// desktop and a long press on a phone.
#[test]
fn a_conversations_row_offers_a_menu() {
    // Nothing open: a phone draws the list or the conversation, never both,
    // and the list is what this is about.
    let mut state = a_conversation();
    state.open = None;
    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    let row = h
        .get_all(egui_kittest::kittest::by().label_contains("Ada"))
        .map(|n| n.rect())
        .next()
        .expect("Ada's row in the list");
    long_press(&mut h, row.center());
    h.run_steps(3);

    let said = labels(&h);
    for want in ["Mute", "Put away", "Leave", "Block"] {
        assert!(
            said.iter().any(|l| l == want),
            "the row's menu does not offer {want:?}: {said:?}"
        );
    }
    nothing_runs_off_the_edge(&h, "the conversation list with a row menu open");
}

/// **Put away means off the list, with a way back to it.**
///
/// Nothing is sent and the other party cannot tell — that is the whole
/// difference between this and leaving. So the only thing that can go wrong
/// is a conversation that cannot be found again, which is what the second
/// half of this is about.
///
/// Driven through the menu rather than by seeding the state, because the
/// press is the part that had never existed.
#[test]
fn a_conversation_put_away_leaves_the_list_and_can_be_found() {
    let mut state = a_conversation();
    state.open = None;
    // **Nothing waiting in the one being filed.** Something waiting brings a
    // conversation back on purpose, which is asserted where that rule lives
    // (`Filed::stays_away`); a fixture with unread messages would exercise
    // that rule here instead of this one, and the case would fail for a
    // reason that has nothing to do with the press.
    let label = {
        let c = state
            .conversations
            .iter_mut()
            .find(|c| c.peer.is_some())
            .expect("a direct message in the fixture");
        c.unread = 0;
        c.waiting = false;
        c.mentioned = 0;
        c.label.clone()
    };

    let mut h = harness_phone(state, sigil_chat::Route::Conversations);
    h.run();
    let row = h
        .get_all(egui_kittest::kittest::by().label_contains(&label))
        .map(|n| n.rect())
        .next()
        .expect("its row in the list");
    long_press(&mut h, row.center());
    h.run_steps(3);
    h.get_by_label("Put away").click();
    h.run_steps(3);

    let said = labels(&h);
    assert!(
        !said.contains(&label),
        "a conversation put away is still on the list: {said:?}"
    );
    // And the way back to it says how many are behind it.
    assert!(
        said.iter().any(|l| l.starts_with("Put away (")),
        "nothing leads to what was put away: {said:?}"
    );
    h.get_by_label_contains("Put away (").click();
    h.run_steps(3);
    let said = labels(&h);
    assert!(
        said.contains(&label),
        "the way back leads nowhere: {said:?}"
    );
}

/// **The head of a conversation carries a face, with the presence on it.**
///
/// It drew a bare dot and a name, so the heading of the one screen somebody
/// spends their time on was the last place in sigil where a person appeared
/// without a face — while their row in the list, every bubble under it, the
/// ring and the call card all had one. And the dot alone was a coloured
/// circle belonging to nothing; on the corner of a mark it is what it was
/// always for.
///
/// Asserted by geometry rather than by a label, because a mark has no words:
/// the presence carries the only name in the header that belongs to the
/// mark, and where that name's rectangle *is* says whether a mark was drawn
/// and where.
#[test]
fn the_conversation_head_carries_a_mark() {
    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Conversations);
    h.run();
    h.run();

    let mark = h
        .get_all(egui_kittest::kittest::by().label_contains("offline"))
        .map(|n| n.rect())
        .next()
        .expect("the other party's presence, which the mark carries");
    // Square, and the size a mark is — not a dot, which is a third of it.
    assert!(
        (mark.width() - mark.height()).abs() < 2.0,
        "the presence is not on a square mark: {mark:?}"
    );
    assert!(
        mark.width() >= sigil::tokens::AVATAR_SM - 1.0,
        "the presence is on something {} wide, smaller than a mark ({})",
        mark.width(),
        sigil::tokens::AVATAR_SM
    );
    // And the name reads after it, as it does on every row in the list.
    let name = h
        .get_all(egui_kittest::kittest::by().label_contains("Ada"))
        .map(|n| n.rect())
        .find(|r| (r.center().y - mark.center().y).abs() < mark.height())
        .expect("the conversation's name beside it");
    assert!(
        mark.right() <= name.left() + 1.0,
        "the mark is not before the name: mark {mark:?}, name {name:?}"
    );
}

/// **The press that cannot be taken back is the one that is coloured.**
///
/// Sigil says "this ends the conversation for everybody in it and cannot be
/// undone" in the destructive colour, and then drew `Yes, destroy it` in
/// exactly the same grey as the `Cancel` beside it — so the row where the
/// decision is actually made was the one row that did not say which of the
/// two was which.
///
/// Read off the paint list rather than the tree, because a colour has no
/// label: the words are laid into a galley with their colour on them, and
/// that is the only place it exists.
#[test]
fn a_confirmation_colours_the_grave_half_and_not_the_other() {
    fn coloured(h: &mut Harness<'static>, word: &str) -> Option<egui::Color32> {
        fn walk(shape: &egui::Shape, word: &str, out: &mut Option<egui::Color32>) {
            match shape {
                egui::Shape::Text(text) => {
                    if text.galley.job.text.contains(word) {
                        *out = text
                            .galley
                            .job
                            .sections
                            .first()
                            .map(|s| s.format.color)
                            .or(Some(text.fallback_color));
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        walk(s, word, out);
                    }
                }
                _ => {}
            }
        }
        h.run();
        let mut found = None;
        for shape in &h.output().shapes {
            walk(&shape.shape, word, &mut found);
        }
        found
    }

    let mut h = harness_phone(a_conversation(), sigil_chat::Route::Settings);
    h.run();
    h.get_by_label("Destroy this conversation").click();
    h.run_steps(3);

    let theme = sigil::theme::dark();
    let grave = coloured(&mut h, "Yes, destroy it").expect("the confirming press is on screen");
    assert_eq!(
        grave, theme.destructive,
        "the press that ends a conversation for everybody is not coloured as one"
    );
    let safe = coloured(&mut h, "Cancel").expect("the way out beside it");
    assert_ne!(
        safe, theme.destructive,
        "the way out is coloured as though it were the grave one"
    );
}
