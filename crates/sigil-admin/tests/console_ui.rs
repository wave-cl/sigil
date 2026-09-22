//! What the operator console says, and what it refuses to do quietly.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, theme};
use sigil_admin::{AdminApp, AdminState, Answer};
use sqnr_core::PubKey;

fn account() -> Account {
    Account::unlocked_for_test([1u8; 32])
}

fn harness(state: AdminState) -> Harness<'static> {
    sized(state, egui::vec2(900.0, 700.0), sigil::Form::Desktop)
}

/// The same console on a phone: 360 points, and the form that says the
/// controls are pressed with a finger.
///
/// The console had no phone render at all, and it is the pane most likely to
/// be a desktop's: it is rows of operations with their buttons beside them.
fn harness_phone(state: AdminState) -> Harness<'static> {
    sized(state, egui::vec2(360.0, 804.0), sigil::Form::Phone)
}

/// How wide the pane's contents came out. See
/// `the_console_is_not_wider_than_the_phone`.
type Drawn = std::rc::Rc<std::cell::Cell<f32>>;

fn sized(state: AdminState, size: egui::Vec2, form: sigil::Form) -> Harness<'static> {
    sized_measured(state, size, form).0
}

fn sized_measured(
    state: AdminState,
    size: egui::Vec2,
    form: sigil::Form,
) -> (Harness<'static>, Drawn) {
    let drawn: Drawn = std::rc::Rc::new(std::cell::Cell::new(0.0));
    let width = drawn.clone();
    let mut app = AdminApp::new();
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    let h = Harness::builder().with_size(size).build_ui(move |ui| {
        let ctx = ui.ctx().clone();
        sigil::Form::install(&ctx, form);
        theme::install(&ctx, theme::light(), theme::dark());
        ctx.set_theme(egui::Theme::Dark);
        let t = sigil::ColorTheme::current(&ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.surface_primary)
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

fn up() -> AdminState {
    AdminState {
        admin: Some(PubKey::new([1u8; 32])),
        exchange: Some(PubKey::new([2u8; 32])),
        healthy: Some(true),
        ..Default::default()
    }
}

/// Nothing is signed without being shown first.
///
/// `sign_and_submit`'s `on_review` callback runs *during* signing, after the
/// nonce is fetched — it reports and cannot ask. So the question has to be
/// asked by the interface, before the transaction exists at all, and a console
/// that submitted straight from a button would sign things nobody had read.
#[test]
fn an_operation_is_shown_before_it_is_signed() {
    let mut h = harness(up());
    h.run();
    // Nothing pending yet, so no confirmation.
    assert!(!text_of(&h).contains("Sign this?"));

    h.get_by_label("Enable").click();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Sign this?"),
        "pressing an operation asks first: {said}"
    );
    assert!(
        said.contains("Sign and submit") && said.contains("Cancel"),
        "and offers both answers: {said}"
    );
    // A batch is applied atomically at the exchange, so what is agreed to is
    // the batch and not the operations one at a time.
    assert!(
        said.contains("all of it or none of it"),
        "and says what agreeing means: {said}"
    );
}

/// The confirmation is not something you can scroll away from.
#[test]
fn the_console_is_hidden_while_a_signature_is_being_asked_for() {
    let mut h = harness(up());
    h.run();
    assert!(text_of(&h).contains("Whitelist"));
    h.get_by_label("Enable").click();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Relay peers"),
        "the rest of the console stands down while a signature is being asked \
         for -- a question that can be scrolled past is one answered by \
         accident: {said}"
    );
}

/// Cancelling signs nothing and leaves the console where it was.
#[test]
fn cancelling_leaves_nothing_pending() {
    let mut h = harness(up());
    h.run();
    h.get_by_label("Enable").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    let said = text_of(&h);
    assert!(!said.contains("Sign this?"), "{said}");
    assert!(said.contains("Whitelist"), "the console is back: {said}");
}

/// "It is answering" and "you may administer it" are different facts.
#[test]
fn health_is_not_presented_as_authority() {
    let mut h = harness(up());
    h.run();
    let said = text_of(&h);
    assert!(said.contains("answering"), "{said}");
    assert!(
        said.contains("its answer, not this window's"),
        "the console must not let a healthy exchange read as an admitted one: {said}"
    );
}

/// A refusal is shown as what the exchange said, not as a summary of it.
#[test]
fn a_refusal_is_shown_as_it_came() {
    let mut state = up();
    state.answers = vec![Answer {
        asked: "enable the managed whitelist".into(),
        said: "forbidden (403) not an administrator".into(),
        refused: true,
    }];
    let mut h = harness(state);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("not an administrator"), "{said}");
    assert!(said.contains("enable the managed whitelist"), "{said}");
    // And where the eye is: above the sections, not in a list at the foot
    // of the page, where an applied operation looked like one that did
    // nothing.
    let reply = said.find("forbidden (403)").unwrap();
    let first_section = said.find("Whitelist").unwrap();
    assert!(
        reply < first_section,
        "the newest answer is below the controls: {said}"
    );
}

