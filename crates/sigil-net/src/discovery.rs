//! Where to find the exchange.
//!
//! The same layers every sqex client speaks through, in the same order: what
//! the caller was told explicitly, then the environment, then `~/.sqnr/config`,
//! and last the domain of the identity's primary SIP-38 handle. Resolution
//! itself lives in `sqex_discovery::target`, because — as the CLI's own comment
//! puts it — three copies of it is what produced two bugs in a day. This only
//! assembles the layers.
//!
//! # The handle layer
//!
//! A claimed name *is* the default exchange, so `~/.sqnr/config` no longer
//! needs a `server =` pointer and the CLI's stopped writing one. sigil read
//! only the first three layers, so against a config written since then it
//! decided no exchange was configured at all: the ring listener never started
//! and placing a call refused. It worked only if you set `SQEX_SERVER`
//! yourself, which is not something anyone should have to know.
//!
//! The handle comes from the cleartext `<identity>.handles` sidecar, so reading
//! it needs no passphrase — which matters, because the exchange has to be known
//! before there is any way to ask one anything.

use std::path::Path;

use sqnr::config::Config;

/// The layers a caller can speak through, most specific first.
///
/// `explicit` is whatever the interface was given directly — a server typed
/// into a settings field. Pass an empty layer for "nothing said here".
///
/// `identity` is the identity file whose handle sidecar to read. `None` skips
/// that layer, which is what a caller with no identity open should pass.
pub fn layers(
    explicit: sqex_discovery::Layer,
    cfg: &Config,
    identity: Option<&Path>,
) -> Vec<sqex_discovery::Layer> {
    let mut layers = vec![
        explicit,
        sqex_discovery::Layer {
            server: env_nonempty("SQEX_SERVER"),
            host: env_nonempty("SQEX_SERVER_HOST"),
            key: env_nonempty("SQEX_SERVER_KEY"),
        },
        // The config is `sqnr`'s type and has no `server_host`, so the pairing
        // rule is read off the two fields it does have: a server *with* a key
        // is an address to dial, a server without one is a domain to discover.
        match (&cfg.server, &cfg.server_key) {
            (Some(s), Some(k)) => sqex_discovery::Layer {
                host: Some(s.clone()),
                key: Some(k.clone()),
                ..Default::default()
            },
            (Some(s), None) => sqex_discovery::Layer {
                server: Some(s.clone()),
                ..Default::default()
            },
            _ => sqex_discovery::Layer::default(),
        },
    ];
    // Lowest priority, so anything said explicitly still wins.
    if let Some(path) = identity
        && let Some(domain) = sqex_proto::handles::primary_domain(path)
    {
        layers.push(sqex_discovery::Layer {
            server: Some(domain),
            ..Default::default()
        });
    }
    layers
}

/// An empty layer, for an interface that has nothing of its own to add.
pub fn nothing_explicit() -> sqex_discovery::Layer {
    sqex_discovery::Layer::default()
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Whether any layer names an exchange at all.
///
/// Worth asking before offering to place a call: "no exchange configured" is a
/// different thing to say than letting the dial fail with whatever resolution
/// happens to complain about.
pub fn any_configured(layers: &[sqex_discovery::Layer]) -> bool {
    layers
        .iter()
        .any(|l| l.server.is_some() || l.host.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_config_and_no_environment_names_nothing() {
        let cfg = Config::default();
        let l = layers(nothing_explicit(), &cfg, None);
        // The environment of the test process may legitimately have SQEX_SERVER
        // set; only assert about the layers we control.
        assert!(l[0].server.is_none() && l[0].host.is_none());
        assert!(l[2].server.is_none() && l[2].host.is_none());
    }

    #[test]
    fn a_config_server_without_a_key_is_a_domain_to_discover() {
        let cfg = Config {
            server: Some("ex.squic.org".into()),
            ..Config::default()
        };
        let l = layers(nothing_explicit(), &cfg, None);
        assert_eq!(l[2].server.as_deref(), Some("ex.squic.org"));
        assert!(l[2].host.is_none(), "a domain is discovered, not dialled");
        assert!(any_configured(&l));
    }

    #[test]
    fn a_config_server_with_a_key_is_an_address_to_dial() {
        let cfg = Config {
            server: Some("95.216.183.51:443".into()),
            server_key: Some("abc".into()),
            ..Config::default()
        };
        let l = layers(nothing_explicit(), &cfg, None);
        assert_eq!(l[2].host.as_deref(), Some("95.216.183.51:443"));
        assert_eq!(l[2].key.as_deref(), Some("abc"));
        assert!(
            l[2].server.is_none(),
            "an address is dialled, not discovered"
        );
    }

    /// The gap this closes. `~/.sqnr/config` stopped carrying a `server =`
    /// pointer when a claimed name became the default exchange, and sigil read
    /// only the first three layers -- so against a config written since then it
    /// decided nothing was configured, never started the ring listener, and
    /// refused to place a call. It worked only if you knew to set SQEX_SERVER.
    #[test]
    fn the_identity_handle_names_the_exchange_when_nothing_else_does() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity");
        std::fs::write(&identity, "irrelevant: only the sidecar is read").unwrap();
        std::fs::write(
            handles_path(&identity),
            "# a comment, which the sidecar allows\ncolin@squic.org\nother@elsewhere.org\n",
        )
        .unwrap();

        let cfg = Config::default();
        let l = layers(nothing_explicit(), &cfg, Some(&identity));
        assert!(
            any_configured(&l),
            "a claimed handle is enough on its own: {l:?}"
        );
        let last = l.last().expect("a handle layer was added");
        assert_eq!(
            last.server.as_deref(),
            Some("squic.org"),
            "the *primary* handle's domain, and only the domain"
        );
    }

    /// It is the lowest priority, so anything said explicitly still wins.
    #[test]
    fn a_handle_never_outranks_what_was_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity");
        std::fs::write(&identity, "x").unwrap();
        std::fs::write(handles_path(&identity), "colin@squic.org\n").unwrap();

        let explicit = sqex_discovery::Layer {
            server: Some("typed-in".into()),
            ..Default::default()
        };
        let l = layers(explicit, &Config::default(), Some(&identity));
        assert_eq!(l[0].server.as_deref(), Some("typed-in"));
        assert_eq!(l.last().unwrap().server.as_deref(), Some("squic.org"));
    }

    /// No identity, no handle layer -- and nothing else configured means
    /// nothing configured, which is what the interface must be able to say.
    #[test]
    fn without_an_identity_there_is_no_handle_layer() {
        let l = layers(nothing_explicit(), &Config::default(), None);
        assert_eq!(l.len(), 3, "the three original layers and no more");
    }

    /// An identity with no sidecar contributes nothing rather than failing: a
    /// missing hint is not an error.
    #[test]
    fn an_identity_without_a_handle_contributes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity");
        std::fs::write(&identity, "x").unwrap();
        let l = layers(nothing_explicit(), &Config::default(), Some(&identity));
        assert_eq!(l.len(), 3);
    }

    fn handles_path(identity: &std::path::Path) -> std::path::PathBuf {
        sqex_proto::handles::path_for(identity)
    }

    #[test]
    fn what_the_interface_says_outranks_the_config() {
        let cfg = Config {
            server: Some("from-config".into()),
            ..Config::default()
        };
        let explicit = sqex_discovery::Layer {
            server: Some("typed-in".into()),
            ..Default::default()
        };
        let l = layers(explicit, &cfg, None);
        assert_eq!(l[0].server.as_deref(), Some("typed-in"));
        assert_eq!(l[2].server.as_deref(), Some("from-config"));
    }
}
