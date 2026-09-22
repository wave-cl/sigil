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

/// One emoji on a message, and who sent it.
///
/// **Who, not how many**: a count is what a chip draws, and who reacted is
/// what the person pressing it wants. The names are already resolved --
/// whatever this conversation calls each of them, and "You" for oneself --
/// because the bubble has no way to look a key up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reaction {
    /// The emoji itself, as it went over the wire.
    pub emoji: String,
    /// Who sent it, named.
    pub who: Vec<String>,
    /// Whether one of them is oneself.
    pub ours: bool,
}

impl Reaction {
    /// How many sent it.
    pub fn count(&self) -> usize {
        self.who.len()
    }

    /// The people, for a row that names them.
    pub fn names(&self) -> String {
        self.who.join(", ")
    }
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
    /// The thumbnail of the first picture the quoted message carries.
    pub preview: Option<Thumb<'a>>,
}

/// A thumbnail and the blob it is of: the id is what the picture is
/// registered under, so two quotes of two pictures never share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thumb<'a> {
    pub id: &'a str,
    pub bytes: &'a std::sync::Arc<[u8]>,
}

impl Thumb<'_> {
    /// Where the picture is registered: by blob, the same name the
    /// message's own attachment uses, so the bytes are shared with it.
    pub fn uri(&self) -> String {
        format!("bytes://{}-preview", self.id)
    }
}

/// How wide the thumbnail in a quote is drawn: two small lines, square.
pub fn quote_picture_side(ui: &egui::Ui) -> f32 {
    (ui.text_style_height(&egui::TextStyle::Small) + tokens::SPACING_XS) * 2.0
}

pub struct Bubble<'a> {
    /// This message, and no other on the pane: what the strip, its picker
    /// and its menu are keyed on.
    ///
    /// **Not derived from the ui.** Every message's child ui carries the
    /// same id — a row is a row — so an id taken from it was one id for
    /// the whole transcript: the pointer over one strip counted as over
    /// all of them, and every message drew a strip on the same spot. The
    /// caller has the one thing that tells messages apart, its place in the
    /// channel, and passes it here.
    pub id: egui::Id,
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
    /// SIP-43: the exchange the sender says they posted this through, where
    /// that is not where the conversation lives. Already a name: the caller
    /// resolves the key by its own pin store, or shortens it.
    pub via: Option<&'a str>,
    /// The body was removed. Draws a tombstone; see the module note.
    pub redacted: bool,
    /// What is being replied to, if anything.
    pub reply_to: Option<Quote<'a>>,
    /// The emoji on this message, and who sent each.
    pub reactions: &'a [Reaction],
    /// The emoji this person sends most, for the picker's own row. Empty
    /// until they have sent any.
    pub frequent: &'a [String],
    /// Offer a direct message with the sender: somebody else, seen in a
    /// conversation that is not already the one with them.
    pub direct: bool,
    /// Who it mentions, from the message's own parts and not from its words:
    /// each drawn with the key beside the name (SIP-21), because the name is
    /// this client's word for the key and the key is the fact.
    pub mentions: &'a [Mentioned<'a>],
    /// One of them is the reader.
    pub mentions_me: bool,
    /// The author's key was verified: this reader compared its safety words
    /// with them (SIP-41). Drawn as a mark beside the name -- the one mark a
    /// name may carry, since it is the reader's own and not anybody's claim.
    pub verified: bool,
    /// Ours, and still inside the window in which a rewrite lands. Past it
    /// every reader drops the rewrite (SIP-19), so offering one would be
    /// offering a button that does nothing.
    pub editable: bool,
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

/// Somebody a message mentions, as the bubble draws them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mentioned<'a> {
    /// What this client calls them.
    pub label: &'a str,
    /// The whole key; the bubble shortens it.
    pub key: &'a str,
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
    /// Fetch the file at this index, which was too big to fetch unasked.
    pub fetch: Option<usize>,
    /// What was done to the video at this index.
    pub video: Option<(usize, crate::VideoAction)>,
    /// Forward the file at this index somewhere else.
    pub forward: Option<usize>,
    /// Go to the message this one replies to, by its place in the channel.
    pub jump: Option<u64>,
    /// Open a direct message with whoever sent this.
    pub direct: bool,
    /// Something done about somebody the message mentions, from the card
    /// that opens on their name.
    pub mentioned: Option<MentionAction>,
    /// Compare safety words with the author (SIP-41).
    pub verify: bool,
    /// SIP-56: report this message to the room's admins.
    pub report: bool,
}

/// What the card on a mentioned name offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionAction {
    /// Their whole key, as text.
    pub key: String,
    /// What this client calls them.
    pub label: String,
    pub what: MentionDo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MentionDo {
    /// Open the conversation with them.
    Direct,
    /// Put their key on the clipboard.
    CopyKey,
    /// Start a message to them here: `@name ` into the composer.
    Mention,
    /// Compare safety words with them (SIP-41).
    Verify,
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
    // Wrapped at the pane: a system line is a sentence, and one about who
    // let whom hold a copy of what ran off the right edge of a phone.
    let text = label.into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        ui.available_width(),
        egui::TextStyle::Body,
    );
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

    // Three quarters of the pane. Nothing is reserved beside the bubble any
    // more: what used to sit there hangs off its corner now, on its own
    // layer, and takes no room in the row.
    let available = ui.available_width();
    // More of a narrow pane: three quarters of a phone's width is a column
    // of short lines, and the far side of the bubble has nothing to share
    // the row with.
    let widest = if available < tokens::NARROW_WIDTH {
        WIDEST_NARROW
    } else {
        WIDEST
    };
    let limit = (available * widest).max(160.0);
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
            strip(ui, b, bubble, &mut action);
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
            strip(ui, b, bubble, &mut action);
        });
    }

    action
}

