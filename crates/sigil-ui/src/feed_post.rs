//! One post in a SIP-88 feed.
//!
//! A conversation's message is a bubble: it has a side, because a message is
//! between two or more people and which of them said it is the first thing to
//! know. A feed post has no side — everything in a feed is by one person, and
//! a timeline is by people you chose. So it is a row, full width, with the
//! author at the top of it, which is what every publishing interface has
//! settled on for the same reason.
//!
//! # What is deliberately not here
//!
//! No reaction, no reply, no comment: SIP-88 §Nothing comes in. There is no
//! inbound path of any kind, so a control offering one would be a control
//! that cannot work.
//!
//! No count of how many times a post was quoted, which SIP-89 §Counting
//! forbids outright: no party can enumerate the feeds holding a quote, so any
//! number is a count of the feeds this client happens to read, and rendering
//! a sample as a total is the claim the stack refuses elsewhere.

use sigil::{ColorTheme, tokens};

/// Why a post's body is not here, as the row says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absent {
    /// Its author took it off this exchange.
    Withdrawn,
    /// The exchange dropped it, with nothing from the author behind it.
    /// **Drawn differently from a withdrawal**, which SIP-32 requires: one is
    /// a person's act and the other is an operator's.
    Removed,
}

/// A post, as a row needs it. Plain data; the caller decides everything.
pub struct FeedPost<'a> {
    /// The author's key, for the mark and as the thing that never lies.
    pub key: &'a str,
    /// Their picture, where one has been seen. `None` draws the identicon.
    pub picture: Option<&'a egui::TextureHandle>,
    /// What this client calls them. **Self-declared and attested by nobody**
    /// (SIP-21), so it is drawn as ordinary text with the key beside it.
    /// `None` falls back to the key, which is never wrong.
    pub named: Option<&'a str>,
    /// The author's own feed, so the row can say so rather than make the
    /// reader compare keys.
    pub mine: bool,
    /// Already formatted, and already clamped by the caller to the lesser of
    /// the author's claim and the exchange's observation.
    pub at: &'a str,
    /// The author's own claim, where it is far enough from `at` to be worth
    /// marking as one. **Never drawn as the time**: SIP-88 says an
    /// implementation MUST NOT present `issued_at` as established.
    pub claimed: Option<&'a str>,
    pub text: &'a str,
    /// A later edit replaced the words. SIP-19: presenting an edit as the
    /// original hides that the text changed after it was read.
    pub edited: bool,
    /// The body is not here, and which of the two reasons it is.
    pub absent: Option<Absent>,
    /// Parts of a kind this reader does not know. SIP-19 asks a client to say
    /// the post carried something it could not display.
    pub unknown: usize,
    /// What a SIP-89 citation on this post resolved to, already in words, or
    /// `None` where the post cites nothing. The caller resolves; this draws.
    pub cites: Option<Cited<'a>>,
    /// SIP-88 §Attachments: the files this post carries.
    pub files: &'a [Shown<'a>],
    /// SIP-90: how this post regards what it cites.
    ///
    /// **A post with a regard and no words is the reaction itself**, and is
    /// drawn as one — the emoji with who gave it, over what they gave it to.
    /// There is no reaction anywhere else and no count of them: no party can
    /// enumerate the feeds holding one, so what a reader is shown is the
    /// regards this client actually read, each with a name on it.
    pub regard: Option<&'a str>,
    /// SIP-88 §Succession: this post is the first its new holder signed, and
    /// who held the feed before. Drawn as a break above the post.
    ///
    /// **A MUST, and the one SIP-88 says is most likely to ship missing.** A
    /// stolen key is the capture of everything an account ever published,
    /// under a chain that stays unbroken across the seam — so nothing in the
    /// log tells a reader the person writing it changed, and this is the only
    /// thing that can.
    pub after_seam: Option<&'a str>,
}

/// One file on a post, as the row draws it.
///
/// **The sender's own thumbnail, which travelled inside the post.** SIP-18
/// has carried a preview since it was written — "rendering one means decoding
/// the image, and a terminal client has nothing to show it on. The field
/// exists for a client that does" — and in a feed it arrives with the post
/// rather than needing a fetch of its own. So a picture is on the screen the
/// moment the post is, and fetching the full blob is a second act.
pub struct Shown<'a> {
    /// What it is, in words: `[image 1920x1080, 2.1 MB]`. Drawn where there
    /// is no picture, and read out where there is.
    pub described: &'a str,
    /// The sender's thumbnail, already decoded by the caller.
    pub picture: Option<&'a egui::TextureHandle>,
}

