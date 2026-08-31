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
    let mut account = account;
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
                        account: &mut account,
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

fn unlocked(dir: &std::path::Path) -> Account {
    let path = dir.join("identity");
    sqnr::identity::generate(&path, None).unwrap();
    Account::discover(Some(path))
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
    assert!(
        said.contains("Nobody yet"),
        "an empty list says what to do about it: {said}"
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
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    h.snapshot("chat_dark");
}