/// Reactions, reply and the rest — on a strip that **hangs off the bubble's
/// top-outer corner**, on its own layer.
///
/// # Why there
///
/// Under the bubble they pushed everything below them down as the pointer
/// moved along a conversation, so reading with the mouse anywhere near the
/// transcript made it twitch. Beside it they took a column of the pane on
/// both sides, whether or not the pointer was anywhere. Off the corner, as a
/// foreground area, they take no room in the row at all and move nothing.
///
/// Shown while the pointer is over the message — or over the strip itself,
/// which is a different layer and so has to be asked separately — so a
/// transcript at rest is a transcript and not a field of buttons.
fn strip(ui: &mut egui::Ui, b: &Bubble<'_>, bubble: egui::Rect, action: &mut BubbleAction) {
    let id = b.id.with("strip");
    let reach = bubble.expand2(egui::vec2(0.0, tokens::SPACING_XS));
    // **Not for a message that has just arrived under the pointer while
    // moving.** A scroll carries message after message under a pointer that
    // has not moved, and each one under it would get a strip for a frame: a
    // flicker of pills up the pane, and a new area every frame, which is a
    // sizing pass every frame and so a repaint every frame for as long as
    // the scroll lasts. A message reveals its strip once it has held still
    // for a frame — but a strip already up **follows** its message when the
    // layout shifts under it (the composer growing, say): an area that
    // moves costs nothing, and one that hides and comes back costs a
    // sizing pass each time.
    let seen = id.with("top");
    let was = ui.ctx().data(|d| d.get_temp::<f32>(seen));
    ui.ctx().data_mut(|d| d.insert_temp(seen, bubble.top()));
    let still = was == Some(bubble.top());
    let up = ui
        .ctx()
        .memory(|m| m.areas().visible_last_frame(&crate::emoji::strip_layer(id)));
    // **Under a finger there is no hover.** A tap on the bubble reveals the
    // strip and a tap anywhere else puts it away; a press held still opens
    // a menu of the same actions where the finger is. Neither happens under
    // a pointer, where hover already does it. Keyed on whether a finger has
    // ever been seen, so a desktop with a touchscreen gets it too.
    let me = b.id;
    let touched = ui.ctx().input(|i| i.has_touch_screen());
    let revealed = egui::Id::new("sigil-message-revealed");
    let mut shown = ui.ctx().data(|d| d.get_temp::<egui::Id>(revealed)) == Some(me);
    let menu = id.with("long-press");
    // Where the tap that asked for the strip to go away landed, if one
    // did: checked against the strip once it has been drawn.
    let mut hide_after: Option<egui::Pos2> = None;
    if touched && !b.redacted {
        let (tap, at, long, press) = ui.input(|i| {
            let p = &i.pointer;
            let press = p.press_start_time();
            // Held, and held still: egui counts a press held past its click
            // duration as "decidedly dragging" whether or not it moved, so
            // stillness is measured from where the press began.
            let still = p
                .press_origin()
                .zip(p.latest_pos())
                .is_some_and(|(began, now)| began.distance(now) < LONG_PRESS_SLOP);
            let long = p.primary_down()
                && still
                && press.is_some_and(|began| i.time - began > LONG_PRESS_SECS);
            (p.primary_clicked(), p.interact_pos(), long, press)
        });
        // **Only the part of the bubble that is on screen.** `reach` is the
        // bubble's *layout* rect, and a message scrolled half out of view
        // still has one -- running up behind the app bar, which is a
        // different panel entirely. `Rect::contains` knows nothing of clip
        // rects or layers, so a tap on the bar at a y inside that hidden
        // part counted as a tap on the message: on a phone, one press on
        // More put a strip on the topmost partly-visible message, which
        // nobody had touched, and it outlived the menu that went up with
        // it. Intersecting with the clip rect is what makes the hit test
        // agree with what can actually be seen and pressed.
        let hittable = reach.intersect(ui.clip_rect());
        let inside = at.is_some_and(|p| hittable.contains(p));
        // **A tap on the strip is not a tap away from the message.** The
        // strip is put away by a tap anywhere else -- and "anywhere else"
        // included the strip itself, so the release that should have sent a
        // reaction took the strip down *before* it was drawn that frame, and
        // the emoji under the finger never existed to be clicked. Seen on the
        // phone: the strip closed and nothing was sent. The strip's own
        // rectangle, as it was drawn last pass, is what says where it is;
        // `layer_id_at` answers about layers built this pass and is not it.
        if tap {
            if inside && !shown {
                ui.ctx().data_mut(|d| d.insert_temp(revealed, me));
                shown = true;
            } else if shown {
                // **Decided after the strip is drawn, not before.** A tap
                // anywhere else puts the strip away -- and "anywhere else"
                // included the strip itself, so the release that should
                // have sent a reaction took the strip down first and the
                // emoji under the finger was never drawn to be clicked.
                // Seen on the phone: the strip closed and nothing was sent.
                // The strip says whether the pointer is on it; the strip
                // stays up this pass either way, which nobody can see.
                hide_after = at;
            }
        }
        // Once per press: the press that opened the menu goes on being
        // held, and must not open it again every frame.
        let fired = id.with("long-press-fired");
        let already = ui.ctx().data(|d| d.get_temp::<f64>(fired));
        if long && inside && already != press {
            if let Some(began) = press {
                ui.ctx().data_mut(|d| d.insert_temp(fired, began));
            }
            // Where the finger is, remembered: the menu is drawn there for
            // as long as it is open, and the finger will have gone.
            if let Some(p) = at {
                ui.ctx().data_mut(|d| d.insert_temp(menu.with("at"), p));
            }
            egui::Popup::open_id(ui.ctx(), menu);
        }
    }
    let over = (still || up)
        && (shown
            || ui.rect_contains_pointer(reach)
            || ui
                .ctx()
                .pointer_hover_pos()
                .is_some_and(|p| ui.ctx().layer_id_at(p) == Some(crate::emoji::strip_layer(id))));

    // The long-press menu, drawn where the finger was, for as long as it is
    // open. The same actions the strip's More menu offers, plus Reply, so
    // nothing needs the small icons to be reached.
    if touched
        && egui::Popup::is_id_open(ui.ctx(), menu)
        && let Some(at) = ui.ctx().data(|d| d.get_temp::<egui::Pos2>(menu.with("at")))
    {
        // **Inside what the system leaves.** A popup is its own layer and
        // no panel's inset applies to it, so a menu opened near the foot of
        // a phone grew into the navigation bar and its last row could not
        // be pressed. egui measures a popup in a sizing pass before it is
        // shown, so the height is known from the first drawn frame; the
        // anchor is lifted by whatever does not fit.
        let at = {
            let safe = sigil::Insets::safe_rect(ui.ctx());
            let tall = ui
                .ctx()
                .memory(|m| m.area_rect(menu))
                .map(|r| r.height())
                .unwrap_or(0.0);
            let mut at = at;
            if tall > 0.0 && at.y + tall > safe.bottom() {
                at.y = (safe.bottom() - tall).max(safe.top());
            }
            at
        };
        egui::Popup::new(
            menu,
            ui.ctx().clone(),
            egui::PopupAnchor::Position(at),
            ui.layer_id(),
        )
        .kind(egui::PopupKind::Menu)
        .open_memory(None)
        .show(|ui| {
            if crate::icon_item(ui, crate::Icon::Reply, "Reply").clicked() {
                action.reply = true;
                ui.close();
            }
            more_menu(ui, b, action);
        });
    }

    // **A menu opened from the strip keeps it up.**
    //
    // The picker and the More menu are drawn beside the strip, outside both
    // it and the bubble — so moving the pointer into one left both regions,
    // the strip stopped being drawn, and the popup went with it. Visible and
    // unreachable, the same defect the controls had when they were under
    // the bubble.
    //
    // One slot in memory rather than a flag per message: only one message
    // can be under the pointer at a time, so remembering *which* is enough.
    let slot = egui::Id::new("sigil-message-controls");
    let any_open = egui::Popup::is_any_open(ui.ctx());
    if over {
        ui.ctx().data_mut(|d| d.insert_temp(slot, me));
    } else if !any_open && ui.ctx().data(|d| d.get_temp::<egui::Id>(slot)) == Some(me) {
        // **Forgotten once it is neither shown nor holding a menu.** The
        // slot names the message whose strip may stay while a menu is
        // open -- and it kept that name after the strip had gone, so any
        // popup at all (the identity's, a phone's title) brought the last
        // strip back onto the transcript.
        ui.ctx().data_mut(|d| d.remove::<egui::Id>(slot));
    }
    let holding = any_open && ui.ctx().data(|d| d.get_temp::<egui::Id>(slot)) == Some(me);
    if !(over || holding) || b.redacted {
        // A tap away from a strip that is not even drawn this pass: there
        // is nothing under the finger to have taken it.
        if hide_after.is_some() {
            hide_strip(ui.ctx());
        }
        return;
    }

    // **A phone spends its width on the reactions**: nine cells across a
    // 360-point pane are 28 points each and missed by a finger, which is
    // what "the emoji do nothing" was. Reply moves inside the More menu
    // there, and the cells are the finger's size.
    let phone = sigil::Form::of(ui.ctx()).is_phone();
    // What we have already sent on this message: those cells are drawn held
    // down, because pressing one again is what takes it back.
    let chosen: Vec<&str> = b
        .reactions
        .iter()
        .filter(|r| r.ours)
        .map(|r| r.emoji.as_str())
        .collect();
    let strip = crate::emoji::Strip {
        id,
        bubble,
        mine: b.mine,
        clip: ui.clip_rect(),
        frequent: b.frequent,
        chosen: &chosen,
        extras: if phone { 1 } else { 2 },
    };
    let strip_rect = crate::emoji::strip(ui, strip, action, |ui, action, cell| {
        if !phone && crate::emoji::cell_icon(ui, crate::Icon::Reply, "Reply", cell).clicked() {
            action.reply = true;
        }
        let more = crate::emoji::cell_icon(ui, crate::Icon::More, "More", cell);
        egui::Popup::menu(&more).show(|ui| {
            if phone && crate::icon_item(ui, crate::Icon::Reply, "Reply").clicked() {
                action.reply = true;
                ui.close();
            }
            more_menu(ui, b, action);
        });
    });
    // The press the tap ended, against the strip as it was just drawn: the
    // position, not a layer test, because a release leaves no pointer over
    // anything by the time this runs.
    if hide_after.is_some_and(|p| !strip_rect.contains(p)) {
        hide_strip(ui.ctx());
    }
}

