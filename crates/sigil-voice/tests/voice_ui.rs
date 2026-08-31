//! What the voice app looks like in each of the states a person meets.
//!
//! Snapshots are `#[ignore]`d so an ordinary `cargo test` needs no GPU; the
//! accessibility assertions below run everywhere, because what the interface
//! *says* matters more than what it looks like and is cheaper to check.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::account::Account;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::theme;
use sigil_voice::VoiceApp;

/// Drive the app with a given account, as the shell would.
fn harness(account: Account, dark: bool) -> Harness<'static> {
    let mut app = VoiceApp::new();
    let mut account = account;
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            // A panel, as the shell gives it: filling the window, with the
            // shell's own margin. Rendering straight into the root Ui would
            // snapshot a layout nobody ever sees.
            let theme = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        account: &mut account,
                        hidden: false,
                        notify: &sigil::Silent,
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

/// Everything the interface says, as one string.
///
/// Both `label` and `value`, because accesskit puts them in different places:
/// an interactive widget carries its text as a label, a plain one carries it as
/// a value. Reading only labels sees buttons and no prose.
fn text_of(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

fn unlocked_account(dir: &std::path::Path) -> Account {
    let path = dir.join("identity");
    sqnr::identity::generate(&path, None).unwrap();
    Account::discover(Some(path))
}

/// A sealed identity must ask for a passphrase, in the window, with no terminal
/// anywhere. This is the whole reason `Account` is a state machine.
#[test]
fn a_sealed_identity_asks_in_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, Some("open sesame")).unwrap();

    let mut h = harness(Account::discover(Some(path)), true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("passphrase"), "it asks: {said}");
    assert!(
        said.contains("Unlock"),
        "and gives you a way to answer: {said}"
    );
}

/// Somebody with no identity should be told what to run, and why a YubiKey is
/// not an option -- that exclusion is the protocol's, not an oversight, and
/// saying so here is kinder than a failure four steps later.
#[test]
fn a_missing_identity_says_what_to_do_and_why_a_card_will_not_work() {
    let mut h = harness(
        Account::Missing {
            path: "/nowhere/identity".into(),
        },
        true,
    );
    h.run();
    let said = text_of(&h);
    assert!(said.contains("sqnr keygen"), "{said}");
    assert!(said.contains("YubiKey"), "{said}");
}

/// Unlocked and idle: your own key in full, and somewhere to put theirs.
#[test]
fn an_unlocked_identity_offers_a_call() {
    let dir = tempfile::tempdir().unwrap();
    let account = unlocked_account(dir.path());
    let me = account.unlocked().unwrap().me().to_string();

    let mut h = harness(account, true);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Call"), "there is a way to place one: {said}");
    assert!(
        said.contains(&me),
        "your own key is shown in full, not abbreviated away (SIP-21)"
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn voice_locked_dark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, Some("open sesame")).unwrap();
    let mut h = harness(Account::discover(Some(path)), true);
    h.run();
    h.snapshot("voice_locked_dark");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn voice_idle_dark() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked_account(dir.path()), true);
    h.run();
    h.snapshot("voice_idle_dark");
}

/// A room secret on screen must always carry what it means. This is the
/// security model, and it is not what a group chat trains people to expect:
/// there is no owner, nobody can be removed, and anyone given it can pass it
/// on. `/kick` in chat removes somebody and rotates the key; a room cannot, and
/// the two must never look like the same control.
#[test]
fn a_room_secret_always_says_it_cannot_be_taken_back() {
    let dir = tempfile::tempdir().unwrap();
    let account = unlocked_account(dir.path());
    let mut h = harness(account, true);
    h.run();

    // Before minting, there is a way in and the caveat is not yet shouted.
    let before = text_of(&h);
    assert!(
        before.contains("Join"),
        "there is a way into a room: {before}"
    );
    assert!(
        before.contains("New room"),
        "and a way to mint one: {before}"
    );

    // Mint one, and the warning must appear alongside it.
    h.get_by_label("New room").click();
    h.run();
    let after = text_of(&h);
    assert!(
        after.contains("cannot be taken back"),
        "a secret on screen says what holding it means: {after}"
    );
    assert!(
        after.contains("mint a new room"),
        "and what to do instead of removing somebody: {after}"
    );
}

/// Minting puts a real secret in the field -- one that parses back as a room.
#[test]
fn minting_a_room_produces_a_usable_secret() {
    let dir = tempfile::tempdir().unwrap();
    let account = unlocked_account(dir.path());
    let mut h = harness(account, true);
    h.run();
    h.get_by_label("New room").click();
    h.run();

    let said = text_of(&h);
    // The field's contents reach the tree as a value; find the one that parses.
    let minted = said
        .split(" | ")
        .find(|s| s.parse::<sigil_net::RoomId>().is_ok())
        .unwrap_or_else(|| panic!("no parseable room secret on screen: {said}"));
    // A length *floor*, not an equality. base58 is not fixed-width, and 32
    // random bytes are shorter than the maximum more often than one would
    // guess. Measured over 20,000 values: 44 chars 94.4% of the time, 43 chars
    // 5.5%, and 41-42 about one time in a thousand.
    //
    // `assert_eq!(len, 44)` therefore failed roughly one run in eighteen, and I
    // put the first such failure down to a stale build without looking. A
    // corrected guess of `43..=44` would still have failed one run in a
    // thousand -- which is the worse bug, because it comes back only when
    // somebody else is watching.
    //
    // The parse above is the real check. This only catches a truncation.
    assert!(
        minted.len() >= 40,
        "a room secret should not be this short ({} chars): {minted}",
        minted.len()
    );
}

