//! What the chat app says in the states somebody actually meets.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::account::Account;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::theme;
use sigil_chat::ChatApp;

fn harness(account: Account) -> Harness<'static> {
    let mut app = ChatApp::new();
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
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

/// An open identity with a **random** key.
///
/// Right for a behaviour test and **wrong for a snapshot**: `generate` mints a
/// fresh key every run, and any view that draws a key in full then renders
/// differently every time. Use [`fixed`] where pixels are compared.
fn unlocked(dir: &std::path::Path) -> Account {
    let path = dir.join("identity");
    sqnr::identity::generate(&path, None).unwrap();
    Account::discover(Some(path))
}

/// An open identity whose key is the same on every run. Touches no filesystem.
fn fixed() -> Account {
    Account::unlocked_for_test([5u8; 32])
}

#[test]
fn a_sealed_identity_cannot_chat_yet_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, Some("open sesame")).unwrap();
    let mut h = harness(Account::discover(Some(path)));
    h.run();
    assert!(text_of(&h).contains("Unlock your identity"));
}

/// The connection light says the word as well as the colour. It matters more
/// here than anywhere: while the link is down, messages do not arrive, and
/// nothing happening looks exactly like nobody writing.
#[test]
fn the_connection_state_is_said_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("connected") || said.contains("reconnecting") || said.contains("offline"),
        "the link is named, not merely coloured: {said}"
    );
}

/// You can write to somebody who has never written to you, which needs their
/// key -- there is nothing else to look them up by.
#[test]
fn somebody_can_be_added_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Write to"),
        "the field is labelled, not only hinted: {said}"
    );
    assert!(said.contains("Add"), "{said}");
    // The property, not the wording: an empty list has to say *both* that it
    // is empty and what to do about it. A bare "nothing here" leaves somebody
    // looking for a control that is already on screen.
    assert!(
        said.contains("No conversations yet"),
        "an empty list says it is empty: {said}"
    );
    assert!(
        said.contains("key"),
        "and says what to do about it, which needs their key: {said}"
    );
}

/// A key that is not a key is refused where it was typed, rather than swallowed.
#[test]
fn a_bad_key_is_refused_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    // Nothing has been typed, so the empty field is not a key.
    h.get_by_label("Add").click();
    h.run();
    assert!(text_of(&h).contains("not a key"), "{}", text_of(&h));
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn chat_dark() {
    // A fixed key, because this pane draws one in full and a generated key
    // makes a snapshot that can never pass twice.
    let mut h = harness(fixed());
    h.run();
    h.snapshot("chat_dark");
}

/// Drawing the same state twice must produce the same thing.
///
/// # Why this exists
///
/// `chat_dark` began failing the moment a view drew the account key in full,
/// because its identity was **generated** — a fresh key every run, so the
/// snapshot could never pass twice. It was not caught locally because
/// `snapshot-test --update` followed by one verify only proves the file that
/// was just written matches the run that wrote it.
///
/// This is the cheap general form of that check: no renderer, no PNG, no
/// platform. Anything non-deterministic that reaches the screen — a generated
/// key, a live clock, a map iterated in hash order — shows up here as two
/// different readings of the same state, in the test that names the problem
/// rather than in a pixel diff on CI.
#[test]
fn the_same_state_draws_the_same_way_twice() {
    let read = || {
        let mut h = harness(fixed());
        h.run();
        text_of(&h)
    };
    assert_eq!(
        read(),
        read(),
        "something drawn here changes between runs, so no snapshot of it can pass twice"
    );
}
