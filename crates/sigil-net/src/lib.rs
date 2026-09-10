//! The seam between the sqex protocol and something with a window.
//!
//! `sqex_voice::engine` holds a call; this runs one on a task and turns what it
//! reports into something an interface can draw without ever blocking on the
//! network.

pub mod call;
pub mod discovery;
pub mod held;

pub use call::{CallHandle, CallState, Dial, Phase, spawn_call, spawn_room};
pub use held::{Connections, Held};
pub use sqex_voice::engine::{CallOpts, Endpoint, Event, PeerStatus};

/// A room is named by a secret, and holding it is what membership consists of.
pub use sqex_proto::room::RoomId;

/// Re-exported so an interface can build a layer without depending on
/// `sqex-discovery` directly.
pub use sqex_discovery::Layer;

/// Which exchange a set of layers names, without dialling anything.
///
/// The domain, when the layers name one; `None` when they name an address,
/// because an address is not a domain and a SIP-38 handle is `name@domain`.
/// Pure — the DNS work happens later, in `resolve`.
pub fn domain_of(layers: &[Layer]) -> Option<String> {
    match sqex_discovery::target::resolve(layers) {
        Ok(sqex_discovery::Target::Discover(domain)) => Some(domain),
        _ => None,
    }
}

#[cfg(test)]
mod domain_tests {
    use super::*;

    /// A domain is a domain and an address is not.
    ///
    /// This is what a SIP-38 handle needs: `name@domain`. Composing one from
    /// an address gives `name@203.0.113.1`, which is not a handle and resolves
    /// nowhere — so the answer for an address is nothing, not a best effort.
    #[test]
    fn the_domain_is_taken_only_from_a_layer_that_names_one() {
        assert_eq!(
            domain_of(&[Layer {
                server: Some("squic.org".into()),
                ..Default::default()
            }]),
            Some("squic.org".to_string())
        );
        // A host with a key is an address to dial, not a domain to discover.
        assert_eq!(
            domain_of(&[Layer {
                host: Some("127.0.0.1:443".into()),
                key: Some("abc".into()),
                ..Default::default()
            }]),
            None
        );
        assert_eq!(domain_of(&[Layer::default()]), None);
        assert_eq!(domain_of(&[]), None);
    }

    /// The layers are in precedence order, and the first that names anything
    /// wins — the same rule the connection itself is made under, or the handle
    /// would be composed from a different exchange than the one being talked
    /// to.
    #[test]
    fn the_first_layer_that_names_something_decides() {
        let layers = [
            Layer::default(),
            Layer {
                server: Some("first.example".into()),
                ..Default::default()
            },
            Layer {
                server: Some("second.example".into()),
                ..Default::default()
            },
        ];
        assert_eq!(domain_of(&layers), Some("first.example".to_string()));
    }
}
