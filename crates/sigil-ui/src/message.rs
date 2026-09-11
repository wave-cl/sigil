//! One message in a transcript, and the furniture around it.
//!
//! Plain data only, like the rest of this crate: the caller maps its own types
//! onto [`Bubble`], so the wire format is understood in one place and this is
//! not it.
//!
//! # What must survive being made pretty
//!
//! Three of these are not decoration and a nicer-looking client that drops them
//! is a worse one:
//!
//! - **A redaction keeps the entry and empties the body — the gap *is* the
//!   record.** A deleted message must leave a visible tombstone, not vanish;
//!   the opposite of pruning, which leaves nothing, deliberately.
//! - **An edit is marked.** Presenting an edit as though it were the original
//!   hides that the text changed after somebody read it.
//! - **A name never appears without its key reachable.** A name is an assertion
//!   (SIP-21) and a key is not.

use sigil::{ColorTheme, tokens};

/// How far a message is known to have got.
///
/// **Under-claiming is the only safe direction.** `Read` means everybody in the
/// conversation is known to have read it, so in a group it waits for the last
/// of them; an account that opted out of receipts reports no reading at all and
/// must not be counted as having read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receipt {
    /// Handed to the session, not yet acknowledged by the exchange.
    Sending,
    /// The exchange has it.
    Sent,
    /// Everybody has fetched it.
    Delivered,
    /// Everybody has read it.
    Read,
    /// It did not go. The text belongs back in the composer.
    Failed,
}

impl Receipt {
    /// The word for it. Always available, and the only thing the accessibility
    /// tree can read: a tick is a convention somebody has to already know, and
    /// no assistive technology can get meaning out of a glyph.
    pub fn word(self) -> &'static str {
        match self {
            Receipt::Sending => "sending",
            Receipt::Sent => "sent",
            Receipt::Delivered => "delivered",
            Receipt::Read => "read",
            Receipt::Failed => "did not send",
        }
    }

    /// How many ticks it draws. Two for delivered and read, which are then
    /// told apart by colour *and* by the word behind them.
    fn ticks(self) -> usize {
        match self {
            Receipt::Delivered | Receipt::Read => 2,
            _ => 1,
        }
    }
}

/// Draw a receipt.
///
/// **Painted, not written.** `✓` and `✓✓` are not in the fonts egui bundles
/// and came out as `□ □` — the same trap `dot` records, where `●`/`○` rendered
/// as tofu and no accessibility assertion could see it, because the
/// accessibility tree carries the *string* and the string was fine. Only a
/// snapshot catches it, and only if somebody looks at the snapshot.
///
/// Sending and failure are not ticks at all, so they keep their own shapes: a
/// dot for in-flight, and a bar for a message that did not go.
pub fn receipt(ui: &mut egui::Ui, receipt: Receipt, colour: egui::Color32) -> egui::Response {
    let h = ui.text_style_height(&egui::TextStyle::Small);
    let tick = h * 0.5;
    let width = receipt_width(ui, receipt);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    // The word, for anything that cannot see the paint.
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, receipt.word()));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let stroke = egui::Stroke::new(tokens::STROKE_THIN, colour);
    match receipt {
        Receipt::Sending => {
            painter.circle_filled(rect.center(), tokens::STROKE_THIN, colour);
        }
        Receipt::Failed => {
            let x = rect.center().x;
            painter.line_segment(
                [
                    egui::pos2(x, rect.top() + h * 0.15),
                    egui::pos2(x, rect.bottom() - h * 0.35),
                ],
                stroke,
            );
            painter.circle_filled(
                egui::pos2(x, rect.bottom() - h * 0.15),
                tokens::STROKE_THIN,
                colour,
            );
        }
        _ => {
            for n in 0..receipt.ticks() {
                let left = rect.left() + n as f32 * tick * 0.6;
                let mid = rect.center().y;
                painter.line_segment(
                    [
                        egui::pos2(left, mid),
                        egui::pos2(left + tick * 0.35, mid + tick * 0.45),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        egui::pos2(left + tick * 0.35, mid + tick * 0.45),
                        egui::pos2(left + tick, mid - tick * 0.5),
                    ],
                    stroke,
                );
            }
        }
    }
    response
}

/// One message, as the interface needs it.
#[derive(Default)]
/// What a message replies to, as the reply shows it.
///
/// The author and the words are what is drawn: "↳ 57: llll" names a number
/// nobody has memorised. The sequence number is what a **click** goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote<'a> {
    pub seq: u64,
    pub who: &'a str,
    pub said: &'a str,
}

pub struct Bubble<'a> {
    /// The author's key, in full.
    pub key: &'a str,
    /// Their display name, if a profile has been seen. Never shown alone.
    pub name: Option<&'a str>,
    /// Their self-declared title.
    ///
    /// **Not drawn beside the name.** SIP-21 forbids rendering a title as a
    /// badge, in channel-role styling, or next to a verification mark, because
    /// it asserts standing directly and nobody attests it — "Exchange
    /// Administrator" does the social engineering by itself. It belongs where
    /// somebody goes looking for it, next to the key.
    pub title: Option<&'a str>,
    pub text: &'a str,
    /// A short time, already formatted. The full one belongs on hover.
    pub at: &'a str,
    /// Ours, so it sits on the other side and carries a receipt.
    pub mine: bool,
    /// Continues the previous message from the same author, so the avatar and
    /// the name are left off and the gap above is smaller.
    pub grouped: bool,
    pub edited: bool,
    /// The body was removed. Draws a tombstone; see the module note.
    pub redacted: bool,
    /// What is being replied to, if anything.
    pub reply_to: Option<Quote<'a>>,
    /// Emoji, how many sent it, and whether we are one of them.
    pub reactions: &'a [(String, usize, bool)],
    /// What happened to the message after it was said. Drawn **under** the
    /// bubble rather than inside it: it is not part of what was said, and
    /// having it in there made the bubble taller than its own contents, which
    /// everything measuring against the bubble then inherited.
    pub receipt: Option<Receipt>,
    /// Files it carries.
    pub attachments: &'a [crate::Attachment<'a>],
    /// What SIP-31 concluded about the entry, in a word, with the long form
    /// for a hover. `None` when there is nothing to say — which is almost
    /// always, and is why this is not a badge every message wears.
    ///
    /// **A fork is not a gap.** A gap is ordinary and a fork is evidence, and
    /// the two must not be drawn alike: a client that coloured every gap as
    /// tampering would cry wolf on every channel with a retention window.
    pub standing: Option<(&'a str, &'a str)>,
    /// Whether that word is the one that is evidence.
    pub alarming: bool,
}

