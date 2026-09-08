//! Switching identity must stop the session for the identity left behind.
//!
//! # Why this test is written the way it is
//!
//! The failure it guards against is **silent**. `ChatApp` used to start its
//! session behind `if self.session.is_some() { return }` and then never look
//! again, so changing identity left the previous key's session running:
//! connected, polling, succeeding, and the wrong person. Nothing errors, no
//! assertion about the visible interface fails, and the only symptom is that
//! messages go out as somebody else.
//!
//! So the assertions here are about **which keys have a session**, not about
//! anything drawn. A test that checked the interface would have passed
//! throughout the whole time the bug existed.

use std::path::PathBuf;

use sigil::accounts::Accounts;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, Silent};
use sigil_chat::ChatApp;
use sqnr_core::PubKey;

/// Run one `update` pass against this roster.
fn pass(app: &mut ChatApp, accounts: &mut Accounts, egui_ctx: &egui::Context) {
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts,
        hidden: true,
        notify: &Silent,
    };
    app.update(&mut ctx, egui_ctx);
}

fn key_of(account: &Account) -> PubKey {
    account.unlocked().expect("an open account").me()
}

/// An app pointed at a throwaway store and an exchange that will never answer.
///
/// Never answering is fine and deliberate: this is about which sessions exist,
/// not about what they achieve. A session that cannot connect is still a
/// session holding a store lock and still one that must be stopped.
fn app_at(root: PathBuf) -> ChatApp {
    let mut app = ChatApp::new();
    app.set_store_root_for_test(root);
    app.set_exchange_for_test(
        "127.0.0.1:1",
        // Any well-formed key; nothing is dialled successfully.
        &PubKey::new([7u8; 32]).to_string(),
    );
    app
}

#[tokio::test(flavor = "multi_thread")]
async fn every_unlocked_identity_gets_its_own_session() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let two = Account::unlocked_for_test([2u8; 32]);
    let (a, b) = (key_of(&one), key_of(&two));
    let mut accounts = Accounts::of(vec![one, two]);

    pass(&mut app, &mut accounts, &egui_ctx);

    let mut running = app.running_as_for_test();
    running.sort_by_key(|k| k.to_string());
    let mut want = vec![a, b];
    want.sort_by_key(|k| k.to_string());
    assert_eq!(
        running, want,
        "both identities must be live -- a message for the one not on screen \
         is exactly the one that would otherwise be missed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_an_identity_stops_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let two = Account::unlocked_for_test([2u8; 32]);
    let (a, b) = (key_of(&one), key_of(&two));
    let mut accounts = Accounts::of(vec![one, two]);

    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(app.running_as_for_test().len(), 2);

    // Put the first away. This is the moment the old code got wrong.
    assert!(accounts.lock(0));
    pass(&mut app, &mut accounts, &egui_ctx);

    let running = app.running_as_for_test();
    assert!(
        !running.contains(&a),
        "the closed identity is still running a session -- sigil is acting as \
         somebody it no longer holds"
    );
    assert_eq!(running, vec![b], "and the one still held must keep running");
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_which_identity_is_shown_stops_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let mut accounts = Accounts::of(vec![
        Account::unlocked_for_test([1u8; 32]),
        Account::unlocked_for_test([2u8; 32]),
    ]);
    pass(&mut app, &mut accounts, &egui_ctx);
    let before = app.running_as_for_test();

    assert!(accounts.switch_to(1));
    pass(&mut app, &mut accounts, &egui_ctx);

    // "All live, one shown": looking elsewhere is not the same as putting an
    // account away, and must not cost the other one its messages.
    assert_eq!(before, app.running_as_for_test());
}

/// One identity at two exchanges is **two** sessions.
///
/// # Why the pair and not the key
///
/// The identity is the same key at every exchange and nothing else is. A
/// direct message's channel identifier is derived from its two accounts, so
/// one conversation has identical channel bytes everywhere it exists — which
/// is why the store scopes every row by exchange and SIP-31 binds the exchange
/// into every entry signature. A client keyed on the account alone would hold
/// one session and show one exchange's conversations as though they were all
/// of them.
#[tokio::test(flavor = "multi_thread")]
async fn one_identity_at_two_exchanges_is_two_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let me = key_of(&one);
    let mut accounts = Accounts::of(vec![one]);

    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(
        app.running_at_for_test(),
        vec![(me, String::new())],
        "one identity starts at its default exchange, which has no name"
    );

    assert!(accounts.add_exchange(0, "indra.org"));
    pass(&mut app, &mut accounts, &egui_ctx);

    assert_eq!(
        app.running_at_for_test(),
        vec![(me, String::new()), (me, "indra.org".to_string())],
        "adding one gives a second session, not a replacement"
    );
    // One key, two sessions. A test counting identities would see one and miss
    // the whole point.
    assert_eq!(app.running_as_for_test(), vec![me]);
}

/// Dropping an exchange stops that session and leaves the other running.
#[tokio::test(flavor = "multi_thread")]
async fn dropping_an_exchange_stops_only_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let me = key_of(&one);
    let mut accounts = Accounts::of(vec![one]);
    accounts.add_exchange(0, "indra.org");
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(app.running_at_for_test().len(), 2);

    assert!(accounts.drop_exchange(0, "indra.org"));
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(
        app.running_at_for_test(),
        vec![(me, String::new())],
        "the one dropped stops; the one kept does not"
    );
}

/// Closing the identity stops every one of its exchanges.
///
/// The same silent failure as before, one level up: a session left running for
/// an identity that has been put away keeps connecting and keeps succeeding,
/// and now there can be several of them.
#[tokio::test(flavor = "multi_thread")]
async fn closing_an_identity_stops_all_of_its_exchanges() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let mut accounts = Accounts::of(vec![Account::unlocked_for_test([1u8; 32])]);
    accounts.add_exchange(0, "indra.org");
    accounts.add_exchange(0, "squic.org");
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(app.running_at_for_test().len(), 3);

    assert!(accounts.lock(0));
    pass(&mut app, &mut accounts, &egui_ctx);
    assert!(
        app.running_at_for_test().is_empty(),
        "every exchange goes with the identity: {:?}",
        app.running_at_for_test()
    );
}
