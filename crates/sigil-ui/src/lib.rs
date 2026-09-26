//! Widgets shared between sigil's apps.
//!
//! Deliberately knows nothing of the protocol: everything here takes plain
//! data, so this does not become a second place the wire format is understood.

pub mod attachment;
pub mod call_card;
pub mod clock;
pub mod conversation_row;
pub mod dot;
pub mod emoji;
pub mod exchange;
pub mod gif;
pub mod identicon;
pub mod message;
pub mod qr;
pub mod roster;
pub mod search_hit;
pub mod video;
pub mod working;

pub use attachment::{Attachment, AttachmentAction, GalleryAction, attachment, gallery};
// **A size, written the one way.** `human` lived inside `attachment`, where
// anything that was not an attachment did not find it -- so the backup quota
// and the mailbox each printed a raw byte count instead, and the desktop's
// downloader grew a third one. Named here so the next screen that has bytes
// to show has somewhere obvious to look.
pub use attachment::human;
pub use clock::{brief, clock, day_label, day_of, deadline, stamp};
pub use conversation_row::{ConversationRow, conversation_row, one_line};
pub use dot::{dot, state_icon};
pub use exchange::{ExchangeAction, ExchangeRow, exchange_control};
pub use video::{Standing, Video, VideoAction, video};
pub use working::working;
// Re-exported from the host crate, where the `App` trait names one -- see
// `sigil::icon`. Every `sigil_ui::Icon` still resolves.
pub use identicon::{
    MARK_KEY, Presence, avatar, identicon, identicon_layer, identicon_of, identicon_raster,
    presence, presence_hover,
};

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
    // After, so it is tried first: egui asks the most recently added loader
    // before the rest, and `egui_extras`'s gif loader decodes on the thread
    // that asks. See `gif`.
    if !ctx.is_loader_installed(gif::GifLoader::ID) {
        ctx.add_image_loader(std::sync::Arc::new(gif::GifLoader::default()));
    }
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
///
/// # Except on a phone
///
/// A phone's launcher icon is not that disc: `scripts/launcher-icon` draws
/// the mark of the all-ones key (see [`MARK_KEY`]), which is what sits on the
/// home screen and what somebody tapped a second before they saw this. So on
/// a phone this is that mark, and on a desktop it is the disc that is in the
/// Dock. Either way it is the icon of the thing they just opened, which is
/// the only thing a mark on a welcome screen is for.
pub fn mark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    if sigil::Form::of(ui.ctx()).is_phone() {
        return identicon(ui, MARK_KEY, size);
    }
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

/// How narrow a field may be squeezed before its action moves below it.
///
/// Enough for a readable fragment of what goes in one -- a key, a domain, a
/// credential -- rather than a dozen characters and an ellipsis.
const FIELD_MIN: f32 = 200.0;