/// A citation, in whichever of its states it is.
///
/// **Not one string.** SIP-89 §When it cannot be resolved lists eleven
/// outcomes and says a reader MUST NOT collapse them, because "withdrawn by
/// its author", "no longer held" and "could not be reached" are facts about
/// different things and a person acts differently on each.
pub struct Cited<'a> {
    /// Who wrote the cited post, once it resolved. `None` before it has:
    /// SIP-89 carries no author name in the part on purpose, so that there is
    /// nothing to put in front of a reader that looks like the quoted person
    /// speaking before anything has been checked.
    pub named: Option<&'a str>,
    /// The cited post's words, where they were fetched and verified.
    pub text: Option<&'a str>,
    /// What to say instead, where there are no words to show. Already a
    /// sentence; the caller picks it from the state it resolved to.
    pub instead: Option<&'a str>,
}

/// What somebody pressed on a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostPress {
    /// The author: go to their feed.
    Author,
    /// SIP-90: regard this post, with the emoji given.
    ///
    /// **Not a reaction that goes anywhere near the author.** It publishes a
    /// post in the regarder's own feed naming this one, because that is the
    /// only log they may append to. The author learns of it only if somebody
    /// tells them, which is the same as the rest of publishing.
    Regard(&'static str),
    /// Carry this into my own feed, with words of my own (SIP-89).
    Quote,
    /// Take mine off this exchange (SIP-88 §Withdrawal).
    Withdraw,
    /// Open the cited post's own feed.
    Cited,
}

/// How big the author's mark is, and so how wide the gutter beside a post.
///
/// **The body is indented under the name and not under the mark.** An
/// earlier draft put the whole post at the left edge on the argument that a
/// 40-point gutter down a phone is a sixth of the screen spent on alignment
/// and a feed is mostly words. That argument is about the cost and not about
/// what is bought: a timeline is read by skimming down one column of faces
/// and names, and the gutter is what makes that column exist. Every post
/// after the first then starts where the last one's words started, so the
/// eye does not have to find the beginning of each.
const MARK: f32 = 40.0;

/// How big a regard's cell is on a post.
///
/// **Painted, not typed.** An emoji in a string literal here is a tofu box:
/// the bundled text font does not have them, and `sigil_ui::emoji::paint`
/// draws the bundled SVG instead. `sigil_emoji::QUICK` is the same short row
/// the chat's quick reactions use, so the two places offer the same gestures
/// and a reader learns one set.
const REGARD_CELL: f32 = 28.0;

/// The tallest a picture on a post is drawn.
///
/// A portrait photograph is three times as tall as it is wide, and at a
/// phone's full width that is a post nobody can scroll past. The thumbnail
/// keeps its shape and is bounded by whichever of the two binds first.
const PICTURE_TALL: f32 = 320.0;

