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
                        unfocused: false,
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
    assert!(accounts.add_exchange(0, "a.example"));
    assert!(accounts.add_exchange(0, "b.example"));
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
    assert!(accounts.add_exchange(0, "a.example"));
    assert!(accounts.add_exchange(0, "b.example"));
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
                    notify: &sigil::Silent,
                    connections: &Default::default(),
                };
                app.chrome_ui(&mut app_ctx, ui);
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
