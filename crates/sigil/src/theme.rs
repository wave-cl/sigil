//! Colour, in three layers.
//!
//! 1. Private consts naming the raw palette, so a colour is chosen once.
//! 2. [`ColorTheme`], a flat `Copy` struct carrying *both* the fields egui's
//!    [`egui::Visuals`] needs and semantic names of our own (`surface_elevated`,
//!    `interactive_hover`, `speaking`) that `Visuals` has nowhere to put.
//! 3. [`ColorTheme::current`], which reads the active theme back out of egui's
//!    own temp data — so any widget anywhere gets semantic colour from nothing
//!    but a `&egui::Context`, with no plumbing through call sites.
//!
//! Both themes are registered with egui up front by [`install`], so switching
//! between them is `ctx.set_theme(..)` and never a rebuild.

use egui::{Color32, CornerRadius, Stroke, Style, Theme, Visuals};

use crate::tokens;

// -- the palette -------------------------------------------------------------
// sigil's own, not borrowed. A cold blue-violet for the accent, because the
// destructive and warning colours have to stay unmistakably distinct from it
// and a warm accent leaves too little room.

const ACCENT: Color32 = Color32::from_rgb(0x6E, 0x8B, 0xFF);
const ACCENT_DIM: Color32 = Color32::from_rgb(0x4A, 0x63, 0xC8);
// **Three states, two themes, and a contrast floor.** Each of these carries
// words at body size -- a refusal is the one nobody can afford not to read --
// so each has to clear 4.5 against all three surfaces of the theme it is in.
// A red that reads on near-black does not read on near-white, and the light
// theme's were between 3.24 and 4.00: legible-looking to whoever picked them,
// and thin for everybody else. `muted_text_stays_legible_on_every_surface`
// fails if any of these six drifts back under.
const DESTRUCTIVE: Color32 = Color32::from_rgb(0xED, 0x5F, 0x71);
const WARNING: Color32 = Color32::from_rgb(0xE8, 0xA8, 0x4B);
const SUCCESS: Color32 = Color32::from_rgb(0x4C, 0xC0, 0x8A);

// The same three, darkened for a light ground. Same hue and saturation,
// stepped down in value until each clears the floor.
const L_DESTRUCTIVE: Color32 = Color32::from_rgb(0xB5, 0x49, 0x57);
const L_WARNING: Color32 = Color32::from_rgb(0x99, 0x60, 0x00);
const L_SUCCESS: Color32 = Color32::from_rgb(0x1A, 0x7D, 0x4F);

// Dark surfaces, lightest last.
const D_BASE: Color32 = Color32::from_rgb(0x14, 0x15, 0x19);
const D_SURFACE: Color32 = Color32::from_rgb(0x1B, 0x1D, 0x22);
const D_RAISED: Color32 = Color32::from_rgb(0x24, 0x27, 0x2E);
const D_EDGE: Color32 = Color32::from_rgb(0x33, 0x37, 0x40);
const D_EDGE_STRONG: Color32 = Color32::from_rgb(0x45, 0x4A, 0x56);
const D_TEXT: Color32 = Color32::from_rgb(0xEC, 0xED, 0xF0);
const D_TEXT_2: Color32 = Color32::from_rgb(0xA8, 0xAD, 0xB8);
const D_TEXT_3: Color32 = Color32::from_rgb(0x74, 0x7A, 0x87);

// Light surfaces, darkest last.
const L_BASE: Color32 = Color32::from_rgb(0xFB, 0xFB, 0xFD);
const L_SURFACE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const L_RAISED: Color32 = Color32::from_rgb(0xF2, 0xF3, 0xF6);
const L_EDGE: Color32 = Color32::from_rgb(0xDE, 0xE0, 0xE6);
const L_EDGE_STRONG: Color32 = Color32::from_rgb(0xC2, 0xC6, 0xD0);
const L_TEXT: Color32 = Color32::from_rgb(0x16, 0x18, 0x1D);
const L_TEXT_2: Color32 = Color32::from_rgb(0x53, 0x58, 0x63);
const L_TEXT_3: Color32 = Color32::from_rgb(0x83, 0x89, 0x95);