/// Put away whatever message's strip is showing.
fn hide_strip(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sigil-message-revealed")));
}

/// How long a finger holds still before it is a long press and not a tap.
/// egui's own click duration, which it does not expose.
const LONG_PRESS_SECS: f64 = 0.8;
/// How far a finger may wander and still be holding still: egui's own
/// click distance.
const LONG_PRESS_SLOP: f32 = 6.0;

/// The More menu's items: on the strip, and on the long-press menu.
fn more_menu(ui: &mut egui::Ui, b: &Bubble<'_>, action: &mut BubbleAction) {
    {
        {
            // Rows with their icons, the shape every menu in sigil has:
            // this one was six named buttons of six widths, staircased down
            // a phone -- seen on the device, and the last menu still
            // drawn that way.
            if b.editable && crate::icon_item(ui, crate::Icon::Pencil, "Edit").clicked() {
                action.edit = true;
                ui.close();
            }
            if crate::icon_item(ui, crate::Icon::Close, "Delete").clicked() {
                action.redact = true;
                ui.close();
            }
            if crate::icon_item(ui, crate::Icon::Copy, "Copy key").clicked() {
                action.copy_key = true;
                ui.close();
            }
            // The reply to a room that belongs to one person in it. Absent in
            // the direct message itself and on one's own messages, where it
            // would open the conversation already open, or none.
            if b.direct && crate::icon_item(ui, crate::Icon::Compose, "Direct message").clicked() {
                action.direct = true;
                ui.close();
            }
            // Somebody else's key, to compare words with; ours needs none.
            if !b.mine
                && crate::icon_item(ui, crate::Icon::Verified, "Compare safety words").clicked()
            {
                action.verify = true;
                ui.close();
            }
            // SIP-56: somebody else's message, to the room's admins.
            if !b.mine && crate::icon_item(ui, crate::Icon::Flag, "Report…").clicked() {
                action.report = true;
                ui.close();
            }
            // Saving lives here rather than on the picture: it is the one
            // action nobody takes often, and it had the loudest place on
            // the bubble. One entry per file when there are several --
            // named, since "Save file" on a gallery of four said nothing
            // about which -- and the plain word when there is one.
            match b.attachments {
                [] => {}
                [_] => {
                    if crate::icon_item(ui, crate::Icon::Save, "Save file").clicked() {
                        action.save = Some(0);
                        ui.close();
                    }
                    if crate::icon_item(ui, crate::Icon::Forward, "Forward file").clicked() {
                        action.forward = Some(0);
                        ui.close();
                    }
                }
                many => {
                    for (i, a) in many.iter().enumerate() {
                        let word = format!("Save {}", preview(a.described, 24));
                        if crate::icon_item(ui, crate::Icon::Save, &word).clicked() {
                            action.save = Some(i);
                            ui.close();
                        }
                    }
                    for (i, a) in many.iter().enumerate() {
                        let word = format!("Forward {}", preview(a.described, 24));
                        if crate::icon_item(ui, crate::Icon::Forward, &word).clicked() {
                            action.forward = Some(i);
                            ui.close();
                        }
                    }
                }
            }
        }
    }
}

