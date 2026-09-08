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
    /// The mark, and the word for it.
    ///
    /// Both, always. A tick is a convention somebody has to already know, and
    /// the accessibility tree cannot read a glyph's meaning out of it.
    pub fn mark(self) -> (&'static str, &'static str) {
        match self {
            Receipt::Sending => ("·", "sending"),
            Receipt::Sent => ("✓", "sent"),
            Receipt::Delivered => ("✓✓", "delivered"),
            Receipt::Read => ("✓✓", "read"),
            Receipt::Failed => ("!", "did not send"),
        }
    }
}

/// One message, as the interface needs it.
#[derive(Default)]
pub struct Bubble<'a> {
    /// The author's key, in full.
    pub key: &'a str,
    /// Their display name, if a profile has been seen. Never shown alone.
    pub name: Option<&'a str>,
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
}

impl BubbleAction {
    pub fn is_none(&self) -> bool {
        *self == BubbleAction::default()
    }
}

/// A date, once, above the first message of each day.
pub fn day_separator(ui: &mut egui::Ui, label: &str) {
    let theme = ColorTheme::current(ui.ctx());
    ui.add_space(tokens::SPACING_MD);
    ui.horizontal(|ui| {
        let line = |ui: &mut egui::Ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width() * 0.5 - 40.0, 1.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .hline(rect.x_range(), rect.center().y, (1.0, theme.border_default));
        };
        line(ui);
        ui.colored_label(theme.text_muted, label);
        line(ui);
    });
    ui.add_space(tokens::SPACING_XS);
}

/// The frozen line above the first message that was unread on opening.
///
/// **Frozen on entry, not live.** Reading advances the read mark, so a divider
/// that tracked it would disappear exactly when somebody wanted to see where
/// they had got to.
pub fn unread_divider(ui: &mut egui::Ui, count: usize) {
    let theme = ColorTheme::current(ui.ctx());
    ui.add_space(tokens::SPACING_SM);
    ui.horizontal(|ui| {
        ui.colored_label(
            theme.accent,
            match count {
                1 => "1 new message".to_string(),
                n => format!("{n} new messages"),
            },
        );
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter()
            .hline(rect.x_range(), rect.center().y, (1.0, theme.accent));
    });
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
pub fn bubble(ui: &mut egui::Ui, b: &Bubble<'_>) -> BubbleAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = BubbleAction::default();

    ui.add_space(if b.grouped {
        tokens::SPACING_XS
    } else {
        tokens::SPACING_SM
    });

    let layout = if b.mine {
        egui::Layout::right_to_left(egui::Align::TOP)
    } else {
        egui::Layout::left_to_right(egui::Align::TOP)
    };

    ui.with_layout(layout, |ui| {
        // The avatar column keeps its width even when grouped, so a run of
        // messages from one person stays aligned instead of stepping sideways.
        if !b.mine {
            // The gutter is the *same width* either way, so a run of messages
            // from one person stays in a column instead of stepping sideways
            // as the avatar comes and goes. `add_space` and a widget are not
            // interchangeable here: item spacing follows a widget and not a
            // space, which is exactly the few pixels that made them disagree.
            let size = tokens::AVATAR_SM;
            if b.grouped {
                ui.add_space(size + ui.spacing().item_spacing.x);
            } else {
                crate::identicon(ui, b.key, size);
            }
        }

        let fill = if b.mine {
            theme.accent_muted
        } else {
            theme.surface_elevated
        };

        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() * 0.78).max(120.0));
            let frame = egui::Frame::NONE
                .fill(fill)
                .corner_radius(tokens::RADIUS_LG)
                .inner_margin(egui::Margin::symmetric(
                    tokens::SPACING_MD as i8,
                    tokens::SPACING_SM as i8,
                ));
            let inner = frame.show(ui, |ui| {
                if !b.grouped && !b.mine {
                    author_line(ui, b, &theme);
                }
                if let Some((who, stub)) = b.reply_to {
                    reply_stub(ui, who, stub, &theme);
                }
                if b.redacted {
                    // The tombstone. Deleting the row instead would destroy the
                    // one thing a redaction is for: the record that something
                    // was here.
                    ui.label(
                        egui::RichText::new("Deleted")
                            .italics()
                            .color(theme.text_muted),
                    );
                } else {
                    ui.add(egui::Label::new(b.text).wrap().selectable(true));
                }
                // Our own bubble is filled with the accent, and `text_muted`
                // is chosen to sit on a *surface*. On the accent it comes out
                // near-invisible -- which is how the time and the "edited"
                // mark disappeared from exactly the messages whose delivery
                // somebody most wants to check.
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
                    if let Some(receipt) = b.receipt {
                        let (mark, word) = receipt.mark();
                        let colour = match receipt {
                            Receipt::Failed => theme.destructive,
                            Receipt::Read if !b.mine => theme.accent,
                            _ => quiet,
                        };
                        ui.colored_label(colour, egui::RichText::new(mark).small())
                            .on_hover_text(word);
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

            // The per-message menu. Everything the terminal client's pick mode
            // offers, on a control that does not need to be discovered.
            inner.response.context_menu(|ui| {
                if ui.button("Reply").clicked() {
                    action.reply = true;
                    ui.close();
                }
                if b.mine && !b.redacted && ui.button("Edit").clicked() {
                    action.edit = true;
                    ui.close();
                }
                if !b.redacted && ui.button("Delete").clicked() {
                    action.redact = true;
                    ui.close();
                }
                if ui.button("Copy key").clicked() {
                    action.copy_key = true;
                    ui.close();
                }
            });
        });
    });

    action
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
    match b.name {
        Some(name) => {
            // A profile name is self-declared and nobody attests it, so it is
            // drawn as ordinary text and the key is one hover away. It must
            // never be styled as though the exchange vouched for it.
            ui.label(egui::RichText::new(name).strong().color(theme.accent))
                .on_hover_text(b.key.to_string());
        }
        None => {
            ui.label(
                egui::RichText::new(short(b.key))
                    .strong()
                    .monospace()
                    .color(theme.accent),
            )
            .on_hover_text(b.key.to_string());
        }
    }
}

