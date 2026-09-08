//! Icons, painted.
//!
//! # Why none of these is a character
//!
//! egui bundles a small font set and it does not contain the symbols an
//! interface reaches for. `✓`, `✓✓` and `↳` all shipped here as `□`, and
//! nothing caught it: the accessibility tree carries the *string*, which was
//! correct, so every text assertion passed. Before that, `●`/`○` did the same
//! in `dot`. Only a snapshot shows it, and only if somebody looks at the
//! snapshot.
//!
//! So these are drawn with lines and arcs. That costs a few dozen lines each
//! and buys an interface that cannot render as boxes on somebody else's
//! machine, in a font they have and we did not test.
//!
//! # Every icon says a word
//!
//! An icon is a convention somebody has to already know, and no assistive
//! technology can get meaning out of a shape. So each one emits a
//! `WidgetInfo::labeled` with the word for what it does, and carries the same
//! word as a tooltip. **A control that is only a picture is a control some
//! people cannot use.**

use sigil::{ColorTheme, tokens};

/// What an icon depicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Place a call.
    Call,
    /// End one.
    HangUp,
    /// Channel settings.
    Settings,
    /// Who is in this conversation.
    People,
    /// This account's devices.
    Device,
    /// Make something new.
    Plus,
    /// Start a new conversation.
    Compose,
    /// Search.
    Search,
    /// Send a file.
    Attach,
    /// Send what is written.
    Send,
    /// Go back.
    Back,
    /// Put away, cancel, dismiss.
    Close,
    /// Ask again.
    Refresh,
    /// Edit.
    Pencil,
    /// A public channel.
    Public,
    /// Reply to a message.
    Reply,
    /// React to one.
    React,
    /// More, on one message.
    More,
}

impl Icon {
    /// The word for it. The tooltip, and what the accessibility tree carries.
    pub fn word(self) -> &'static str {
        match self {
            Icon::Call => "Call",
            Icon::HangUp => "Hang up",
            Icon::Settings => "Settings",
            Icon::People => "Members",
            Icon::Device => "Devices",
            Icon::Plus => "New",
            Icon::Compose => "New conversation",
            Icon::Search => "Search",
            Icon::Attach => "Attach a file",
            Icon::Send => "Send",
            Icon::Back => "Back",
            Icon::Close => "Close",
            Icon::Refresh => "Refresh",
            Icon::Pencil => "Edit",
            Icon::Public => "Public channel",
            Icon::Reply => "Reply",
            Icon::React => "React",
            Icon::More => "More",
        }
    }
}

