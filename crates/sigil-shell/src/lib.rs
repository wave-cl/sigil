//! The sigil shell: the app roster, the global history, and the chrome around
//! them.
//!
//! A library as well as a binary so the shell can be rendered headlessly by the
//! tests in `tests/`, which is the only way to see what it looks like without a
//! screen.

mod platform_app;
mod shell;

pub use platform_app::PlatformApp;
pub use shell::Shell;
