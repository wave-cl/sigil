//! One identity, **one** connection: chat and a call on the same one.
//!
//! # What this file used to say
//!
//! It was a spike, and it asked the opposite question: can one identity hold a
//! chat connection *and* a voice connection at once? It can — `sqexd` says an
//! identity may hold several, and the test proved it rather than trusting the
//! reading. sigil was built on that answer, and dialled twice.
//!
//! The same spike measured what the second connection costs, and that
//! measurement is why this file now argues the other way: **the exchange fans a
//! relayed datagram out to every connection an identity holds**
//! (`sqexd/src/server.rs`, `Connections`). So while a call is up, every audio
//! frame is written twice — once to the connection carrying the call, and once
//! to the connection carrying chat, where nothing reads it. Two connections
//! also mean two handshakes, two sockets and two keep-alive timers, and a
//! handshake at the moment somebody presses answer is a handshake somebody is
//! waiting through.
//!
//! So a call now rides the connection the chat client already holds, and these
//! are the two things worth knowing about that: it works, and the duplicate is
//! gone.

use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_net::{CallOpts, Endpoint, Held, Phase, spawn_call};
use sqex_chat::client::Chat;
use sqex_chat::store::Store;
use sqex_proto::session::{DatagramFrame, Open, OpenAck, OpenState, Session};
use sqex_proto::timeline::Timeline;
use sqex_voice::audio::{Sink, Source};
use sqnr::Client;
use sqnr_core::{PubKey, SoftwareSigner};

mod harness;
use harness::server_in;

fn identity(b: u8) -> ([u8; 32], PubKey) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    (sk.to_bytes(), PubKey::new(sk.verifying_key().to_bytes()))
}

fn signer(b: u8) -> SoftwareSigner {
    SoftwareSigner::new(SigningKey::from_bytes(&[b; 32]))
}

fn ephemeral() -> (x25519_dalek::StaticSecret, [u8; 32]) {
    let s = x25519_dalek::StaticSecret::random_from_rng(rand_core::OsRng);
    let p = x25519_dalek::PublicKey::from(&s).to_bytes();
    (s, p)
}

