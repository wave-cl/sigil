//! Widgets shared between sigil's apps.
//!
//! Deliberately knows nothing of the protocol: everything here takes plain
//! data, so this does not become a second place the wire format is understood.

pub mod attachment;
pub mod clock;
pub mod conversation_row;
pub mod dot;

pub mod identicon;
pub mod message;
pub mod roster;
pub mod working;

pub use attachment::{Attachment, AttachmentAction, attachment};
pub use clock::{brief, clock, day_label, day_of, stamp};
pub use conversation_row::{ConversationRow, conversation_row};
pub use dot::dot;
pub use working::working;
// Re-exported from the host crate, where the `App` trait names one -- see
// `sigil::icon`. Every `sigil_ui::Icon` still resolves.
pub use identicon::{avatar, identicon, identicon_of};

/// Teach egui how to decode an image.
///
/// # Why nothing drew
///
/// `egui::Image::from_bytes` does not decode anything itself — it hands the
/// bytes to a registered loader, and with none registered it draws a broken
/// picture. Nothing called this, so **every image attachment came out as a red
/// triangle**, which reads as "this file is damaged" and was nothing of the
/// sort. `egui_extras` is already a dependency with `all_loaders`; it was
/// simply never switched on.
///
/// Call it once per `Context`, beside `theme::install`. Idempotent.
pub fn install_loaders(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// sigil's own mark: a disc in the accent.
///
/// # Why a disc and not a picture
///
/// It is what the application icon and the tray icon are —
/// `packaging/icon.py` draws exactly this, from the same colour, and says why
/// there is no icon file in the repository: a checked-in blob is one more
/// thing to drift from the mark it is supposed to match, and one nobody can
/// diff. Drawing it here from the theme keeps the third copy from being a
/// fourth number.
///
/// It follows the theme, so it is the brighter accent on a dark ground and
/// the deeper one on a light ground — the same mark, legible on both, rather
/// than one fixed colour that is wrong on one of them.
pub fn mark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let theme = sigil::ColorTheme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        // The same inset the icon uses, so the disc does not sit flush to its
        // own bounds and read as larger than everything beside it.
        ui.painter()
            .circle_filled(rect.center(), size * 0.43, theme.accent);
    }
    response
}

/// A text field, at the size a text field should be.
///
/// # Why this exists rather than a `TextEdit` at each call site
///
/// Every field in sigil was egui's default: one line of text tall, with a
/// placeholder that vanished the moment anybody typed. Two problems in one
/// control. It reads as a rule somebody wrote on rather than a box to fill in,
/// and it is a small target; and the only thing saying what it was for
/// disappeared exactly when somebody might have wanted to check.
///
/// So: [`tokens::FIELD_MD`] tall, padded, and the hint is a **sentence about
/// what the field does** rather than a one-word restatement of its label. The
/// label stays outside it and stays visible, because a hint never reaches the
/// accessibility tree at all.
pub fn field(ui: &mut egui::Ui, buf: &mut String, hint: &str, width: f32) -> egui::Response {
    field_as(ui, buf, hint, width, false)
}

/// The same, for something that must not be shown as it is typed.
///
/// A separate function rather than a flag at every call site, so that a field
/// which should be masked cannot be written unmasked by leaving an argument
/// off — and so the two share the one rule about how tall a field is and where
/// its text sits in it.
pub fn password_field(
    ui: &mut egui::Ui,
    buf: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    field_as(ui, buf, hint, width, true)
}

fn field_as(
    ui: &mut egui::Ui,
    buf: &mut String,
    hint: &str,
    width: f32,
    password: bool,
) -> egui::Response {
    // The vertical padding is **measured against the line**, not a token. A
    // fixed 8px in a 40px box leaves the text sitting a few pixels above
    // centre — not enough to name, enough to look wrong in every field at
    // once.
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let above = ((sigil::tokens::FIELD_MD - line) / 2.0).max(0.0);
    ui.add_sized(
        [width, sigil::tokens::FIELD_MD],
        egui::TextEdit::singleline(buf)
            .password(password)
            .hint_text(hint)
            .margin(egui::Margin::symmetric(
                sigil::tokens::SPACING_MD as i8,
                above as i8,
            )),
    )
}
pub use message::{
    Bubble, BubbleAction, Quote, Receipt, bubble, day_separator, reaction_chip, short, system_line,
    unread_divider, unread_pill,
};
pub use roster::{Row, roster};
pub use sigil::icon;
pub use sigil::icon::{Icon, icon_button, icon_button_named, icon_button_tinted, icon_item};
