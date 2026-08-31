//! What the voice app looks like in each of the states a person meets.
//!
//! Snapshots are `#[ignore]`d so an ordinary `cargo test` needs no GPU; the
//! accessibility assertions below run everywhere, because what the interface
//! *says* matters more than what it looks like and is cheaper to check.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::account::Account;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::theme;
use sigil_voice::VoiceApp;

/// Drive the app with a given account, as the shell would.
fn harness(account: Account, dark: bool) -> Harness<'static> {
    let mut app = VoiceApp::new();
    let mut account = account;
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
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
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        account: &mut account,
                        hidden: false,
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
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
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked_account(dir.path()), true);
    h.run();
    h.snapshot("voice_idle_dark");
}
