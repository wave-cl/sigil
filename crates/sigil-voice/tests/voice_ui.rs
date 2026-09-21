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
    sized(
        account,
        dark,
        egui::vec2(900.0, 600.0),
        sigil::Form::Desktop,
    )
}

/// The Calls app at 360 points, with the form that says it is touched.
///
/// It had no phone render, and it is where a room secret is minted and
/// pasted -- long strings in boxes, which is the shape that does not fit.
fn harness_phone(account: Account) -> Harness<'static> {
    sized(account, true, egui::vec2(360.0, 804.0), sigil::Form::Phone)
}

/// How wide the pane's contents came out. See
/// `the_calls_pane_is_not_wider_than_the_phone`.
type Drawn = std::rc::Rc<std::cell::Cell<f32>>;

fn sized(account: Account, dark: bool, size: egui::Vec2, form: sigil::Form) -> Harness<'static> {
    sized_measured(account, dark, size, form).0
}

fn sized_measured(
    account: Account,
    dark: bool,
    size: egui::Vec2,
    form: sigil::Form,
) -> (Harness<'static>, Drawn) {
    let drawn: Drawn = std::rc::Rc::new(std::cell::Cell::new(0.0));
    let width = drawn.clone();
    let mut app = VoiceApp::new();
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    let h = Harness::builder().with_size(size).build_ui(move |ui| {
        let ctx = ui.ctx().clone();
        sigil::Form::install(&ctx, form);
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
                    .inner_margin(egui::Margin::same(form.body_margin() as i8)),
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
                width.set(ui.min_rect().width() + 2.0 * form.body_margin());
            });
    });
    (h, drawn)
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
    // A *fixed* identity. This screen shows the key in full, and
    // `sqnr::identity::generate` mints a random one -- so with a generated
    // account the snapshot renders differently every run and can never pass
    // twice. Mine was written by UPDATE_SNAPSHOTS and never checked until CI
    // checked it.
    let mut h = harness(Account::unlocked_for_test([4u8; 32]), true);
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

/// Drawing the same state twice must produce the same thing.
///
/// The cheap general form of the check that `voice_idle_dark` learned the hard
/// way: a generated identity renders a different key every run, so any
/// snapshot of a view that draws one can never pass twice. No renderer, no
/// PNG, no platform — anything non-deterministic reaching the screen shows up
/// here, in a test that names the problem, rather than as a pixel diff on CI.
#[test]
fn the_same_state_draws_the_same_way_twice() {
    let read = || {
        let mut h = harness(Account::unlocked_for_test([4u8; 32]), true);
        h.run();
        text_of(&h)
    };
    assert_eq!(
        read(),
        read(),
        "something drawn here changes between runs, so no snapshot of it can pass twice"
    );
}

/// The Calls app on a phone.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn voice_phone() {
    let mut h = harness_phone(Account::unlocked_for_test([1u8; 32]));
    h.run();
    h.run();
    h.snapshot("voice_phone");
}

/// Calls fits a phone, without anybody rendering it and looking.
///
/// It did not: its key boxes asked for 420 points in a 360-point pane, so
/// the box left the screen and the button after it was off the pane
/// entirely -- and the ui, grown to what was drawn in it, then had both of
/// this screen's explanations clipped mid-word.
///
/// No renderer, so it runs in an ordinary `cargo test`.
#[test]
fn the_calls_pane_is_not_wider_than_the_phone() {
    const PHONE: f32 = 360.0;
    let (mut h, drawn) = sized_measured(
        Account::unlocked_for_test([1u8; 32]),
        true,
        egui::vec2(PHONE, 804.0),
        sigil::Form::Phone,
    );
    h.run();
    h.run();
    let width = drawn.get();
    assert!(width > 0.0, "Calls drew nothing, so this proves nothing");
    assert!(
        width <= PHONE + 1.0,
        "Calls draws {width} points wide in a {PHONE}-point pane"
    );
}