/// The widest a bubble may be, as a share of the pane it is in.
///
/// Three quarters. It was 0.72 of the pane *less* the controls' room, which on
/// an ordinary window came to about two thirds, and a short message still
/// took two lines because the time was always drawn under the text.
const WIDEST: f32 = 0.75;
/// The same, below `tokens::NARROW_WIDTH`.
const WIDEST_NARROW: f32 = 0.85;

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
    let spans = mention_spans(b.text, b.mentions);
    let body = if b.redacted {
        measure("Deleted", egui::TextStyle::Body)
    } else {
        // The words as they are drawn: a name in them is bold, and bold is
        // wider.
        let job = words_job(
            ui,
            b.text,
            &spans,
            f32::INFINITY,
            egui::Color32::PLACEHOLDER,
            egui::Color32::PLACEHOLDER,
        );
        ui.ctx().fonts_mut(|f| f.layout_job(job)).rect.width()
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
    if let Some(via) = b.via {
        meta += gap + measure(&format!("via {via}"), egui::TextStyle::Small);
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
    } + if b.verified && !b.grouped && !b.mine {
        gap + ui.text_style_height(&egui::TextStyle::Body)
    } else {
        0.0
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
                + if q.preview.is_some() {
                    quote_picture_side(ui) + gap
                } else {
                    0.0
                }
        })
        .unwrap_or(0.0);
    // A picture asks for the size it will be drawn at, not for everything.
    // `INFINITY` made every message carrying a file as wide as the pane
    // allowed, including one whose whole content is `[image, 28 KiB]`.
    // A video is drawn in its own shape, so a portrait one is narrower than
    // the picture width and the bubble should be too: at the picture width
    // it was a tall video with a wide blue field beside it.
    let files = b
        .attachments
        .iter()
        .map(|a| match &a.video {
            Some(v) => crate::video::bubble_size(v.shape).x,
            None => crate::attachment::PICTURE,
        })
        .fold(0.0f32, f32::max);

    // One line when the words and their furniture sit side by side inside the
    // limit. A picture always gets its own rows; so does a tombstone, whose
    // one word is not the message.
    // A mention whose name is not in the words is a chip on a row of its
    // own under them; the widest is a width the bubble has to have, and a
    // row is a row, so such a message is never one line.
    let unmatched = unmatched_mentions(b, &spans);
    let chips = unmatched
        .iter()
        .map(|m| measure(&format!("@{}", m.label), egui::TextStyle::Body))
        .fold(0.0f32, f32::max);
    let together = body + gap * 2.0 + meta;
    let one_line = b.attachments.is_empty()
        && unmatched.is_empty()
        && !b.redacted
        && together + PAD_X * 2.0 <= limit;
    let content = if one_line { together } else { body.max(meta) };
    let width = content.max(author).max(reply).max(files).max(chips) + PAD_X * 2.0;
    Fit {
        width: width.clamp(120.0f32.min(limit), limit),
        one_line,
    }
}

/// Where each mention's name is in the words: the first whole-word
/// occurrence of `@name`, by the name *this* client has for the key --
/// never the sender's spelling, which is text like any other. Byte ranges
/// into `text`, in order, none overlapping; the index is into `mentions`.
///
/// A sender who writes one name over a part that points at somebody else
/// gets no highlight for it: the part is then drawn as a chip under the
/// words, naming whoever the key really is.
fn mention_spans(text: &str, mentions: &[Mentioned<'_>]) -> Vec<(std::ops::Range<usize>, usize)> {
    let mut spans: Vec<(std::ops::Range<usize>, usize)> = Vec::new();
    for (i, m) in mentions.iter().enumerate() {
        let token = format!("@{}", m.label);
        let mut from = 0;
        while let Some(at) = text[from..].find(&token) {
            let start = from + at;
            let end = start + token.len();
            let before_ok = start == 0 || text[..start].ends_with(char::is_whitespace);
            let after_ok = end == text.len() || !text[end..].starts_with(char::is_alphanumeric);
            let free = !spans.iter().any(|(r, _)| r.start < end && start < r.end);
            if before_ok && after_ok && free {
                spans.push((start..end, i));
                break;
            }
            from = start + 1;
        }
    }
    spans.sort_by_key(|(r, _)| r.start);
    spans
}

/// The mentions with no name in the words.
fn unmatched_mentions<'a>(
    b: &'a Bubble<'a>,
    spans: &[(std::ops::Range<usize>, usize)],
) -> Vec<&'a Mentioned<'a>> {
    b.mentions
        .iter()
        .enumerate()
        .filter(|(i, _)| !spans.iter().any(|(_, j)| j == i))
        .map(|(_, m)| m)
        .collect()
}