/// What the reader did to a message.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BubbleAction {
    /// An emoji to add or remove.
    pub react: Option<String>,
    pub reply: bool,
    pub edit: bool,
    pub redact: bool,
    /// Copy the author's key.
    pub copy_key: bool,
    /// Save the file at this index.
    pub save: Option<usize>,
    /// Look at the file at this index, full size.
    pub open: Option<usize>,
    /// Ask the exchange for a file it refused, again.
    pub retry: bool,
    /// Forward the file it carries somewhere else.
    pub forward: bool,
    /// Go to the message this one replies to, by its place in the channel.
    pub jump: Option<u64>,
}

impl BubbleAction {
    pub fn is_none(&self) -> bool {
        *self == BubbleAction::default()
    }
}

/// The bubble's own padding.
///
/// Generous. It was cut to 8×4 and that was too tight — the words touched the
/// shape holding them. These are the token steps nearest a request for "about
/// ten more pixels"; they stay on the 4px grid, which everything else in the
/// interface is measured against.
///
/// **Named, because two places have to agree about it.** `wanted` measures a
/// message to decide how wide its bubble should be and adds this on; `body`
/// draws it. A width computed from one padding and drawn with another is a
/// bubble that wraps a line it had room for, and the two were separate
/// numbers that happened to match.
const PAD_X: f32 = tokens::SPACING_LG;
const PAD_Y: f32 = tokens::SPACING_MD;

/// The emoji offered by the picker.
///
/// A short list, like the terminal client's, and for the same reason: it is
/// what fits without becoming a grid nobody wants. **Any emoji can still be
/// sent** — the wire carries the string, not an index into this — so this is a
/// convenience and not the set.
pub const REACTIONS: &[&str] = &[
    "\u{1f44d}",
    "\u{1f389}",
    "\u{1f9e1}",
    "\u{1f602}",
    "\u{1f914}",
    "\u{1f440}",
];

/// A label centred in the transcript with a rule either side of it.
///
/// The shape every whole-conversation marker takes -- the day, the unread
/// mark, and what happened to the channel. Centred because none of them was
/// said by anybody: a marker pinned to the left reads as a message from
/// whoever is on that side, and one of these is not from a person at all.
/// `rule` paints a hairline either side; `None` centres the words alone.
fn centred(
    ui: &mut egui::Ui,
    label: egui::WidgetText,
    rule: Option<egui::Color32>,
) -> egui::Response {
    // Measured, not guessed. The rules used to be `available_width() * 0.5 -
    // 40.0`, which centres only a label that happens to be 80px wide and puts
    // everything else off to one side.
    let text = label.into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body);
    let gap = ui.spacing().item_spacing.x;
    let width = ((ui.available_width() - text.size().x) * 0.5 - gap).max(0.0);
    let row = ui.horizontal(|ui| {
        let line = |ui: &mut egui::Ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
            if let Some(colour) = rule {
                ui.painter()
                    .hline(rect.x_range(), rect.center().y, (1.0, colour));
            }
        };
        line(ui);
        ui.add(egui::Label::new(text).selectable(false));
        line(ui);
    });
    // `horizontal` senses nothing on its own, so the row it returns would
    // never report a hover -- the same trap as a `Frame`'s response.
    row.response.interact(egui::Sense::hover())
}

/// A date, once, above the first message of each day.
pub fn day_separator(ui: &mut egui::Ui, label: &str) {
    let theme = ColorTheme::current(ui.ctx());
    ui.add_space(tokens::SPACING_MD);
    let _ = centred(
        ui,
        egui::RichText::new(label).color(theme.text_muted).into(),
        Some(theme.border_default),
    );
    ui.add_space(tokens::SPACING_XS);
}

/// Something that happened *to* the conversation rather than in it.
///
/// A membership or metadata change, which the exchange writes and signs
/// itself (SIP-16). Drawn as a marker and never as a bubble: nobody said it,
/// and a client that dressed it as a message would be putting words in the
/// mouth of whoever it names.
/// Returns the row, so a caller can hang the keys it names off it.
pub fn system_line(ui: &mut egui::Ui, said: &str) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    ui.add_space(tokens::SPACING_SM);
    let row = centred(
        ui,
        egui::RichText::new(said)
            .small()
            .color(theme.text_muted)
            .into(),
        // No rules. The day separator and the unread mark both have them, and
        // a third thing wearing the same clothes reads as one of those two --
        // this one is a sentence, and should look like one.
        None,
    );
    ui.add_space(tokens::SPACING_XS);
    row
}

/// The frozen line above the first message that was unread on opening.
///
/// **Frozen on entry, not live.** Reading advances the read mark, so a divider
/// that tracked it would disappear exactly when somebody wanted to see where
/// they had got to.
pub fn unread_divider(ui: &mut egui::Ui, count: usize) {
    let theme = ColorTheme::current(ui.ctx());
    ui.add_space(tokens::SPACING_SM);
    let _ = centred(
        ui,
        egui::RichText::new(match count {
            1 => "1 new message".to_string(),
            n => format!("{n} new messages"),
        })
        .color(theme.accent)
        .into(),
        Some(theme.accent),
    );
    ui.add_space(tokens::SPACING_XS);
}