/// The console acts on the exchange chosen in the title strip -- the same
/// answer the chat reads -- and its control offers the identity's exchanges.
///
/// Two named exchanges and no default: the borrowing rule alone declines to
/// choose, so before somebody chooses there is no console at all; after,
/// there is one, on the one chosen, and choosing the other moves it.
#[test]
fn the_console_follows_the_exchange_chosen_in_the_title_strip() {
    use sigil_net::{Connections, Held};
    let account = account();
    let me = account.unlocked().unwrap().me();
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    assert!(accounts.add_exchange(0, "a.example", None));
    assert!(accounts.add_exchange(0, "b.example", None));
    // Slots for both, as the chat's sessions would lend them: empty, so the
    // console waits on them rather than dialling.
    let connections = Connections::new();
    connections.lend(me, "a.example", Held::empty());
    connections.lend(me, "b.example", Held::empty());

    let mut app = AdminApp::new();
    let egui_ctx = egui::Context::default();
    let mut nav = Navigator::default();
    let mut run = |accounts: &mut sigil::accounts::Accounts, app: &mut AdminApp| {
        let mut app_ctx = AppContext {
            navigator: &mut nav,
            accounts,
            unfocused: false,
            away: false,
            notify: &sigil::Silent,
            connections: &connections,
        };
        app.update(&mut app_ctx, &egui_ctx);
    };
    run(&mut accounts, &mut app);
    assert_eq!(
        app.acting_on_for_test(me),
        None,
        "nothing chosen, two to pick from: no console"
    );

    accounts.show_exchange(me, Some("b.example".into()));
    run(&mut accounts, &mut app);
    assert_eq!(app.acting_on_for_test(me).as_deref(), Some("b.example"));

    accounts.show_exchange(me, Some("a.example".into()));
    run(&mut accounts, &mut app);
    assert_eq!(app.acting_on_for_test(me).as_deref(), Some("a.example"));

    // A choice of something the identity is not connected to is not acted
    // on: the console stays where it can actually reach.
    accounts.show_exchange(me, Some("c.example".into()));
    run(&mut accounts, &mut app);
    assert_eq!(app.acting_on_for_test(me), None);
}

/// The control in the title strip lists the identity's exchanges by name
/// and marks the one the console is on.
#[test]
fn the_title_strip_offers_the_exchanges_to_administer() {
    let account = account();
    let me = account.unlocked().unwrap().me();
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    assert!(accounts.add_exchange(0, "a.example", None));
    assert!(accounts.add_exchange(0, "b.example", None));
    accounts.show_exchange(me, Some("b.example".into()));
    let mut app = AdminApp::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 200.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut nav = Navigator::default();
                let mut app_ctx = AppContext {
                    navigator: &mut nav,
                    accounts: &mut accounts,
                    unfocused: false,
                    away: false,
                    notify: &sigil::Silent,
                    connections: &Default::default(),
                };
                // The nav entry being drawn. These harnesses show an app's
                // root, which has no name of its own.
                let token: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(());
                app.chrome_ui(&mut app_ctx, ui, &token);
            });
        });
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("b.example"),
        "the chosen one is shown: {said}"
    );
    h.get_by_label("Exchange").click();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("a.example") && said.contains("b.example"),
        "{said}"
    );
    assert!(
        !said.contains("Add a domain"),
        "adding is the chat's: {said}"
    );
}

