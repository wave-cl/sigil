//! Somebody rings, and a program that was doing nothing finds out.
//!
//! This is the difference between a dialler and a telephone. `sqex-voice`
//! could only ever place a call that the other side was already expecting,
//! because the exchange will not disclose that somebody is waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_net::{Endpoint, listen};
use sqnr::Client;
use sqnr_core::{PubKey, SoftwareSigner};

mod harness;
use harness::server_in;

fn signer(b: u8) -> (SoftwareSigner, PubKey, [u8; 32]) {
    let seed = [b; 32];
    let sk = SigningKey::from_bytes(&seed);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    (SoftwareSigner::new(sk), public, seed)
}

/// Wait for a condition, or give up. Polling the handle is what an interface
/// does between frames, so testing that way is testing the real thing.
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

#[tokio::test]
async fn an_idle_client_is_told_that_somebody_is_calling() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (_a_signer, a_id, a_seed) = signer(1);
    let (b_signer, b_id, _) = signer(2);

    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let mut listener = listen(endpoint, b_signer, move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });

    // B is doing nothing at all, and must still end up listening.
    assert!(
        until(|| listener.state().listening, 10).await,
        "the listener should come up: {:?}",
        listener.state().trouble
    );

    // A rings, having never spoken to B.
    let mut a = Client::connect_as(addr, &server_pub, &a_seed)
        .await
        .unwrap();
    sqex_voice::ring::ring(&mut a, b_id).await.unwrap();

    let mut got = Vec::new();
    let rang = until(
        || {
            got.extend(listener.drain());
            !got.is_empty()
        },
        15,
    )
    .await;
    assert!(rang, "B should have been told, without asking");
    assert_eq!(got[0].from, a_id, "and by whom");
    assert!(
        wakes.load(Ordering::Relaxed) > 0,
        "an interface that is never woken never shows the ring"
    );

    // The same ring must not arrive twice: a phone that keeps ringing after
    // you have looked at it is worse than one that does not ring.
    let before = got.len();
    tokio::time::sleep(Duration::from_secs(3)).await;
    got.extend(listener.drain());
    assert_eq!(got.len(), before, "a ring is delivered once");

    listener.stop();
}

/// Blocking is set on the listener and takes effect without restarting it.
#[tokio::test]
async fn a_blocked_caller_never_rings() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (_, a_id, a_seed) = signer(1);
    let (b_signer, b_id, _) = signer(2);

    let mut listener = listen(endpoint, b_signer, || {});
    assert!(
        until(|| listener.state().listening, 10).await,
        "listener up"
    );
    listener.set_blocked(vec![a_id]);

    let mut a = Client::connect_as(addr, &server_pub, &a_seed)
        .await
        .unwrap();
    sqex_voice::ring::ring(&mut a, b_id).await.unwrap();

    // Long enough for several sweeps to have had the chance.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        listener.drain().is_empty(),
        "a blocked caller does not ring"
    );
    listener.stop();
}

/// A phone that has quietly stopped ringing is worse than one that is
/// obviously broken, so not-listening is a state the interface can read.
#[tokio::test]
async fn a_listener_that_cannot_reach_the_exchange_says_so() {
    // An address nothing is answering on.
    let endpoint = Endpoint {
        address: "127.0.0.1:1".parse().unwrap(),
        server: PubKey::new([9u8; 32]),
    };
    let (b_signer, _, _) = signer(2);
    let listener = listen(endpoint, b_signer, || {});

    let said = until(
        || !listener.state().listening && listener.state().trouble.is_some(),
        15,
    )
    .await;
    assert!(said, "it must say it is not listening");
    let trouble = listener.state().trouble.unwrap();
    assert!(
        trouble.contains("not listening for calls"),
        "in words somebody can act on: {trouble}"
    );
    listener.stop();
}
