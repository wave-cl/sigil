//! How the columns of the deck share the window.
//!
//! Kept as a pure function of two numbers so the rule can be tested without a
//! window, a GPU, or a frame. The layout question — "do these columns fit?" —
//! is arithmetic, and only the answer needs egui.

use crate::tokens::{COLUMN_GUTTER, COLUMN_MIN_WIDTH, NARROW_WIDTH};

/// How to lay the deck out for a given window width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// The window is too narrow for a deck at all: show one column, and let
    /// navigation rather than space move between views.
    Single,
    /// Every column gets an equal share of the window.
    Shared { column_width: f32 },
    /// The columns will not fit at a usable width, so each takes a fixed one
    /// and the strip scrolls horizontally. Better a column you can read and
    /// must scroll to than five you cannot.
    Scrolling { column_width: f32 },
}

/// Decide the layout for `columns` columns in a window `available` wide.
///
/// This is a **runtime** decision about width, not a compile-time one about
/// platform: narrowing a desktop window collapses the deck live, and widening
/// it brings the columns back.
pub fn layout(available: f32, columns: usize) -> Layout {
    if columns <= 1 || available < NARROW_WIDTH {
        return Layout::Single;
    }
    let share = available / columns as f32 - COLUMN_GUTTER;
    if share < COLUMN_MIN_WIDTH {
        Layout::Scrolling {
            column_width: COLUMN_MIN_WIDTH,
        }
    } else {
        Layout::Shared {
            column_width: share,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_window_shows_one_column_however_many_are_open() {
        assert_eq!(layout(400.0, 3), Layout::Single);
        assert_eq!(layout(NARROW_WIDTH - 1.0, 5), Layout::Single);
    }

    #[test]
    fn one_column_is_always_single_however_wide_the_window() {
        assert_eq!(layout(3000.0, 1), Layout::Single);
        assert_eq!(layout(3000.0, 0), Layout::Single);
    }

    #[test]
    fn columns_share_a_wide_window() {
        let Layout::Shared { column_width } = layout(1600.0, 2) else {
            panic!("two columns fit easily in 1600px");
        };
        assert_eq!(column_width, 1600.0 / 2.0 - COLUMN_GUTTER);
    }

    #[test]
    fn columns_stop_sharing_once_a_share_would_be_unreadable() {
        // Six columns in 1200px is 170px each -- narrower than a roster row.
        let l = layout(1200.0, 6);
        assert_eq!(
            l,
            Layout::Scrolling {
                column_width: COLUMN_MIN_WIDTH
            }
        );
    }

    /// The boundary is the interesting part: one column either side of it.
    #[test]
    fn the_switch_to_scrolling_happens_exactly_at_the_minimum_width() {
        let n = 3usize;
        // Width at which each share is exactly COLUMN_MIN_WIDTH.
        let exact = (COLUMN_MIN_WIDTH + COLUMN_GUTTER) * n as f32;
        assert_eq!(
            layout(exact, n),
            Layout::Shared {
                column_width: COLUMN_MIN_WIDTH
            }
        );
        assert_eq!(
            layout(exact - 1.0, n),
            Layout::Scrolling {
                column_width: COLUMN_MIN_WIDTH
            }
        );
    }

    /// A scrolling deck is wider than the window on purpose; the caller needs
    /// to know how much to give the scroll area.
    #[test]
    fn a_scrolling_deck_is_wider_than_its_window() {
        let columns = 6;
        let Layout::Scrolling { column_width } = layout(1200.0, columns) else {
            panic!("six columns do not fit in 1200px");
        };
        assert!(column_width * columns as f32 > 1200.0);
    }
}