/// Draw one post. Returns what was pressed, if anything.
pub fn feed_post(ui: &mut egui::Ui, post: &FeedPost<'_>) -> Option<PostPress> {
    let theme = ColorTheme::current(ui.ctx());
    let mut pressed = None;

    // **Above the post, across the column.** A seam is a fact about the
    // join between two posts, so it goes where the join is; drawn inside the
    // post it would read as something about that post.
    if let Some(from) = post.after_seam {
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                theme.warning,
                egui::RichText::new(format!(
                    "This feed changed hands here. Everything below was published by \
                     {from}, who is not whoever holds it now."
                ))
                .small(),
            );
        });
        ui.add_space(tokens::SPACING_SM);
        ui.separator();
    }
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(0, tokens::SPACING_SM as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // **The mark takes a gutter and everything else takes the
            // column beside it.** See [`MARK`]: this is the shape that makes
            // a timeline skimmable, and it costs the width of one face.
            ui.horizontal_top(|ui| {
                if crate::avatar(ui, post.key, post.picture, MARK)
                    .on_hover_text("Their feed")
                    .clicked()
                {
                    pressed = Some(PostPress::Author);
                }
                ui.add_space(tokens::SPACING_SM);
                ui.vertical(|ui| {
                    // Zeroed, as `message::centred` does it: the head is
                    // lines nobody taps, and a phone raises every row to 44
                    // points. Scoped to the head alone — the controls at the
                    // foot are tapped and keep their height.
                    ui.scope(|ui| {
                        ui.spacing_mut().interact_size.y = 0.0;
                        // **One line, name then time.** Two lines of head
                        // over one line of words reads as a record rather
                        // than as somebody saying something.
                        ui.horizontal_wrapped(|ui| {
                            let name = post.named.unwrap_or(post.key);
                            ui.add(
                                egui::Label::new(egui::RichText::new(name).strong())
                                    .truncate()
                                    .selectable(false),
                            );
                            if post.mine {
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new("you").small(),
                                );
                            }
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(format!("· {}", post.at)).small(),
                            );
                            // **A claim, said as one.** The author's clock is
                            // signed by the author and nothing else; the
                            // exchange's is signed by nobody. Neither is a
                            // fact and only one of them can be ordered by.
                            if let Some(claimed) = post.claimed {
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new(format!("· author says {claimed}"))
                                        .small()
                                        .italics(),
                                )
                                .on_hover_text(
                                    "The time the author put on it, which nothing checks. \
                                     The time beside it is when this exchange saw it.",
                                );
                            }
                            if post.edited {
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new("· edited").small(),
                                );
                            }
                        });
                    });
                    ui.add_space(tokens::SPACING_XS);
                    // SIP-90: the regard, where there is one. Above the citation it
                    // is about, because the emoji is what this post *says* and the
                    // citation is what it says it about.
                    if let Some(emoji) = post.regard {
                        ui.horizontal_wrapped(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(tokens::REGARD_SIZE, tokens::REGARD_SIZE),
                                egui::Sense::hover(),
                            );
                            crate::emoji::paint(ui, emoji, rect, theme.text_primary);
                            ui.colored_label(
                                theme.text_secondary,
                                egui::RichText::new(match post.named {
                                    Some(name) => format!("{name} regarded this"),
                                    None => "regarded this".to_string(),
                                })
                                .small(),
                            );
                        });
                    }
                    match post.absent {
                        // **Two sentences, not one.** A reader acting on "the author
                        // took this down" and on "the operator dropped it" does
                        // different things, and SIP-32 requires the second be
                        // visible as the second.
                        Some(Absent::Withdrawn) => {
                            ui.colored_label(
                                theme.text_secondary,
                                egui::RichText::new("The author took this post off this exchange.")
                                    .italics(),
                            );
                        }
                        Some(Absent::Removed) => {
                            ui.colored_label(
                                theme.warning,
                                egui::RichText::new(
                                    "This exchange no longer holds this post, and its author did \
                             not say to remove it.",
                                )
                                .italics(),
                            );
                        }
                        None => {
                            if !post.text.is_empty() {
                                ui.add(egui::Label::new(post.text).wrap().selectable(true));
                            }
                            if post.unknown > 0 {
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new(match post.unknown {
                                        1 => "It carried something this version cannot show."
                                            .to_string(),
                                        n => format!(
                                            "It carried {n} things this version cannot show."
                                        ),
                                    })
                                    .small(),
                                );
                            }
                        }
                    }

                    // The files, between the words and whatever is cited: a picture
                    // belongs to the post that carries it, and a citation is about
                    // somebody else's.
                    for file in post.files {
                        ui.add_space(tokens::SPACING_XS);
                        match file.picture {
                            Some(texture) => {
                                // **Bounded by the column and by a height**, so a tall
                                // picture does not push the next post off the screen
                                // and a wide one does not widen the pane — a ui grows
                                // to what is drawn in it.
                                let room = ui.available_width();
                                let size = texture.size_vec2();
                                let scale = (room / size.x).min(PICTURE_TALL / size.y).min(1.0);
                                ui.add(
                                    egui::Image::new(texture)
                                        .fit_to_exact_size(size * scale)
                                        .corner_radius(tokens::RADIUS_MD),
                                )
                                .on_hover_text(file.described);
                            }
                            None => {
                                ui.colored_label(
                                    theme.text_secondary,
                                    egui::RichText::new(file.described).small(),
                                );
                            }
                        }
                    }

                    if let Some(cited) = &post.cites {
                        ui.add_space(tokens::SPACING_XS);
                        if cited_ui(ui, cited, &theme) {
                            pressed = Some(PostPress::Cited);
                        }
                    }

                    ui.add_space(tokens::SPACING_XS);
                    // **Wrapped, because the gutter took the width it needs
                    // from this row.** A `horizontal` goes on placing past
                    // the edge rather than stopping, so the last regard was
                    // drawn half off a 360-point screen the moment the post
                    // was indented — and the phone-width guard did not see
                    // it, which is its own finding.
                    ui.horizontal_wrapped(|ui| {
                        // **Tightened, because the row has to fit.** Seven
                        // marks at the ordinary gap are wider than a phone's
                        // column once the gutter is taken off it, and a row
                        // that wraps for its last glyph is a row with a
                        // stray glyph under it. Wrapping is still on, so a
                        // narrower pane than this gives way rather than
                        // clipping.
                        ui.spacing_mut().item_spacing.x = tokens::SPACING_XS;
                        // **No reaction and no reply**, because a feed has no inbound
                        // path at all (SIP-88 §Nothing comes in). Carrying it into
                        // one's own feed is the whole of what a reader can do with
                        // somebody else's post, and it is SIP-89's.
                        // **Marks and not words, which is what an action row
                        // is.** `named_control` draws the word on a phone,
                        // and "Quote" beside "Take it off" beside six
                        // regards wrapped onto a second line for one glyph.
                        // The word is still what a screen reader is given.
                        if post.absent.is_none()
                            && sigil::icon::icon_button_named(ui, crate::Icon::Quote, "Quote")
                                .on_hover_text(
                                    "Carry this into your own feed, with words of your own. It is \
                             a pointer, not a copy: if the author withdraws it, it goes \
                             from your post too.",
                                )
                                .clicked()
                        {
                            pressed = Some(PostPress::Quote);
                        }
                        // SIP-90. A short row of the ordinary ones rather than a
                        // picker: a regard is a small gesture and a dialog to make
                        // one is a dialog nobody opens.
                        if post.absent.is_none() {
                            for quick in sigil_emoji::QUICK {
                                if crate::emoji::glyph(ui, quick, REGARD_CELL, false)
                                    .on_hover_text(
                                        "Publishes this in your own feed, naming theirs. Nothing \
                                 reaches them: a feed has no way in, and nobody can count \
                                 these.",
                                    )
                                    .clicked()
                                {
                                    pressed = Some(PostPress::Regard(quick));
                                }
                            }
                        }
                        if post.mine
                            && post.absent.is_none()
                            && sigil::icon::icon_button_named(ui, crate::Icon::Bin, "Take it off")
                                .on_hover_text(
                                    "Takes it off this exchange. It does not take it back: a feed \
                             post is public, and everybody who read it has a copy that \
                             verifies without asking anybody.",
                                )
                                .clicked()
                        {
                            pressed = Some(PostPress::Withdraw);
                        }
                    });
                });
            });
        });
    ui.separator();
    pressed
}

