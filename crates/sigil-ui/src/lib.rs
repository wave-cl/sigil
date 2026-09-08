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
pub use message::{
    Bubble, BubbleAction, Receipt, bubble, day_separator, reaction_chip, system_line,
    unread_divider, unread_pill,
};
pub use roster::{Row, roster};
pub use sigil::icon;
pub use sigil::icon::{Icon, icon_button, icon_button_named, icon_button_tinted};
