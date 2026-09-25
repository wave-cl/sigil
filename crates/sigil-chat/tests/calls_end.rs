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
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::accounts::Accounts;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, Silent};
use sigil_chat::ChatApp;
use sqnr_core::{PubKey, SoftwareSigner};

fn pass(app: &mut ChatApp, accounts: &mut Accounts, egui_ctx: &egui::Context) {
    pass_telling(app, accounts, egui_ctx, &Silent);
}

/// A pass whose platform is watching, for the one test about what it is told.
fn pass_telling(
    app: &mut ChatApp,
    accounts: &mut Accounts,
    egui_ctx: &egui::Context,
    notify: &dyn sigil::app::Notify,
) {
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts,
        unfocused: true,
        away: false,
        notify,
        connections: &Default::default(),
    };
    app.update(&mut ctx, egui_ctx);
}

/// A platform that writes down whether it was told a call is up.
///
/// Not `Silent`, which is what every other test here uses: silence is what a
/// working notifier looks like from inside a test, and this test is about
/// something being said.
#[derive(Default)]
struct Watching(std::cell::RefCell<Vec<Option<String>>>);

impl Watching {
    /// What it was told since it was last asked.
    fn told(&self) -> Vec<Option<String>> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

impl sigil::app::Notify for Watching {
    fn notice(&self, _notice: sigil::Notice<'_>) -> bool {
        false
    }
    fn calling(&self, with: Option<&str>) {
        self.0.borrow_mut().push(with.map(|s| s.to_string()));
    }
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
    let looking_at = shown.unlocked().expect("an open account").me();
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
                    away: false,
                    notify: &Silent,
                    connections: &Default::default(),
                };
                // **Through `render_nav`, as the shell does.** The call bar is drawn
                // there and not in `render`, so that it survives every route: a
                // call was invisible the moment somebody opened Members or
                // Settings. Driving `render` directly here would be testing an
                // entry the application never takes on its own.
                let token: std::rc::Rc<dyn std::any::Any> =
                    std::rc::Rc::new(sigil_chat::Route::Conversations);
                let _ = app.render_nav(&mut app_ctx, ui, &token);
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
    //
    // Neither identity has published a profile, so what names one is the
    // short form of its key: the first four characters, three dots, the last
    // four. Built here from that rule rather than by calling `short`, so this
    // tests that the **right** identity is named and not that two functions
    // agree with each other.
    let short_form = |k: &PubKey| -> String {
        let chars: Vec<char> = k.to_string().chars().collect();
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}...{tail}")
    };
    let banner = said
        .iter()
        .find(|l| l.contains("In a call") || l.contains("Connecting"))
        .expect("the banner was found above");
    assert!(
        banner.contains(&short_form(&elsewhere)),
        "the call does not say which identity is in it: {banner:?}"
    );
    // The negative half, and the one that matters: the identity on screen is
    // *not* the one in the call, and a banner naming it would be worse than
    // one naming nobody. Asserted on the banner rather than on the screen,
    // because the identity being looked at has its own short key in its own
    // header, exactly as it should.
    assert!(
        !banner.contains(&short_form(&looking_at)),
        "the banner names the identity on screen, which is not the one in the \
         call: {banner:?}"
    );
}

/// The bar says which way the audio is going: "direct" once the exchange
/// has introduced the two, "via exchange" when it is relayed, and nothing
/// while that is not yet settled.
#[tokio::test(flavor = "multi_thread")]
async fn the_call_bar_says_which_way_the_audio_goes() {
    use egui_kittest::Harness;

    for (path, why, word, absent) in [
        (
            Some(sigil_net::Path::Direct),
            None,
            Some("direct"),
            "via exchange",
        ),
        (
            Some(sigil_net::Path::Relayed),
            Some("the other side did not ask to be introduced".to_string()),
            Some("via exchange"),
            "direct",
        ),
        // **Unset is the ordinary case, not an undecided one.**
        // `Event::Relayed` is emitted only by the `sqex-voice` binary, so
        // on the library path a window takes, `path` stays `None` for
        // every relayed call -- and this row used to say nothing at all.
        // The only route to a direct call reports itself, so a call that
        // is up and has not reported one is being carried.
        (None, None, Some("via exchange"), "direct"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let egui_ctx = egui::Context::default();
        let mut app = app_at(dir.path().to_path_buf());
        let one = Account::unlocked_for_test([1u8; 32]);
        let me = one.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![one]);
        pass(&mut app, &mut accounts, &egui_ctx);

        let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            path,
            why: why.clone(),
            me: Some(me),
            ..Default::default()
        });
        app.hold_call_for_test(me, [3u8; 32], 9, handle);

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
                        away: false,
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                });
            });
        harness.run_steps(3);
        let said = labels(&harness);
        assert!(
            said.iter().any(|l| l == "In a call"),
            "the call is not on screen: {said:?}"
        );
        match word {
            Some(word) => assert!(
                said.iter().any(|l| l == word),
                "{path:?}: the bar does not say {word:?}: {said:?}"
            ),
            None => assert!(
                !said.iter().any(|l| l == "direct"),
                "nothing is settled yet, and the bar says direct: {said:?}"
            ),
        }
        assert!(
            !said.iter().any(|l| l == absent),
            "{path:?}: the bar says {absent:?} as well: {said:?}"
        );
    }
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