async fn open_session(client: &mut Client, peer: PubKey, eph_pub: [u8; 32]) -> OpenAck {
    let (code, body) = client
        .post(
            "/session/open",
            Open {
                peer,
                ephemeral: eph_pub,
            }
            .encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
    OpenAck::decode(&body).unwrap()
}

async fn chat_at(
    addr: std::net::SocketAddr,
    server_pub: [u8; 32],
    b: u8,
    store_path: &Path,
) -> Chat {
    let (seed, me) = identity(b);
    let client = Client::connect_as(addr, &server_pub, &seed).await.unwrap();
    let store = Store::open(&seed, Some(store_path)).unwrap();
    let mut chat = Chat::new(client, seed, me, PubKey::new(server_pub), store);
    chat.top_up_prekeys().await.unwrap();
    chat
}

/// How many connections the exchange has accepted since it started.
///
/// Asked **through a connection that already exists**, because a probe of its
/// own would open one and so change the number it came to ask about.
async fn accepted(client: &Client) -> u64 {
    let (code, body) = client.requests().get("/status").await.unwrap();
    assert_eq!(code, 200);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["connections"].as_u64().unwrap()
}

fn said(timeline: &Timeline) -> Vec<String> {
    timeline
        .messages()
        .filter(|m| m.is_visible())
        .filter_map(|m| m.post.body_text().map(|t| t.to_string()))
        .collect()
}

fn tone_to(path: &Path, seconds: u64) -> CallOpts {
    CallOpts {
        source: Source::Tone,
        sink: Sink::Wav(path.to_path_buf()),
        seconds: Some(seconds),
        dtx: false,
        ..CallOpts::default()
    }
}

/// A call placed on the connection chat already holds, opening nothing new.
///
/// The whole of this stage in one test: the call runs to its end on a borrowed
/// connection, the exchange accepts no connection while it does, and the chat
/// client that lent it goes on sending afterwards — which it would not if
/// lending had cost it its connection when the call let go.
#[tokio::test]
async fn a_call_rides_the_connection_chat_already_holds() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;

    let (_, a_id) = identity(1);
    let (_, b_id) = identity(2);
    let mut a_chat = chat_at(addr, server_pub, 1, &dir.path().join("a.db")).await;
    let mut b_chat = chat_at(addr, server_pub, 2, &dir.path().join("b.db")).await;

    // Two connections exist and two is all there should ever be: one per
    // identity, whatever either of them goes on to do.
    let watching = a_chat.connection().expect("a live connection");
    let before = accepted(&watching).await;

    // What a chat *session* lends is a slot holding whatever is live, because
    // the session redials and a connection handed out a minute ago may be
    // closed. Here there is no session, so the slot is filled by hand.
    let at = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let lend = |chat: &Chat| {
        let held = Held::empty();
        held.set(Some((chat.connection().expect("a connection to lend"), at)));
        held
    };

    let a_wav = dir.path().join("a.wav");
    let b_wav = dir.path().join("b.wav");
    let mut a = spawn_call(
        lend(&a_chat),
        signer(1),
        b_id,
        20,
        tone_to(&a_wav, 1),
        || {},
    );
    let b = spawn_call(
        lend(&b_chat),
        signer(2),
        a_id,
        20,
        tone_to(&b_wav, 1),
        || {},
    );

    let ended = tokio::time::timeout(Duration::from_secs(30), async {
        while a.state().phase != Phase::Ended {
            a.changed().await.unwrap();
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "the call should run and end: {:?}",
        a.state()
    );
    assert_eq!(
        a.state().trouble,
        None,
        "the call should not have failed on a borrowed connection"
    );

    // **The point.** Not one handshake between them for the call.
    let after = accepted(&watching).await;
    assert_eq!(
        before,
        after,
        "placing a call opened {} connection(s); it is supposed to use the one \
         already there",
        after - before
    );

    // And lending did not cost the chat client its connection when the call
    // finished with it: this is the failure that would show up as "the window
    // goes offline whenever a call ends".
    let channel = a_chat.dm_with(&b_id);
    a_chat.open_dm(&b_id).await.unwrap();
    a_chat.ensure_epoch(&channel).await.unwrap();
    a_chat
        .send(&channel, "after the call, on the same connection")
        .await
        .expect("the chat client should still have its connection");
    let mut timeline = Timeline::default();
    b_chat.open_dm(&a_id).await.unwrap();
    b_chat.poll(&channel, &mut timeline, 0).await.unwrap();
    assert_eq!(
        said(&timeline),
        vec!["after the call, on the same connection".to_string()],
    );
    b.hang_up();
}

/// The duplicate delivery, and the fact that there is nothing left to duplicate.
///
/// This is the measurement that turned the original spike's answer around, kept
/// because it is the reason rather than a detail: an exchange writes a relayed
/// datagram to *every* connection the recipient identity holds. Hold one and a
/// frame arrives once. Hold two and the second copy is written to a connection
/// with no use for it — for every frame, for the length of the call.
#[tokio::test]
async fn a_frame_arrives_once_for_an_identity_holding_one_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;

    let (a_seed, a_id) = identity(1);
    let (b_seed, b_id) = identity(2);

    let mut a_voice = Client::connect_as(addr, &server_pub, &a_seed)
        .await
        .unwrap();
    let mut b_only = Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .unwrap();

    let (a_eph, a_eph_pub) = ephemeral();
    let (b_eph, b_eph_pub) = ephemeral();
    open_session(&mut a_voice, b_id, a_eph_pub).await;
    let b_ack = open_session(&mut b_only, a_id, b_eph_pub).await;
    let a_ack = open_session(&mut a_voice, b_id, a_eph_pub).await;
    assert_eq!(a_ack.state, OpenState::Established);
    let sid = a_ack.session_id;
    let a_sess = Session::derive(&a_seed, &a_eph, &b_id, &a_ack.peer_ephemeral).unwrap();
    let b_sess = Session::derive(&b_seed, &b_eph, &a_id, &b_ack.peer_ephemeral).unwrap();

    let send = |seq: u64, what: &'static [u8]| {
        let sealed = a_sess.seal_datagram(seq, what).unwrap();
        a_voice
            .send_datagram(
                DatagramFrame {
                    session_id: sid,
                    seq,
                    ciphertext: sealed,
                }
                .encode(),
            )
            .unwrap();
    };

    send(0, b"one frame");
    let bytes = tokio::time::timeout(Duration::from_secs(2), b_only.read_datagram())
        .await
        .expect("the frame should arrive")
        .unwrap();
    let frame = DatagramFrame::decode(&bytes).unwrap();
    assert_eq!(
        b_sess.open(frame.seq, &frame.ciphertext).unwrap(),
        b"one frame"
    );

    // Once. Nothing is holding a second connection for it to be copied to.
    let again = tokio::time::timeout(Duration::from_millis(500), b_only.read_datagram()).await;
    assert!(
        again.is_err(),
        "a second copy of the frame arrived; the identity holds one connection"
    );

    // And the cost of the arrangement this replaces, in one line: a second
    // connection for the same identity, reading nothing and wanting nothing,
    // is handed every frame anyway.
    let b_spare = Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .unwrap();
    send(1, b"and again");
    let wanted = tokio::time::timeout(Duration::from_secs(2), b_only.read_datagram())
        .await
        .expect("the session's own connection receives it")
        .unwrap();
    let spare = tokio::time::timeout(Duration::from_secs(2), b_spare.read_datagram())
        .await
        .expect("and so does the one that has no use for it")
        .unwrap();
    assert_eq!(
        wanted, spare,
        "the fanout delivers the identical frame to every connection, which is \
         why sigil holds one"
    );
}