/// A count of unread messages, for a conversation row.
pub fn unread_pill(ui: &mut egui::Ui, count: u32) {
    if count == 0 {
        return;
    }
    let theme = ColorTheme::current(ui.ctx());
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    egui::Frame::NONE
        .fill(theme.accent)
        .corner_radius(tokens::RADIUS_PILL)
        .inner_margin(egui::Margin::symmetric(tokens::SPACING_SM as i8, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(&text)
                    .color(egui::Color32::WHITE)
                    .small(),
            );
        });
}

/// Draw one message, and report what was done to it.
///
/// # Aligning one's own messages
///
/// Three shapes were tried before this one and all three left them on the
/// left: `allocate_ui_with_layout` with a zero height, `scope` plus
/// `set_width`, and a `horizontal` wrapping a right-to-left layout. In every
/// case the child sizes itself to its content, so there is nothing for a
/// right-to-left layout to push against and the result looks *almost* right —
/// which is why it survived several passes.
///
/// What works is `Layout::top_down(Align::Max)` with the bubble as a **direct**
/// child. A frame sizes to its content and the layout right-aligns it. Nothing
/// may be wrapped around it that takes the full width, which is why the two
/// sides are laid out differently rather than by one path with a flag.
pub fn bubble(ui: &mut egui::Ui, b: &Bubble<'_>) -> BubbleAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = BubbleAction::default();

    // A run from one person is one block of speech, so the gap inside it is
    // barely a gap; the gap between two people is what separates them.
    ui.add_space(if b.grouped {
        tokens::SPACING_XXS
    } else {
        tokens::SPACING_XS
    });

    // Room for the controls beside the bubble, always — so a message does not
    // move sideways when the pointer arrives.
    let aside = (tokens::BUTTON_MD + ui.spacing().item_spacing.x) * 3.0;
    // **Three quarters of the pane, and never so wide the controls fall off.**
    // The first is the rule; the second is what the rule has to give way to
    // on a pane narrow enough that a quarter of it is less than three buttons.
    let available = ui.available_width();
    let limit = (available * WIDEST).min(available - aside).max(160.0);
    if b.mine {
        // Measured, not filled. A frame in a top-down layout takes the width
        // it is given, so capping at the limit made every message the same
        // width as the longest one it was allowed to be — a wall of identical
        // blocks rather than a conversation.
        let Fit { width, one_line } = fit(ui, b, limit);
        // **Right to left**: the bubble is placed first and lands against the
        // right edge, and the controls go to its left. The previous shape was
        // `top_down(Align::Max)` with the frame as a direct child, which
        // right-aligns one thing and has nowhere to put a second.
        // `Center`, so the controls sit against the middle of the bubble
        // rather than its top edge. On a bubble of several lines a top-aligned
        // row reads as belonging to the first one.
        //
        // **Allocated to a row, not to whatever is left.** `with_layout` takes
        // the ui's whole remaining size, so `Center` centred the controls in
        // the rest of the transcript rather than against the bubble: on a
        // one-word message they landed seventy-five pixels below it, drifting
        // further the more empty space there was. `ui.horizontal` -- which the
        // other side of the conversation uses, and which is why that side was
        // right -- starts a row at `interact_size.y` and lets it grow to what
        // is put in it. This is that, right to left.
        //
        // **Allocated as a row, not as whatever is left.** `with_layout` takes
        // the ui's entire remaining size, so `Center` centred the controls in
        // the rest of the transcript rather than against the message: on a
        // one-word bubble they landed **seventy-five pixels below it**, and
        // further the more empty space there was under the conversation. This
        // is what `ui.horizontal` does -- the shape the other side of the
        // conversation uses, and the reason that side was right all along --
        // written out because the direction has to be right to left.
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        ui.allocate_ui_with_layout(row, layout, |ui| {
            let bubble = ui
                .scope_builder(
                    egui::UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Max)),
                    |ui| {
                        ui.set_max_width(width);
                        body(ui, b, one_line, &theme, &mut action)
                    },
                )
                .inner;
            controls(ui, b, bubble, &mut action);
        });
    } else {
        ui.horizontal(|ui| {
            // The avatar column keeps its width even when grouped, so a run of
            // messages from one person stays in a column instead of stepping
            // sideways as the avatar comes and goes. `add_space` and a widget
            // are not interchangeable: item spacing follows a widget and not a
            // space, which is exactly the few pixels that made them disagree.
            let size = tokens::AVATAR_SM;
            if b.grouped {
                ui.add_space(size + ui.spacing().item_spacing.x);
            } else {
                crate::identicon(ui, b.key, size);
            }
            let Fit { width, one_line } = fit(ui, b, limit);
            let bubble = ui
                .scope_builder(
                    egui::UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        ui.set_max_width(width);
                        body(ui, b, one_line, &theme, &mut action)
                    },
                )
                .inner;
            controls(ui, b, bubble, &mut action);
        });
    }

    action
}