/// A notification pressed leads to its conversation: the identity it came
/// to is switched to, and the conversation opened, at the exchange it came
/// from. One for an identity this window does not hold is not this app's.
#[tokio::test(flavor = "multi_thread")]
async fn a_pressed_notification_switches_to_its_identity_and_opens_its_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());

    let one = Account::unlocked_for_test([1u8; 32]);
    let two = Account::unlocked_for_test([2u8; 32]);
    let second = two.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one, two]);
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(accounts.active_index(), 0);
    assert_eq!(app.running_at_for_test().len(), 2, "both sessions up");

    let target = sigil::Target {
        identity: second,
        exchange: String::new(),
        channel: [7u8; 32],
        answer: false,
    };
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts: &mut accounts,
        unfocused: true,
        away: false,
        notify: &Silent,
        connections: &Default::default(),
    };
    // What the app asks its session is written down while a fixed state is
    // installed; the sessions are real and the exchange answers nothing,
    // so what was asked is the thing to look at.
    app.show_state_for_test(sigil_chat::ChatState::default());
    assert!(app.open(&mut ctx, &target), "this app's to open");
    assert_eq!(
        accounts.active_index(),
        1,
        "switched to the identity it came to"
    );
    let asked = app.asked_for_test().join(" | ");
    assert!(
        asked.contains(&format!("Show({:?})", [7u8; 32])),
        "the conversation is asked for: {asked}"
    );

    // Somebody else's identity: not ours, nothing moved.
    let stranger = sigil::Target {
        identity: PubKey::new([9u8; 32]),
        exchange: String::new(),
        channel: [7u8; 32],
        answer: false,
    };
    let mut nav = Navigator::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts: &mut accounts,
        unfocused: true,
        away: false,
        notify: &Silent,
        connections: &Default::default(),
    };
    assert!(!app.open(&mut ctx, &stranger));
    assert_eq!(accounts.active_index(), 1);
}