/// Every colour sigil draws with, for one of the two themes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTheme {
    // Fields egui's Visuals wants.
    pub panel_fill: Color32,
    pub extreme_bg_color: Color32,
    pub window_fill: Color32,
    pub window_stroke: Color32,
    pub text_color: Color32,
    pub hyperlink_color: Color32,
    pub error_fg_color: Color32,
    pub warn_fg_color: Color32,
    pub selection_bg: Color32,
    pub selection_stroke: Color32,

    // Semantic surfaces.
    pub surface_primary: Color32,
    pub surface_secondary: Color32,
    pub surface_elevated: Color32,

    // Semantic text.
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // Semantic actions.
    pub accent: Color32,
    /// The accent, muted enough to sit *behind* text.
    ///
    /// [`accent`](Self::accent) is a foreground colour and putting body text on
    /// it fails contrast in the light theme. This is the one for a filled
    /// surface — a sent message's bubble, a selected row.
    pub accent_muted: Color32,
    pub destructive: Color32,
    pub warning: Color32,
    pub success: Color32,

    // Borders.
    pub border_default: Color32,
    pub border_strong: Color32,

    // Interactive states.
    pub interactive_hover: Color32,
    pub interactive_pressed: Color32,

    // sigil's own vocabulary, which no general design system would have.
    /// Ring around an avatar while that peer is speaking.
    pub speaking: Color32,
    /// The connection light: link up, retrying, gone. Always drawn beside the
    /// *word*, never alone — a colour is not a message.
    pub link_up: Color32,
    pub link_retrying: Color32,
    pub link_gone: Color32,
}

pub fn dark() -> ColorTheme {
    ColorTheme {
        panel_fill: D_BASE,
        extreme_bg_color: Color32::from_rgb(0x0D, 0x0E, 0x11),
        window_fill: D_SURFACE,
        window_stroke: D_EDGE,
        text_color: D_TEXT,
        hyperlink_color: ACCENT,
        error_fg_color: DESTRUCTIVE,
        warn_fg_color: WARNING,
        selection_bg: ACCENT_DIM,
        selection_stroke: ACCENT,

        surface_primary: D_BASE,
        surface_secondary: D_SURFACE,
        surface_elevated: D_RAISED,

        text_primary: D_TEXT,
        text_secondary: D_TEXT_2,
        text_muted: D_TEXT_3,

        accent: ACCENT,
        accent_muted: ACCENT_DIM,
        destructive: DESTRUCTIVE,
        warning: WARNING,
        success: SUCCESS,

        border_default: D_EDGE,
        border_strong: D_EDGE_STRONG,

        interactive_hover: D_RAISED,
        interactive_pressed: D_EDGE,

        speaking: SUCCESS,
        link_up: SUCCESS,
        link_retrying: WARNING,
        link_gone: DESTRUCTIVE,
    }
}

pub fn light() -> ColorTheme {
    ColorTheme {
        panel_fill: L_BASE,
        extreme_bg_color: Color32::WHITE,
        window_fill: L_SURFACE,
        window_stroke: L_EDGE,
        text_color: L_TEXT,
        hyperlink_color: ACCENT_DIM,
        error_fg_color: L_DESTRUCTIVE,
        warn_fg_color: L_WARNING,
        selection_bg: Color32::from_rgb(0xD4, 0xDC, 0xFF),
        selection_stroke: ACCENT_DIM,

        surface_primary: L_BASE,
        surface_secondary: L_SURFACE,
        surface_elevated: L_RAISED,

        text_primary: L_TEXT,
        text_secondary: L_TEXT_2,
        text_muted: L_TEXT_3,

        accent: ACCENT_DIM,
        accent_muted: ACCENT,
        destructive: L_DESTRUCTIVE,
        warning: L_WARNING,
        success: L_SUCCESS,

        border_default: L_EDGE,
        border_strong: L_EDGE_STRONG,

        interactive_hover: L_RAISED,
        interactive_pressed: L_EDGE,

        speaking: L_SUCCESS,
        link_up: L_SUCCESS,
        link_retrying: L_WARNING,
        link_gone: L_DESTRUCTIVE,
    }
}

// -- stashing, so widgets need no plumbing -----------------------------------

const STASH_DARK: &str = "sigil_theme_dark";
const STASH_LIGHT: &str = "sigil_theme_light";