/// The cited post, under the one citing it. Returns whether it was pressed.
fn cited_ui(ui: &mut egui::Ui, cited: &Cited<'_>, theme: &ColorTheme) -> bool {
    let mut opened = false;
    egui::Frame::NONE
        .fill(theme.surface_elevated)
        .corner_radius(tokens::RADIUS_MD)
        .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 0.0;
                // The author's name only once it has been fetched and
                // checked. Before that there is nothing to name, and SIP-89
                // keeps it that way on purpose.
                if let Some(named) = cited.named {
                    ui.add(
                        egui::Label::new(egui::RichText::new(named).small().strong())
                            .truncate()
                            .selectable(false),
                    );
                }
                match (cited.text, cited.instead) {
                    (Some(text), _) => {
                        ui.add(egui::Label::new(text).wrap().selectable(false));
                        opened = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new("Open this post").small().underline(),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .clicked();
                    }
                    // **Every one of these names its own unavailability**,
                    // and none of them is silence. SIP-89's table is written
                    // in the same tone SIP-19 settled on for a reply whose
                    // target is missing.
                    (None, Some(said)) => {
                        ui.colored_label(
                            theme.text_secondary,
                            egui::RichText::new(said).small().italics(),
                        );
                    }
                    (None, None) => {
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new("Looking for the post this cites…")
                                .small()
                                .italics(),
                        );
                    }
                }
            });
        });
    opened
}