/// **The platform is told a call began and ended, once each.**
///
/// `CallService.kt` was written for Android and nothing ever started it:
/// `Notify` had no call-began hook, so there was nowhere for it to be started
/// *from*. Android stops an app that is not in front -- microphone or no
/// microphone -- unless a foreground service says otherwise, so without this
/// a call on the phone ends when the screen does.
///
/// Once is the whole difficulty. This is derived from the calls the window
/// holds, on every pass, and a pass is sixty a second; telling the platform
/// each time would post a notification sixty times a second, which is not a
/// call, it is a fault -- and it would look identical to working, because the
/// last one posted is the one on the screen.
///
/// # Why the call does not die on its own here
///
/// Two earlier versions used a real call to an address nothing answers on,
/// as the tests above do, and both were flaky on CI in opposite directions:
/// on macOS the call outlived the first assertion and died during the next
/// three passes, so the *ending* was announced there; on Linux, where a
/// refused connection is immediate, it was already gone before the first
/// pass and nothing was announced at all. The lifetime of that handle is how
/// fast a machine refuses a connection, and no arrangement of passes around
/// it is a fixed point.
///
/// So the call is a `for_test` one, which stays up until something ends it,
/// and what ends it is the Hang up button -- the path a person takes, and
/// deterministic because it is a press rather than a race.
#[tokio::test(flavor = "multi_thread")]
async fn the_platform_is_told_a_call_began_and_ended_once_each() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut app = app_at(dir.path().to_path_buf());
    let mut accounts = Accounts::of(vec![one]);
    let watching = Watching::default();

    pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    assert_eq!(
        watching.told(),
        Vec::<Option<String>>::new(),
        "with no call, the platform is told nothing at all -- not told 'no call'"
    );

    // A call that stays up: `Phase::Live`, and nothing reaps it.
    app.hold_call_for_test(
        me,
        [3u8; 32],
        9,
        sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            me: Some(me),
            ..Default::default()
        }),
    );
    pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    let told = watching.told();
    assert_eq!(told.len(), 1, "the call beginning is said once: {told:?}");
    assert!(
        told[0].is_some(),
        "a call with no conversation to name is still a call: {told:?}"
    );

    // Four more passes with the call still up.
    for _ in 0..4 {
        pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    }
    assert!(
        !app.calls_for_test().is_empty(),
        "the call ended by itself, so the next assertion would be about \
         nothing"
    );
    assert_eq!(
        watching.told(),
        Vec::<Option<String>>::new(),
        "a call that is still up is not announced again"
    );

    // Ended the way a person ends one: the button on the call bar. A harness
    // is needed because the control is drawn rather than called.
    let held = std::rc::Rc::new(std::cell::RefCell::new(app));
    let shared = held.clone();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Option<String>>::new()));
    let noted = seen.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let telling = Watching::default();
            let mut nav = Navigator::default();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &telling,
                connections: &Default::default(),
            };
            let mut app = shared.borrow_mut();
            app.update(&mut app_ctx, &ctx);
            // **Through `render_nav`, as the shell does.** The call bar is drawn
            // there and not in `render`, so that it survives every route: a
            // call was invisible the moment somebody opened Members or
            // Settings. Driving `render` directly here would be testing an
            // entry the application never takes on its own.
            let token: std::rc::Rc<dyn std::any::Any> =
                std::rc::Rc::new(sigil_chat::Route::Conversations);
            let _ = app.render_nav(&mut app_ctx, ui, &token);
            noted.borrow_mut().extend(telling.told());
        });
    harness.run_steps(3);
    seen.borrow_mut().clear();

    harness.get_by_label("Hang up").click();
    harness.run_steps(3);
    let after = seen.borrow().clone();
    assert!(
        held.borrow().calls_for_test().is_empty(),
        "Hang up did not end the call, so this says nothing: {after:?}"
    );
    assert_eq!(
        after,
        vec![None],
        "the ending is said once, and says there is no call: {after:?}"
    );

    // And stays ended without being said again.
    harness.run_steps(3);
    assert_eq!(
        seen.borrow().clone(),
        vec![None],
        "a call that has ended is announced again on later passes"
    );
}

/// A call in progress on a phone, keeping every route the app asked for.
///
/// `phone_call` builds a fresh `Navigator` each pass and drops what it was
/// asked, which is fine for a test about what is *drawn*. These are about what
/// pressing something *does*, so the requests are drained and kept.
/// What the app asked the shell for, pass by pass.
type Routes = std::rc::Rc<std::cell::RefCell<Vec<sigil_chat::Route>>>;
/// Which calls the window still holds, read after each pass.
type Held = std::rc::Rc<std::cell::RefCell<Vec<PubKey>>>;

/// A call that is up, direct, and not muted.
fn live() -> sigil_net::CallState {
    sigil_net::CallState {
        phase: sigil_net::Phase::Live,
        path: Some(sigil_net::Path::Direct),
        ..Default::default()
    }
}

/// A phone with one live call, drawing `route`.
///
/// `Route::Conversations` gets the bar; `Route::Call(me)` gets the card. The
/// harness records what the app asked to navigate to rather than following it,
/// so a case says which screen it is on rather than inferring it.
fn phone_call_routes(
    route: sigil_chat::Route,
    state: sigil_net::CallState,
) -> (
    egui_kittest::Harness<'static>,
    Routes,
    Held,
    tempfile::TempDir,
) {
    use egui_kittest::Harness;

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());
    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);

    let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
        me: Some(me),
        ..state
    });
    app.hold_call_for_test(me, [3u8; 32], 9, handle);
    // `Route::Call` is keyed by identity, and only this harness knows the key.
    let route = match route {
        sigil_chat::Route::Call(_) => sigil_chat::Route::Call(me),
        other => other,
    };

    let routes: Routes = Default::default();
    let asked = routes.clone();
    // Which calls the app still holds, read after each pass: a hang-up has to
    // end the call, and a test that only watched the routes could not tell a
    // press that did nothing from one that did the right thing.
    let held: Held = Default::default();
    let living = held.clone();
    let margin = sigil::Form::Phone.body_margin();
    let h = Harness::builder()
        .with_size(egui::vec2(360.0, 804.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
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
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(route.clone());
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                    for request in nav.take() {
                        let token = match request {
                            sigil::navigator::NavRequest::PushActive(e)
                            | sigil::navigator::NavRequest::ReplaceActive(e) => e.token,
                            sigil::navigator::NavRequest::Push(e)
                            | sigil::navigator::NavRequest::Replace(e) => e.token,
                            _ => continue,
                        };
                        if let Some(route) = token.downcast_ref::<sigil_chat::Route>() {
                            asked.borrow_mut().push(route.clone());
                        }
                    }
                    *living.borrow_mut() = app.calls_for_test();
                });
        });
    (h, routes, held, dir)
}