/// Reply, react, and the rest — **beside** the message rather than under it.
///
/// # Why beside
///
/// Under the bubble they pushed everything below them down as the pointer
/// moved along a conversation, so reading with the mouse anywhere near the
/// transcript made it twitch. Beside, they sit in the space that is already
/// empty: to the right of somebody else's message and to the left of one's
/// own, which is also the side each has room on.
///
/// Shown while the pointer is over the message, so a transcript at rest is a
/// transcript and not a field of buttons. `reach` covers the bubble **and the
/// controls** — a region stopping at the bubble's edge made them impossible to
/// press, because moving towards one left the region and the row stopped being
/// drawn before the click landed.
fn controls(ui: &mut egui::Ui, b: &Bubble<'_>, bubble: egui::Rect, action: &mut BubbleAction) {
    let aside = (tokens::BUTTON_MD + ui.spacing().item_spacing.x) * 3.0;
    let reach = bubble
        .expand2(egui::vec2(0.0, tokens::SPACING_XS))
        .translate(egui::vec2(if b.mine { -aside } else { aside } / 2.0, 0.0))
        .expand2(egui::vec2(aside / 2.0, 0.0));
    let over = ui.rect_contains_pointer(reach);

    // **A menu opened from these keeps them up.**
    //
    // The reaction picker and the rest are drawn below their button, which is
    // outside `reach` — so moving the pointer down into one left the region,
    // the controls stopped being drawn, and the popup went with them. The
    // picker was visible and could not be reached, which is the same defect as
    // the controls themselves had when they were under the bubble.
    //
    // One slot in memory rather than a flag per message: only one message can
    // be under the pointer at a time, so remembering *which* is enough, and
    // the alternative is inventing a stable identity for a message that the
    // widget deliberately does not have.
    let slot = egui::Id::new("sigil-message-controls");
    let me = ui.id();
    if over {
        ui.ctx().data_mut(|d| d.insert_temp(slot, me));
    }
    let holding = egui::Popup::is_any_open(ui.ctx())
        && ui.ctx().data(|d| d.get_temp::<egui::Id>(slot)) == Some(me);
    if !(over || holding) || b.redacted {
        return;
    }

    if crate::icon_button(ui, crate::Icon::Reply).clicked() {
        action.reply = true;
    }

    // Painted openers, not glyph labels. `menu_button` takes text, and the two
    // obvious characters for these — a face and an ellipsis — are exactly the
    // sort this font set has already turned into boxes twice. `Popup::menu`
    // takes any response, so the button can be a shape we drew ourselves.
    let react = crate::icon_button(ui, crate::Icon::React);
    egui::Popup::menu(&react).show(|ui| {
        ui.horizontal(|ui| {
            for emoji in REACTIONS {
                if ui.button(*emoji).clicked() {
                    action.react = Some((*emoji).to_string());
                    ui.close();
                }
            }
        });
    });

    let more = crate::icon_button(ui, crate::Icon::More);
    egui::Popup::menu(&more).show(|ui| {
        if b.mine && ui.button("Edit").clicked() {
            action.edit = true;
            ui.close();
        }
        if ui.button("Delete").clicked() {
            action.redact = true;
            ui.close();
        }
        if ui.button("Copy key").clicked() {
            action.copy_key = true;
            ui.close();
        }
        if !b.attachments.is_empty() {
            // Saving lives here rather than on the picture: it is the one
            // action nobody takes often, and it had the loudest place on the
            // bubble.
            if ui.button("Save file").clicked() {
                action.save = Some(0);
                ui.close();
            }
            if ui.button("Forward file").clicked() {
                action.forward = true;
                ui.close();
            }
        }
    });
}

/// The widest a bubble may be, as a share of the pane it is in.
///
/// Three quarters. It was 0.72 of the pane *less* the controls' room, which on
/// an ordinary window came to about two thirds, and a short message still
/// took two lines because the time was always drawn under the text.
const WIDEST: f32 = 0.75;

/// How wide a bubble wants to be, and whether one line is enough.
struct Fit {
    width: f32,
    /// The text and everything after it -- the time, the receipt, a word
    /// about the entry -- fit on one row within the limit, so they are drawn
    /// on one row. A message like "Give it a week." is a single line the way
    /// it is in any other messenger, rather than a line of words over a line
    /// of furniture.
    one_line: bool,
}

/// Measured, so a short message is a short bubble.
///
/// A frame in a top-down layout takes the width it is given, so capping at
/// the limit alone made every message the same width as the longest one it
/// was allowed to be -- a wall of identical blocks rather than a conversation.
fn fit(ui: &egui::Ui, b: &Bubble<'_>, limit: f32) -> Fit {
    let measure = |text: &str, style: egui::TextStyle| {
        let font = style.resolve(ui.style());
        ui.ctx()
            .fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, egui::Color32::PLACEHOLDER))
            .rect
            .width()
    };
    let gap = ui.spacing().item_spacing.x;
    let body = if b.redacted {
        measure("Deleted", egui::TextStyle::Body)
    } else {
        measure(b.text, egui::TextStyle::Body)
    };
    // The furniture after the text, with the gap before each piece: the
    // time, then "edited", a word about how the entry stands, and the
    // receipt. Every one of them is drawn on the row `meta_row` draws, so
    // every one of them is counted here -- a row measured narrower than it
    // draws is a frame that grows past the width it was given.
    let mut meta = measure(b.at, egui::TextStyle::Small);
    if b.edited {
        meta += gap + measure("edited", egui::TextStyle::Small);
    }
    if let Some((word, _)) = b.standing {
        meta += gap + measure(word, egui::TextStyle::Small);
    }
    if let Some(r) = b.receipt {
        meta += gap + receipt_width(ui, r);
    }
    let author = match (b.grouped, b.mine, b.name) {
        (false, false, Some(name)) => measure(name, egui::TextStyle::Body),
        (false, false, None) => measure(&short(b.key), egui::TextStyle::Body),
        _ => 0.0,
    };
    // The rule down its left and the gap after it are part of the line.
    // Left out, the bubble was measured too narrow for what it then drew, and
    // a `horizontal` does not wrap — so the frame grew past the width it had
    // been given and ran off the edge of the pane.
    let reply = b
        .reply_to
        .map(|q| {
            measure(&format!("{}: {}", q.who, q.said), egui::TextStyle::Small)
                + tokens::STROKE_THICK
                + tokens::SPACING_XXS
                + gap * 2.0
        })
        .unwrap_or(0.0);
    // A picture asks for the size it will be drawn at, not for everything.
    // `INFINITY` made every message carrying a file as wide as the pane
    // allowed, including one whose whole content is `[image, 28 KiB]`.
    let files = if b.attachments.is_empty() {
        0.0
    } else {
        crate::attachment::PICTURE
    };

    // One line when the words and their furniture sit side by side inside the
    // limit. A picture always gets its own rows; so does a tombstone, whose
    // one word is not the message.
    let together = body + gap * 2.0 + meta;
    let one_line = b.attachments.is_empty() && !b.redacted && together + PAD_X * 2.0 <= limit;
    let content = if one_line { together } else { body.max(meta) };
    let width = content.max(author).max(reply).max(files) + PAD_X * 2.0;
    Fit {
        width: width.clamp(120.0f32.min(limit), limit),
        one_line,
    }
}