/// The words with each mentioned name in the accent, laid out to `wrap`.
fn words_job(
    ui: &egui::Ui,
    text: &str,
    spans: &[(std::ops::Range<usize>, usize)],
    wrap: f32,
    plain: egui::Color32,
    accent: egui::Color32,
) -> egui::text::LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut at = 0;
    for (range, _) in spans {
        if range.start > at {
            job.append(
                &text[at..range.start],
                0.0,
                egui::TextFormat::simple(body.clone(), plain),
            );
        }
        job.append(
            &text[range.clone()],
            0.0,
            egui::TextFormat {
                font_id: body.clone(),
                color: accent,
                underline: egui::Stroke::new(1.0, accent),
                ..Default::default()
            },
        );
        at = range.end;
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, egui::TextFormat::simple(body, plain));
    }
    job
}

/// The message's words, with the mentioned names marked in them, and a
/// card with the mark, the name and the whole key on hovering one -- the
/// key is one gesture away because the name is nobody's to vouch for
/// (SIP-21). The words stay selectable.
fn words(
    ui: &mut egui::Ui,
    b: &Bubble<'_>,
    theme: &ColorTheme,
    quiet: egui::Color32,
    wrap: bool,
    action: &mut BubbleAction,
) {
    let spans = mention_spans(b.text, b.mentions);
    if spans.is_empty() {
        let label = egui::Label::new(b.text).selectable(true);
        ui.add(if wrap { label.wrap() } else { label });
        return;
    }
    let plain = ui.visuals().text_color();
    let accent = if b.mine {
        theme.text_primary
    } else {
        theme.accent
    };
    let width = if wrap {
        ui.available_width()
    } else {
        f32::INFINITY
    };
    let job = words_job(ui, b.text, &spans, width, plain, accent);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let response = ui.add(egui::Label::new(galley.clone()).selectable(true));
    // Which name is under the pointer, if any: the galley says which
    // character, the spans say whose it is. Hovering it says who they are;
    // pressing it opens the card with the things to do about them, anchored
    // on the name. Every name's card is offered every pass, whether or not
    // the pointer is still on the name -- an open card has to survive the
    // pointer moving onto it.
    let under = response
        .interact_pointer_pos()
        .or(response.hover_pos())
        .and_then(|pos| {
            let index = galley.cursor_from_pos(pos - response.rect.min).index.0;
            let byte = b
                .text
                .char_indices()
                .nth(index)
                .map(|(i, _)| i)
                .unwrap_or(b.text.len());
            spans.iter().position(|(r, _)| r.contains(&byte))
        });
    let chars = |byte: usize| b.text[..byte].chars().count();
    for (n, (range, i)) in spans.iter().enumerate() {
        let m = &b.mentions[*i];
        let start = galley.pos_from_cursor(egui::text::CCursor::new(chars(range.start)));
        let end = galley.pos_from_cursor(egui::text::CCursor::new(chars(range.end)));
        let name =
            egui::Rect::from_min_max(start.min, end.max).translate(response.rect.min.to_vec2());
        let id = response.id.with(("mention", *i));
        let here = under == Some(n);
        if here {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if !egui::Popup::is_id_open(ui.ctx(), id) {
                response
                    .clone()
                    .on_hover_ui(|ui| mention_card(ui, m, quiet));
            }
        }
        mention_popup(ui, id, name, m, quiet, here && response.clicked(), action);
    }
}

/// The card with the things to do about a mentioned name, opened by a
/// press on it and closed by the next press anywhere.
fn mention_popup(
    ui: &mut egui::Ui,
    id: egui::Id,
    anchor: egui::Rect,
    m: &Mentioned<'_>,
    quiet: egui::Color32,
    open_now: bool,
    action: &mut BubbleAction,
) {
    let theme = ColorTheme::current(ui.ctx());
    egui::Popup::new(
        id,
        ui.ctx().clone(),
        egui::PopupAnchor::ParentRect(anchor),
        ui.layer_id(),
    )
    .open_memory(open_now.then_some(egui::SetOpenCommand::Bool(true)))
    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
    .show(|ui| {
        ui.set_min_width(220.0);
        mention_card(ui, m, quiet);
        ui.separator();
        let mut did = |what: MentionDo| {
            action.mentioned = Some(MentionAction {
                key: m.key.to_string(),
                label: m.label.to_string(),
                what,
            });
        };
        if crate::icon_item(ui, crate::Icon::Compose, "Direct message").clicked() {
            did(MentionDo::Direct);
        }
        if crate::icon_item(ui, crate::Icon::Pencil, "Mention them here").clicked() {
            did(MentionDo::Mention);
        }
        if crate::icon_item(ui, crate::Icon::Copy, "Copy key").clicked() {
            did(MentionDo::CopyKey);
        }
        if crate::icon_item(ui, crate::Icon::Verified, "Compare safety words").clicked() {
            did(MentionDo::Verify);
        }
        let _ = theme;
    });
}

