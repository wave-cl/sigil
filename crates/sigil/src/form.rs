//! What kind of thing sigil is running in, and what the system draws over it.
//!
//! **Layout is decided by width, not by this.** A narrow desktop window
//! collapses to one pane the same way a phone does, from
//! [`crate::layout`], and nothing here changes that. [`Form`] says only what
//! a phone *is* that a narrow window is not: it is touched, its system bars
//! and keyboard lie over the surface rather than beside it, and it has no
//! window chrome of its own. The host sets it once; the desktop never does
//! and reads [`Form::Desktop`], so every desktop path is the arm that was
//! there before this existed.
//!
//! Touch itself is not a form. Whether a finger has been seen is
//! `ctx.input(|i| i.has_touch_screen())`, and a desktop with a touchscreen
//! gets the touch behaviour too.

use crate::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Form {
    #[default]
    Desktop,
    Phone,
}

const KEY: &str = "sigil_form";

impl Form {
    /// Say what this is. Once, by the host, before the theme is installed;
    /// a later call changes the answer for the next pass.
    pub fn install(ctx: &egui::Context, form: Form) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(KEY), form));
    }

    /// What this is. [`Form::Desktop`] when nobody said.
    pub fn of(ctx: &egui::Context) -> Form {
        ctx.data(|d| d.get_temp(egui::Id::new(KEY)))
            .unwrap_or_default()
    }

    pub fn is_phone(self) -> bool {
        matches!(self, Form::Phone)
    }

    /// The side of an icon button's hit target: a finger's on a phone.
    pub fn button_size(self) -> f32 {
        match self {
            Form::Desktop => tokens::BUTTON_MD,
            Form::Phone => tokens::BUTTON_LG,
        }
    }

    /// How wide the rail is.
    pub fn rail_width(self) -> f32 {
        match self {
            Form::Desktop => tokens::RAIL_WIDTH,
            Form::Phone => tokens::RAIL_TOUCH,
        }
    }

    /// The margin the body keeps from the rail and the edges.
    pub fn body_margin(self) -> f32 {
        match self {
            Form::Desktop => tokens::SPACING_LG,
            Form::Phone => tokens::SPACING_MD,
        }
    }
}

/// What the system draws over the surface, in points: a status bar at the
/// top, a gesture bar -- or the keyboard, while it is up -- at the bottom, a
/// cutout at a side. The shell keeps everything clear of them. On a desktop
/// only `top` is ever set, for macOS's window buttons.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Insets {
    /// Where these are kept, so anything drawn outside the shell's panels
    /// can ask. **A popup is not in a panel**: the shell keeps its panels
    /// clear of the system's bars, and a menu opened near the foot of a
    /// phone grew straight down into the navigation bar, where the last row
    /// cannot be pressed because the system takes the touch. Seen on the
    /// device, on a message's long-press menu.
    const KEY: &'static str = "sigil_insets";

    pub fn install(ctx: &egui::Context, insets: Insets) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(Self::KEY), insets.clamped()));
    }

    /// What the system draws over, as the shell last said. Nothing, when
    /// nobody said -- a desktop, or a test.
    pub fn of(ctx: &egui::Context) -> Insets {
        ctx.data(|d| d.get_temp(egui::Id::new(Self::KEY)))
            .unwrap_or(Insets::NONE)
    }

    /// The screen less what the system draws over: where a menu may go.
    pub fn safe_rect(ctx: &egui::Context) -> egui::Rect {
        let insets = Insets::of(ctx);
        let screen = ctx.content_rect();
        egui::Rect::from_min_max(
            screen.min + egui::vec2(insets.left, insets.top),
            screen.max - egui::vec2(insets.right, insets.bottom),
        )
    }

    pub const NONE: Insets = Insets {
        top: 0.0,
        bottom: 0.0,
        left: 0.0,
        right: 0.0,
    };

    /// Only the top, which is what a desktop has.
    pub fn top(points: f32) -> Insets {
        Insets {
            top: points.max(0.0),
            ..Insets::NONE
        }
    }

    /// Never negative: a system that reports a negative inset is a bug, and
    /// a panel with a negative size is a panic.
    pub fn clamped(self) -> Insets {
        Insets {
            top: self.top.max(0.0),
            bottom: self.bottom.max(0.0),
            left: self.left.max(0.0),
            right: self.right.max(0.0),
        }
    }
}

