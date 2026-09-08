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
                public: false,
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
                public: true,
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
    h.snapshot("transcript_dark");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn transcript_light() {
    let mut h = harness(false);
    h.run();
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