impl ColorTheme {
    /// The theme matching whatever egui is currently drawing in.
    ///
    /// egui's [`Visuals`] has nowhere to keep our semantic names, so both
    /// themes are stashed in its temp data by [`install`] and read back here.
    /// The fallback matters: a `Context` that never went through `install` —
    /// a bare test harness, say — still gets sensible colour rather than a
    /// panic or a black screen.
    pub fn current(ctx: &egui::Context) -> ColorTheme {
        let is_dark = ctx.theme() == Theme::Dark;
        let id = egui::Id::new(if is_dark { STASH_DARK } else { STASH_LIGHT });
        ctx.data(|d| d.get_temp(id))
            .unwrap_or_else(|| if is_dark { dark() } else { light() })
    }

    /// Map onto egui's own visuals. Only the fields egui knows about; the
    /// semantic ones are carried separately and read via [`current`].
    pub fn visuals(&self, base: Visuals) -> Visuals {
        let radius = CornerRadius::same(tokens::RADIUS_MD as u8);
        let mut v = base;

        v.panel_fill = self.panel_fill;
        v.extreme_bg_color = self.extreme_bg_color;
        v.window_fill = self.window_fill;
        v.window_stroke = Stroke::new(tokens::STROKE_THIN, self.window_stroke);
        v.hyperlink_color = self.hyperlink_color;
        v.error_fg_color = self.error_fg_color;
        v.warn_fg_color = self.warn_fg_color;
        v.window_corner_radius = CornerRadius::same(tokens::RADIUS_LG as u8);

        v.selection.bg_fill = self.selection_bg;
        v.selection.stroke = Stroke::new(tokens::STROKE_THIN, self.selection_stroke);

        // Widget states. `noninteractive`'s fg_stroke is what plain `ui.label`
        // text is drawn with, so it carries text_primary rather than a border
        // colour -- the one field in here that is not what its name suggests.
        v.widgets.noninteractive.bg_fill = self.surface_primary;
        v.widgets.noninteractive.weak_bg_fill = self.surface_primary;
        v.widgets.noninteractive.bg_stroke = Stroke::new(tokens::STROKE_THIN, self.border_default);
        v.widgets.noninteractive.fg_stroke = Stroke::new(tokens::STROKE_THIN, self.text_primary);
        v.widgets.noninteractive.corner_radius = radius;

        v.widgets.inactive.bg_fill = self.surface_elevated;
        v.widgets.inactive.weak_bg_fill = self.surface_secondary;
        v.widgets.inactive.bg_stroke = Stroke::new(tokens::STROKE_THIN, self.border_default);
        v.widgets.inactive.fg_stroke = Stroke::new(tokens::STROKE_THIN, self.text_secondary);
        v.widgets.inactive.corner_radius = radius;

        v.widgets.hovered.bg_fill = self.interactive_hover;
        v.widgets.hovered.weak_bg_fill = self.interactive_hover;
        v.widgets.hovered.bg_stroke = Stroke::new(tokens::STROKE_THIN, self.border_strong);
        v.widgets.hovered.fg_stroke = Stroke::new(tokens::STROKE_MEDIUM, self.text_primary);
        v.widgets.hovered.corner_radius = radius;

        v.widgets.active.bg_fill = self.interactive_pressed;
        v.widgets.active.weak_bg_fill = self.interactive_pressed;
        v.widgets.active.bg_stroke = Stroke::new(tokens::STROKE_THIN, self.accent);
        v.widgets.active.fg_stroke = Stroke::new(tokens::STROKE_MEDIUM, self.text_primary);
        v.widgets.active.corner_radius = radius;

        v.widgets.open.bg_fill = self.surface_elevated;
        v.widgets.open.weak_bg_fill = self.surface_elevated;
        v.widgets.open.bg_stroke = Stroke::new(tokens::STROKE_THIN, self.border_default);
        v.widgets.open.fg_stroke = Stroke::new(tokens::STROKE_THIN, self.text_primary);
        v.widgets.open.corner_radius = radius;

        // A spinner over a not-yet-loaded avatar is more distracting than the
        // gap it fills, and every roster row would have one on a cold start.
        v.image_loading_spinners = false;
        v
    }
}

