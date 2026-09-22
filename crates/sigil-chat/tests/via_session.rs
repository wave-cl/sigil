//! SIP-85: a sigil session reached through the home. The session opens a
//! tunnel at A (the home), dials B (the target) through it, and B sees A's
//! address under this identity's own key. A home that does not carry
//! connections, or a home that is the target itself, is a session that
//! never comes up and says why.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, LinkState, session};
use sigil_net::{Dial, Endpoint};
use sqex_proto::home::{Move, Moving};
use sqexd::config::FileConfig;
use sqexd::server::Server;
use sqnr_core::{PubKey, SoftwareSigner};

struct Exchange {
    endpoint: Endpoint,
    server: Arc<Server>,
    _dir: tempfile::TempDir,
}

/// An exchange on loopback that finds `found` without DNS, carrying
/// connections for its members or not.
async fn exchange(tunnel: bool, domain: &str, found: &[(&str, PubKey, SocketAddr)]) -> Exchange {
    for _ in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let listen = {
            let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            s.local_addr().unwrap()
        };
        let key_path = dir.path().join("host_key");
        let (server_sk, _) = squic::generate_keypair();
        std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
        let config_toml = format!(
            "listen = {:?}\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
             welcome_channel = \"\"\ndomain = {domain:?}\nopen_peering = true\n\
             tunnel = {tunnel}\nhome_secs = 1\n",
            listen.to_string(),
            key_path.to_string_lossy(),
            dir.path().join("sqex.state").to_string_lossy(),
        );
        let file: FileConfig = toml::from_str(&config_toml).unwrap();
        let config = file.resolve().unwrap();
        let (signing_key, _pub) =
            squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
        let map = found
            .iter()
            .map(|(d, k, a)| ((*d).to_string(), (*k, *a)))
            .collect();
        let Ok(bound) =
            sqexd::bind_with(config, None, signing_key, sqexd::relay::Find::Fixed(map)).await
        else {
            continue;
        };
        let ex = Exchange {
            endpoint: Endpoint {
                address: bound.local_addr,
                server: bound.public_key,
            },
            server: Arc::clone(&bound.server),
            _dir: dir,
        };
        tokio::spawn(async move {
            let _ = sqexd::serve(bound).await;
        });
        return ex;
    }
    panic!("no free port in five tries");
}

fn signer(b: u8) -> (SoftwareSigner, PubKey, [u8; 32]) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    let seed = sk.to_bytes();
    (SoftwareSigner::new(sk), public, seed)
}

