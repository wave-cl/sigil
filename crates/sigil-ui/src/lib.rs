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

pub use attachment::{Attachment, AttachmentAction, attachment};
pub use clock::{brief, clock, day_label, day_of, stamp};
pub use conversation_row::{ConversationRow, conversation_row};
pub use dot::dot;
// Re-exported from the host crate, where the `App` trait names one -- see
// `sigil::icon`. Every `sigil_ui::Icon` still resolves.
pub use identicon::{avatar, identicon, identicon_of};

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
    ui.add_sized(
        [width, sigil::tokens::FIELD_MD],
        egui::TextEdit::singleline(buf)
            .hint_text(hint)
            .margin(egui::Margin::symmetric(
                sigil::tokens::SPACING_MD as i8,
                sigil::tokens::SPACING_SM as i8,
            )),
    )
}
pub use message::{
    Bubble, BubbleAction, Receipt, bubble, day_separator, reaction_chip, system_line,
    unread_divider, unread_pill,
};
pub use roster::{Row, roster};
pub use sigil::icon;
pub use sigil::icon::{Icon, icon_button, icon_button_named, icon_button_tinted};