/// Register both themes with egui and stash them for [`ColorTheme::current`].
///
/// Called once at startup. Registering *both* is what makes switching a
/// preference change rather than a rebuild.
pub fn install(ctx: &egui::Context, light_theme: ColorTheme, dark_theme: ColorTheme) {
    ctx.set_visuals_of(Theme::Light, light_theme.visuals(Visuals::light()));
    ctx.set_visuals_of(Theme::Dark, dark_theme.visuals(Visuals::dark()));
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new(STASH_LIGHT), light_theme);
        d.insert_temp(egui::Id::new(STASH_DARK), dark_theme);
    });
    // The form the host installed before this, if any. A desktop installs
    // none and gets the style it always had; a phone gets the touch scale.
    let form = crate::Form::of(ctx);
    ctx.all_styles_mut(|style| {
        custom_style(style);
        if form.is_phone() {
            phone_style(style);
        }
    });
}

/// What a finger needs that a pointer does not, and a text scale to match
/// a screen held at arm's length: taller controls, more padding, more slop
/// around a target, a scroll bar that stays out of the way, and body text a
/// size up. Only ever applied on [`crate::Form::Phone`].
fn phone_style(style: &mut Style) {
    use egui::{FontFamily, FontId, TextStyle};
    style.spacing.interact_size.y = tokens::BUTTON_LG;
    style.spacing.button_padding = egui::vec2(tokens::SPACING_MD, tokens::SPACING_SM + 2.0);
    style.interaction.interact_radius = 8.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    for (text_style, size) in [
        (TextStyle::Body, 15.0),
        (TextStyle::Button, 15.0),
        (TextStyle::Small, 12.0),
        (TextStyle::Heading, 20.0),
    ] {
        style
            .text_styles
            .insert(text_style, FontId::new(size, FontFamily::Proportional));
    }
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );
}

