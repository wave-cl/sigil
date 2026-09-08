//! The sigil host: the contract apps are written against, and the pieces every
//! app shares.
//!
//! - [`app`] — the [`App`](app::App) trait and the per-pass [`AppContext`](app::AppContext).
//! - [`nav`] — a navigation stack with no business logic in it.
//! - [`navigator`] — the shell's global history and the queue apps ask through.
//! - [`icon`] — the icon vocabulary, painted; see its note for why it is here.
//! - [`theme`] — colour, in three layers.
//! - [`tokens`] — the dimensions everything is built from.
//!
//! Nothing here knows about calls or messages. `sigil-voice` and `sigil-chat`
//! do, and the shell knows about neither.

pub mod account;
pub mod accounts;
pub mod app;
pub mod deck;
pub mod icon;
pub mod nav;
pub mod navigator;
pub mod theme;
pub mod tokens;

pub use account::{Account, Unlocked};
pub use accounts::Accounts;
pub use app::{App, AppAction, AppContext, AppResponse, Notify, Silent, TabNotifications};
pub use deck::{Layout, layout};
pub use icon::{Icon, icon_button, icon_button_named, icon_button_tinted};
pub use nav::{Discarded, NavStack};
pub use navigator::{ActiveEntry, AppId, NavEntry, NavRequest, Navigator};
pub use theme::ColorTheme;
