//! The disc first: a session whose exchange is known by key draws the
//! conversations and the lines this machine holds before -- and whether or
//! not -- the exchange answers. The window that showed nothing until DNS,
//! a tunnel and a handshake had all finished (2026-09-22) is the control.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};

async fn exchange_in(dir: &Path) -> (SocketAddr, [u8; 32]) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\ndomain = \"e.test\"\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
    );
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind_with(
        config,
        None,
        signing_key,
        sqexd::relay::Find::Fixed(Default::default()),
    )
    .await
    .unwrap();
    let addr = bound.local_addr;
    let server_pub = bound.public_key.to_bytes();
    tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });
    (addr, server_pub)
}

fn signer(b: u8) -> (SoftwareSigner, PubKey) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    (SoftwareSigner::new(sk), public)
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

fn start_at(endpoint: Endpoint, signer: SoftwareSigner, store: &Path) -> ChatHandle {
    session::start(endpoint, signer, Some(store.to_path_buf()), || {})
}

async fn up(chat: &ChatHandle, me: PubKey) {
    assert!(
        until(
            || chat.state().me == Some(me) && chat.state().link == LinkState::Up,
            15
        )
        .await,
        "the session should come up: {:?}",
        chat.state().trouble
    );
}

#[tokio::test]
async fn what_the_disc_holds_is_drawn_before_the_exchange_answers() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub) = exchange_in(dir.path()).await;
    let live = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, alice) = signer(0x81);
    let (b_signer, bob) = signer(0x82);
    let store = dir.path().join("alice.db");

    // A conversation with a line in it, on the disc.
    let a1 = start_at(live, a_signer, &store);
    let bobs = start_at(live, b_signer, &dir.path().join("bob.db"));
    up(&a1, alice).await;
    up(&bobs, bob).await;
    bobs.send(Cmd::OpenDm(alice));
    a1.send(Cmd::OpenDm(bob));
    assert!(
        until(
            || a1.state().open.is_some() && bobs.state().open.is_some(),
            15
        )
        .await
    );
    a1.send(Cmd::Send("kept on this machine".into()));
    assert!(
        until(
            || bobs
                .state()
                .lines
                .iter()
                .any(|l| l.text == "kept on this machine"),
            20
        )
        .await
    );
    let dm = a1.state().open.unwrap();
    a1.stop();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The same store, the same key, an address nobody answers at: the list
    // and the lines come from the disc while the link is still connecting.
    let dead = Endpoint {
        address: "127.0.0.1:1".parse().unwrap(),
        server: PubKey::new(server_pub),
    };
    let (a_signer, _) = signer(0x81);
    let a2 = start_at(dead, a_signer, &store);
    assert!(
        until(
            || a2.state().conversations.iter().any(|c| c.channel == dm),
            5
        )
        .await,
        "the conversation was not drawn from the disc: {:?} (link {:?}, trouble {:?})",
        a2.state().conversations,
        a2.state().link,
        a2.state().trouble
    );
    assert_ne!(
        a2.state().link,
        LinkState::Up,
        "the exchange at 127.0.0.1:1 answered?"
    );
    a2.send(Cmd::Show(dm));
    assert!(
        until(
            || a2
                .state()
                .lines
                .iter()
                .any(|l| l.text == "kept on this machine"),
            5
        )
        .await,
        "the line was not drawn from the disc: {:?}",
        a2.state().lines
    );
}