/// How wide a receipt mark is drawn, so a row can be measured to fit one.
///
/// The same arithmetic as [`receipt`], which is the point: a mark measured
/// one width and drawn another is a row that overflows its bubble.
fn receipt_width(ui: &egui::Ui, receipt: Receipt) -> f32 {
    let tick = ui.text_style_height(&egui::TextStyle::Small) * 0.5;
    match receipt {
        Receipt::Delivered | Receipt::Read => tick * 1.6,
        _ => tick,
    }
}

/// The bubble itself: the frame, what is in it, and the reactions under it.
fn body(
    ui: &mut egui::Ui,
    b: &Bubble<'_>,
    one_line: bool,
    theme: &ColorTheme,
    action: &mut BubbleAction,
) -> egui::Rect {
    let fill = if b.mine {
        theme.accent_muted
    } else {
        theme.surface_elevated
    };
    let frame = egui::Frame::NONE
        .fill(fill)
        // Properly round, and tight around the words. A 12px radius on a
        // two-line bubble reads as a box with the corners taken off; this is
        // the shape a message has.
        .corner_radius(tokens::RADIUS_PILL)
        .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8));
    // **Computed here, and given to everything drawn on this bubble.** Every
    // one of these used `text_muted`, which is chosen for a surface and comes
    // out at 1.25 against the accent: the reply being answered and the name of
    // an attached file were both grey-on-blue. They were also the two things
    // that say what a message is *about*.
    let quiet = if b.mine {
        faded(theme.text_primary, fill)
    } else {
        theme.text_muted
    };
    // The quoted rule is the accent, which on an accent-filled bubble is the
    // bubble. On one's own it is the text colour instead, where it reads as
    // the mark it is.
    let rule = if b.mine {
        theme.text_primary
    } else {
        theme.accent
    };
    let inner = frame.show(ui, |ui| {
        // **Inside the bubble, always left to right.** The right-alignment
        // that puts one's own message on the right is a property of where the
        // bubble sits, not of what is in it — inherited, it reversed the
        // metadata row into "read edited 11:00" and would reverse any text
        // that wrapped.
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            if !b.grouped && !b.mine {
                author_line(ui, b, theme);
            }
            if let Some(q) = b.reply_to
                && reply_stub(ui, q, quiet, rule)
            {
                action.jump = Some(q.seq);
            }
            // Before the text: a message is usually a picture *with* a caption
            // rather than a caption with a picture attached.
            if !b.redacted {
                for (i, a) in b.attachments.iter().enumerate() {
                    let did = crate::attachment(ui, a, fill);
                    if did.save {
                        action.save = Some(i);
                    }
                    if did.open {
                        action.open = Some(i);
                    }
                    if did.retry {
                        action.retry = true;
                    }
                }
            }
            if b.redacted {
                // The tombstone. Deleting the row instead would destroy the one
                // thing a redaction is for: the record that something was here.
                ui.label(
                    egui::RichText::new("Deleted")
                        .italics()
                        .color(theme.text_muted),
                );
                meta_row_beneath(ui, b, theme, quiet);
            } else if one_line {
                // **One row.** The words, then the time and the receipt after
                // them, sitting on the baseline the way they do in every other
                // messenger -- and not, as they were, on a second line under
                // a message that only needed one. `fit` has already found that
                // it all fits, so the label is not asked to wrap.
                //
                // Bottom-aligned, and allocated the height of one line of
                // body text so that alignment has something to be relative
                // to: a `with_layout` here would take the rest of the pane.
                let row = egui::vec2(
                    ui.available_width(),
                    ui.text_style_height(&egui::TextStyle::Body),
                );
                ui.allocate_ui_with_layout(
                    row,
                    egui::Layout::left_to_right(egui::Align::Max),
                    |ui| {
                        ui.add(egui::Label::new(b.text).selectable(true));
                        // **Against the right edge**, whatever the bubble's
                        // width came out as. A bubble has a minimum width, and
                        // "ok" does not reach it, so drawn straight after the
                        // words the time sat in the middle of the bubble with
                        // empty fill to its right. Safe as a `with_layout`
                        // here: the row was allocated a size, so "the rest"
                        // is the rest of the row and not the rest of the pane.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                            meta_row(ui, b, theme, quiet)
                        });
                    },
                );
            } else {
                if !b.text.is_empty() {
                    ui.add(egui::Label::new(b.text).wrap().selectable(true));
                }
                meta_row_beneath(ui, b, theme, quiet);
            }
        });
    });

    if !b.reactions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (emoji, count, ours) in b.reactions {
                if reaction_chip(ui, emoji, *count, *ours).clicked() {
                    action.react = Some(emoji.clone());
                }
            }
        });
    }

    inner.response.rect
}