/// **Pressing the bar opens the call.** The bar is the call minimised, and
/// before this there was nowhere for it to lead.
// `CallHandle::for_test` spawns, so this wants a runtime like every
// other call test here.
#[tokio::test(flavor = "multi_thread")]
async fn the_bar_opens_the_card() {
    let (mut h, routes, held, _dir) = phone_call_routes(sigil_chat::Route::Conversations, live());
    h.run_steps(3);
    assert_eq!(
        held.borrow().len(),
        1,
        "no call is held, so this says nothing"
    );
    routes.borrow_mut().clear();

    // What the bar actually offers, before pressing it: if "Open the call" is
    // not a node, the press below lands on nothing and the assertion would
    // fail for a reason that has nothing to do with the route.
    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "Open the call"),
        "the bar is not a control at all: {said:?}"
    );
    h.get_by_label("Open the call").click();
    h.run_steps(3);
    h.run_steps(3);

    let asked = routes.borrow().clone();
    assert!(
        asked
            .iter()
            .any(|r| matches!(r, sigil_chat::Route::Call(_))),
        "pressing the bar asked for no call screen: {asked:?}"
    );
    assert_eq!(
        held.borrow().len(),
        1,
        "and opening the card must not end the call"
    );
}

/// **Hanging up from the bar does not also open the card.**
///
/// The hang-up is a control inside a frame that is itself a control now, and
/// egui gives a press to the innermost widget that sensed it. If that did not
/// hold, every hang-up would throw somebody onto a screen for the call they
/// just ended -- which pops itself on the next pass, so it would read as a
/// flicker in the navigation rather than as a fault in the bar.
// `CallHandle::for_test` spawns, so this wants a runtime like every
// other call test here.
#[tokio::test(flavor = "multi_thread")]
async fn hanging_up_from_the_bar_does_not_open_the_card() {
    let (mut h, routes, held, _dir) = phone_call_routes(sigil_chat::Route::Conversations, live());
    h.run_steps(3);
    assert_eq!(
        held.borrow().len(),
        1,
        "no call is held, so this says nothing"
    );
    routes.borrow_mut().clear();

    h.get_by_label("Hang up").click();
    h.run_steps(3);
    h.run_steps(3);

    assert!(
        held.borrow().is_empty(),
        "the hang-up did not end the call, so what follows says nothing"
    );
    let asked = routes.borrow().clone();
    assert!(
        !asked
            .iter()
            .any(|r| matches!(r, sigil_chat::Route::Call(_))),
        "hanging up also opened the call screen: {asked:?}"
    );
}

/// A call in progress, on a phone.
///
/// The call bar had never been drawn at 360 points -- the test above uses a
/// 1000-point window, which is the width at which a row of controls always
/// fits. It is also the one screen somebody is looking at *while* holding the
/// phone to their ear, so a control that has slid off the edge is a call they
/// cannot end.
fn phone_call(
    with: &str,
) -> (
    egui_kittest::Harness<'static>,
    std::rc::Rc<std::cell::Cell<f32>>,
    tempfile::TempDir,
) {
    use egui_kittest::Harness;

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());
    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);

    let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
        phase: sigil_net::Phase::Live,
        path: Some(sigil_net::Path::Direct),
        me: Some(me),
        ..Default::default()
    });
    app.hold_call_for_test(me, [3u8; 32], 9, handle);
    let _ = with;

    let drawn = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let width = drawn.clone();
    let margin = sigil::Form::Phone.body_margin();
    let h = Harness::builder()
        .with_size(egui::vec2(360.0, 804.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
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
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                    width.set(ui.min_rect().width() + 2.0 * margin);
                });
        });
    (h, drawn, dir)
}