/// The card for a mentioned name: the mark, the name and the whole key.
fn mention_card(ui: &mut egui::Ui, m: &Mentioned<'_>, quiet: egui::Color32) {
    ui.horizontal(|ui| {
        crate::identicon(ui, m.key, tokens::AVATAR_MD);
        ui.vertical(|ui| {
            ui.strong(m.label);
            // The whole key, and in two even halves where it will not fit
            // on one line: on a phone it wrapped with one character on the
            // second line, which reads as a typo. Measured, not guessed --
            // the card is as wide as the pane it is in.
            let text = egui::RichText::new(m.key).monospace().small();
            let mut font = egui::TextStyle::Small.resolve(ui.style());
            font.family = egui::FontFamily::Monospace;
            let whole = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(m.key.to_string(), font, egui::Color32::PLACEHOLDER)
                    .rect
                    .width()
            });
            if whole <= ui.available_width() || m.key.len() < 8 {
                ui.label(text);
            } else {
                let (head, tail) = m.key.split_at(m.key.len().div_ceil(2));
                ui.label(
                    egui::RichText::new(format!("{head}\n{tail}"))
                        .monospace()
                        .small(),
                );
            }
            ui.colored_label(
                quiet,
                egui::RichText::new("The name is what this client calls the key.").small(),
            );
        });
    });
}

/// The mentions whose name is not in the words, one chip each under them,
/// so a mention is never invisible: a sender who wrote one name over a
/// part pointing at somebody else has that somebody named here.
fn mention_chips(
    ui: &mut egui::Ui,
    b: &Bubble<'_>,
    theme: &ColorTheme,
    quiet: egui::Color32,
    action: &mut BubbleAction,
) {
    let spans = mention_spans(b.text, b.mentions);
    let unmatched = unmatched_mentions(b, &spans);
    if unmatched.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (i, m) in unmatched.into_iter().enumerate() {
            let chip = ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("@{}", m.label))
                        .strong()
                        .color(theme.accent),
                )
                .sense(egui::Sense::click()),
            );
            if chip.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let id = chip.id.with(("mention-chip", i));
            if !egui::Popup::is_id_open(ui.ctx(), id) {
                chip.clone().on_hover_ui(|ui| mention_card(ui, m, quiet));
            }
            mention_popup(ui, id, chip.rect, m, quiet, chip.clicked(), action);
        }
    });
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
        .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
        // A message that names the reader is outlined in the accent: the one
        // bubble in a room worth finding again. Not on one's own -- naming
        // oneself is not being addressed.
        .stroke(if b.mentions_me && !b.mine {
            egui::Stroke::new(tokens::STROKE_THICK, theme.accent)
        } else {
            egui::Stroke::NONE
        });
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
        // **A row in a bubble is a line of text, not a control.** The
        // phone's theme makes every row a finger tall for the buttons'
        // sake, and that put a finger's height between a name and the
        // words under it. Inside the bubble a row is as tall as its line.
        if sigil::Form::of(ui.ctx()).is_phone() {
            ui.spacing_mut().interact_size.y =
                ui.text_style_height(&egui::TextStyle::Body) + tokens::SPACING_XS;
        }
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
                // Two or more pictures and clips are a gallery; one, or a
                // file that is neither, is drawn as its own row.
                let pictures: Vec<(usize, &crate::Attachment<'_>)> = b
                    .attachments
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        a.kind == crate::attachment::IMAGE || a.kind == crate::attachment::VIDEO
                    })
                    .collect();
                let in_gallery = pictures.len() >= 2;
                if in_gallery {
                    let did = crate::gallery(ui, &pictures, fill);
                    if let Some(i) = did.open {
                        action.open = Some(i);
                    }
                    if let Some(i) = did.play {
                        action.video = Some((
                            i,
                            crate::VideoAction {
                                open: true,
                                ..Default::default()
                            },
                        ));
                    }
                }
                for (i, a) in b.attachments.iter().enumerate() {
                    if in_gallery && pictures.iter().any(|(j, _)| *j == i) {
                        continue;
                    }
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
                    if did.fetch {
                        action.fetch = Some(i);
                    }
                    if let Some(v) = did.video {
                        action.video = Some((i, v));
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
                        words(ui, b, theme, quiet, false, action);
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
                    words(ui, b, theme, quiet, true, action);
                }
                mention_chips(ui, b, theme, quiet, action);
                meta_row_beneath(ui, b, theme, quiet);
            }
        });
    });

    let rect = inner.response.rect;
    if !b.reactions.is_empty() {
        // **Hung off the bubble's bottom edge**, half over it and half
        // below, from the corner nearest the pane's middle. In a row of
        // their own under the bubble they read as a separate thing -- a
        // message of their own -- rather than as marks on this one.
        let half = crate::emoji::chip_height(ui) / 2.0;
        let area = egui::Rect::from_min_max(
            egui::pos2(rect.left() + PAD_X, rect.bottom() - half),
            egui::pos2(rect.right() - PAD_X, rect.bottom() + half),
        );
        // From the inner corner: one's own bubble sits against the right
        // edge, so its marks hang from its left; everybody else's from the
        // right.
        let layout = if b.mine {
            egui::Layout::left_to_right(egui::Align::Min)
        } else {
            egui::Layout::right_to_left(egui::Align::Min)
        };
        let hung = ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(layout), |ui| {
            for r in b.reactions {
                // **A press names the people; it does not take the mark
                // back.** A chip is two shades and a number, and the one
                // question a reader has of it is who -- which used to be
                // unanswerable, while a finger that brushed it silently
                // undid one's own reaction. Taking it back is still here,
                // said in words, one row further in.
                let chip = reaction_chip(ui, &r.emoji, r.count(), r.ours);
                egui::Popup::menu(&chip).show(|ui| who_reacted(ui, b.reactions, action));
            }
        });
        // The part that hangs below is room the next message must not take.
        let below = (hung.response.rect.bottom() - rect.bottom()).max(0.0);
        ui.allocate_space(egui::vec2(0.0, below));
    }

    rect
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
/// **Always against the bubble's right edge, the time last.** Laid out
/// from the right, so the pieces are placed in reverse -- the time first,
/// then where it came from, then "edited", then the word about the entry,
/// then the receipt -- and read left to right as receipt, standing, edited,
/// via, time. The caller supplies a right-to-left layout that has been
/// given a bounded row: see the two call sites for why it must be bounded.
fn meta_row(ui: &mut egui::Ui, b: &Bubble<'_>, theme: &ColorTheme, quiet: egui::Color32) {
    // **The time is the furthest right, always.** Laid out from the right,
    // so it goes first; everything else reads to its left, nearest first:
    // where it came from, whether it was edited, the word about the entry,
    // the receipt.
    ui.colored_label(quiet, egui::RichText::new(b.at).small());
    // SIP-43: said by the sender, not by any exchange; named by what this
    // machine knows the key as.
    if let Some(via) = b.via {
        ui.colored_label(quiet, egui::RichText::new(format!("via {via}")).small())
            .on_hover_text(format!(
                "The sender posted this through {via}, which carried it to where the \
                 conversation lives. Their word, signed with the message."
            ));
    }
    if b.edited {
        ui.colored_label(quiet, egui::RichText::new("edited").small());
    }
    // SIP-31 **requires** a fork be surfaced, so this is a word in the
    // message and not a line in a diagnostics pane somebody would have to go
    // and look at.
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
    // One row: the name, and the mark beside it when there is one -- a
    // vertical would put the shield on a line of its own under the name.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = tokens::SPACING_XS;
        // **Truncated, because a `horizontal` never wraps.** `fit` measures
        // the author and then clamps the bubble to three quarters of the
        // pane, so a display name longer than that was measured, capped, and
        // then drawn at its full length anyway -- and the frame grew to it.
        // On a phone "Alexandra Constantinopoulos-Whitmore" put every bubble
        // after it ten points past the right edge. An ellipsis is what the
        // chat list already does with the same name, so the two agree; the
        // whole of it is in Members, in full and selectable.
        let mut line = ui.add(egui::Label::new(text).truncate());
        if let Some(title) = b.title {
            line = line.on_hover_text(format!("{title} — self-declared, verified by nobody"));
        }
        let _ = line;
        if b.verified {
            verified_mark(ui);
        }
    });
}

