//! Where to find the exchange.
//!
//! The same three layers every sqex client speaks through, in the same order:
//! what the caller was told explicitly, then the environment, then
//! `~/.sqnr/config`. Resolution itself lives in `sqex_discovery::target`,
//! because — as the CLI's own comment puts it — three copies of it is what
//! produced two bugs in a day. This only assembles the layers.

use sqnr::config::Config;

/// The layers a caller can speak through, most specific first.
///
/// `explicit` is whatever the interface was given directly — a server typed
/// into a settings field. Pass an empty layer for "nothing said here".
pub fn layers(explicit: sqex_discovery::Layer, cfg: &Config) -> [sqex_discovery::Layer; 3] {
    [
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
    ]
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
pub fn any_configured(layers: &[sqex_discovery::Layer; 3]) -> bool {
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
        let l = layers(nothing_explicit(), &cfg);
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
        let l = layers(nothing_explicit(), &cfg);
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
        let l = layers(nothing_explicit(), &cfg);
        assert_eq!(l[2].host.as_deref(), Some("95.216.183.51:443"));
        assert_eq!(l[2].key.as_deref(), Some("abc"));
        assert!(
            l[2].server.is_none(),
            "an address is dialled, not discovered"
        );
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
        let l = layers(explicit, &cfg);
        assert_eq!(l[0].server.as_deref(), Some("typed-in"));
        assert_eq!(l[2].server.as_deref(), Some("from-config"));
    }
}