/// How large the reader asked for text to be, as a multiple of sigil's own
/// sizes.
///
/// **A phone has a text-size setting and sigil read none of it.** Android's
/// `Configuration.fontScale` is 0.85, 1.0, 1.15 or 1.3 from its own slider
/// and reaches 2.0 from the accessibility one; nothing in winit or eframe
/// reports it, so the phone drew one size of type whatever the system had
/// been set to. The host says what it is, as it says the [`Form`], and
/// [`crate::theme::install`] is what applies it.
///
/// **Text, not everything.** That setting scales type; Display size scales
/// the whole interface and arrives as a density change, which egui already
/// has as `zoom_factor`. Scaling controls and spacing here would be the
/// second setting done under the first one's name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextScale(f32);

impl Default for TextScale {
    fn default() -> Self {
        TextScale(1.0)
    }
}

impl TextScale {
    const KEY: &'static str = "sigil_text_scale";

    /// The band sigil honours. **The top is what the layout is held to**, by
    /// `no_widget_runs_off_a_phone_when_the_text_is_turned_up`, which is also
    /// what WCAG asks a layout to survive -- an OEM slider that goes further
    /// would be honoured past the last size anybody measured, and the rows it
    /// broke would be off the side of the screen where nobody can see that
    /// they broke. The floor is Android's own smallest.
    pub const SMALLEST: f32 = 0.85;
    pub const LARGEST: f32 = 2.0;

    /// Say how large text should be. Before the theme is installed, which is
    /// what applies it; installing the theme again is how a change lands.
    pub fn install(ctx: &egui::Context, scale: f32) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(Self::KEY), TextScale::new(scale)));
    }

    /// Sigil's own sizes, when nobody said -- a desktop, or a test.
    pub fn of(ctx: &egui::Context) -> TextScale {
        ctx.data(|d| d.get_temp(egui::Id::new(Self::KEY)))
            .unwrap_or_default()
    }

    /// **Clamped, and a number that is not one is refused.** This comes from
    /// the platform, and nought or a NaN multiplied into every font size is
    /// an interface with no text in it at all -- a blank screen rather than a
    /// wrong one, and nothing on it to say why.
    pub fn new(scale: f32) -> TextScale {
        if scale.is_finite() {
            TextScale(scale.clamp(Self::SMALLEST, Self::LARGEST))
        } else {
            TextScale::default()
        }
    }

    pub fn factor(self) -> f32 {
        self.0
    }

    pub fn is_one(self) -> bool {
        self.0 == 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_nobody_told_draws_sigils_own_text_size() {
        let ctx = egui::Context::default();
        assert!(TextScale::of(&ctx).is_one());
        TextScale::install(&ctx, 1.3);
        assert_eq!(TextScale::of(&ctx).factor(), 1.3);
    }

    #[test]
    fn a_text_scale_the_platform_could_send_never_empties_the_screen() {
        // Nought would multiply every font size to nothing, and a NaN would
        // take the layout with it. Both are what a platform is free to send.
        assert!(TextScale::new(0.0).factor() >= TextScale::SMALLEST);
        assert!(TextScale::new(f32::NAN).is_one());
        assert!(TextScale::new(f32::INFINITY).is_one());
        // Past the top of the band the layout was measured at.
        assert_eq!(TextScale::new(3.5).factor(), TextScale::LARGEST);
        assert_eq!(TextScale::new(0.2).factor(), TextScale::SMALLEST);
    }

    #[test]
    fn a_context_nobody_told_is_a_desktop() {
        let ctx = egui::Context::default();
        assert_eq!(Form::of(&ctx), Form::Desktop);
        Form::install(&ctx, Form::Phone);
        assert_eq!(Form::of(&ctx), Form::Phone);
        assert!(Form::Phone.button_size() > Form::Desktop.button_size());
        assert!(Form::Phone.rail_width() > Form::Desktop.rail_width());
    }

    #[test]
    fn insets_never_go_negative() {
        let i = Insets {
            top: -3.0,
            bottom: 5.0,
            left: 0.0,
            right: -1.0,
        }
        .clamped();
        assert_eq!(
            i,
            Insets {
                top: 0.0,
                bottom: 5.0,
                left: 0.0,
                right: 0.0
            }
        );
        assert_eq!(Insets::top(-2.0), Insets::NONE);
    }
}
