//! Two sigil chat sessions holding a conversation through a real exchange.
//!
//! Driven exactly as the interface drives them — send a command, read a
//! snapshot, never await the network — so what passes here is what the window
//! does.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};

async fn server_in(dir: &Path) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
    );
    let config_path = dir.join("sqexd.toml");
    std::fs::write(&config_path, &config_toml).unwrap();
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _pub) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind(config, Some(config_path), signing_key)
        .await
        .unwrap();
    let addr = bound.local_addr;
    let server_pub = bound.public_key.to_bytes();
    let handle = tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });
    (addr, server_pub, handle)
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

#[tokio::test]
async fn two_sessions_hold_a_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(1);
    let (b_signer, b_id) = signer(2);

    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));

    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up: {:?}",
        alice.state().trouble
    );

    // Bob has to be listening for the conversation to be sealed to him: a
    // direct message can be opened with somebody who has never run a client,
    // but nothing can be sealed to a device with no prekeys (SIP-23).
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open: {:?}",
        alice.state().trouble
    );

    alice.send(Cmd::Send("through a real exchange".into()));

    let arrived = until(
        || {
            bob.state()
                .lines
                .iter()
                .any(|l| l.text == "through a real exchange")
        },
        20,
    )
    .await;
    assert!(arrived, "Bob should receive it: {:?}", bob.state());

    // And it is marked as his correspondent's rather than his own, which is
    // what decides the side of the window it is drawn on.
    let line = bob
        .state()
        .lines
        .into_iter()
        .find(|l| l.text == "through a real exchange")
        .unwrap();
    assert_eq!(line.who, a_id);
    assert!(!line.mine, "a message from somebody else is not ours");

    // Alice sees her own, and knows it.
    let hers = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "through a real exchange" && l.mine)
        },
        20,
    )
    .await;
    assert!(hers, "the sender sees their own message as theirs");

    alice.stop();
    bob.stop();
}

/// The store is `flock`ed for the life of a session, because two interactive
/// clients would each keep their own idea of the next message counter and
/// reusing one costs the confidentiality of two messages.
#[tokio::test]
async fn a_second_session_on_one_account_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let store = dir.path().join("a.db");

    let (a_signer, a_id) = signer(1);
    let first = start_at(endpoint, a_signer, &store);
    assert!(
        until(|| first.state().me == Some(a_id), 15).await,
        "the first comes up"
    );

    // A second one on the same store must refuse rather than quietly corrupt
    // the counter.
    let (again, _) = signer(1);
    let second = start_at(endpoint, again, &store);
    let refused = until(|| second.state().trouble.is_some(), 10).await;
    assert!(refused, "a second session must be refused");
    let trouble = second.state().trouble.unwrap();
    assert!(
        trouble.contains("already using this account"),
        "and say why, in words somebody can act on: {trouble}"
    );

    first.stop();
    second.stop();
}

/// The link is a value the interface can draw, with a word for each state --
/// a colour on its own is not a message.
#[tokio::test]
async fn the_link_state_has_words_for_every_case() {
    assert_eq!(LinkState::Up.word(), "connected");
    assert_eq!(LinkState::Retrying.word(), "reconnecting…");
    assert_eq!(LinkState::Gone.word(), "offline");
}