/// A press on Add or Remove with something that is not a key says so, in
/// words that say what it was, rather than doing nothing. A shortened key
/// copied off the screen was the case that looked like the exchange
/// refusing.
#[test]
fn a_key_that_is_not_one_is_refused_in_words() {
    let mut h = harness(up());
    h.run();
    // The first box and the first Add on the screen are the whitelist's.
    fn field<'a>(h: &'a Harness<'static>) -> egui_kittest::Node<'a> {
        h.get_all(
            egui_kittest::kittest::by()
                .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
        )
        .next()
        .expect("a key box")
    }
    fn add(h: &Harness<'static>) {
        h.get_all_by_label("Add").next().expect("an Add").click();
    }
    for (typed, expect) in [
        ("8qbH…VfeR", "shortened for display"),
        ("colin@squic.org", "a handle, not a key"),
        ("not a key", "not a key"),
    ] {
        let f = field(&h);
        f.focus();
        f.type_text(typed);
        h.run();
        add(&h);
        h.run();
        let said = text_of(&h);
        assert!(
            !said.contains("Sign this?"),
            "{typed:?} was proposed: {said}"
        );
        assert!(
            said.contains(expect),
            "{typed:?} refused without saying why: {said}"
        );
        // The box keeps what was typed, to be corrected rather than retyped;
        // cleared here for the next case.
        let f = field(&h);
        f.focus();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.key_press(egui::Key::Backspace);
        h.run();
    }
    // A whole key is proposed, and the words go.
    let f = field(&h);
    f.focus();
    f.type_text(&PubKey::new([9u8; 32]).to_string());
    h.run();
    add(&h);
    h.run();
    let said = text_of(&h);
    assert!(said.contains("Sign this?"), "{said}");
    assert!(!said.contains("not a key"), "{said}");
}

/// The console on a desktop.
///
/// Its partner, so the narrow arm cannot be improved at the wide one's
/// expense without the diff saying so. There was no render of either.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn console_dark() {
    let mut h = harness(up());
    h.run();
    h.run();
    h.snapshot("console_dark");
}

/// The console, drawn on a phone.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn console_phone() {
    let mut h = harness_phone(up());
    h.run();
    h.run();
    h.snapshot("console_phone");
}

/// The same console with an exchange's reply in it, as long as one really is.
///
/// # Why the empty one is not enough
///
/// `up()` is a console nobody has asked anything. Every interesting thing on
/// that screen arrives as an *answer*: a whitelist is a column of base58
/// keys, an audit tail is lines of JSON, and neither has a space in it for
/// dozens of characters at a stretch. A width check run on a pane with
/// nothing in it agrees with itself.
fn answered() -> AdminState {
    let mut state = up();
    let keys: Vec<String> = (1..=4)
        .map(|i| PubKey::new([i as u8; 32]).to_string())
        .collect();
    state.answers = vec![Answer {
        asked: "whitelist/list at an-exchange-with-a-long-name.example.org".into(),
        said: format!(
            "{{\n  \"keys\": [\n    \"{}\"\n  ]\n}}",
            keys.join("\",\n    \"")
        ),
        refused: false,
    }];
    state
}

/// The console with an answer in it fits a phone too.
#[test]
fn an_answer_does_not_make_the_console_wider_than_the_phone() {
    const PHONE: f32 = 360.0;
    let (mut h, drawn) = sized_measured(answered(), egui::vec2(PHONE, 804.0), sigil::Form::Phone);
    h.run();
    h.run();
    let width = drawn.get();
    assert!(
        width > 0.0,
        "the console drew nothing, so this proves nothing"
    );
    assert!(
        width <= PHONE + 1.0,
        "an answer makes the console {width} points wide in a {PHONE}-point pane"
    );
}

