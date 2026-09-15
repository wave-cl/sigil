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
use sigil::{Account, theme};
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

fn a_conversation() -> ChatState {
    let channel = [9u8; 32];
    ChatState {
        me: Some(me()),
        exchange: Some(PubKey::new([3u8; 32])),
        domain: Some("squic.org".into()),
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
                waiting: false,
                preview: Some("anybody may join this one".into()),
                at: Some(NOW - 2 * DAY),
                public: Some(true),
                group: true,
                typing: false,
            },
        ],
        open: Some(channel),
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
                reactions: Vec::new(),
                reply_to: None,
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
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
                reactions: vec![("\u{1f44d}".to_string(), 2, true)],
                reply_to: None,
                receipt: Some(Receipt::Read),
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
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
                    id: "abc123".into(),
                }],
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
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
                reactions: Vec::new(),
                reply_to: None,
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
                mentions: Vec::new(),
                me_mentioned: false,
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
            },
        )]
        .into_iter()
        .collect(),
        mine: Person {
            name: Some("me".into()),
            title: None,
            handle: Some("me@squic.org".into()),
        },
        found: Vec::new(),
        searched: false,
        note: None,
        members: vec![
            Member {
                account: me(),
                admin: true,
            },
            Member {
                account: them(),
                admin: false,
            },
        ],
        i_am_admin: true,
        topic: String::new(),
        ringing: Vec::new(),
        mentions: Vec::new(),
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
        }],
        // The whole conversation, so the paging control is out of the way of
        // everything else here. `a_paged_conversation` is what covers it.
        earlier: 0,
        locked_out: None,
    }
}

fn harness(dark: bool) -> Harness<'static> {
    harness_with(a_conversation(), dark)
}

