//! Design tokens: the dimensions sigil is built from.
//!
//! Deliberately dimensionless and theme-independent — a token says *how much*,
//! never *what colour*. Colour lives in [`crate::theme`]. Everything that would
//! otherwise be a magic number in a `ui` function belongs here, so that
//! changing the rhythm of the whole application is one edit rather than three
//! hundred.

/// Spacing, on a 4px base.
pub const SPACING_XS: f32 = 4.0;
pub const SPACING_SM: f32 = 8.0;
pub const SPACING_MD: f32 = 12.0;
pub const SPACING_LG: f32 = 16.0;
pub const SPACING_XL: f32 = 24.0;
pub const SPACING_XXL: f32 = 32.0;

/// Corner radii. `PILL` is deliberately larger than any control it is used on,
/// since a corner radius is clamped to half the shorter side.
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_PILL: f32 = 18.0;

pub const STROKE_THIN: f32 = 1.0;
pub const STROKE_MEDIUM: f32 = 1.5;
pub const STROKE_THICK: f32 = 2.0;

pub const ICON_SM: f32 = 16.0;
pub const ICON_MD: f32 = 24.0;
pub const ICON_LG: f32 = 32.0;

/// Avatar and identicon sizes. `XL` is the profile view; `SM` is a roster row.
pub const AVATAR_SM: f32 = 24.0;
pub const AVATAR_MD: f32 = 38.0;
pub const AVATAR_LG: f32 = 48.0;
pub const AVATAR_XL: f32 = 80.0;

pub const BUTTON_SM: f32 = 28.0;
pub const BUTTON_MD: f32 = 34.0;
pub const BUTTON_LG: f32 = 44.0;

pub const OPACITY_DISABLED: f32 = 0.38;
pub const OPACITY_MUTED: f32 = 0.60;
pub const OPACITY_OVERLAY: f32 = 0.50;

/// egui animation time, in seconds.
pub const ANIM_SPEED: f32 = 0.05;

/// Below this width the deck stops sharing the remainder between columns and
/// gives each a fixed width inside a horizontal scroll instead.
pub const COLUMN_MIN_WIDTH: f32 = 320.0;

/// Slack subtracted per column when dividing the window, so columns do not sit
/// flush against each other or the window edge.
pub const COLUMN_GUTTER: f32 = 30.0;

/// Below this window width the deck collapses to a single column and the
/// typography drops to the compact scale. A *runtime* check, not a platform
/// one: narrowing a desktop window must collapse the layout live.
pub const NARROW_WIDTH: f32 = 550.0;