/// What a form row's action looks like.
///
/// # Why an icon is the default and a word the exception
///
/// A word beside a field is a word the reader has already been given: the
/// label says what the field is, the hint says what goes in it, and "Set"
/// after both says only "yes, that one". Three of those down one settings
/// pane is a column of the same word taking a third of the row each time. An
/// icon says it in a square, and -- through [`icon_button_named`] -- still
/// reaches a screen reader with the word, which is the rule the icon set was
/// written under: a control that is only a picture is a control some people
/// cannot use.
///
/// A word stays where no picture means the thing. "Write credential" and
/// "Register this device" are not a tick; drawing one for them would say
/// *confirm* over an action that is neither obvious nor undoable, and a
/// picture somebody has to guess at is worth less than a plain sentence.
#[derive(Clone, Copy)]
pub enum Action<'a> {
    /// A picture, named for the tooltip and the accessibility tree.
    Mark(Icon, &'a str),
    /// A word, for what no picture says.
    Word(&'a str),
}

impl Action<'_> {
    /// Draw it, and say whether it was pressed.
    fn show(self, ui: &mut egui::Ui) -> bool {
        match self {
            Action::Mark(icon, word) => icon_button_named(ui, icon, word).clicked(),
            Action::Word(word) => ui.button(word).clicked(),
        }
    }

    /// How wide it will be, so the field can be given the rest.
    fn width(self, ui: &egui::Ui) -> f32 {
        match self {
            // Square, and a finger's side on a phone.
            Action::Mark(..) => sigil::Form::of(ui.ctx()).button_size(),
            Action::Word(word) => {
                let galley = ui.painter().layout_no_wrap(
                    word.to_string(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    egui::Color32::PLACEHOLDER,
                );
                galley.size().x + ui.spacing().button_padding.x * 2.0
            }
        }
    }
}

/// A labelled field with at most one action beside it, at the width there is.
///
/// # Why a shared row rather than a `horizontal` per pane
///
/// Every form in sigil was written the same way: `ui.horizontal(|ui| { label;
/// field(300.0); button })`. On a desktop that is fine. On a 360-point pane it
/// is not, and it failed in two ways at once. The label eats the width the
/// field needed, so a field asking for a key in base58 showed eight
/// characters of it; and because each pane's labels are different lengths --
/// "Name", "Topic", "Keep messages for" -- every field on one screen came out
/// a different width, which reads as three unrelated controls rather than one
/// form.
///
/// So below [`tokens::NARROW_WIDTH`] the label goes **above** the field, and
/// the field takes every point the action does not. The action's width is
/// measured rather than guessed: the row lays out right to left, the button
/// takes what it needs, and the field is given exactly the remainder. A guess
/// is what [`crate::field`]'s callers were doing with a 130-point constant,
/// and a guess that is wrong overflows the pane -- which on a phone drags
/// every row after it out with it, because egui grows a ui to whatever is
/// drawn in it.
///
/// Returns the field's response, and whether the action was pressed (`false`
/// when there is no action).
pub fn labelled_field(
    ui: &mut egui::Ui,
    label: &str,
    buf: &mut String,
    hint: &str,
    action: Option<Action<'_>>,
) -> (egui::Response, bool) {
    if ui.available_width() >= sigil::tokens::NARROW_WIDTH {
        // The desktop's row, unchanged: this is the arm that was there
        // before, so nothing about a wide window moves.
        let mut out = None;
        ui.horizontal(|ui| {
            ui.label(label);
            let width = 300.0f32.min((ui.available_width() - 130.0).max(120.0));
            let response = field(ui, buf, hint, width);
            let clicked = action.map(|a| a.show(ui)).unwrap_or(false);
            out = Some((response, clicked));
        });
        return out.expect("the row runs its contents");
    }

    let theme = sigil::ColorTheme::current(ui.ctx());
    // An empty label is a field whose heading and prose above it already say
    // what it is for -- a caption repeating them would be noise. It still
    // comes through here, so its action is placed by the same rule as every
    // other one and a pane does not end up with one button right-aligned
    // and the next left-aligned for no reason anybody could name.
    if !label.is_empty() {
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(theme.text_secondary),
        );
        ui.add_space(sigil::tokens::SPACING_XS);
    }
    let mut out = None;
    // **A row, not the rest of the pane.** `with_layout` on a vertical ui
    // takes the whole remaining rectangle, so a right-to-left layout asked
    // for inside one centres its contents in what is left of the screen --
    // which put this field halfway down an otherwise empty Members pane,
    // several hundred points below its own label. Allocating the row's own
    // height is what makes it a row.
    // **How wide the action is decides whether it fits beside the field.**
    // "Set" is four characters and leaves the field nearly the whole pane;
    // "Write credential" is a third of it, and asking for a key in base58 in
    // what is left showed fourteen characters of a forty-four character
    // answer. So the button is measured, and when the field would be left
    // less than a readable minimum the button goes under it instead --
    // right-aligned, where a form's action belongs. Measured rather than a
    // list of labels, because the next long label would not be on the list.
    let wide_enough = action
        .map(|a| ui.available_width() - a.width(ui) - sigil::tokens::SPACING_SM >= FIELD_MIN)
        .unwrap_or(true);

    if wide_enough {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), sigil::tokens::FIELD_MD),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let clicked = match action {
                    Some(a) => {
                        let hit = a.show(ui);
                        ui.add_space(sigil::tokens::SPACING_SM);
                        hit
                    }
                    None => false,
                };
                // Whatever is left, which is why this cannot overflow.
                let width = ui.available_width();
                let response = field(ui, buf, hint, width);
                out = Some((response, clicked));
            },
        );
    } else {
        let width = ui.available_width();
        let response = field(ui, buf, hint, width);
        ui.add_space(sigil::tokens::SPACING_SM);
        let mut clicked = false;
        ui.allocate_ui_with_layout(
            egui::vec2(width, sigil::tokens::BUTTON_MD),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if let Some(a) = action {
                    clicked = a.show(ui);
                }
            },
        );
        out = Some((response, clicked));
    }
    out.expect("the row runs its contents")
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

