//! The sigil host: the contract apps are written against, and the pieces every
//! app shares.
//!
//! - [`app`] — the [`App`](app::App) trait and the per-pass [`AppContext`](app::AppContext).
//! - [`nav`] — a navigation stack with no business logic in it.
//! - [`navigator`] — the shell's global history and the queue apps ask through.
//! - [`theme`] — colour, in three layers.
//! - [`tokens`] — the dimensions everything is built from.
//!
//! Nothing here knows about calls or messages. `sigil-voice` and `sigil-chat`
//! do, and the shell knows about neither.

pub mod app;
pub mod deck;
pub mod nav;
pub mod navigator;
pub mod theme;
pub mod tokens;

pub use app::{App, AppAction, AppContext, AppResponse, TabNotifications};
pub use deck::{Layout, layout};
pub use nav::{Discarded, NavStack};
pub use navigator::{ActiveEntry, AppId, NavEntry, NavRequest, Navigator};
pub use theme::ColorTheme;