/// The call bar fits a phone, and its controls are on the screen.
#[tokio::test(flavor = "multi_thread")]
async fn the_call_bar_fits_a_phone() {
    const PHONE: f32 = 360.0;
    let (mut h, drawn, _dir) = phone_call("Alexandra Constantinopoulos-Whitmore");
    h.run_steps(3);
    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "In a call"),
        "no call on screen, so this proves nothing about its width: {said:?}"
    );
    let width = drawn.get();
    assert!(
        width <= PHONE + 1.0,
        "a call draws {width} points wide in a {PHONE}-point pane: a control \
         that has slid off the edge is a call somebody cannot end"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
async fn phone_in_a_call() {
    let (mut h, _, _dir) = phone_call("Alexandra Constantinopoulos-Whitmore");
    h.run_steps(3);
    h.snapshot("phone_in_a_call");
}

/// **On a phone the reason a call is relayed is drawn, not hovered.**
///
/// Which of the four reasons it is — the other side did not ask, this
/// side's preference is off, the connection is carried through the home,
/// or the punch failed — is the one fact about a call somebody can act on,
/// and it was behind `on_hover_text`, which on a phone is a place nothing
/// can reach.
#[tokio::test(flavor = "multi_thread")]
async fn on_a_phone_the_reason_a_call_is_relayed_is_on_the_screen() {
    use egui_kittest::Harness;

    const WHY: &str = "the other side did not ask to be introduced";
    for (form, shown) in [(sigil::Form::Phone, true), (sigil::Form::Desktop, false)] {
        let (wide, tall) = if form.is_phone() {
            (360.0f32, 804.0f32)
        } else {
            (1000.0f32, 620.0f32)
        };
        let dir = tempfile::tempdir().unwrap();
        let egui_ctx = egui::Context::default();
        let mut app = app_at(dir.path().to_path_buf());
        let one = Account::unlocked_for_test([1u8; 32]);
        let me = one.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![one]);
        pass(&mut app, &mut accounts, &egui_ctx);

        let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            path: Some(sigil_net::Path::Relayed),
            why: Some(WHY.to_string()),
            me: Some(me),
            ..Default::default()
        });
        app.hold_call_for_test(me, [3u8; 32], 9, handle);

        let mut harness = Harness::builder()
            .with_size(egui::vec2(wide, tall))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
                sigil::Form::install(&ctx, form);
                ctx.set_theme(egui::Theme::Dark);
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        accounts: &mut accounts,
                        unfocused: false,
                        away: false,
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                });
            });
        harness.run_steps(3);
        let said = labels(&harness);
        let on_screen = said.iter().any(|l| l.contains(WHY));
        assert_eq!(
            on_screen, shown,
            "{form:?}: the reason on screen was {on_screen}, wanted {shown}: {said:?}"
        );
        // Either way the word itself is there.
        assert!(said.iter().any(|l| l == "via exchange"), "{said:?}");
    }
}

/// **A call that hears nothing says so.**
///
/// The engine raises `deaf` once frames have gone out and not one has come
/// back; with no discontinuous transmission a peer in a call sends fifty a
/// second, so silence is a fault and never a quiet room. The bar used to
/// draw a green dot and a running clock either way, which is how five
/// minutes of a field test looked perfect and carried no sound.
///
/// The `false` case is the control: it proves the assertion can fail, so
/// the `true` case passing means something.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_hears_nothing_says_so() {
    use egui_kittest::Harness;

    const SILENCE: &str = "Nothing is coming through from the other side.";

    for deaf in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let egui_ctx = egui::Context::default();
        let mut app = app_at(dir.path().to_path_buf());
        let one = Account::unlocked_for_test([1u8; 32]);
        let me = one.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![one]);
        pass(&mut app, &mut accounts, &egui_ctx);

        let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            me: Some(me),
            deaf,
            // Both cases carry a line, so the difference the assertion
            // measures is whether it is *drawn*, never whether it exists.
            stats: Some("sent 900 · recv 0 · loss 100.0%".to_string()),
            ..Default::default()
        });
        app.hold_call_for_test(me, [3u8; 32], 9, handle);

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
                        away: false,
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                });
            });
        harness.run_steps(3);
        let said = labels(&harness);
        // The call is on screen either way: a deaf call is still a call,
        // and the point is what it admits, not that it disappears.
        assert!(
            said.iter().any(|l| l == "In a call"),
            "the call is not on screen: {said:?}"
        );
        assert_eq!(
            said.iter().any(|l| l == SILENCE),
            deaf,
            "deaf={deaf}: the bar said the wrong thing about what it hears: {said:?}"
        );
        // The dot carries it too, for anything that cannot read a colour.
        assert_eq!(
            said.iter()
                .any(|l| l == "connected, but nothing is arriving"),
            deaf,
            "deaf={deaf}: the dot said the wrong thing: {said:?}"
        );
        // And the numbers come out unasked, because this is the one moment
        // they are the point -- a phone cannot hover the clock to ask for
        // them, and "nothing is coming through" is worth more with its own
        // evidence under it.
        assert_eq!(
            said.iter().any(|l| l.contains("recv 0")),
            deaf,
            "deaf={deaf}: the numbers were wrong to be there or wrong to be missing: {said:?}"
        );
    }
}