/// A harness showing one of the app's inner routes, through `render_nav` —
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
    let mut app = ChatApp::new();
    app.set_now_for_test(NOW);
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    for name in extra {
        assert!(accounts.add_exchange(0, name));
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
                                notify: &sigil::Silent,
                                connections: &Default::default(),
                            };
                            app.chrome_ui(&mut app_ctx, ui);
                        },
                    );
                });
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
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
                        notify: &sigil::Silent,
                        connections: &Default::default(),
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
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
    assert!(
        asked.borrow().is_empty(),
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
            reactions: Vec::new(),
            reply_to: None,
            receipt: None,
            attachments: Vec::new(),
            standing: Default::default(),
            mentions: Vec::new(),
            me_mentioned: false,
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
    let mut h = harness(true);
    h.run();
    h.snapshot("list_dark");
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
/// second line in a corner block and pushed it down rather than out.
#[test]
fn having_no_name_offers_the_way_to_claim_one() {
    let mut state = a_conversation();
    state.mine.handle = None;
    let mut h = harness_with(state, true);
    h.run();
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
    assert!(
        mark.left() >= time.right(),
        "the receipt is before the time: receipt {mark:?}, time {time:?}"
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
    let mut h = harness_at(state, sigil_chat::Route::Devices);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("only copy"),
        "the warning has to say the store is unrecoverable: {said}"
    );
    assert!(
        said.contains("Link a second device"),
        "and what to do about it: {said}"
    );
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

/// The exchange a conversation list belongs to is on screen, in full.
///
/// It is the key a receipt verifies under, and the one a client must pin
/// independently of whatever it is connected to. A name for it is not enough.
#[test]
fn the_exchange_this_list_belongs_to_is_shown_in_full() {
    let mut h = harness(true);
    h.run();
    open_identity(&mut h);
    let key = PubKey::new([3u8; 32]).to_string();
    assert!(
        text_of(&h).contains(&key),
        "the exchange's key belongs on screen: {}",
        text_of(&h)
    );
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
/// The only way in was a menu item two clicks behind a chevron, which is a
/// long way for the one control that fixes what the header is saying.
#[test]
fn clicking_your_own_name_opens_your_profile() {
    let mut h = harness_with(a_conversation(), true);
    h.run();
    assert!(
        !text_of(&h).contains("Your profile"),
        "the profile is open before anybody asked for it"
    );

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
    h.get_by_label("me").click();
    h.run();
    // The field carries the published name as its value, so the name appears
    // twice on screen: once in the header, once in the box.
    let said = text_of(&h);
    assert!(
        said.matches("me").count() > 1,
        "the profile opened without the name it is meant to be editing: {said}"
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

    let said = text_of(&h);
    assert!(
        said.contains(&short_form(&me())),
        "nothing on screen names this identity at all: {said}"
    );
    assert!(
        !said.contains(&me().to_string()),
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
        text: "the thing that was said".into(),
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

    // **The hit's own channel, not any `Show` at all.** With nothing open the
    // app opens the latest conversation by itself on the first pass, so
    // `starts_with("Show")` passed with the click six hundred pixels off the
    // row -- a test of the fixture rather than of the press.
    let wanted = format!("Show({:?})", [8u8; 32]);
    assert!(
        asked.borrow().contains(&wanted),
        "pressing a search result did not open it: {:?}",
        asked.borrow()
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
        said.contains("Channel settings"),
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
        reactions: vec![],
        reply_to: None,
        receipt: Some(Receipt::Read),
        attachments: Vec::new(),
        standing: Default::default(),
        mentions: Vec::new(),
        me_mentioned: false,
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
        reactions: vec![],
        reply_to: None,
        receipt: Some(Receipt::Read),
        attachments: Vec::new(),
        standing: Default::default(),
        mentions: Vec::new(),
        me_mentioned: false,
    });
    let mut h = harness_with(state, true);
    h.run();

    let marks: Vec<egui::Rect> = h.get_all_by_label("read").map(|n| n.rect()).collect();
    assert!(
        marks.len() >= 3,
        "three of our messages carry a receipt: {marks:?}"
    );
    let rightmost = marks.iter().map(|r| r.right()).fold(f32::MIN, f32::max);
    for m in &marks {
        assert!(
            (m.right() - rightmost).abs() < 1.0,
            "a receipt is short of the bubble's right edge: {m:?}, against \
             {rightmost} for the others"
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

    // The quote, which reads "Ada: message 4".
    h.get_by_label_contains("Ada: message 4").click();
    for _ in 0..6 {
        h.run();
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
    for _ in 0..8 {
        h.run();
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

/// "Edit your profile" sits beside "Switch identity", in the same shape.
///
/// It was a plain button on its own, between your key and the exchange --
/// among the facts about the identity rather than the things done to it.
/// Now it is the row above the other action, and drawn the same way: full
/// width, with an icon.
#[test]
fn editing_your_profile_is_beside_switching_identity_and_shaped_like_it() {
    let mut h = harness(true);
    h.run();
    open_identity(&mut h);

    let edit = h.get_by_label("Edit your profile").rect();
    let switch = h.get_by_label("Switch identity").rect();
    assert!(
        (edit.left() - switch.left()).abs() < 1.0 && (edit.width() - switch.width()).abs() < 1.0,
        "the two are not the same shape: edit {edit:?}, switch {switch:?}"
    );
    assert!(
        edit.bottom() <= switch.top() && switch.top() - edit.bottom() < 12.0,
        "the two are not beside each other: edit {edit:?}, switch {switch:?}"
    );
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
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
            *recorder.borrow_mut() = app.asked_for_test().to_vec();
        });
    h.run();
    h.get_by_label("Send").click();
    h.run();
    h.run();
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
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    // The pictures are decoded on egui's loader thread; give it a moment.
    for _ in 0..20 {
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
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
        ("\u{1f389}".to_string(), 3, true),
        ("\u{1f44d}".to_string(), 1, false),
    ];
    state.lines[n - 2].reactions = vec![("\u{2764}".to_string(), 1, true)];
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
    h.run();
    h.run();
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

    // Sent again, and taken this time: nothing comes back.
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
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
    state.lines[n - 2].reactions = vec![("\u{2764}".to_string(), 1, true)];
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