/// Paint one inside `rect`, in `colour`.
pub fn draw(painter: &egui::Painter, rect: egui::Rect, icon: Icon, colour: egui::Color32) {
    // Everything is drawn against a unit square and scaled, so an icon is the
    // same shape at any size and the set stays visually consistent.
    let s = rect.width().min(rect.height());
    let o = rect.center() - egui::vec2(s / 2.0, s / 2.0);
    let p = |x: f32, y: f32| o + egui::vec2(x * s, y * s);
    // Weight scaled to the icon, with a floor: at 16px a hairline stroke is
    // there but not legible, and an icon nobody can make out is worse than the
    // word it replaced.
    let stroke = egui::Stroke::new((s * 0.11).max(1.5), colour);
    let line = |a: egui::Pos2, b: egui::Pos2| painter.line_segment([a, b], stroke);
    let path = |pts: Vec<egui::Pos2>| {
        painter.add(egui::Shape::line(pts, stroke));
    };

    match icon {
        Icon::Call | Icon::HangUp => {
            // A handset: two ends and a bar between them.
            let pts = vec![
                p(0.24, 0.28),
                p(0.30, 0.22),
                p(0.44, 0.34),
                p(0.38, 0.42),
                p(0.58, 0.62),
                p(0.66, 0.56),
                p(0.78, 0.70),
                p(0.72, 0.76),
            ];
            path(pts);
            if icon == Icon::HangUp {
                // Struck through, so refusing is not the same shape as taking.
                line(p(0.20, 0.80), p(0.80, 0.20));
            }
        }
        Icon::Settings => {
            // Three sliders, which reads at 16px where a cogwheel does not.
            for (i, x) in [0.28f32, 0.5, 0.72].iter().enumerate() {
                line(p(*x, 0.18), p(*x, 0.82));
                let y = [0.62f32, 0.36, 0.54][i];
                painter.circle_filled(p(*x, y), s * 0.09, colour);
            }
        }
        Icon::People => {
            // Two heads and shoulders.
            painter.circle_stroke(p(0.38, 0.34), s * 0.14, stroke);
            path(vec![
                p(0.18, 0.78),
                p(0.22, 0.60),
                p(0.54, 0.60),
                p(0.58, 0.78),
            ]);
            painter.circle_stroke(p(0.68, 0.36), s * 0.10, stroke);
            path(vec![p(0.62, 0.60), p(0.80, 0.60), p(0.84, 0.74)]);
        }
        Icon::Device => {
            // A slab with a button.
            painter.rect_stroke(
                egui::Rect::from_min_max(p(0.30, 0.16), p(0.70, 0.84)),
                s * 0.10,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(p(0.44, 0.74), p(0.56, 0.74));
        }
        Icon::Plus => {
            line(p(0.5, 0.22), p(0.5, 0.78));
            line(p(0.22, 0.5), p(0.78, 0.5));
        }
        Icon::Compose => {
            // A speech bubble with a plus in it. `Plus` alone appears three
            // times in one column as a field's submit; this is the one that
            // makes a *conversation*, and it should not be the same shape.
            let (a, b) = (p(0.14, 0.18), p(0.86, 0.68));
            painter.rect_stroke(
                egui::Rect::from_min_max(a, b),
                s * 0.16,
                stroke,
                egui::StrokeKind::Middle,
            );
            // The tail, which is what makes it a bubble and not a box.
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.28, 0.66), p(0.46, 0.66), p(0.28, 0.86)],
                colour,
                egui::Stroke::NONE,
            ));
            line(p(0.5, 0.28), p(0.5, 0.58));
            line(p(0.35, 0.43), p(0.65, 0.43));
        }
        Icon::Search => {
            painter.circle_stroke(p(0.44, 0.44), s * 0.24, stroke);
            line(p(0.62, 0.62), p(0.80, 0.80));
        }
        Icon::Attach => {
            // A paperclip: one stroke down, round the bottom, and back up
            // short of where it started. Drawn wide, because the first version
            // was a narrow squiggle that read as nothing at all.
            let mut clip = vec![p(0.70, 0.24), p(0.70, 0.66)];
            // The turn at the bottom, so it reads as a clip and not a bracket.
            for step in 0..=10 {
                let a = std::f32::consts::PI * (step as f32 / 10.0);
                clip.push(p(0.5 + 0.20 * a.cos(), 0.66 + 0.14 * a.sin()));
            }
            clip.push(p(0.30, 0.30));
            // The short return, which is what makes it a clip rather than a U.
            path(clip);
            path(vec![
                p(0.30, 0.30),
                p(0.38, 0.22),
                p(0.50, 0.26),
                p(0.50, 0.62),
            ]);
        }
        Icon::Send => {
            // An arrow leaving to the right.
            line(p(0.20, 0.50), p(0.76, 0.50));
            path(vec![p(0.56, 0.30), p(0.78, 0.50), p(0.56, 0.70)]);
        }
        Icon::Back => {
            line(p(0.80, 0.50), p(0.24, 0.50));
            path(vec![p(0.44, 0.30), p(0.22, 0.50), p(0.44, 0.70)]);
        }
        Icon::Close => {
            line(p(0.26, 0.26), p(0.74, 0.74));
            line(p(0.74, 0.26), p(0.26, 0.74));
        }
        Icon::Refresh => {
            // An arc that stops short, and a head at its end pointing the way
            // it was going. Two earlier attempts put a head *on* the stroke,
            // where it merged into it and the whole thing read as a bare C.
            let r = s * 0.28;
            let c = p(0.5, 0.5);
            let at = |a: f32| c + egui::vec2(a.cos() * r, a.sin() * r);
            let (from, to) = (std::f32::consts::PI * 0.25, std::f32::consts::PI * 1.55);
            let arc: Vec<egui::Pos2> = (0..=28)
                .map(|step| at(from + (to - from) * step as f32 / 28.0))
                .collect();
            path(arc);

            let end = at(to);
            // The tangent, which is the direction the arc was travelling.
            let tangent = egui::vec2(-(to).sin(), (to).cos());
            let normal = egui::vec2(-tangent.y, tangent.x);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    end + tangent * s * 0.26,
                    end - tangent * s * 0.04 + normal * s * 0.17,
                    end - tangent * s * 0.04 - normal * s * 0.17,
                ],
                colour,
                egui::Stroke::NONE,
            ));
        }
        Icon::Pencil => {
            path(vec![
                p(0.24, 0.76),
                p(0.30, 0.58),
                p(0.66, 0.22),
                p(0.78, 0.34),
                p(0.42, 0.70),
            ]);
            line(p(0.42, 0.70), p(0.24, 0.76));
        }
        Icon::Reply => {
            // An arrow turning back on itself: in, then up and away.
            path(vec![
                p(0.78, 0.76),
                p(0.72, 0.52),
                p(0.44, 0.44),
                p(0.44, 0.30),
            ]);
            // The head, on the end that is going somewhere.
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.44, 0.18), p(0.56, 0.38), p(0.32, 0.38)],
                colour,
                egui::Stroke::NONE,
            ));
        }
        Icon::React => {
            // A face. Anything else for "react" is a guess at an emoji, and
            // the wire carries any string a client cares to send.
            painter.circle_stroke(p(0.5, 0.5), s * 0.32, stroke);
            painter.circle_filled(p(0.39, 0.42), s * 0.055, colour);
            painter.circle_filled(p(0.61, 0.42), s * 0.055, colour);
            let mouth: Vec<egui::Pos2> = (0..=10)
                .map(|step| {
                    let a = std::f32::consts::PI * (0.15 + 0.7 * step as f32 / 10.0);
                    p(0.5 + 0.20 * a.cos(), 0.56 + 0.16 * a.sin())
                })
                .collect();
            path(mouth);
        }
        Icon::More => {
            for x in [0.26f32, 0.5, 0.74] {
                painter.circle_filled(p(x, 0.5), s * 0.075, colour);
            }
        }
        Icon::Public => {
            // A globe: a circle with a meridian and an equator.
            let r = s * 0.30;
            let c = p(0.5, 0.5);
            painter.circle_stroke(c, r, stroke);
            line(c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0));
            let mut left = Vec::new();
            for step in 0..=16 {
                let t = step as f32 / 16.0;
                let y = -r + 2.0 * r * t;
                let x = (1.0 - (y / r).powi(2)).max(0.0).sqrt() * r * 0.5;
                left.push(c + egui::vec2(x, y));
            }
            path(left.clone());
            path(
                left.iter()
                    .map(|q| egui::pos2(2.0 * c.x - q.x, q.y))
                    .collect(),
            );
        }
    }
}

