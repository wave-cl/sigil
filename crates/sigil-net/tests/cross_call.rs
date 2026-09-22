//! **SIP-39: a call placed here for somebody at another exchange.**
//!
//! [`spawn_call`](sigil_net::spawn_call) dials a key at *this* exchange and
//! rendezvous with it there. A person whose account lives somewhere else has
//! no session here to meet in, so the call is placed at this exchange for
//! `name@domain` and this exchange carries it to theirs, which rings them.
//!
//! # What this proves, and what it does not
//!
//! The route is posted and the exchange's answer comes back in words. What
//! it does not prove is a call that connects: that needs the far exchange,
//! the far person, and `engine::answer` standing by on their side, which is
//! SIP-39's other half and is not wired here yet. So the assertion is the
//! refusal -- which is the thing a person actually meets when they call
//! somebody who is not there, and which was previously "that is not a key"
//! before anything was sent at all.
//!
//! Against a real `sqexd`, on sqex's own principle: a client that was only
//! ever tested against a mock has tested the mock.

use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_net::{CallHandle, CallOpts, Endpoint, Phase, spawn_cross_call};
use sqnr_core::{PubKey, SoftwareSigner};

mod harness;
use harness::server_in;

fn signer(b: u8) -> (SoftwareSigner, PubKey) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    (SoftwareSigner::new(sk), public)
}

/// Wait for the call to end, as the interface does.
async fn settled(h: &mut CallHandle) -> sigil_net::CallState {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let s = h.state();
            if s.phase == Phase::Ended {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the call should end within thirty seconds")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_for_somebody_at_another_exchange_is_carried_and_answered_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (me, _) = signer(0x51);

    // Nobody, at an exchange this one does not federate with. The point is
    // that the request goes *out* and the answer comes back: a name this
    // exchange cannot reach is the shortest way to that answer.
    let mut call = spawn_cross_call(
        endpoint,
        me,
        "nobody@elsewhere.test".to_string(),
        3,
        CallOpts::default(),
        || {},
    );
    let ended = settled(&mut call).await;

    let said = ended.trouble.unwrap_or_default();
    assert!(
        !said.is_empty(),
        "the call ended with nothing to say, so nothing was asked of the \
         exchange"
    );
    // **Not any refusal: the exchange's.** It answers `Rejected` for a name
    // it cannot reach, and that comes back as "the call was refused". A
    // local failure would read quite differently -- "that is not a key" is
    // what this used to say, before anything was sent at all.
    assert!(
        said.contains("the call was refused"),
        "the call did not reach the exchange, or its answer was not carried \
         back: {said}"
    );
    assert!(
        !said.contains("not a key"),
        "the handle was still being read as a key: {said}"
    );
}