async fn until<F: FnMut() -> bool>(mut f: F, secs: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    while tokio::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Home A that carries, target B that A finds by name.
async fn pair() -> (Exchange, Exchange) {
    let b = exchange(false, "b.test", &[]).await;
    let a = exchange(
        true,
        "a.test",
        &[("b.test", b.endpoint.server, b.endpoint.address)],
    )
    .await;
    (a, b)
}

/// Make the identity a member of `a`: an account homed there by its Move.
async fn home_at(a: &Exchange, seed: &[u8; 32]) {
    let mut c = sqnr::Client::connect_as(a.endpoint.address, a.endpoint.server.as_bytes(), seed)
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (code, _) = c
        .post(
            "/account/move",
            Moving {
                mv: Move::sign(seed, &a.endpoint.server, now),
                domain: "a.test".into(),
                origins: vec![],
            }
            .encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
}

fn via(a: &Exchange, b: &Exchange, signer: SoftwareSigner, store: &Path) -> ChatHandle {
    session::start(
        Dial::Via {
            home: Box::new(Dial::At(a.endpoint)),
            target: Box::new(Dial::At(b.endpoint)),
            target_domain: "b.test".into(),
        },
        signer,
        Some(store.to_path_buf()),
        || {},
    )
}

#[tokio::test]
async fn a_session_through_the_home_reaches_the_target_as_itself() {
    let (a, b) = pair().await;
    let (signer, me, seed) = signer(0x85);
    home_at(&a, &seed).await;
    let dir = tempfile::tempdir().unwrap();

    let chat = via(&a, &b, signer, &dir.path().join("me.db"));
    assert!(
        until(
            || chat.state().me == Some(me) && chat.state().link == LinkState::Up,
            15
        )
        .await,
        "the session should come up through the home: {:?}",
        chat.state().trouble
    );
    let state = chat.state();
    assert_eq!(
        state.exchange,
        Some(b.endpoint.server),
        "the session is at B"
    );
    assert_eq!(state.domain.as_deref(), Some("b.test"));
    assert_eq!(
        state.carried.as_deref(),
        Some(a.endpoint.address.to_string().as_str()),
        "the interface is told which home carries it"
    );
    assert_eq!(a.server.tunnels_open(), 1, "A carries one tunnel");

    // **What B saw.** The connection came from A's tunnel socket, under this
    // identity's own key and identity -- never the home's.
    let seen = b.server.last_peer_addr().expect("B accepted the session");
    let a_sockets = a.server.tunnel_sockets();
    assert_eq!(a_sockets.len(), 1);
    assert_eq!(
        seen.port(),
        a_sockets[0].port(),
        "B saw {seen}, A bound {}",
        a_sockets[0]
    );
    assert_eq!(b.server.last_peer_identity(), Some(me));
    let x_of = |k: &[u8; 32]| {
        squic::crypto::ed25519_public_to_x25519(k)
            .unwrap()
            .to_bytes()
    };
    assert_eq!(b.server.last_peer_key(), Some(x_of(me.as_bytes())));
    assert_ne!(
        b.server.last_peer_key(),
        Some(x_of(a.endpoint.server.as_bytes()))
    );
    // And nothing reached B any other way.
    let status: serde_json::Value = {
        let mut c =
            sqnr::Client::connect_as(b.endpoint.address, b.endpoint.server.as_bytes(), &seed)
                .await
                .unwrap();
        let (_, body) = c.get("/status").await.unwrap();
        serde_json::from_slice(&body).unwrap()
    };
    assert_eq!(
        status["connections"], 2,
        "the session's connection and this status probe"
    );
}

/// A home with carriage off refuses the ALPN at the handshake; the session
/// says so rather than connecting some other way.
#[tokio::test]
async fn a_home_that_does_not_carry_is_a_session_that_says_so() {
    let b = exchange(false, "b.test", &[]).await;
    let a = exchange(
        false,
        "a.test",
        &[("b.test", b.endpoint.server, b.endpoint.address)],
    )
    .await;
    let (signer, _me, seed) = signer(0x86);
    home_at(&a, &seed).await;
    let dir = tempfile::tempdir().unwrap();
    let chat = via(&a, &b, signer, &dir.path().join("me.db"));
    assert!(
        until(
            || chat
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("does not carry connections")),
            15
        )
        .await,
        "expected the home's refusal, got {:?}",
        chat.state().trouble
    );
    // `exchange` names the store's exchange from the moment the disc is
    // drawn, before anything is dialled; that nothing was *reached* is the
    // link, and B's own account of who arrived.
    assert_ne!(chat.state().link, LinkState::Up, "the link came up");
    assert_eq!(
        b.server.last_peer_identity(),
        None,
        "B never saw this identity"
    );
}

/// The home is the target: nothing to carry, and a loop if it were.
#[tokio::test]
async fn the_home_cannot_be_reached_through_itself() {
    let (a, _b) = pair().await;
    let (signer, _me, seed) = signer(0x87);
    home_at(&a, &seed).await;
    let dir = tempfile::tempdir().unwrap();
    let chat = session::start(
        Dial::Via {
            home: Box::new(Dial::At(a.endpoint)),
            target: Box::new(Dial::At(a.endpoint)),
            target_domain: "a.test".into(),
        },
        signer,
        Some(dir.path().join("me.db")),
        || {},
    );
    assert!(
        until(
            || chat
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("is your home")),
            15
        )
        .await,
        "expected the refusal, got {:?}",
        chat.state().trouble
    );
    assert_eq!(a.server.tunnels_open(), 0);
}