fn reply_stub(ui: &mut egui::Ui, who: &str, stub: &str, theme: &ColorTheme) {
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: tokens::SPACING_SM as i8,
            ..Default::default()
        })
        .show(ui, |ui| {
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(format!("↳ {who}: {stub}")).small(),
            );
        });
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
pub fn short(key: &str) -> String {
    let n = key.len().min(8);
    format!("{}…", &key[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_receipt_says_a_word_as_well_as_a_mark() {
        // A tick is a convention; the word is what the accessibility tree can
        // read out and what somebody who does not know the convention gets.
        for r in [
            Receipt::Sending,
            Receipt::Sent,
            Receipt::Delivered,
            Receipt::Read,
            Receipt::Failed,
        ] {
            let (mark, word) = r.mark();
            assert!(!mark.is_empty(), "{r:?} has no mark");
            assert!(word.len() > 2, "{r:?} has no word: {word}");
        }
    }

    #[test]
    fn delivered_and_read_differ_in_more_than_colour() {
        // They share a mark, so the word is the only thing telling them apart
        // and it has to.
        assert_ne!(Receipt::Delivered.mark().1, Receipt::Read.mark().1);
    }

    #[test]
    fn a_short_key_still_leads_somewhere() {
        let s = short("2vXsQ9pC3nR7bK1mW8dF");
        assert!(s.ends_with('…'), "must not look like a whole key: {s}");
        assert!(s.len() < 12);
    }

    #[test]
    fn short_never_panics_on_a_key_shorter_than_the_window() {
        assert_eq!(short(""), "…");
        assert_eq!(short("ab"), "ab…");
    }
}
