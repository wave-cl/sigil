//! A call the window is carrying, and how it stops.
//!
//! # The bug this is here for
//!
//! A call holds the microphone, and the only thing that let go of it was a
//! button — drawn from the identity **on screen**. Answer as one identity, look
//! at another, and the bar went away with the only control that ends a call.
//! The audio went on being captured and sent to a room, with nothing in the
//! window admitting it; quitting sigil was the way out, and the microphone
//! light was the only sign. Found by hand, on a real call, which is the only
//! way it could have been found.

use std::path::PathBuf;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use egui_kittest::kittest::NodeT;
use sigil::accounts::Accounts;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, Silent};
use sigil_chat::ChatApp;
use sqnr_core::{PubKey, SoftwareSigner};

fn pass(app: &mut ChatApp, accounts: &mut Accounts, egui_ctx: &egui::Context) {
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts,
        unfocused: true,
        notify: &Silent,
        connections: &Default::default(),
    };
    app.update(&mut ctx, egui_ctx);
}

fn app_at(root: PathBuf) -> ChatApp {
    let mut app = ChatApp::new();
    app.set_store_root_for_test(root);
    app.set_exchange_for_test("127.0.0.1:1", &PubKey::new([7u8; 32]).to_string());
    app
}

/// A call that has ended is let go of, without anybody pressing anything.
///
/// The handle is a real call to an address nothing is listening on, so it fails
/// and reports `Ended` — the same state a call reaches when the far end leaves,
/// the room goes, or the connection does. Until this was reaped, such a call
/// stayed in the window's hands: the bar said "Connecting…" for ever, and the
/// task behind it held the microphone.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_has_ended_is_not_still_held() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);

    // A call that cannot succeed: nothing answers on port 1.
    let handle = sigil_net::spawn_call(
        sigil_net::Endpoint {
            address: "127.0.0.1:1".parse().unwrap(),
            server: PubKey::new([7u8; 32]),
        },
        SoftwareSigner::new(SigningKey::from_bytes(&[1u8; 32])),
        PubKey::new([2u8; 32]),
        1,
        Default::default(),
        || {},
    );
    app.hold_call_for_test(me, [3u8; 32], 9, handle);
    assert_eq!(
        app.calls_for_test(),
        vec![me],
        "the window should be carrying the call it was just handed"
    );

    // No button is pressed here, and none is drawn: `update` is what a window
    // does whether or not anybody is looking at this app.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        pass(&mut app, &mut accounts, &egui_ctx);
        if app.calls_for_test().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        app.calls_for_test().is_empty(),
        "a call that ended is still being held, and whatever it was doing to \
         the microphone it is still doing"
    );
}

/// A call is visible from wherever the reader is, and says whose it is.
///
/// The bar used to be drawn inside the conversation view from the identity on
/// screen, so it was hidden by going to the list, by a narrow window showing
/// the other pane, and above all by switching identity — which is exactly what
/// somebody does after answering as one of their identities. The control that
/// ends the call went with it.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_is_shown_even_while_looking_at_another_identity() {
    use egui_kittest::Harness;

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    // Two identities, and the call belongs to the one *not* being shown.
    let shown = Account::unlocked_for_test([1u8; 32]);
    let other = Account::unlocked_for_test([2u8; 32]);
    let elsewhere = other.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![shown, other]);
    pass(&mut app, &mut accounts, &egui_ctx);

    let handle = sigil_net::spawn_call(
        sigil_net::Endpoint {
            address: "127.0.0.1:1".parse().unwrap(),
            server: PubKey::new([7u8; 32]),
        },
        SoftwareSigner::new(SigningKey::from_bytes(&[2u8; 32])),
        PubKey::new([3u8; 32]),
        60,
        Default::default(),
        || {},
    );
    app.hold_call_for_test(elsewhere, [3u8; 32], 9, handle);

    // Rendered as the *first* identity: the one with no call.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            egui::CentralPanel::default().show(ui, |ui| {
                let mut nav = Navigator::default();
                let mut app_ctx = AppContext {
                    navigator: &mut nav,
                    accounts: &mut accounts,
                    unfocused: false,
                    notify: &Silent,
                    connections: &Default::default(),
                };
                let _ = app.render(&mut app_ctx, ui);
            });
        });
    // `run` and not `run_steps` would never settle: the conversation list is
    // still loading and its mark asks for another frame for ever. See
    // `sigil_ui::working`.
    harness.run_steps(3);

    // Everything on screen, gathered rather than queried: a `get_*` query with
    // no match panics, which is the wrong shape for asking whether something
    // is there at all.
    let said = labels(&harness);
    // Either wording will do -- "In a call as X" once audio is flowing,
    // "Connecting… as X" before that. What must not happen is neither.
    assert!(
        said.iter()
            .any(|l| l.contains("In a call") || l.contains("Connecting")),
        "the call is nowhere on screen: a microphone is open and the window \
         says nothing about it. What it does say: {said:?}"
    );
    // And it says whose, because a hang-up that ends somebody else's call has
    // to name them.
    let whose = elsewhere.to_string();
    assert!(
        said.iter().any(|l| l.contains(&whose)),
        "the call does not say which identity is in it: {said:?}"
    );
}

/// Every piece of text on screen, labels and values alike.
///
/// A plain `Label` carries its text as the node's *value* and has no label at
/// all, so collecting only labels finds no ordinary text — and an assertion
/// that something is present then fails for the wrong reason.
fn labels(h: &egui_kittest::Harness<'static>) -> Vec<String> {
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
    let mut out = Vec::new();
    walk(h.root(), &mut out);
    out
}