/// **A call across two exchanges says which way the sound goes.**
///
/// Neither side dialled the other, so there is no introduction to report and
/// `path` stays unsettled — which left the bar silent about a call whose
/// media is travelling through two exchanges. The ordinary call is the
/// control: nothing is settled for it either, and it must stay silent.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_across_two_exchanges_says_so() {
    use egui_kittest::Harness;

    for cross in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let egui_ctx = egui::Context::default();
        let mut app = app_at(dir.path().to_path_buf());
        let one = Account::unlocked_for_test([1u8; 32]);
        let me = one.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![one]);
        pass(&mut app, &mut accounts, &egui_ctx);

        // `path` is None in both: that is the state a cross-exchange call is
        // always in, and the difference has to come from the call being one.
        let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            me: Some(me),
            ..Default::default()
        });
        if cross {
            app.hold_cross_call_for_test(me, handle);
        } else {
            app.hold_call_for_test(me, [3u8; 32], 9, handle);
        }

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
                        away: false,
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                });
            });
        harness.run_steps(3);
        let said = labels(&harness);
        assert_eq!(
            said.iter().any(|l| l == "via both exchanges"),
            cross,
            "cross={cross}: the bar said the wrong thing about the path: {said:?}"
        );
        // The ordinary call is the control, and it is not silent either --
        // it is carried by one exchange, and says so. The two wordings must
        // not be confused: one exchange between you is not two.
        assert_eq!(
            said.iter().any(|l| l == "via exchange"),
            !cross,
            "cross={cross}: the single-exchange wording is wrong here: {said:?}"
        );
        assert!(
            !said.iter().any(|l| l == "direct"),
            "cross={cross}: no introduction was made and the bar says direct: {said:?}"
        );
    }
}

/// **A setting that cannot apply here says so.**
///
/// "Calls connect directly" is on by default, and a call to somebody at
/// another exchange is relayed by both of them whatever it says — SIP-39
/// §Rationale, "always relay; no direct-connect attempt first". So somebody
/// who turns that switch on, calls a person at another exchange and reads
/// "via both exchanges" has been handed a label and no reason, and the
/// reasonable thing to conclude is that the switch does nothing. Found by
/// being asked why the other side never asked for an introduction.
///
/// The single-exchange call is the control: it is carried too, and must
/// *not* blame a setting that could really have applied to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_cross_exchange_call_says_the_setting_could_not_apply() {
    use egui_kittest::Harness;

    const NAMED: &str = "Calls connect directly";
    for cross in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let egui_ctx = egui::Context::default();
        let mut app = app_at(dir.path().to_path_buf());
        let one = Account::unlocked_for_test([1u8; 32]);
        let me = one.unlocked().expect("an open account").me();
        let mut accounts = Accounts::of(vec![one]);
        pass(&mut app, &mut accounts, &egui_ctx);

        // Unsettled in both: that is the state a cross-exchange call is
        // always in, so the difference has to come from the call being one.
        let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
            phase: sigil_net::Phase::Live,
            me: Some(me),
            ..Default::default()
        });
        if cross {
            app.hold_cross_call_for_test(me, handle);
        } else {
            app.hold_call_for_test(me, [3u8; 32], 9, handle);
        }

        // A phone, where the reason is drawn rather than hovered -- which is
        // the only form a test can read it in.
        let mut harness = Harness::builder()
            .with_size(egui::vec2(360.0, 804.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
                sigil::Form::install(&ctx, sigil::Form::Phone);
                ctx.set_theme(egui::Theme::Dark);
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        accounts: &mut accounts,
                        unfocused: false,
                        away: false,
                        notify: &Silent,
                        connections: &Default::default(),
                    };
                    // **Through `render_nav`, as the shell does.** The call bar is drawn
                    // there and not in `render`, so that it survives every route: a
                    // call was invisible the moment somebody opened Members or
                    // Settings. Driving `render` directly here would be testing an
                    // entry the application never takes on its own.
                    let token: std::rc::Rc<dyn std::any::Any> =
                        std::rc::Rc::new(sigil_chat::Route::Conversations);
                    let _ = app.render_nav(&mut app_ctx, ui, &token);
                });
            });
        harness.run_steps(3);
        let said = labels(&harness);
        let named = said.iter().any(|l| l.contains(NAMED));
        assert_eq!(
            named, cross,
            "cross={cross}: naming the setting was {named}, wanted {cross}: {said:?}"
        );
    }
}

