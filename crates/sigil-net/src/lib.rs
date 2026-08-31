//! The seam between the sqex protocol and something with a window.
//!
//! `sqex_voice::engine` holds a call; this runs one on a task and turns what it
//! reports into something an interface can draw without ever blocking on the
//! network.

pub mod call;
pub mod discovery;
pub mod ring;

pub use call::{CallHandle, CallState, Dial, Phase, spawn_call, spawn_room};
pub use ring::{Incoming, ListenerState, RingListener, listen};
pub use sqex_voice::engine::{CallOpts, Endpoint, Event, PeerStatus};

/// A room is named by a secret, and holding it is what membership consists of.
pub use sqex_proto::room::RoomId;

/// Re-exported so an interface can build a layer without depending on
/// `sqex-discovery` directly.
pub use sqex_discovery::Layer;