/// A button that is an icon.
///
/// The word travels with it: as a tooltip, and in the accessibility tree.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    icon_button_as(ui, icon, icon.word(), None)
}

/// The same, saying what it does *here*.
///
/// A shape means different things in different places — a plus is "new" beside
/// a list and "write to them" beside a key field — and the word is the part
/// that has to be right, because it is what somebody who cannot see the shape
/// is given.
pub fn icon_button_named(ui: &mut egui::Ui, icon: Icon, word: &str) -> egui::Response {
    icon_button_as(ui, icon, word, None)
}

/// The same, in a colour of its own — for anything destructive.
pub fn icon_button_tinted(
    ui: &mut egui::Ui,
    icon: Icon,
    tint: Option<egui::Color32>,
) -> egui::Response {
    icon_button_as(ui, icon, icon.word(), tint)
}

fn icon_button_as(
    ui: &mut egui::Ui,
    icon: Icon,
    word: &str,
    tint: Option<egui::Color32>,
) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let size = egui::vec2(tokens::BUTTON_MD, tokens::BUTTON_MD);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), word));
    if ui.is_rect_visible(rect) {
        // A hit target the size of a finger, with the mark drawn smaller
        // inside it. A 16px icon with a 16px target is a control people miss.
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_MD, theme.interactive_hover);
        }
        let colour = tint.unwrap_or(if response.hovered() {
            theme.text_primary
        } else {
            theme.text_secondary
        });
        // Drawn at `ICON_MD` inside a `BUTTON_MD` target: the mark has to be
        // big enough to read and the target big enough to hit, and those are
        // two different sizes.
        let inner = egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(tokens::ICON_MD, tokens::ICON_MD),
        );
        draw(ui.painter(), inner, icon, colour);
    }
    response.on_hover_text(word.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_says_a_word() {
        // An icon is a convention somebody has to already know, and no
        // assistive technology can read meaning out of a shape.
        for icon in [
            Icon::Call,
            Icon::HangUp,
            Icon::Settings,
            Icon::People,
            Icon::Device,
            Icon::Plus,
            Icon::Compose,
            Icon::Search,
            Icon::Attach,
            Icon::Send,
            Icon::Back,
            Icon::Close,
            Icon::Refresh,
            Icon::Pencil,
            Icon::Public,
            Icon::Reply,
            Icon::React,
            Icon::More,
        ] {
            let word = icon.word();
            assert!(word.len() > 2, "{icon:?} has no word");
            // ASCII, because this crate's own chrome must not depend on a
            // glyph the bundled fonts lack -- see the module note.
            assert!(word.is_ascii(), "{icon:?}'s word is not ASCII: {word}");
        }
    }

    #[test]
    fn taking_a_call_and_refusing_one_are_not_the_same_word() {
        assert_ne!(Icon::Call.word(), Icon::HangUp.word());
    }
}