/// **What a call is carrying is one tap away, and not before.**
///
/// The engine reports a line every second — sent, received, loss, late,
/// duplicate, concealed, underruns, round trip — and nothing ever drew it,
/// so a call carrying nothing looked exactly like a call carrying
/// everything. The clock opens it. The closed case is the control: it
/// proves the assertion can fail.
#[tokio::test(flavor = "multi_thread")]
async fn the_clock_opens_what_a_call_is_carrying() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    const LINE: &str = "sent 120 · recv 0 · loss 100.0%";

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());
    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);

    // **Not deaf**, deliberately. A call that hears nothing draws its
    // numbers unasked -- that is the whole point of the other test -- so
    // using one here would measure the deafness and never the clock.
    let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
        phase: sigil_net::Phase::Live,
        me: Some(me),
        deaf: false,
        stats: Some(LINE.to_string()),
        ..Default::default()
    });
    app.hold_call_for_test(me, [3u8; 32], 9, handle);

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
                    away: false,
                    notify: &Silent,
                    connections: &Default::default(),
                };
                // **Through `render_nav`, as the shell does.** The call bar is drawn
                // there and not in `render`, so that it survives every route: a
                // call was invisible the moment somebody opened Members or
                // Settings. Driving `render` directly here would be testing an
                // entry the application never takes on its own.
                let token: std::rc::Rc<dyn std::any::Any> =
                    std::rc::Rc::new(sigil_chat::Route::Conversations);
                let _ = app.render_nav(&mut app_ctx, ui, &token);
            });
        });
    harness.run_steps(3);

    // Closed: a working call needs no numbers, so none are drawn.
    let said = labels(&harness);
    assert!(
        !said.iter().any(|l| l.contains("sent 120")),
        "the numbers are on screen before anybody asked: {said:?}"
    );
    // **The way in is the bar, and the clock inside the card.**
    //
    // The bar's clock used to be the toggle, because on a handset there was
    // nowhere else to look. There is now: the bar opens the call, and the
    // card carries the numbers. Leaving the clock a control as well made the
    // middle of the bar do something other than what the bar does -- egui
    // gives a press to the innermost widget that senses it -- so a tap aimed
    // at the call opened the numbers instead.
    let clock = said
        .iter()
        .find(|l| l.len() == 5 && l.as_bytes()[2] == b':')
        .cloned()
        .unwrap_or_else(|| panic!("no clock on the call bar: {said:?}"));
    // **Its role, not its presence.** The clock is still drawn and still in
    // the tree -- `query_by_label` finds a label as readily as a button, which
    // is exactly the confusion that makes "is it there" the wrong question.
    // What must have changed is that it is no longer something to press.
    fn role_of(h: &egui_kittest::Harness<'static>, label: &str) -> Option<String> {
        fn walk(node: egui_kittest::Node<'_>, label: &str, out: &mut Option<String>) {
            let n = node.accesskit_node();
            // Both, as `labels` does: a plain label carries its text as a
            // `value` where a button carries it as a `label`, which is itself
            // the difference being asserted here.
            let says = n
                .label()
                .map(|l| l.to_string())
                .or_else(|| n.value().map(|v| v.to_string()));
            if says.as_deref() == Some(label) && out.is_none() {
                *out = Some(format!("{:?}", n.role()));
            }
            for c in node.children() {
                walk(c, label, out);
            }
        }
        let mut found = None;
        walk(h.root(), label, &mut found);
        found
    }
    let role = role_of(&harness, &clock).expect("the clock is still drawn");
    assert!(
        !role.contains("Button"),
        "the bar's clock is a {role}, so a tap on the bar has two meanings"
    );

    harness.get_by_label("Open the call").click();
    harness.run_steps(3);
    // The card is a route, and this harness holds one pane rather than a
    // navigator, so what it can say is that the press asked for the call --
    // `the_bar_opens_the_card` holds that, and `call_card_ui` draws the
    // numbers from the same `Live::detail` this used to toggle.
}