/// Style that is not colour: animation speed, and the spacing rhythm.
fn custom_style(style: &mut Style) {
    style.animation_time = tokens::ANIM_SPEED;
    style.spacing.item_spacing = egui::vec2(tokens::SPACING_SM, tokens::SPACING_SM);
    style.spacing.button_padding = egui::vec2(tokens::SPACING_MD, tokens::SPACING_SM);
    style.spacing.menu_margin = egui::Margin::same(tokens::SPACING_SM as i8);
    // Tooltips that wait are tooltips nobody sees, and several of sigil's carry
    // the full key behind a shortened one.
    style.interaction.tooltip_delay = 0.1;
    style.interaction.show_tooltips_only_when_still = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stash has a fallback for a `Context` that never saw `install`, and
    /// the fallback must be the *right* theme rather than merely a valid one.
    #[test]
    fn current_falls_back_to_the_matching_theme_without_install() {
        let ctx = egui::Context::default();
        ctx.set_theme(Theme::Dark);
        assert_eq!(ColorTheme::current(&ctx), dark());
        ctx.set_theme(Theme::Light);
        assert_eq!(ColorTheme::current(&ctx), light());
    }

    #[test]
    fn install_makes_current_return_the_installed_theme() {
        let ctx = egui::Context::default();
        let mut custom = dark();
        custom.accent = Color32::from_rgb(1, 2, 3);
        install(&ctx, light(), custom);
        ctx.set_theme(Theme::Dark);
        assert_eq!(ColorTheme::current(&ctx).accent, Color32::from_rgb(1, 2, 3));
        ctx.set_theme(Theme::Light);
        assert_eq!(
            ColorTheme::current(&ctx).accent,
            light().accent,
            "the other theme is untouched"
        );
    }

    /// Muted text must still be legible against the surface it sits on. A
    /// contrast floor is the one property of a palette worth asserting: it is
    /// the failure that looks fine to whoever picked the colours.
    /// **The touch scale reaches a phone and not a desktop.**
    ///
    /// `phone_style` is a third again on every control, a bigger hit radius
    /// and body text a size up. Applied to a desktop it would grow the whole
    /// interface; left off a phone it would leave every target pointer-sized
    /// on the one device that has no pointer. Neither says anything: both
    /// are an interface that draws.
    #[test]
    fn the_touch_scale_is_a_phones_and_only_a_phones() {
        let desktop = egui::Context::default();
        install(&desktop, light(), dark());
        let pointer = desktop.style_of(egui::Theme::Dark).spacing.interact_size.y;

        let phone = egui::Context::default();
        crate::Form::install(&phone, crate::Form::Phone);
        install(&phone, light(), dark());
        let finger = phone.style_of(egui::Theme::Dark).spacing.interact_size.y;

        assert_eq!(
            finger,
            tokens::BUTTON_LG,
            "a phone's controls are a finger tall"
        );
        assert!(
            pointer < finger,
            "a desktop got the touch scale too: {pointer} against {finger}"
        );
        assert!(
            phone.style_of(egui::Theme::Dark).text_styles[&egui::TextStyle::Body].size
                > desktop.style_of(egui::Theme::Dark).text_styles[&egui::TextStyle::Body].size,
            "a phone's body text is a size up"
        );
    }

    /// **The form has to be installed before the theme**, and a form
    /// installed after takes effect on the next one.
    ///
    /// `install` reads the form once, while it is building the style. A host
    /// that sets the form afterwards gets a phone with pointer-sized targets
    /// -- on the device only, and silently, because an interface with small
    /// buttons is an interface. Both are installed every pass in the shell
    /// and in the harnesses, which is what makes the order recoverable
    /// rather than permanent; this says so out loud.
    #[test]
    fn a_form_installed_after_the_theme_arrives_with_the_next_theme() {
        let ctx = egui::Context::default();
        install(&ctx, light(), dark());
        crate::Form::install(&ctx, crate::Form::Phone);
        assert!(
            ctx.style_of(egui::Theme::Dark).spacing.interact_size.y < tokens::BUTTON_LG,
            "the style was built before the form was known, so it is still \
             the pointer's -- the order matters and this is the wrong one"
        );

        // The next pass installs both again, as every host does.
        install(&ctx, light(), dark());
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).spacing.interact_size.y,
            tokens::BUTTON_LG,
            "and the pass after picks it up, which is what makes the wrong \
             order a flicker rather than a phone nobody can press"
        );
    }

    #[test]
    fn muted_text_stays_legible_on_every_surface() {
        fn luminance(c: Color32) -> f32 {
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
        fn ratio(a: Color32, b: Color32) -> f32 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }
        // Every pair that falls short, not the first: a palette is fixed as a
        // palette, and one number at a time hides the shape of the problem.
        let mut thin: Vec<String> = Vec::new();
        for (name, t) in [("dark", dark()), ("light", light())] {
            for (surface_name, surface) in [
                ("primary", t.surface_primary),
                ("secondary", t.surface_secondary),
                ("elevated", t.surface_elevated),
            ] {
                let r = ratio(t.text_muted, surface);
                assert!(
                    r >= 3.0,
                    "{name}/{surface_name}: muted text contrast {r:.2} is below 3.0"
                );
            }

            // A sent message is body text on `accent_muted`, so that pair has
            // to clear the *body* bar rather than the large-text one. Getting
            // this wrong makes your own messages the unreadable ones, in one
            // theme only, which is exactly the sort of thing that ships.
            let r = ratio(t.text_primary, t.accent_muted);
            assert!(
                r >= 4.5,
                "{name}: body text on a sent bubble is {r:.2}, below 4.5"
            );

            // **Every other colour sigil writes words in.** The three above
            // were the ones somebody had thought about; these are the rest,
            // and each of them carries something at body size on each of the
            // three surfaces: the prose under every heading is
            // `text_secondary`, a sender's name and a link are `accent`, and
            // a refusal -- the one nobody can afford not to read -- is
            // `destructive`.
            for (fg_name, fg) in [
                ("primary", t.text_primary),
                ("secondary", t.text_secondary),
                ("accent", t.accent),
                ("destructive", t.destructive),
                ("warning", t.warning),
                ("success", t.success),
            ] {
                for (surface_name, surface) in [
                    ("primary", t.surface_primary),
                    ("secondary", t.surface_secondary),
                    ("elevated", t.surface_elevated),
                ] {
                    let r = ratio(fg, surface);
                    if r < 4.5 {
                        thin.push(format!("{name}: {fg_name} on {surface_name} is {r:.2}"));
                    }
                }
            }
        }
        assert!(
            thin.is_empty(),
            "{} pair(s) of colours are below the body-text floor of 4.5:\n  {}",
            thin.len(),
            thin.join("\n  ")
        );
    }
}