/// The Calls app on a phone, carrying whatever call it was handed.
fn phone_with(app: VoiceApp) -> Harness<'static> {
    let mut app = app;
    let mut accounts = sigil::accounts::Accounts::of(vec![Account::unlocked_for_test([1u8; 32])]);
    let margin = sigil::Form::Phone.body_margin();
    Harness::builder()
        .with_size(egui::vec2(360.0, 804.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(margin as i8)),
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

/// A room of a dozen on a phone: the roster scrolls, Leave does not move.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
async fn voice_phone_in_a_room() {
    let mut h = phone_with(a_room_of(12));
    h.run();
    h.run();
    h.snapshot("voice_phone_in_a_room");
}

/// A call in a room with `n` people in it.
fn a_room_of(n: usize) -> VoiceApp {
    let mut app = VoiceApp::new();
    let present: Vec<sigil_net::PeerStatus> = (0..n)
        .map(|i| sigil_net::PeerStatus {
            identity: sqnr_core::PubKey::new([i as u8 + 10; 32]),
            speaking: i % 3 == 0,
            level: 0.4,
            loss_pct: 2.1,
            concealed: 3,
            buffered: 9,
        })
        .collect();
    app.hold_call_for_test(sigil_net::CallHandle::for_test(sigil_net::CallState {
        phase: sigil_net::Phase::Live,
        room: Some(sigil_net::RoomId::generate()),
        present,
        connecting: 2,
        ..Default::default()
    }));
    app
}

/// **Hang up stays on the screen, however many people are in the room.**
///
/// The call view draws the roster and *then* the control that ends the call.
/// On a phone each roster row is a key, a meter and a line about the path --
/// two lines, since the detail sits under the row there -- so a room of a
/// dozen is several hundred points, and Leave was below the fold with
/// nothing to scroll. A call somebody cannot end is a microphone that stays
/// open, and the only way out is force-stopping the app.
///
/// Twelve is not a stress test. It is a team.
#[tokio::test(flavor = "multi_thread")]
async fn the_control_that_ends_a_call_stays_on_a_phones_screen() {
    const TALL: f32 = 804.0;
    let mut h = phone_with(a_room_of(12));
    h.run();
    h.run();
    let leave = h
        .get_all_by_label_contains("Leave")
        .map(|n| n.rect())
        .next()
        .expect("a call in a room offers Leave");
    assert!(
        leave.bottom() <= TALL,
        "Leave sits at y {:.0}..{:.0} of {TALL}: a dozen people in the room \
         have pushed the only way out of the call off the screen",
        leave.top(),
        leave.bottom()
    );

    // **And it still ends the call.** Moving a control into a panel is the
    // sort of change that leaves a button which looks right and does
    // nothing: the press has to reach the same `hang_up` it did before.
    assert!(
        text_of(&h).contains("In a room"),
        "the call is not up, so pressing Leave proves nothing"
    );
    h.get_all_by_label_contains("Leave")
        .next()
        .expect("Leave")
        .click();
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains("In a room"),
        "Leave is on the screen and does not end the call: {}",
        text_of(&h)
    );
}

/// **A `sigil://room/<secret>` link, once somebody has said yes to it.**
///
/// Two of the three kinds of link are this app's, and both are dangerous in
/// the same quiet way: a room's membership *is* holding its secret, so
/// joining one cannot be taken back and there is nobody to remove you. The
/// shell asks first; by the time `follow` is called the question has been
/// answered.
///
/// What is asserted is that it goes through the same door the button does --
/// the field is filled and the press is made on the next pass, with the same
/// refusals in the same words. A second way in that refused differently is
/// how the CLI's two copies of this drifted apart.
#[test]
fn a_room_link_fills_the_field_and_presses_join() {
    // One identity, opened again for each `AppContext`: generating a second
    // is refused, and rightly -- `sqnr` will not write over one.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, None).unwrap();
    let account = || Account::discover(Some(path.clone()));
    let (mut h, app) = shared(account());
    h.run();
    h.run();
    assert!(
        !text_of(&h).contains(ROOM),
        "the room secret is on screen before any link was followed, so this \
         test cannot tell the two apart"
    );

    let took = {
        let mut app = app.borrow_mut();
        let mut nav = Navigator::default();
        let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
        let mut ctx = AppContext {
            navigator: &mut nav,
            accounts: &mut accounts,
            unfocused: false,
            away: false,
            notify: &sigil::Silent,
            connections: &Default::default(),
        };
        app.follow(&mut ctx, &sigil::Link::Room(ROOM.to_string()))
    };
    assert!(took, "the Calls app did not take a room link");
    h.run();
    h.run();
    let after = text_of(&h);
    assert!(
        after.contains(ROOM),
        "the secret from the link is not in the room field: {after}"
    );
    // **And the press was made.** The fixture is forty-four base58
    // characters, which is what `sigil::deeplink` insists on, and thirty-three
    // bytes, which is not a room -- so `join_room` refuses it where it parses
    // it, in the same words the Join button would, and reaches no network.
    // That refusal is the proof that the button was pressed: the field alone
    // would be there whether anything happened or not.
    assert!(
        after.contains("that is not a room secret"),
        "the field was filled and Join was never pressed: {after}"
    );

    // A contact is not this app's errand, and saying so is what lets the
    // shell hand it to the one whose it is.
    let mine = {
        let mut app = app.borrow_mut();
        let mut nav = Navigator::default();
        let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
        let mut ctx = AppContext {
            navigator: &mut nav,
            accounts: &mut accounts,
            unfocused: false,
            away: false,
            notify: &sigil::Silent,
            connections: &Default::default(),
        };
        app.follow(
            &mut ctx,
            &sigil::Link::Contact(sqnr_core::PubKey::new([4u8; 32])),
        )
    };
    assert!(!mine, "the Calls app took a link that belongs to the chat");
}

/// A room secret shaped the way `sigil::deeplink` insists on: forty or more
/// base58 characters, and nothing else.
const ROOM: &str = "TestRoomSecretNotARea1RoomDoNotUseAAAAAAAAAA";

/// The same harness as [`sized`], with the app shared so a test can call the
/// `App` trait on it between passes.
#[allow(clippy::type_complexity)]
fn shared(account: Account) -> (Harness<'static>, std::rc::Rc<std::cell::RefCell<VoiceApp>>) {
    let app = std::rc::Rc::new(std::cell::RefCell::new(VoiceApp::new()));
    let drawn = app.clone();
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    let h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Desktop);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let theme = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(theme.surface_primary))
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
                    let _ = drawn.borrow_mut().render(&mut app_ctx, ui);
                });
        });
    (h, app)
}
