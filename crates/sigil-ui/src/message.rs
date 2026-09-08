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
    let width = match receipt {
        Receipt::Delivered | Receipt::Read => tick * 1.6,
        _ => tick,
    };
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
    /// Author and a stub of what is being replied to.
    ///
    /// The author, not the sequence number: "↳ 57: llll" names a number nobody
    /// has memorised.
    pub reply_to: Option<(&'a str, &'a str)>,
    /// Emoji, how many sent it, and whether we are one of them.
    pub reactions: &'a [(String, usize, bool)],
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
    /// Forward the file it carries somewhere else.
    pub forward: bool,
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

    let limit = (ui.available_width() * 0.72).max(160.0);
    if b.mine {
        // Measured, not filled. A frame in a top-down layout takes the width
        // it is given, so capping at the limit made every message the same
        // width as the longest one it was allowed to be — a wall of identical
        // blocks rather than a conversation.
        let width = wanted(ui, b).clamp(120.0, limit);
        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
            ui.set_max_width(width);
            body(ui, b, &theme, &mut action);
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
            ui.vertical(|ui| {
                ui.set_max_width(limit);
                body(ui, b, &theme, &mut action);
            });
        });
    }

    action
}

/// How wide this bubble would like to be, before any cap.
///
/// The widest line it holds, laid out without wrapping, plus the frame's own
/// margins. Anything with a picture in it asks for everything, because an
/// image is sized by the space it is given rather than by its text.
fn wanted(ui: &egui::Ui, b: &Bubble<'_>) -> f32 {
    if !b.attachments.is_empty() {
        return f32::INFINITY;
    }
    let measure = |text: &str, style: egui::TextStyle| {
        let font = style.resolve(ui.style());
        ui.ctx()
            .fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, egui::Color32::PLACEHOLDER))
            .rect
            .width()
    };
    let body = if b.redacted {
        measure("Deleted", egui::TextStyle::Body)
    } else {
        measure(b.text, egui::TextStyle::Body)
    };
    // The furniture under the text: a time, possibly "edited", and a receipt.
    let mut meta = measure(b.at, egui::TextStyle::Small) + tokens::SPACING_XL;
    if b.edited {
        meta += measure("edited", egui::TextStyle::Small) + tokens::SPACING_SM;
    }
    if let Some((word, _)) = b.standing {
        meta += measure(word, egui::TextStyle::Small) + tokens::SPACING_SM;
    }
    let author = match (b.grouped, b.mine, b.name) {
        (false, false, Some(name)) => measure(name, egui::TextStyle::Body),
        (false, false, None) => measure(&short(b.key), egui::TextStyle::Body),
        _ => 0.0,
    };
    let reply = b
        .reply_to
        .map(|(who, stub)| measure(&format!("{who}: {stub}"), egui::TextStyle::Small))
        .unwrap_or(0.0);
    body.max(meta).max(author).max(reply) + PAD_X * 2.0
}

/// The bubble itself: the frame, what is in it, and the reactions under it.
fn body(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme, action: &mut BubbleAction) {
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
            if let Some((who, stub)) = b.reply_to {
                reply_stub(ui, who, stub, theme);
            }
            // Before the text: a message is usually a picture *with* a caption
            // rather than a caption with a picture attached.
            if !b.redacted {
                for (i, a) in b.attachments.iter().enumerate() {
                    if crate::attachment(ui, a).save {
                        action.save = Some(i);
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
            } else if !b.text.is_empty() {
                ui.add(egui::Label::new(b.text).wrap().selectable(true));
            }

            // Our own bubble is filled with the accent, and `text_muted` is chosen
            // to sit on a *surface*. On the accent it comes out near-invisible --
            // which is how the time and the "edited" mark disappeared from exactly
            // the messages whose delivery somebody most wants to check.
            let quiet = if b.mine {
                faded(theme.text_primary, theme.accent_muted)
            } else {
                theme.text_muted
            };
            ui.horizontal(|ui| {
                ui.colored_label(quiet, egui::RichText::new(b.at).small());
                if b.edited {
                    ui.colored_label(quiet, egui::RichText::new("edited").small());
                }
                // SIP-31 **requires** a fork be surfaced, so this is a word in
                // the message and not a line in a diagnostics pane somebody
                // would have to go and look at.
                if let Some((word, means)) = b.standing {
                    let colour = if b.alarming { theme.destructive } else { quiet };
                    ui.colored_label(colour, egui::RichText::new(word).small())
                        .on_hover_text(means);
                }
                if let Some(r) = b.receipt {
                    let colour = match r {
                        Receipt::Failed => theme.destructive,
                        Receipt::Read if !b.mine => theme.accent,
                        Receipt::Read => theme.success,
                        _ => quiet,
                    };
                    receipt(ui, r, colour).on_hover_text(r.word());
                }
            });
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

    // **The controls, on the message.** These lived only in a right-click
    // menu, which is a control nobody finds: replying and reacting are the two
    // most common things anybody does to a message, and both were hidden
    // behind a gesture with nothing on screen suggesting it.
    //
    // Shown while the pointer is over the message, so a transcript at rest is
    // a transcript and not a field of buttons.
    // `Frame::show`'s response senses nothing, so `hovered()` on it is always
    // false. The pointer has to be asked about the rect directly, and the
    // reactions row below the bubble counts as part of the message.
    // The bubble, **and the row of controls under it**.
    //
    // A region that stopped at the bubble's edge made the controls impossible
    // to press: they are drawn below it, so moving the pointer towards one
    // left the region, the row stopped being drawn, and the click landed on
    // nothing. Visible and unusable, which is worse than absent.
    let mut reach = inner
        .response
        .rect
        .expand2(egui::vec2(0.0, tokens::SPACING_SM));
    reach.max.y += tokens::BUTTON_MD;
    let over = ui.rect_contains_pointer(reach);
    if over && !b.redacted {
        ui.horizontal(|ui| {
            if crate::icon_button(ui, crate::Icon::Reply).clicked() {
                action.reply = true;
            }

            // Painted openers, not glyph labels. `menu_button` takes text, and
            // the two obvious characters for these — a face and an ellipsis —
            // are exactly the sort this font set has already turned into boxes
            // twice. `Popup::menu` takes any response, so the button can be a
            // shape we drew ourselves.
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
                if !b.attachments.is_empty() && ui.button("Forward file").clicked() {
                    action.forward = true;
                    ui.close();
                }
            });
        });
    }
}