/// **While it is still connecting, nothing is claimed.**
///
/// The control for the rule that an unset `path` on a live call means
/// "carried". Before the call is up there is nothing to be carried yet, and
/// a word that appears and then changes under somebody reading it is worse
/// than a word that waits.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_still_connecting_says_nothing_about_the_path() {
    use egui_kittest::Harness;

    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = app_at(dir.path().to_path_buf());
    let one = Account::unlocked_for_test([1u8; 32]);
    let me = one.unlocked().expect("an open account").me();
    let mut accounts = Accounts::of(vec![one]);
    pass(&mut app, &mut accounts, &egui_ctx);

    let handle = sigil_net::CallHandle::for_test(sigil_net::CallState {
        phase: sigil_net::Phase::Connecting,
        me: Some(me),
        ..Default::default()
    });
    app.hold_call_for_test(me, [3u8; 32], 9, handle);

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
                    away: false,
                    notify: &Silent,
                    connections: &Default::default(),
                };
                // **Through `render_nav`, as the shell does.** The call bar is drawn
                // there and not in `render`, so that it survives every route: a
                // call was invisible the moment somebody opened Members or
                // Settings. Driving `render` directly here would be testing an
                // entry the application never takes on its own.
                let token: std::rc::Rc<dyn std::any::Any> =
                    std::rc::Rc::new(sigil_chat::Route::Conversations);
                let _ = app.render_nav(&mut app_ctx, ui, &token);
            });
        });
    harness.run_steps(3);
    let said = labels(&harness);
    assert!(
        said.iter().any(|l| l == "Connecting…"),
        "the call is not on screen: {said:?}"
    );
    for word in ["direct", "via exchange", "via both exchanges"] {
        assert!(
            !said.iter().any(|l| l == word),
            "it claimed {word:?} before the call was up: {said:?}"
        );
    }
}

/// **The card's mute reaches the call, and the call is what the card reads.**
///
/// The button does not remember its own state: it draws `CallState::muted`,
/// which `CallHandle::set_muted` writes. So a press that flips the icon proves
/// the whole path -- control, handle, snapshot, paint -- and a press that only
/// flipped a `bool` in the widget would pass a test that watched the widget.
#[tokio::test(flavor = "multi_thread")]
async fn muting_from_the_card_reaches_the_call() {
    let (mut h, _routes, held, _dir) =
        phone_call_routes(sigil_chat::Route::Call(PubKey::new([0u8; 32])), live());
    h.run_steps(3);
    assert_eq!(
        held.borrow().len(),
        1,
        "no call is held, so the card drew nothing and this says nothing"
    );

    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "Mute your microphone"),
        "the card offers no mute at all: {said:?}"
    );
    assert!(
        !said.iter().any(|l| l == "Unmute your microphone"),
        "it is already muted before anybody pressed anything: {said:?}"
    );

    h.get_by_label("Mute your microphone").click();
    h.run_steps(3);

    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "Unmute your microphone"),
        "pressing mute changed nothing the card can see: {said:?}"
    );
    assert!(
        !said.iter().any(|l| l == "Mute your microphone"),
        "both are offered at once: {said:?}"
    );
    assert_eq!(
        held.borrow().len(),
        1,
        "muting ended the call — a mute is not a hang-up"
    );

    // And back, because a mute nobody can undo is a call nobody can speak on.
    h.get_by_label("Unmute your microphone").click();
    h.run_steps(3);
    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "Mute your microphone"),
        "unmuting did not take: {said:?}"
    );
}

/// A call that is already muted when the card opens says so.
///
/// The control is drawn from the snapshot rather than from anything the card
/// has watched happen, and this is the only case where those differ.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_muted_before_the_card_opened_says_so() {
    let (mut h, _routes, held, _dir) = phone_call_routes(
        sigil_chat::Route::Call(PubKey::new([0u8; 32])),
        sigil_net::CallState {
            muted: true,
            ..live()
        },
    );
    h.run_steps(3);
    assert_eq!(held.borrow().len(), 1, "no call is held");
    let said = labels(&h);
    assert!(
        said.iter().any(|l| l == "Unmute your microphone"),
        "a muted call drew the mute button as though it were open: {said:?}"
    );
}

/// **A phone that cannot route draws no routing control**, rather than a
/// disabled one. `Silent` -- the harness's notifier -- is not routable, which
/// is what every test here runs with.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_on_a_device_that_cannot_route_draws_no_routing_control() {
    let (mut h, _routes, held, _dir) =
        phone_call_routes(sigil_chat::Route::Call(PubKey::new([0u8; 32])), live());
    h.run_steps(3);
    assert_eq!(held.borrow().len(), 1, "no call is held");
    let said = labels(&h);
    for word in ["Play through the loudspeaker", "Play through the earpiece"] {
        assert!(
            !said.iter().any(|l| l == word),
            "{word:?} is offered where the platform cannot do it: {said:?}"
        );
    }
    assert!(
        said.iter().any(|l| l == "Mute your microphone"),
        "and the card drew no controls at all, so this proves nothing: {said:?}"
    );
}
