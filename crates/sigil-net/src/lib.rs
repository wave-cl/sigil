//! The seam between the sqex protocol and something with a window.
//!
//! `sqex_voice::engine` holds a call; this runs one on a task and turns what it
//! reports into something an interface can draw without ever blocking on the
//! network.

pub mod call;
pub mod discovery;
pub mod held;

pub use call::{
    CallHandle, CallState, Dial, Path, Phase, decline_cross, spawn_call, spawn_cross_answer,
    spawn_cross_call, spawn_dm_call, spawn_room,
};
pub use held::{Connections, Held};
pub use sqex_voice::engine::{CallOpts, Endpoint, Event, PeerStatus};
// **The types `CallOpts`' own fields are.** It was re-exported and they were
// not, so a caller outside this workspace could hold a `CallOpts` and had no
// way to name what to put in it -- which is the whole of the API for a call
// that reads a tone instead of a microphone and writes nowhere instead of to
// a speaker, the only shape a test peer can take.
pub use sqex_voice::audio::{Sink, Source};

/// A room is named by a secret, and holding it is what membership consists of.
pub use sqex_proto::room::RoomId;

/// Re-exported so an interface can build a layer without depending on
/// `sqex-discovery` directly.
pub use sqex_discovery::Layer;

/// The key pinned for a domain (SIP-33), without the network: what a client
/// may draw its disc under before the exchange has answered. `None` for a
/// first contact.
pub fn pinned_key_of(domain: &str) -> Option<sqnr_core::PubKey> {
    sqex_discovery::Known::load(&sqex_discovery::known::path())
        .ok()?
        .lookup(domain)
}

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

/// SIP-85: the tunnel a chat session holds at its home, re-exported so the
/// session can open one without depending on `sqex-proto` for it.
pub use sqex_proto::tunnel::Carrier;

/// SIP-85: ask `home` to carry a connection to the exchange at
/// `target_domain`, whose key this identity already holds as `target`.
///
/// One dial to the home under the `sqex-tunnel` ALPN, one `Open`, and a
/// loopback socket the caller then dials with the target's key pinned. A home
/// that does not carry connections fails the handshake, and the error says so.
pub async fn carry(
    home: Endpoint,
    seed: &[u8; 32],
    target: &sqnr_core::PubKey,
    target_domain: &str,
) -> Result<Carrier, String> {
    Carrier::open(
        home.address,
        home.server.as_bytes(),
        seed,
        target.as_bytes(),
        target_domain,
    )
    .await
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
