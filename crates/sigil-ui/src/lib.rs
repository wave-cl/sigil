//! Widgets shared between sigil's apps.
//!
//! Deliberately knows nothing of the protocol: everything here takes plain
//! data, so this does not become a second place the wire format is understood.

pub mod dot;
pub mod roster;

pub use dot::dot;
pub use roster::{Row, roster};
