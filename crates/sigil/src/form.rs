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

#[cfg(test)]
mod tests {
    use super::*;

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