/// The time, and what else there is to say about the entry: "edited", a word
/// about how it stands, and the receipt.
///
/// **In the bubble**, on the same row as the time. It went under the bubble
/// for a while, on the argument that a receipt is what happened to a message
/// rather than part of it -- which is true, and cost every message a second
/// row of its own height plus a mark floating below it with nothing to belong
/// to. Where it sits now is where a reader of any other messenger looks for
/// it, and on a short message it is on the *same* line as the words.
///
/// `quiet` rather than `text_muted`, because the ground here is the bubble's
/// fill and `quiet` is mixed towards it -- see [`faded`]. Failed is still the
/// destructive colour and read is still the accent or the success colour: the
/// ones that mean something keep meaning it.
///
/// **Always against the bubble's right edge.** Laid out from the right, so
/// the pieces are placed in reverse -- the receipt first, then the word about
/// the entry, then "edited", then the time -- and read left to right as time,
/// edited, standing, receipt. The caller supplies a right-to-left layout that
/// has been given a bounded row: see the two call sites for why it must be
/// bounded.
fn meta_row(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme, quiet: egui::Color32) {
    if let Some(r) = b.receipt {
        let colour = match r {
            Receipt::Failed => theme.destructive,
            Receipt::Read if !b.mine => theme.accent,
            Receipt::Read => theme.success,
            _ => quiet,
        };
        receipt(ui, r, colour).on_hover_text(r.word());
    }
    // SIP-31 **requires** a fork be surfaced, so this is a word in the
    // message and not a line in a diagnostics pane somebody would have to go
    // and look at.
    if let Some((word, means)) = b.standing {
        let colour = if b.alarming { theme.destructive } else { quiet };
        ui.colored_label(colour, egui::RichText::new(word).small())
            .on_hover_text(means);
    }
    if b.edited {
        ui.colored_label(quiet, egui::RichText::new("edited").small());
    }
    ui.colored_label(quiet, egui::RichText::new(b.at).small());
}

/// The same row, on a line of its own under the words, against the right.
///
/// Allocated the height of one small line rather than laid out with
/// `with_layout`, which would take the rest of the pane -- the bubble is a
/// frame in a top-down ui, and a frame takes whatever it is given.
fn meta_row_beneath(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme, quiet: egui::Color32) {
    let row = egui::vec2(
        ui.available_width(),
        ui.text_style_height(&egui::TextStyle::Small),
    );
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| meta_row(ui, b, theme, quiet),
    );
}

/// Text that is quieter than the body but still legible on `over`.
///
/// Mixed towards the background rather than taken from a fixed muted colour,
/// because "muted" is only meaningful relative to what it sits on. The
/// palette's own `text_muted` is chosen for a *surface*: on the accent that
/// fills one's own bubble it comes out at a contrast of **1.25**, which is a
/// colour nobody can read.
///
/// The mix is three quarters text. It was 62%, which reads on a surface and
/// gives 2.74 on the accent -- under the 3.0 floor this palette holds itself
/// to everywhere else, and quietly, because that floor is only ever checked
/// against the three surfaces. Three quarters gives 3.30 dark and 3.93 light,
/// and is still plainly quieter than the body text beside it.
pub fn faded(text: egui::Color32, over: egui::Color32) -> egui::Color32 {
    let mix = |a: u8, b: u8| ((a as u16 * 75 + b as u16 * 25) / 100) as u8;
    egui::Color32::from_rgb(
        mix(text.r(), over.r()),
        mix(text.g(), over.g()),
        mix(text.b(), over.b()),
    )
}

/// Who said it: the name if there is one, and the first of the key if not.
///
/// # Why the key is not on hover any more
///
/// It was, and hovering a transcript is not a question anybody is asking: the
/// pointer crosses author lines on its way to the scrollbar, to a reaction, to
/// the composer, and each crossing popped forty-four characters of base58 over
/// the message underneath. A tooltip that fires on the way past is noise, and
/// noise is what gets ignored -- including on the occasion somebody did want
/// the key.
///
/// So the key is a deliberate gesture rather than an accidental one: Members,
/// from the conversation's own header, has every key in full and selectable.
/// What stays on hover is the **title**, because it is not visible anywhere
/// else and SIP-21 requires the caveat travel with it.
fn author_line(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme) {
    let text = match b.name {
        // A profile name is self-declared and nobody attests it, so it is
        // drawn as ordinary text. It must never be styled as though the
        // exchange vouched for it.
        Some(name) => egui::RichText::new(name).strong().color(theme.accent),
        // Nobody can name them. The first characters of the key, in monospace
        // because that is what it is -- and never enough to identify somebody
        // on its own, which is why it is not offered as though it were.
        None => egui::RichText::new(short(b.key))
            .strong()
            .monospace()
            .color(theme.accent),
    };
    let line = ui.label(text);
    if let Some(title) = b.title {
        line.on_hover_text(format!("{title} — self-declared, verified by nobody"));
    }
}

/// What a message is replying to: a bar, a name, and a line of the words.
///
/// **A control.** Pressing the quote goes to the message it quotes -- that is
/// what a quote is for, and it was a caption. The whole row answers, rule and
/// words alike, with the pointing hand and a tooltip saying where it leads.
///
/// **A painted bar rather than an arrow.** `↳` is not in the fonts egui ships
/// with, and drew as nothing.
///
/// Returns whether it was pressed.
fn reply_stub(ui: &mut egui::Ui, q: Quote<'_>, quiet: egui::Color32, rule: egui::Color32) -> bool {
    // Room of its own, on all four sides. The bubble's padding came down and
    // this went with it: the quote ended up jammed against the name above and
    // the words below, reading as a first line of the message rather than as
    // something being quoted.
    ui.add_space(tokens::SPACING_XS);
    let row = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        // Or the words take the press for their own selection and the row
        // never hears it; see `conversation_row`.
        ui.style_mut().interaction.selectable_labels = false;
        ui.horizontal(|ui| {
            // Taller than its text, so the rule reads as a rule. At exactly the
            // line height it is a dash the length of one word.
            let h = ui.text_style_height(&egui::TextStyle::Small) + tokens::SPACING_XS;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(tokens::STROKE_THICK, h), egui::Sense::hover());
            ui.painter().rect_filled(rect, tokens::RADIUS_SM, rule);
            ui.add_space(tokens::SPACING_XXS);
            // **Truncated, not extended.** A `horizontal` layout does not wrap, so
            // a label long enough grows the frame around it — past the width the
            // bubble was given, and off the side of the pane. This is what cuts
            // the quote to the bubble: the session hands over a couple of hundred
            // characters, and what shows is whatever the width allows.
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{}: {}", q.who, q.said))
                        .small()
                        .color(quiet),
                )
                .truncate(),
            );
        });
    });
    ui.add_space(tokens::SPACING_XS);
    let response = row.response;
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
        .on_hover_text("Go to the message this replies to")
        .clicked()
}

