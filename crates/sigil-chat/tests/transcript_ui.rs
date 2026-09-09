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
    Attached, ChatApp, ChatState, Happened, Line, LinkState, Member, Person, Receipt, Summary,
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
        conversations: vec![
            Summary {
                channel,
                peer: Some(them()),
                label: "Ada".into(),
                unread: 2,
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
                    preview: Vec::new(),
                    bytes: None,
                    missing: false,
                    id: "abc123".into(),
                }],
                standing: Default::default(),
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
                reply_to: Some(("me".into(), "mine, on the other side".into())),
                receipt: None,
                attachments: Vec::new(),
                standing: Default::default(),
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
                hidden: false,
                notify: &sigil::Silent,
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
                hidden: false,
                notify: &sigil::Silent,
            };
            let _ = app.render(&mut app_ctx, ui);
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
                hidden: false,
                notify: &sigil::Silent,
            };
            if let Some(action) = app.render(&mut app_ctx, ui).action {
                asks.borrow_mut().push(action);
            }
        })
}

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
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                hidden: false,
                notify: &sigil::Silent,
            };
            let _ = app.render(&mut app_ctx, ui);
        })
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
                        hidden: false,
                        notify: &sigil::Silent,
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
    // The rightmost match, because our own text appears twice -- once as the
    // bubble and once quoted inside the reply below it.
    let mine = h
        .get_all_by_label_contains("mine, on the other side")
        .map(|n| n.rect())
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
                hidden: false,
                notify: &sigil::Silent,
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
                hidden: false,
                notify: &sigil::Silent,
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
    state.lines[n - 1].reply_to = Some(("Ada".into(), "the second one, then".into()));
    state.lines[n - 1].attachments = vec![Attached {
        kind: 0x04,
        described: "[notes.txt, 2.1 kB]".into(),
        size: 2100,
        preview: Vec::new(),
        bytes: None,
        missing: false,
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
    // The hover text carries the key; what matters here is that the interface
    // never *replaces* the key with a name it was handed. A name is an
    // assertion (SIP-21) and the key is the only identity.
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
    // Stepped: a spinner asks for another frame for ever, and `run` waits for
    // the interface to go still.
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
    open_identity(&mut h);
    assert!(text_of(&h).contains("indra.org"), "{}", text_of(&h));

    h.get_by_label("Remove").click();
    h.run();
    open_identity(&mut h);
    assert!(
        !text_of(&h).contains("indra.org"),
        "the exchange is still there: {}",
        text_of(&h)
    );
}

/// A message's controls sit beside it, on the side it has room on.
///
/// Under the bubble they pushed everything below them down as the pointer
/// moved along the transcript, so reading with the mouse anywhere near it made
/// the whole conversation twitch.
#[test]
fn the_controls_are_beside_the_message_and_on_its_free_side() {
    let mut h = harness(true);
    h.run();
    hide_column(&mut h);
    // One of theirs, which sits on the left: the controls belong to its right.
    let bubble = h.get_by_label_contains("the second one, then").rect();
    h.get_by_label_contains("the second one, then").hover();
    h.step();
    h.step();
    let reply = h.get_by_label("Reply").rect();
    assert!(
        reply.left() >= bubble.right(),
        "somebody else's message keeps its controls on the right: \
         {reply:?} against {bubble:?}"
    );
    // And beside it, not under it: the two share vertical space. Overlap
    // rather than containment, because the controls are aligned to the top of
    // the bubble and this measures one line of its text.
    assert!(
        reply.bottom() > bubble.top() && reply.top() < bubble.bottom(),
        "the controls are below the message: {reply:?} against {bubble:?}"
    );
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
    let reply = h.get_by_label("Reply").rect();
    assert!(
        reply.right() <= bubble.left(),
        "one's own message keeps its controls on the right, where the bubble is: \
         {reply:?} against {bubble:?}"
    );
}

/// The controls sit against the middle of the message, not its top edge.
#[test]
fn the_controls_are_vertically_centred_on_the_message() {
    // **One's own message**, which is the side with an alignment of its own:
    // the other side is a plain `horizontal`, which centres already, so a test
    // that hovered one of theirs would pass whatever this branch did.
    let mut state = a_conversation();
    let n = state.lines.len();
    state.lines[n - 1].redacted = false;
    state.lines[n - 1].mine = true;
    state.lines[n - 1].text = "the first line\nand a second one\nand a third".into();
    let mut h = harness_with(state, true);
    h.run();

    let bubble = h.get_by_label_contains("and a third").rect();
    h.get_by_label_contains("and a third").hover();
    h.step();
    h.step();
    let reply = h.get_by_label("Reply").rect();
    // **The controls begin no higher than the words do.** Top-aligned they
    // begin at the frame's padding, above the first line; centred on a bubble
    // of three lines they begin well down it. Stated against the text's own
    // rect because the frame's is not in the accessibility tree.
    assert!(
        reply.top() >= bubble.top(),
        "the controls are pinned to the top edge: {reply:?} against {bubble:?}"
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
    h.get_by_label("React").click();
    h.step();
    // An emoji the picker offers and **this conversation does not already
    // carry**: the fixture has a `👍 2` chip on another message, so looking
    // for a thumb finds one whether or not the picker ever opened.
    let picker = '\u{1f389}';
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

/// One exchange is not a choice, so there is nothing to switch between.
#[test]
fn a_single_exchange_offers_no_switcher() {
    let mut h = harness_at_exchanges(a_conversation(), &[]);
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    assert!(!said.contains("indra.org"), "{said}");
    // But adding one is always offered.
    assert!(said.contains("Add an exchange"), "{said}");
}

/// A second exchange is offered as somewhere to switch to.
#[test]
fn a_second_exchange_appears_as_somewhere_to_switch_to() {
    let mut h = harness_at_exchanges(a_conversation(), &["indra.org"]);
    h.run();
    open_identity(&mut h);
    let said = text_of(&h);
    assert!(
        said.contains("indra.org"),
        "the added exchange is offered: {said}"
    );
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
    // The panic was here, drawing the "Replying to" bar.
    h.run();
    assert!(text_of(&h).contains("Replying to"), "{}", text_of(&h));
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
    let said = text_of(&h);
    assert!(said.contains("Reply"), "hovering offers a reply: {said}");
    assert!(said.contains("React"), "and a reaction: {said}");
    assert!(
        said.contains("More"),
        "and the rest, behind one more control: {said}"
    );
}
