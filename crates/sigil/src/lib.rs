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
pub mod deeplink;
/// What the product is called, where a bar has to say so.
pub const NAME: &str = "Sigil";

pub mod form;
pub mod icon;
pub mod nav;
pub mod navigator;
pub mod prefs;
pub mod quiet;
pub mod theme;
pub mod tokens;
pub mod wake;

pub use account::{Account, Unlocked};
pub use accounts::Accounts;
pub use app::{
    App, AppAction, AppContext, AppResponse, Attention, Notice, Notify, Silent, Sound,
    TabNotifications, Target,
};
pub use deck::{Layout, layout};
pub use deeplink::Link;
pub use form::{Form, Insets};
pub use icon::{Icon, icon_button, icon_button_named, icon_button_tinted};
pub use nav::{Discarded, NavStack};
pub use navigator::{ActiveEntry, AppId, NavEntry, NavRequest, Navigator};
pub use prefs::Prefs;
pub use quiet::Quiet;
pub use theme::ColorTheme;