/// One emoji and how many people sent it. Ours is outlined.
pub fn reaction_chip(ui: &mut egui::Ui, emoji: &str, count: usize, ours: bool) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let text = if count > 1 {
        format!("{emoji} {count}")
    } else {
        emoji.to_string()
    };
    let button = egui::Button::new(egui::RichText::new(&text).small())
        .corner_radius(tokens::RADIUS_PILL)
        .fill(if ours {
            theme.interactive_hover
        } else {
            theme.surface_secondary
        })
        .stroke(if ours {
            // Outlined as well as filled: "I reacted" and "somebody reacted"
            // must not be one shade apart.
            egui::Stroke::new(tokens::STROKE_THIN, theme.accent)
        } else {
            egui::Stroke::NONE
        });
    ui.add(button).on_hover_text(if ours {
        "you reacted — click to take it back"
    } else {
        "react"
    })
}

/// A key, for where the whole one will not fit: its first four characters,
/// three dots, and its last four.
///
/// # Both ends, not one
///
/// This was the first eight characters and an ellipsis, and a prefix is the
/// one part of a key that can be *chosen*: grinding a key whose first eight
/// characters match somebody else's is expensive but not out of reach, and it
/// got easier the moment the whole key stopped being one hover away. Showing
/// both ends means an impostor has to match both, which is a different order
/// of work -- and it is what people are used to reading a key as.
///
/// Three ASCII dots rather than `…`, so the form is the same in a monospace
/// label, a tooltip, a log line and a terminal, and so it cannot go missing
/// from a font the way a glyph can.
///
/// Only ever beside something that leads back to the full key.
///
/// # Characters, not bytes
///
/// This once cut `&key[..8]` and **panicked** on any string whose eighth byte
/// fell inside a character -- an em dash, an accent, an emoji. A key is base58
/// and would never have found it; two callers passed message text, and
/// searching your conversations or replying to a message with a dash in it
/// took the whole application down. A public function that slices a `&str`
/// by byte index is a crash waiting for somebody to type a character.
pub fn short(text: &str) -> String {
    const END: usize = 4;
    const DOTS: &str = "...";
    let chars: Vec<char> = text.chars().collect();
    // Elided only when it saves something. A string of eleven characters is
    // exactly as long as `abcd...wxyz`, and one shorter would come out longer
    // than it went in, claiming there was more when there was less.
    if chars.len() <= END * 2 + DOTS.len() {
        return text.to_string();
    }
    let head: String = chars[..END].iter().collect();
    let tail: String = chars[chars.len() - END..].iter().collect();
    format!("{head}{DOTS}{tail}")
}

