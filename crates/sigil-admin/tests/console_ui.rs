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
    let mut app = AdminApp::new();
    app.show_state_for_test(state);
    let mut accounts = sigil::accounts::Accounts::of(vec![account()]);
    Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
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
                        connections: &Default::default(),
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
}