/// The mark beside a verified name: the shield, in the accent, with the
/// word on it. The reader's own comparison and nobody's claim, which is why
/// it is the one thing a name may wear (SIP-21 forbids the others).
pub fn verified_mark(ui: &mut egui::Ui) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let size = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        sigil::icon::draw(ui.painter(), rect, crate::Icon::Verified, theme.accent);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "verified"));
    response.on_hover_text("You compared safety words with them, and they matched.")
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
            // line height it is a dash the length of one word -- and as tall
            // as the picture, when the quote carries one.
            let h = if q.preview.is_some() {
                quote_picture_side(ui)
            } else {
                ui.text_style_height(&egui::TextStyle::Small) + tokens::SPACING_XS
            };
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(tokens::STROKE_THICK, h), egui::Sense::hover());
            ui.painter().rect_filled(rect, tokens::RADIUS_SM, rule);
            ui.add_space(tokens::SPACING_XXS);
            // The picture being replied to, small, before the words about
            // it: a quote of a picture is a picture.
            if let Some(preview) = q.preview {
                let side = quote_picture_side(ui);
                let (pic, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                let uri = preview.uri();
                ui.ctx().include_bytes(
                    uri.clone(),
                    egui::load::Bytes::Shared(preview.bytes.clone()),
                );
                egui::Image::from_bytes(uri, egui::load::Bytes::Shared(preview.bytes.clone()))
                    .corner_radius(tokens::RADIUS_SM)
                    .show_loading_spinner(false)
                    .paint_at(ui, pic);
            }
            // **Truncated, not extended.** A `horizontal` layout does not wrap, so
            // a label long enough grows the frame around it — past the width the
            // bubble was given, and off the side of the pane. This is what cuts
            // the quote to the bubble: the session hands over a couple of hundred
            // characters, and what shows is whatever the width allows.
            // "Ada: lovely" -- or, for a message nobody can name, what it
            // is: "an earlier message", not ": an earlier message".
            let line = if q.who.is_empty() {
                q.said.to_string()
            } else {
                format!("{}: {}", q.who, q.said)
            };
            ui.add(egui::Label::new(egui::RichText::new(line).small().color(quiet)).truncate());
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

/// What the preview above the composer was asked for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplyPreviewAction {
    /// The × in the corner: stop replying (or rewriting).
    pub cancel: bool,
    /// The quote itself: go to the message it names.
    pub jump: bool,
}

/// The message about to be answered, above the composer, drawn as the reply
/// will draw it: the same quote a reply bubble carries, in a bubble of one's
/// own, with the way out in the top right corner.
///
/// **A bubble, not a bar.** "Replying to: lovely [Cancel]" in a row of plain
/// words looked like a status line, and nothing about it said what the
/// message would look like once sent. This is that message's head, before the
/// words are written under it.
///
/// A rewrite heads the same way, captioned, since a bare quote reads as a
/// reply.
pub fn reply_preview(ui: &mut egui::Ui, q: Quote<'_>, rewriting: bool) -> ReplyPreviewAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = ReplyPreviewAction::default();
    // One's own bubble, since the message this heads will be.
    let fill = theme.accent_muted;
    let quiet = faded(theme.text_primary, fill);
    let rule = theme.text_primary;
    let out = if rewriting {
        "Cancel rewrite"
    } else {
        "Cancel reply"
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(tokens::RADIUS_PILL)
        .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
        .show(ui, |ui| {
            // The × first, so it lands in the corner and the quote takes what
            // is left rather than pushing it off the side.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if crate::icon_button_named(ui, crate::Icon::Close, out)
                    .on_hover_text(out)
                    .clicked()
                {
                    action.cancel = true;
                }
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    if rewriting {
                        ui.label(egui::RichText::new("Rewriting").small().color(quiet));
                    }
                    if reply_stub(ui, q, quiet, rule) {
                        action.jump = true;
                    }
                });
            });
        });
    action
}

