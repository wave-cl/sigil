//! The console on a connection the chat session already holds.
//!
//! # Why this is not obvious
//!
//! This session runs on a **thread and runtime of its own** — see
//! `session::start` — because `sqnr::flow::sign_and_submit` holds a `&dyn Fn`
//! across an await and the resulting future is not `Send`. The connection it
//! borrows was made on the application's runtime, by the chat session. So every
//! request the console makes is issued from one runtime on a connection driven
//! by another, which is exactly the kind of thing that works until it does not.
//!
//! It works, and the first test is here to say so out loud rather than leaving
//! it as an assumption underneath the second.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_admin::session::{self, Cmd};
use sigil_net::{Endpoint, Held};
use sqexd::config::FileConfig;
use sqnr::Client;
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

fn signer(b: u8) -> (SoftwareSigner, [u8; 32]) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    (SoftwareSigner::new(sk.clone()), sk.to_bytes())
}

/// The count, once it has caught up with `want`.
///
/// The exchange counts a connection when it accepts one, which is not the
/// instant the dialler's handshake returns. Every assertion here is about who
/// opened what, so each measurement waits for the accepts already made rather
/// than racing them.
async fn settled_at(client: &Client, want: u64) -> u64 {
    let mut counted = 0;
    for _ in 0..100 {
        counted = accepted(client).await;
        if counted >= want {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    counted
}

/// How many connections the exchange has accepted, asked over one that exists
/// so that asking does not open another.
async fn accepted(client: &Client) -> u64 {
    let (code, body) = client.requests().get("/status").await.unwrap();
    assert_eq!(code, 200);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["connections"].as_u64().unwrap()
}

fn until(f: impl Fn() -> bool, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// A connection made on one runtime answers requests made from another.
///
/// The console's whole arrangement rests on this: its thread has a
/// current-thread runtime, and what it borrows was dialled on the application's
/// multi-threaded one. The first assertion is the instrument checking itself —
/// if the home runtime could not use its own connection, the second would prove
/// nothing.
#[test]
fn a_connection_made_on_one_runtime_answers_on_another() {
    let dir = tempfile::tempdir().unwrap();
    let home = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let client = home.block_on(async {
        let (addr, server_pub, _h) = server_in(dir.path()).await;
        let client = Client::connect(addr, &server_pub).await.unwrap();
        let (code, _) = client.requests().get("/status").await.unwrap();
        assert_eq!(code, 200, "the connection works where it was made");
        client
    });

    let borrowed = client.clone();
    let answered = std::thread::spawn(move || {
        let own = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        own.block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), borrowed.requests().get("/status")).await
        })
    })
    .join()
    .unwrap();
    let (code, _) = answered
        .expect("a request from the other runtime should not time out")
        .unwrap();
    assert_eq!(code, 200);
}

/// The console borrows, and opens nothing of its own.
///
/// It also picks up a reconnection it was never told about: what it holds is a
/// slot, so replacing what is in it — which is what a chat session does when it
/// redials — is enough for the next request to go over the new connection.
#[test]
fn the_console_uses_the_connection_it_was_lent_and_follows_a_redial() {
    let dir = tempfile::tempdir().unwrap();
    let home = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (admin, seed) = signer(7);
    let (addr, server_pub, first, watching) = home.block_on(async {
        let (addr, server_pub, _h) = server_in(dir.path()).await;
        // Standing in for the chat session's connection, and for a second one
        // to watch the exchange's count without adding to it.
        let watching = Client::connect(addr, &server_pub).await.unwrap();
        let first = Client::connect_as(addr, &server_pub, &seed).await.unwrap();
        // Settled before anything is counted: the exchange counts a connection
        // when it accepts one, and a handshake that has returned to the dialler
        // has not necessarily finished being accepted. Measuring across that
        // would read a connection made *before* the console as one the console
        // made — which it did, the first time this was written.
        assert_eq!(
            settled_at(&watching, 2).await,
            2,
            "two connections were made before the console"
        );
        (addr, server_pub, first, watching)
    });
    let at = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let held = Held::empty();
    held.set(Some((first, at)));

    let before = home.block_on(accepted(&watching));
    let console = session::start(held.clone(), admin, || {});

    assert!(
        until(|| console.state().healthy == Some(true), 15),
        "the console should reach the exchange on the connection it was lent: {:?}",
        console.state()
    );
    let after = home.block_on(accepted(&watching));
    assert_eq!(
        before,
        after,
        "the console opened {} connection(s) of its own",
        after - before
    );
    assert_eq!(console.state().exchange, Some(PubKey::new(server_pub)));

    // Now a redial, as the owner performs one: the connection goes, and a
    // different one arrives in its place. The console is told nothing.
    //
    // **The empty half is the half that proves it.** A console that had taken a
    // copy of the connection at startup would go on reporting a healthy
    // exchange here, because the connection it copied is still open — so this
    // is what separates reading the slot from having read it once.
    held.set(None);
    console.send(Cmd::Probe);
    assert!(
        until(|| console.state().healthy == Some(false), 15),
        "with nothing lent, the console has nothing to ask over: {:?}",
        console.state()
    );

    let replacement = home
        .block_on(async { Client::connect_as(addr, &server_pub, &seed).await })
        .unwrap();
    held.set(Some((replacement, at)));
    let moved = home.block_on(settled_at(&watching, after + 1));
    assert_eq!(
        moved,
        after + 1,
        "the replacement should be a new connection"
    );

    console.send(Cmd::Probe);
    assert!(
        until(|| console.state().healthy == Some(true), 15),
        "the console should pick the replacement up without being told: {:?}",
        console.state()
    );
    assert_eq!(
        home.block_on(accepted(&watching)),
        moved,
        "and should still not have dialled anything itself"
    );
}
