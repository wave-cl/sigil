//! What a conversation looks like with messages in it.
//!
//! The data is fixed rather than fetched — see `ChatApp::show_state_for_test`.
//! Everything about *how* a message is drawn is the production path; only where
//! the messages came from is different, and `chat_session.rs` covers that
//! against a real exchange.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, theme};
use sigil_chat::{Attached, ChatApp, ChatState, Line, LinkState, Member, Person, Receipt, Summary};
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
        divider: Some(3),
        unread_on_open: 2,
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
    // Our own key is on screen in full, in the pane about us.
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