/// Text that is quieter than the body but still legible on `over`.
///
/// Mixed towards the background rather than taken from a fixed muted colour,
/// because "muted" is only meaningful relative to what it sits on.
fn faded(text: egui::Color32, over: egui::Color32) -> egui::Color32 {
    let mix = |a: u8, b: u8| ((a as u16 * 62 + b as u16 * 38) / 100) as u8;
    egui::Color32::from_rgb(
        mix(text.r(), over.r()),
        mix(text.g(), over.g()),
        mix(text.b(), over.b()),
    )
}

/// Who said it: the name if there is one, and the key always reachable.
fn author_line(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme) {
    // Whatever is shown, the key is what it leads to.
    let behind = match b.title {
        Some(title) => format!("{}\n{title} — self-declared, verified by nobody", b.key),
        None => b.key.to_string(),
    };
    match b.name {
        Some(name) => {
            // A profile name is self-declared and nobody attests it, so it is
            // drawn as ordinary text and the key is one hover away. It must
            // never be styled as though the exchange vouched for it.
            ui.label(egui::RichText::new(name).strong().color(theme.accent))
                .on_hover_text(behind);
        }
        None => {
            ui.label(
                egui::RichText::new(short(b.key))
                    .strong()
                    .monospace()
                    .color(theme.accent),
            )
            .on_hover_text(behind);
        }
    }
}

/// What a message is replying to: a bar, a name, and a few of the words.
///
/// **A painted bar rather than an arrow.** `↳` is not in the fonts egui
/// bundles and came out as `□`. It is also the better shape — a rule down the
/// left is what every messenger uses, and it does not have to be understood.
fn reply_stub(ui: &mut egui::Ui, who: &str, stub: &str, theme: &ColorTheme) {
    // Room of its own, on all four sides. The bubble's padding came down and
    // this went with it: the quote ended up jammed against the name above and
    // the words below, reading as a first line of the message rather than as
    // something being quoted.
    ui.add_space(tokens::SPACING_XS);
    ui.horizontal(|ui| {
        // Taller than its text, so the rule reads as a rule. At exactly the
        // line height it is a dash the length of one word.
        let h = ui.text_style_height(&egui::TextStyle::Small) + tokens::SPACING_XS;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(tokens::STROKE_THICK, h), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, tokens::RADIUS_SM, theme.accent);
        ui.add_space(tokens::SPACING_XXS);
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(format!("{who}: {stub}")).small(),
        );
    });
    ui.add_space(tokens::SPACING_XS);
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

/// The first characters of a key, for where the whole one will not fit.
///
/// Only ever beside something that leads back to the full key.
///
/// # Characters, not bytes
///
/// This cut `&key[..8]` and **panicked** on any string whose eighth byte falls
/// inside a character — an em dash, an accent, an emoji, which is to say most
/// messages anybody writes. A key is base58 and would never have found it; two
/// callers passed message text, and searching your conversations or replying
/// to a message with a dash in it took the whole application down.
///
/// A public function that slices a `&str` by byte index is a crash waiting for
/// somebody to type a character.
pub fn short(text: &str) -> String {
    let cut: String = text.chars().take(8).collect();
    // Elided only when something was actually cut. Appending `…` to a string
    // that is already whole says there is more, and there is not.
    if cut.chars().count() == text.chars().count() {
        cut
    } else {
        format!("{cut}…")
    }
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
                out.chars().count() <= 9,
                "{text:?} shortened to {out:?}, which is longer than it should be"
            );
            assert!(
                text.starts_with(out.trim_end_matches('…')),
                "{out:?} is not the start of {text:?}"
            );
            let _ = preview(text, 48);
        }
    }

    /// Nothing was cut, so nothing says there is more.
    #[test]
    fn a_short_string_is_not_given_an_ellipsis_it_did_not_earn() {
        assert_eq!(short("abc"), "abc");
        assert_eq!(preview("abc", 48), "abc");
        assert!(short("abcdefghij").ends_with('…'));
    }

    /// A preview goes on one row, whatever is in the message.
    #[test]
    fn a_preview_is_flattened_so_it_cannot_grow_a_row() {
        assert_eq!(preview("two\nlines\there", 48), "two lines here");
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
        assert!(s.ends_with('…'), "must not look like a whole key: {s}");
        assert!(s.len() < 12);
    }

    #[test]
    fn short_never_panics_on_a_string_shorter_than_the_window() {
        // The ellipsis went with the byte slicing: it was appended whether or
        // not anything had been cut, so a two-character string claimed to have
        // more after it. What this test is actually for -- that a string
        // shorter than the window does not panic -- is unchanged.
        assert_eq!(short(""), "");
        assert_eq!(short("ab"), "ab");
    }
}
