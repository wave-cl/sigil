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
        (None, None, None, "via exchange"),
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
                    let _ = app.render(&mut app_ctx, ui);
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

/// **The platform is told a call began, and told once.**
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
/// Through `update`, not by calling the method: the fault being fixed is that
/// nothing *reached* the hook, and a test that calls it directly would have
/// passed the whole time it was unreachable.
#[tokio::test(flavor = "multi_thread")]
async fn the_platform_is_told_a_call_began_and_ended_once_each() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let me = PubKey::new([1u8; 32]);
    let one = Account::unlocked_for_test([1u8; 32]);
    let mut app = app_at(dir.path().to_path_buf());
    let mut accounts = Accounts::of(vec![one]);
    let watching = Watching::default();

    pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    assert_eq!(
        watching.told(),
        Vec::<Option<String>>::new(),
        "with no call, the platform is told nothing at all -- not told 'no call'"
    );

    // A real handle to an address nothing answers on, as the tests above use.
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
    pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    let told = watching.told();
    assert_eq!(told.len(), 1, "the call beginning is said once: {told:?}");
    assert!(
        told[0].is_some(),
        "a call with no conversation to name is still a call: {told:?}"
    );

    // Passes with nothing changed.
    for _ in 0..3 {
        pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    }
    assert_eq!(
        watching.told(),
        Vec::<Option<String>>::new(),
        "a call that is still up is not announced again"
    );

    // And it ends, on its own, because nothing answered. The reaper runs in
    // `update`, so the same pass that notices also says so.
    for _ in 0..40 {
        pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
        if app.calls_for_test().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        app.calls_for_test().is_empty(),
        "the call never ended, so this says nothing about what was announced"
    );
    assert_eq!(watching.told(), vec![None], "the call ending is said once");

    pass_telling(&mut app, &mut accounts, &egui_ctx, &watching);
    assert_eq!(
        watching.told(),
        Vec::<Option<String>>::new(),
        "and stays ended without being said again"
    );
}
