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
use std::time::Duration;

use sigil::accounts::Accounts;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, Silent};
use sigil_chat::ChatApp;
use sqnr_core::PubKey;

/// Run one `update` pass against this roster.
fn pass(app: &mut ChatApp, accounts: &mut Accounts, egui_ctx: &egui::Context) {
    pass_lending(app, accounts, egui_ctx, &Default::default());
}

/// The same, with somewhere to see what the sessions offered to lend.
fn pass_lending(
    app: &mut ChatApp,
    accounts: &mut Accounts,
    egui_ctx: &egui::Context,
    connections: &sigil_net::Connections,
) {
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts,
        unfocused: true,
        notify: &Silent,
        connections,
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

/// Every session offers its connection, and takes the offer back with it.
///
/// This is what stops a call and the administrative console dialling their own:
/// one identity reaches one exchange over one connection, and the chat session
/// is the thing that owns it. The offer is made when the session starts rather
/// than when it connects — what is lent is a slot, filled when the link comes
/// up — so a console started in the same second waits a handshake instead of
/// dialling.
///
/// Nothing here connects (the exchange never answers), which is the point: the
/// offer exists either way, and an empty slot is an honest answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_lends_its_connection_and_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());
    let connections = sigil_net::Connections::new();

    let one = Account::unlocked_for_test([1u8; 32]);
    let two = Account::unlocked_for_test([2u8; 32]);
    let (a, b) = (key_of(&one), key_of(&two));
    let mut accounts = Accounts::of(vec![one, two]);
    pass_lending(&mut app, &mut accounts, &egui_ctx, &connections);

    assert!(
        connections.of(a, "").is_some() && connections.of(b, "").is_some(),
        "each identity's session should offer what it holds"
    );
    assert!(
        connections.of(a, "unknown.example").is_none(),
        "and only for the exchange it is a session for"
    );

    // Closed: the identity is gone, and so is anything that was to be borrowed
    // from it. A console left holding the old slot would be asking as somebody
    // who is no longer here.
    assert!(accounts.lock(0));
    pass_lending(&mut app, &mut accounts, &egui_ctx, &connections);
    assert!(
        connections.of(a, "").is_none(),
        "a closed identity should stop offering a connection"
    );
    assert!(
        connections.of(b, "").is_some(),
        "and the one still open should go on offering one"
    );
}

/// A session that dies is started again.
///
/// **Found by restarting sigil a second after quitting it.** The store's lock
/// is released when the old process exits, and the new one raced it: four
/// sessions were refused their stores, failed, and stayed failed — `reconcile`
/// starts a session for any identity that has none, and a dead session is
/// still one. The window sat there with four identities holding nothing and no
/// way back but closing and opening each of them.
///
/// The exchange here never answers, which is what makes the test possible: a
/// session that cannot publish its prekeys ends with an error, exactly as one
/// refused its store does.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_died_is_started_again() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(app.starts_for_test(), 1, "the session should have started");

    // It cannot reach anything, so it ends. Waited for rather than assumed:
    // what is being tested is what happens *after* it dies.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline && !app.stopped_for_test() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        app.stopped_for_test(),
        "the session should have failed against an exchange that is not there"
    );

    // Nothing happens for a moment: a store locked for good must not become a
    // restart on every frame.
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(
        app.starts_for_test(),
        1,
        "a session that has just died should not be restarted immediately"
    );

    tokio::time::sleep(Duration::from_secs(4)).await;
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(
        app.starts_for_test(),
        2,
        "a dead session was never started again; the identity holds nothing \
         and nothing will change that"
    );
}
