//! The seam between the sqex protocol and something with a window.
//!
//! `sqex_voice::engine` holds a call; this runs one on a task and turns what it
//! reports into something an interface can draw without ever blocking on the
//! network.

pub mod call;

pub use call::{CallHandle, CallState, Phase, spawn_call};
pub use sqex_voice::engine::{CallOpts, Endpoint, Event};
