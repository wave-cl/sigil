//! SIP-45 from the session's side: the wake endpoint the platform offers is
//! left with the exchange, again on every connect, and taken back.
//!
//! **Nothing made the first registration.** The phone's wake window
//! re-registered the endpoint on every wake, and the running app never
//! registered it at all -- so a phone with a distributor installed held an
//! address no exchange had been told, and no wake could arrive to start the
//! window that would have told it. `wake::forget` existed with no caller.
//!
//! Proven the way it matters: the exchange, with the session gone, posts a
//! wake to the endpoint it was left -- a loopback distributor stub here --
//! and after a forget, posts none.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// An exchange that will post wakes to loopback http, which is what a
/// distributor stub in a test is.
async fn server_in(dir: &Path) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nlimits = {{ posts = [0, 0], signals = [0, 0], joins = [0, 0], creates = [0, 0], uploads = [0, 0] }}\nwake_loopback = true\n",
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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    f()
}

fn start_at(endpoint: Endpoint, signer: SoftwareSigner, store: &Path) -> ChatHandle {
    session::start(
        sigil_net::Dial::At(endpoint),
        signer,
        Some(store.to_path_buf()),
        || {},
    )
}

/// A push distributor, as far as an exchange can tell: a POST on loopback,
/// counted.
struct Distributor {
    url: String,
    posts: Arc<Mutex<usize>>,
}

async fn distributor() -> Distributor {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let posts: Arc<Mutex<usize>> = Arc::default();
    let seen = Arc::clone(&posts);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                if matches!(sock.read(&mut buf).await, Ok(n) if n > 0) {
                    *seen.lock().unwrap() += 1;
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                }
            });
        }
    });
    Distributor {
        url: format!("http://127.0.0.1:{port}/up/phone"),
        posts,
    }
}

#[tokio::test]
async fn the_endpoint_is_left_with_the_exchange_which_wakes_it_and_taken_back() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (alice_signer, alice) = signer(0xb0);
    let (bob_signer, bob) = signer(0xb1);
    let d = distributor().await;

    // Bob, a phone: his session is told where he may be woken.
    let bobs = start_at(endpoint, bob_signer, &dir.path().join("bob.db"));
    assert!(until(|| bobs.state().link == LinkState::Up, 15).await);
    bobs.send(Cmd::WakeEndpoint(Some((d.url.clone(), 3600))));
    assert!(
        until(|| bobs.state().wake.as_deref() == Some("registered"), 15).await,
        "the endpoint was not registered: {:?} / {:?}",
        bobs.state().wake,
        bobs.state().trouble
    );

    // Alice writes to him.
    let alices = start_at(endpoint, alice_signer, &dir.path().join("alice.db"));
    assert!(until(|| alices.state().link == LinkState::Up, 15).await);
    alices.send(Cmd::OpenDm(bob));
    assert!(until(|| alices.state().open.is_some(), 15).await);
    assert!(
        until(
            || bobs
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(alice)),
            20
        )
        .await,
        "Bob never saw the conversation"
    );
    // Whether the exchange woke him for the conversation's creation is the
    // exchange's rule -- it wakes a device with no stream, and a session's
    // stream subscribes a moment after its link is up -- so the count
    // before he goes is the baseline, not zero.
    let before = *d.posts.lock().unwrap();

    // Bob's session goes -- the phone put the app away -- and a message
    // arrives. The exchange posts to the endpoint it was left.
    bobs.stop();
    tokio::time::sleep(Duration::from_millis(500)).await;
    alices.send(Cmd::Send("wake up".into()));
    assert!(
        until(|| *d.posts.lock().unwrap() > before, 20).await,
        "the exchange never woke the endpoint it was left"
    );

    // Back, and told again on this connection without being asked: the
    // registration is idempotent and a phone that forgot would go quiet at
    // the end of the ttl. Then the distributor goes away; the endpoint is
    // taken back, and a message wakes nothing.
    let bobs = start_at(endpoint, signer(0xb1).0, &dir.path().join("bob.db"));
    assert!(until(|| bobs.state().link == LinkState::Up, 15).await);
    bobs.send(Cmd::WakeEndpoint(None));
    assert!(
        until(|| bobs.state().wake.as_deref() == Some("forgotten"), 15).await,
        "the endpoint was not forgotten: {:?}",
        bobs.state().wake
    );
    bobs.stop();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = *d.posts.lock().unwrap();
    // A ring, not a message: the exchange wakes a device for a message at
    // most once per 30 s, and one was just sent -- a second message would
    // wake nothing whether or not the endpoint had been forgotten, and
    // this assertion passed with the forget cut out. A ring is urgent and
    // is posted at once, so only the forget can keep it from the stub.
    alices.send(Cmd::Call { direct: false });
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        *d.posts.lock().unwrap(),
        before,
        "woken after the endpoint was taken back"
    );
    alices.stop();
}

/// Told on every connect: a session started with the endpoint already known
/// registers it as it comes up, which is the app's case at launch and after
/// a redial.
#[tokio::test]
async fn the_endpoint_is_registered_when_the_link_comes_up() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (bob_signer, _) = signer(0xb3);
    let d = distributor().await;
    let bobs = start_at(endpoint, bob_signer, &dir.path().join("bob.db"));
    // Before the link is up -- as the app does for a session it starts with
    // the endpoint already in hand.
    bobs.send(Cmd::WakeEndpoint(Some((d.url.clone(), 3600))));
    assert!(
        until(|| bobs.state().wake.as_deref() == Some("registered"), 20).await,
        "not registered as the link came up: {:?}",
        bobs.state().wake
    );
    bobs.stop();
}