/// Drive the app with an account *and* a ring already arrived.
fn ringing_harness(account: Account, from: sqnr_core::PubKey) -> Harness<'static> {
    let mut app = VoiceApp::new();
    app.ring_for_test(from);
    let mut account = account;
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
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
                        account: &mut account,
                        hidden: false,
                        notify: &sigil::Silent,
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

fn a_key() -> sqnr_core::PubKey {
    let sk = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    sqnr_core::PubKey::new(sk.verifying_key().to_bytes())
}

/// A ring shows the caller's key in full and does not claim it is proven.
///
/// Who a ring says it is from is the exchange's observation of who connected,
/// not a signature. An interface that presented that as an established identity
/// would be telling a lie on the protocol's behalf.
#[test]
fn a_ring_shows_the_key_and_does_not_claim_it_is_proven() {
    let dir = tempfile::tempdir().unwrap();
    let caller = a_key();
    let mut h = ringing_harness(unlocked_account(dir.path()), caller);
    h.run();
    let said = text_of(&h);

    assert!(said.contains("Incoming call"), "{said}");
    assert!(
        said.contains(&caller.to_string()),
        "the key, in full: {said}"
    );
    assert!(said.contains("Answer"), "{said}");
    assert!(said.contains("Decline"), "{said}");
    assert!(
        said.contains("not a signature"),
        "it says what the caller's name is worth: {said}"
    );
}

/// Declining is silent, and the call lands in Missed rather than vanishing.
/// There is no way to say "no" to a caller without telling them you are there,
/// and somebody who does not want to be reached should not have to announce it.
#[test]
fn declining_is_silent_and_the_call_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let caller = a_key();
    let mut h = ringing_harness(unlocked_account(dir.path()), caller);
    h.run();
    h.get_by_label("Decline").click();
    h.run();

    let said = text_of(&h);
    assert!(!said.contains("Incoming call"), "the ring is gone: {said}");
    assert!(said.contains("Missed"), "and remembered: {said}");
    assert!(
        said.contains(&caller.to_string()),
        "with who it was: {said}"
    );
    assert!(
        said.contains("Call back"),
        "and a way to answer it late: {said}"
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn voice_ringing_dark() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = ringing_harness(unlocked_account(dir.path()), a_key());
    h.run();
    h.snapshot("voice_ringing_dark");
}

/// The listener must start on its own, from `update`, as soon as there is an
/// identity to listen as.
///
/// This exists because it once did not: it was wired into `render` rather than
/// `update`, so it never ran, and every ringing test passed anyway because they
/// inject a ring rather than receive one. Nothing observed the listener, so
/// nothing noticed that the phone could not ring.
/// A `tokio::test` because `listen` spawns, and spawning without a runtime
/// panics. That is the right behaviour — the shell enters a runtime for the
/// life of the process — but it means this cannot be a plain `#[test]`.
#[tokio::test]
async fn the_listener_starts_by_itself_once_the_identity_is_open() {
    use sigil::app::App;

    let dir = tempfile::tempdir().unwrap();
    let mut account = unlocked_account(dir.path());
    let ctx = egui::Context::default();

    let mut app = VoiceApp::new();
    app.set_exchange_for_test("127.0.0.1:1", "1111111111111111111111111111111111111111111");
    assert!(
        !app.listening_for_test(),
        "nothing is listening before the first pass"
    );

    let mut nav = Navigator::default();
    let mut app_ctx = AppContext {
        navigator: &mut nav,
        account: &mut account,
        hidden: false,
        notify: &sigil::Silent,
    };
    app.update(&mut app_ctx, &ctx);
    assert!(
        app.listening_for_test(),
        "one pass with an open identity is enough to start listening"
    );
}

/// And it must not start without one: there is nothing to listen *as*, and
/// trying would ask for a signer that does not exist.
#[tokio::test]
async fn nothing_listens_while_the_identity_is_sealed() {
    use sigil::app::App;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, Some("open sesame")).unwrap();
    let mut account = Account::discover(Some(path));
    let ctx = egui::Context::default();

    let mut app = VoiceApp::new();
    app.set_exchange_for_test("127.0.0.1:1", "1111111111111111111111111111111111111111111");
    let mut nav = Navigator::default();
    let mut app_ctx = AppContext {
        navigator: &mut nav,
        account: &mut account,
        hidden: false,
        notify: &sigil::Silent,
    };
    app.update(&mut app_ctx, &ctx);
    assert!(!app.listening_for_test());
}

/// A ring is said out loud, not only drawn.
///
/// Without this a call reaches only somebody already looking at the window,
/// which is a dialler rather than a telephone — and it is the reason the whole
/// desktop-integration crate exists.
#[tokio::test]
async fn an_arriving_ring_is_announced() {
    use sigil::app::{App, Notify};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Heard(Mutex<Vec<(String, String)>>);
    impl Notify for Heard {
        fn post(&self, summary: &str, body: &str) -> bool {
            self.0.lock().unwrap().push((summary.into(), body.into()));
            true
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let mut account = unlocked_account(dir.path());
    let ctx = egui::Context::default();
    let heard = Heard::default();
    let caller = a_key();

    let mut app = VoiceApp::new();
    // A ring that arrived from the listener, as `update` would see it.
    app.deliver_ring_for_test(caller);

    let mut nav = Navigator::default();
    let mut app_ctx = AppContext {
        navigator: &mut nav,
        account: &mut account,
        hidden: true,
        notify: &heard,
    };
    app.update(&mut app_ctx, &ctx);

    let said = heard.0.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "one ring, said once: {said:?}");
    assert_eq!(said[0].0, "Incoming call");
    assert!(
        said[0].1.contains(&caller.to_string()),
        "and names the caller in full: {:?}",
        said[0].1
    );
}