/// A field with room for one control **inside** it, at its right end: the
/// box takes `width` and the text stops short of a square `slot` wide, so
/// what goes there -- a magnifier, a paperclip -- sits in the box rather
/// than beside it. Returns the field and the slot's rectangle; draw the
/// control there with [`in_slot`], after the field so the press is its.
pub fn field_with_slot(
    ui: &mut egui::Ui,
    buf: &mut String,
    hint: &str,
    width: f32,
    height: f32,
    slot: f32,
) -> (egui::Response, egui::Rect) {
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let above = ((height - line) / 2.0).max(0.0);
    let response = ui.add_sized(
        [width, height],
        egui::TextEdit::singleline(buf)
            .id_salt(hint)
            .hint_text(hint)
            .margin(egui::Margin {
                left: sigil::tokens::SPACING_MD as i8,
                right: (slot + sigil::tokens::SPACING_XS) as i8,
                top: above as i8,
                bottom: above as i8,
            }),
    );
    let rect = response.rect;
    let at = egui::Rect::from_center_size(
        egui::pos2(
            rect.right() - sigil::tokens::SPACING_XS - slot / 2.0,
            rect.center().y,
        ),
        egui::vec2(slot, slot.min(rect.height())),
    );
    (response, at)
}

/// Draw more than one control in a field's slot, laid out from the right.
///
/// The slot has to have been asked for wide enough -- `field_with_slot`
/// takes the width and keeps the text clear of it -- because a control
/// drawn past the slot is drawn past the field, and a widget outside its
/// clip cannot be pressed.
pub fn in_slot_row<R>(
    ui: &mut egui::Ui,
    slot: egui::Rect,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(slot)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    add(&mut child)
}

/// Draw a control in a field's slot (see [`field_with_slot`]): a child ui
/// over the slot, which takes nothing from the row's own layout.
pub fn in_slot<R>(ui: &mut egui::Ui, slot: egui::Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(slot).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    add(&mut child)
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
            // **Named by its hint, not by its place.** egui's automatic id is
            // the widget's position in the pass, so a field under something
            // that comes and goes -- the mention list above the composer --
            // was a different field each time, with a fresh caret and no
            // focus. A hint is what a field is for, and two fields for one
            // thing on one screen is the case that would clash.
            .id_salt(hint)
            .password(password)
            .hint_text(hint)
            .margin(egui::Margin::symmetric(
                sigil::tokens::SPACING_MD as i8,
                above as i8,
            )),
    )
}
pub use message::{
    Bubble, BubbleAction, Quote, Reaction, Receipt, Thumb, bubble, call_line, copy_separator,
    day_separator, reaction_chip, short, system_line, unread_divider, unread_pill, verified_mark,
};
/// Give a menu the width a menu should be, from inside its body.
///
/// **The first line of every `Popup::menu(..).show(|ui| ..)` in sigil.** A
/// menu's rows are [`icon_item`]s, and an icon row takes the width it is
/// given so the whole row is a hit target; inside a popup the width it is
/// given is the window's, so a menu of two short phrases spanned a phone
/// edge to edge. A row longer than the maximum still grows the menu.
pub fn menu_width(ui: &mut egui::Ui) {
    ui.set_min_width(sigil::tokens::MENU_MIN);
    ui.set_max_width(sigil::tokens::MENU_MAX);
}

pub use call_card::{Call, CallPress, Mic, call_card, call_control};
pub use qr::qr;
pub use roster::{Row, roster};
pub use search_hit::{SearchHit, search_hit};
pub use sigil::icon;
pub use sigil::icon::{
    Icon, apply_button, icon_button, icon_button_as_named, icon_button_named, icon_button_tinted,
    icon_item, icon_item_as, icon_item_counted,
};