/// One emoji and how many people sent it. Ours is outlined.
pub fn reaction_chip(ui: &mut egui::Ui, emoji: &str, count: usize, ours: bool) -> egui::Response {
    crate::emoji::chip(ui, emoji, count, ours)
}

/// Who reacted, and with which emoji: a row per reaction, the emoji and the
/// names beside it.
///
/// Every row is pressable and sends that emoji, which for one's own -- the
/// row with "You" in it -- takes it back. The one place a reaction is
/// undone on purpose, now that a press on the chip is a question rather
/// than an act.
fn who_reacted(ui: &mut egui::Ui, reactions: &[Reaction], action: &mut BubbleAction) {
    ui.set_max_width(WHO_WIDE);
    for r in reactions {
        if crate::emoji::emoji_item(ui, &r.emoji, &r.names()).clicked() {
            action.react = Some(r.emoji.clone());
            ui.close();
        }
    }
}

/// How wide the who-reacted list gets before the names wrap: a phone's pane
/// less its margins, so the list is the same shape on either form.
const WHO_WIDE: f32 = 260.0;

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

    /// A quote's picture is registered by the blob it is of and by nothing
    /// else: named by the quoted message's number, message 12's picture in
    /// one conversation was drawn on message 12's quote in every other,
    /// because egui keeps the first bytes given for a name.
    #[test]
    fn a_quotes_picture_is_named_by_its_blob() {
        let a: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"a"[..]);
        let b: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"b"[..]);
        let one = Thumb {
            id: "blobA",
            bytes: &a,
        };
        let other = Thumb {
            id: "blobB",
            bytes: &b,
        };
        assert_ne!(one.uri(), other.uri());
        assert_eq!(
            one.uri(),
            Thumb {
                id: "blobA",
                bytes: &b
            }
            .uri()
        );
        assert!(one.uri().contains("blobA"));
    }

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
            id: egui::Id::new("plain"),
            key: "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9",
            name: None,
            title: None,
            text,
            at: "12:00",
            mine: false,
            grouped: true,
            edited: false,
            via: None,
            redacted: false,
            reply_to: None,
            reactions: &[],
            frequent: &[],
            receipt: None,
            attachments: files,
            standing: None,
            alarming: false,
            direct: false,
            mentions: &[],
            mentions_me: false,
            verified: false,
            editable: false,
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
                held: false,
                size: 2100,
                id: "abc123",
                video: None,
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

    /// A name is found in the words by this client's spelling of it, as a
    /// whole word, once; a name that is not there is not invented.
    #[test]
    fn a_mentioned_name_is_found_in_the_words_by_our_spelling() {
        let ada = Mentioned {
            label: "Ada",
            key: "k1",
        };
        let adam = Mentioned {
            label: "Adam",
            key: "k2",
        };
        let spans = mention_spans("hi @Ada and @Adam", &[ada, adam]);
        assert_eq!(spans, vec![(3..7, 0), (12..17, 1)]);
        assert_eq!(
            mention_spans("hi @Adam", &[ada]),
            vec![],
            "@Adam is not @Ada"
        );
        assert_eq!(
            mention_spans("mail@Ada.example", &[ada]),
            vec![],
            "inside a word"
        );
        assert_eq!(mention_spans("@Ada @Ada", &[ada]), vec![(0..4, 0)], "once");
        // The sender wrote one name over a part naming somebody else: the
        // words carry no span for it, and the chip will.
        assert_eq!(mention_spans("hi @Eve", &[ada]), vec![]);
    }
    /// **The one pair the palette's own floor does not reach.**
    ///
    /// `theme.rs` checks muted text against the three surfaces, and body
    /// text against `accent_muted` -- the sent bubble's ground. It does not
    /// check what `faded` produces on that ground, and `faded`'s own comment
    /// says so: "that floor is only ever checked against the three
    /// surfaces". This is that check. The time, "edited", a receipt and a
    /// quoted reply are all drawn in it, on one's own messages, in both
    /// themes, and nothing measured them.
    ///
    /// **Why 3.0 and not 4.5.** Body text on a sent bubble is 4.59 in the
    /// dark theme -- barely over its own bar -- so *no* amount of fading
    /// clears 4.5 for something quieter than it. Reaching 4.5 here would
    /// mean changing the accent, which is the product's colour and not this
    /// function's decision. So the floor is the palette's own 3.0, held
    /// deliberately rather than by accident, and the test says which.
    #[test]
    fn quiet_text_on_a_sent_bubble_clears_the_palette_floor() {
        fn channel(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: egui::Color32) -> f32 {
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        fn ratio(a: egui::Color32, b: egui::Color32) -> f32 {
            let (x, y) = (luminance(a), luminance(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        }

        for (name, t) in [
            ("dark", sigil::theme::dark()),
            ("light", sigil::theme::light()),
        ] {
            let fill = t.accent_muted;
            let quiet = faded(t.text_primary, fill);
            let r = ratio(quiet, fill);
            assert!(
                r >= 3.0,
                "{name}: the time and the receipt on one's own bubble are \
                 {r:.2} against it, below the 3.0 this palette holds itself to"
            );
            // And it really is quieter than the body beside it, which is the
            // whole point of fading it: a "quiet" colour that measures the
            // same as the body is a mix that is doing nothing.
            let body = ratio(t.text_primary, fill);
            assert!(
                r < body,
                "{name}: quiet text is {r:.2} and the body is {body:.2} -- \
                 the fade is not fading anything"
            );
        }
    }
}