/// And a picture of it, because a key that wraps mid-word is legible and a
/// key that is cut off is not, and only looking says which this is.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn console_phone_answered() {
    let mut h = harness_phone(answered());
    h.run();
    h.run();
    h.snapshot("console_phone_answered");
}

/// The console fits a phone, without anybody rendering it and looking.
///
/// It did not: its rows were a key box, a label box and two buttons at
/// widths chosen against a 900-point window, so the pane came out half again
/// as wide as the phone -- and egui, which grows a ui to whatever is drawn
/// in it, then laid the prose out for that width and let the pane clip it.
/// Every explanation on the console ended mid-word, and nothing said so,
/// because nothing had ever drawn this screen narrow.
///
/// No renderer, so it runs in an ordinary `cargo test`.
#[test]
fn the_console_is_not_wider_than_the_phone() {
    const PHONE: f32 = 360.0;
    let (mut h, drawn) = sized_measured(up(), egui::vec2(PHONE, 804.0), sigil::Form::Phone);
    h.run();
    h.run();
    let width = drawn.get();
    assert!(
        width > 0.0,
        "the console drew nothing, so this proves nothing"
    );
    assert!(
        width <= PHONE + 1.0,
        "the console draws {width} points wide in a {PHONE}-point pane"
    );
}

/// **A long answer does not push the console off the screen.**
///
/// The last answer is pinned above the operations, because a reply below the
/// fold reads as a button that did nothing. With no height of its own it did
/// exactly that in the other direction: an audit tail of fifty lines is some
/// 2400 points, it is drawn *outside* the scroll area that holds the
/// operations, and on a phone the Whitelist heading sat at y 2392 of an
/// 804-point screen. Every operation the console offers was three screens
/// down, with nothing to scroll.
///
/// Fifty lines is a small audit. The preview scrolls inside its own bound
/// now, and the whole answer is still in Answers, at the foot, which is what
/// this is a preview of.
#[test]
fn a_long_answer_does_not_push_the_console_off_the_screen() {
    const PHONE: f32 = 360.0;
    const TALL: f32 = 804.0;
    let mut state = up();
    let lines: Vec<String> = (0..50)
        .map(|i| {
            format!(
                "  {{\"seq\": {i}, \"op\": \"whitelist/add\", \"by\": \
                 \"4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi\"}}"
            )
        })
        .collect();
    state.answers = vec![Answer {
        asked: "audit last 50".into(),
        said: lines.join("\n"),
        refused: false,
    }];
    let (mut h, _) = sized_measured(state, egui::vec2(PHONE, TALL), sigil::Form::Phone);
    h.run();
    h.run();

    let top = h
        .get_all_by_label_contains("Whitelist")
        .map(|n| n.rect().top())
        .fold(f32::MAX, f32::min);
    assert!(
        top < f32::MAX,
        "the console's first operation is not drawn at all, so this says \
         nothing about where it is"
    );
    assert!(
        top <= TALL,
        "the console's first operation sits at y {top:.0} of {TALL}: the \
         pinned answer has pushed the whole console below the screen"
    );

    // **And on a desktop, where there is room to be greedy.** A scroll area
    // clamps to the height it is given, so on a phone the bound hardly
    // matters -- the pane is short and the clamp does the work. On a
    // 700-point window it would take six hundred of them for a *preview*,
    // and the operations would be below the fold on a desktop instead. That
    // is what `LAST_ANSWER` is for, and without it this is the assertion
    // that notices.
    let (mut wide, _) = sized_measured(
        {
            let mut s = up();
            s.answers = vec![Answer {
                asked: "audit last 50".into(),
                said: lines.join("\n"),
                refused: false,
            }];
            s
        },
        egui::vec2(900.0, 700.0),
        sigil::Form::Desktop,
    );
    wide.run();
    wide.run();
    let top = wide
        .get_all_by_label_contains("Whitelist")
        .map(|n| n.rect().top())
        .fold(f32::MAX, f32::min);
    // The top half of the window: the operations are what the console is
    // for, and a preview of the last reply must not be more than half of
    // what is on screen before them.
    assert!(
        top <= 350.0,
        "on a 700-point window the console's first operation sits at y \
         {top:.0}: a preview is taking the window"
    );
}