/// A line of somebody's message, for a preview.
///
/// Whitespace flattened and control characters removed, because a preview goes
/// on one row and a message with a newline in it would otherwise take the row
/// height of whatever it contains. Cut on a **character** boundary; see
/// [`short`] for what a byte index costs here.
pub fn preview(text: &str, chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > chars {
        let cut: String = flat.chars().take(chars.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything quiet inside one's own bubble is still legible on it.
    ///
    /// # Why this is not covered by the palette's own contrast test
    ///
    /// That one checks `text_muted` against the three **surfaces**, and passes
    /// -- while the same colour on the accent that fills one's own bubble
    /// comes out at 1.25, which is grey on blue and unreadable. A bubble is a
    /// surface somebody reads text on and was not in the list, so the reply
    /// being quoted and the name of an attached file were both unreadable in
    /// every message anyone sent, and nothing failed.
    ///
    /// The floor is the palette's own 3.0. The failing value is asserted too:
    /// without it this passes for any colour at all that happens to be light.
    #[test]
    fn the_quiet_parts_of_ones_own_bubble_are_legible_on_it() {
        fn luminance(c: egui::Color32) -> f32 {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        fn ratio(a: egui::Color32, b: egui::Color32) -> f32 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }

        for (name, t) in [
            ("dark", sigil::theme::dark()),
            ("light", sigil::theme::light()),
        ] {
            let quiet = faded(t.text_primary, t.accent_muted);
            let r = ratio(quiet, t.accent_muted);
            assert!(
                r >= 3.0,
                "{name}: quiet text on one's own bubble is {r:.2}, under the 3.0 floor"
            );
            // The colour this replaced, and the reason it had to be replaced.
            let was = ratio(t.text_muted, t.accent_muted);
            assert!(
                was < 3.0,
                "{name}: `text_muted` on the accent is {was:.2} -- if that is now legible, \
                 this test has stopped being about anything"
            );
            // Quiet, and not merely legible: at the body's own colour there is
            // nothing to tell the message apart from what is said about it.
            assert!(
                r < ratio(t.text_primary, t.accent_muted),
                "{name}: the quiet text is as loud as the message"
            );
        }
    }

    /// The crash: `&s[..8]` landing inside a character.
    ///
    /// An em dash is three bytes, so "hello — world" has its eighth byte in
    /// the middle of one. This took the application down when somebody
    /// searched their conversations, or replied to such a message.
    #[test]
    fn shortening_cuts_characters_and_not_bytes() {
        for text in [
            "hello — world",
            "café au lait, and then some more of it",
            "👍🎉🧡😂🤔👀👍🎉🧡",
            "日本語のメッセージです",
        ] {
            let out = short(text);
            assert!(
                out.chars().count() <= 11,
                "{text:?} shortened to {out:?}, which is longer than it should be"
            );
            match out.split_once("...") {
                Some((head, tail)) => assert!(
                    text.starts_with(head) && text.ends_with(tail),
                    "{out:?} is not the two ends of {text:?}"
                ),
                // Short enough to come back whole.
                None => assert_eq!(out, text),
            }
            let _ = preview(text, 48);
        }
    }

    /// Nothing was cut, so nothing says there is more.
    #[test]
    fn a_short_string_is_not_given_dots_it_did_not_earn() {
        assert_eq!(short("abc"), "abc");
        assert_eq!(preview("abc", 48), "abc");
        assert!(!short("abcdefghij").contains("..."));
        assert!(short("abcdefghijklmnop").contains("..."));
    }

    /// A preview goes on one row, whatever is in the message.
    #[test]
    fn a_preview_is_flattened_so_it_cannot_grow_a_row() {
        assert_eq!(preview("two\nlines\there", 48), "two lines here");
    }

    fn plain<'a>(text: &'a str, files: &'a [crate::Attachment<'a>]) -> Bubble<'a> {
        Bubble {
            key: "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9",
            name: None,
            title: None,
            text,
            at: "12:00",
            mine: false,
            grouped: true,
            edited: false,
            redacted: false,
            reply_to: None,
            reactions: &[],
            receipt: None,
            attachments: files,
            standing: None,
            alarming: false,
        }
    }

    /// A message carrying a file asks for the size a picture is drawn at.
    ///
    /// It used to ask for `INFINITY`, so **every** message with a file was as
    /// wide as the pane allowed — including one whose entire content is a row
    /// reading `[notes.txt, 2.1 kB]`, which then ran off the edge of a narrow
    /// window. Measured here rather than through a rendered transcript,
    /// because what a label reports is the width of its own text whatever the
    /// bubble around it does.
    #[test]
    fn a_file_asks_for_a_picture_and_not_for_the_whole_pane() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let file = crate::Attachment {
                kind: 0x04,
                described: "[notes.txt, 2.1 kB]",
                preview: crate::attachment::no_preview(),
                bytes: None,
                missing: false,
                id: "abc123",
            };
            // A limit wide enough that nothing here is clamped by it: what
            // is being measured is what the bubble *asks for*.
            let asked = fit(ui, &plain("", std::slice::from_ref(&file)), 10_000.0).width;
            assert!(asked.is_finite(), "a file asks for infinite width: {asked}");
            assert!(
                asked <= crate::attachment::PICTURE + PAD_X * 2.0,
                "a file asks for {asked}, wider than a picture is drawn"
            );
            // And the text still wins when there is more of it than that.
            let long = "x".repeat(400);
            assert!(
                fit(ui, &plain(&long, std::slice::from_ref(&file)), 10_000.0).width > asked,
                "a long message with a file is measured by the file"
            );
        });
        output.textures_delta.clear();
    }

    #[test]
    fn every_receipt_has_a_word_for_what_it_means() {
        // The mark is painted, so it reaches nothing but a pair of eyes. The
        // word is what the accessibility tree carries and what somebody who
        // does not already know the convention gets.
        for r in [
            Receipt::Sending,
            Receipt::Sent,
            Receipt::Delivered,
            Receipt::Read,
            Receipt::Failed,
        ] {
            assert!(r.word().len() > 2, "{r:?} has no word: {}", r.word());
        }
    }

    #[test]
    fn delivered_and_read_differ_in_more_than_colour() {
        // They paint the same two ticks, so the word is the only thing telling
        // them apart and it has to.
        assert_eq!(Receipt::Delivered.ticks(), Receipt::Read.ticks());
        assert_ne!(Receipt::Delivered.word(), Receipt::Read.word());
    }

    /// Nothing this crate draws may rely on a glyph the bundled fonts lack.
    ///
    /// `✓`, `✓✓` and `↳` all rendered as `□`, and nothing caught it: the
    /// accessibility tree carries the *string*, which was fine, so every text
    /// assertion passed. Only a snapshot shows it, and only if somebody looks.
    /// So the rule is that sigil's own chrome is ASCII, and anything symbolic
    /// is painted -- exactly what `dot` had to do when `●`/`○` came out as
    /// tofu.
    #[test]
    fn the_words_this_crate_draws_are_all_ascii() {
        let mut said: Vec<&str> = Vec::new();
        for r in [
            Receipt::Sending,
            Receipt::Sent,
            Receipt::Delivered,
            Receipt::Read,
            Receipt::Failed,
        ] {
            said.push(r.word());
        }
        for word in said {
            assert!(
                word.is_ascii(),
                "{word:?} is not ASCII, so it may render as tofu -- paint it instead"
            );
        }
    }

    #[test]
    fn a_short_key_still_leads_somewhere() {
        let s = short("2vXsQ9pC3nR7bK1mW8dF");
        assert!(s.contains("..."), "must not look like a whole key: {s}");
        assert!(s.len() < 12);
    }

    #[test]
    fn short_never_panics_on_a_string_shorter_than_the_window() {
        // The elision went with the byte slicing: it was appended whether or
        // not anything had been cut, so a two-character string claimed to have
        // more after it. What this test is actually for -- that a string
        // shorter than the window does not panic -- is unchanged.
        assert_eq!(short(""), "");
        assert_eq!(short("ab"), "ab");
        // Eleven characters is the length of the short form itself: nothing
        // is saved by cutting it, so it is not cut.
        assert_eq!(short("abcdefghijk"), "abcdefghijk");
        assert_eq!(short("abcdefghijkl"), "abcd...ijkl");
    }

    /// The form itself, on a real key: four, three dots, four.
    #[test]
    fn a_key_is_shown_by_both_its_ends() {
        let key = "8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR";
        assert_eq!(short(key), "8qbH...VfeR");
    }
}
