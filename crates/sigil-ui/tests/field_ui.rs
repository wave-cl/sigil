//! A field says what it is for, to whoever cannot see it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sigil::theme;

fn harness() -> Harness<'static> {
    let mut key = String::new();
    let mut secret = String::new();
    Harness::builder()
        .with_size(egui::vec2(400.0, 200.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::field(ui, &mut key, "key, base58", 200.0);
            sigil_ui::password_field(ui, &mut secret, "passphrase", 200.0);
        })
}

/// **A field has a name, not only a placeholder.**
///
/// egui carries `hint_text` into the accessibility tree as accesskit's
/// *placeholder*, and a placeholder is not a name: it is announced only
/// while the box is empty, and `label()` stays `None`. So every field in
/// sigil arrived as a nameless text input — "text edit, blank" — with
/// nothing to say what goes in it.
///
/// Found sideways, writing a test that tried to count boxes by their hint
/// and counted nothing. `get_by_label` is the assertion because it is the
/// question a screen reader asks: what is this called?
#[test]
fn a_field_is_named_by_what_goes_in_it() {
    let mut h = harness();
    h.run();
    assert!(
        h.query_by_label("key, base58").is_some(),
        "a field with no name: a reader who cannot see the box is told only \
         \"text edit, blank\""
    );
}

/// The same for a passphrase box, which is the one where being unnamed is
/// worst: there is nothing else on that screen to guess from.
#[test]
fn a_passphrase_field_is_named_too() {
    let mut h = harness();
    h.run();
    assert!(
        h.query_by_label("passphrase").is_some(),
        "the passphrase box has no name"
    );
}