/// **Nothing in the console is drawn where a finger cannot get to it.**
///
/// The console is the longest pane sigil has -- rows of operations with
/// their buttons beside them -- and the fault this exists for was here: a
/// fifty-line answer, pinned *outside* the scroll area that holds the
/// operations, put the Whitelist heading at y=2392 of an 804-point screen
/// with nothing to scroll. Every operation the console offers was three
/// screens down.
///
/// "Is it on screen" is the wrong question: this pane is taller than any
/// phone and always will be. So this scrolls to the end with a real wheel
/// over the middle of the screen -- which is what a finger is, and which
/// finds whichever scroll area is under it, including none -- and only then
/// asks whether anything is still below the bottom.
#[test]
fn nothing_in_the_console_is_out_of_reach_on_a_phone() {
    const TALL: f32 = 804.0;
    for (what, state) in [("up", up()), ("answered", answered())] {
        let mut h = harness_phone(state);
        h.run();
        h.run();
        let before = deepest(&h);
        // The control, inside the test: a pane that already fits has nothing
        // to scroll, and then scrolling proves nothing about it.
        assert!(
            before.1 > TALL as f64,
            "the console with {what} drew only {:.0} points on an \
             {TALL}-point screen, so the scroll below is not being asked \
             anything",
            before.1
        );
        // **Down the screen, not only at its middle.** The console nests: an
        // answer's preview is a bounded box inside the pane's own scroll
        // area, and a wheel only ever turns whatever is under the pointer.
        // A sweep at one height leaves every other box untouched, and a
        // widget inside an unscrolled box reports a content position far
        // below the screen -- which reads exactly like one nothing reaches.
        for y in [100.0f32, 300.0, 500.0, 700.0] {
            for _ in 0..40 {
                h.hover_at(egui::pos2(180.0, y));
                h.event(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::default(),
                });
                // Not `run`: a pane with a spinner repaints for ever, and
                // `run` panics when it exceeds its step budget.
                h.run_steps(2);
            }
        }
        let after = deepest(&h);
        assert!(
            after.1 <= TALL as f64 + 1.0,
            "with {what}, {:?} still ends at y={:.0} on an {TALL}-point \
             screen after scrolling to the end, so nothing reaches it",
            after.0,
            after.1
        );
    }
}

/// The deepest thing drawn, and what it is, in points.
///
/// The accesskit node's own bounding box, not `Node::rect`: that one
/// `expect`s a rectangle and the root has none, so it panics -- a test
/// failing for a reason that has nothing to do with the layout.
fn deepest(h: &Harness<'static>) -> (String, f64) {
    fn walk(node: egui_kittest::Node<'_>, ppp: f64, out: &mut Vec<(String, f64)>) {
        let n = node.accesskit_node();
        let name = n
            .label()
            .map(|l| l.to_string())
            .or_else(|| n.value().map(|v| v.to_string()))
            .unwrap_or_else(|| format!("{:?}", n.role()));
        if let Some(b) = n.bounding_box()
            && b.y1 > b.y0
        {
            out.push((name, b.y1 / ppp));
        }
        for c in node.children() {
            walk(c, ppp, out);
        }
    }
    let mut seen = Vec::new();
    walk(h.root(), h.ctx.pixels_per_point() as f64, &mut seen);
    seen.sort_by(|a, b| b.1.total_cmp(&a.1));
    seen.first().cloned().unwrap_or_default()
}
