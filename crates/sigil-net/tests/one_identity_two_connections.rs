//! Spike (a): can one identity hold a voice connection and a chat connection at
//! the same time, against one exchange?
//!
//! The whole of sigil rests on this. `sqex-voice`'s README says "One identity,
//! one client", and its call loop diagnoses the failure it causes — but that
//! warning is about two *processes* each negotiating their own SIP-12 session
//! with the same peer, where the peer keeps one and the other goes deaf.
//! Connections are a different question, and `sqexd/src/server.rs` answers it:
//!
//! > Live connections by the identity that advertised itself on them (SIP-3).
//! > An identity may hold several connections at once; a datagram goes to all
//! > of them, and the peer's session keys mean only the intended one can open it.
//!
//! Close enough to the warning that the design should not rest on reading
//! alone. So: prove it, or find out now.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sqex_chat::client::Chat;
use sqex_chat::store::Store;
use sqex_proto::session::{DatagramFrame, Open, OpenAck, OpenState, Session};
use sqex_proto::timeline::Timeline;
use sqexd::config::FileConfig;
use sqnr::Client;
use sqnr_core::PubKey;

// ---- harness (mirrors sqex-voice/tests/voice_flow.rs) ------------------------

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

fn identity(b: u8) -> ([u8; 32], PubKey) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    (sk.to_bytes(), PubKey::new(sk.verifying_key().to_bytes()))
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

async fn chat_at(addr: SocketAddr, server_pub: [u8; 32], b: u8, store_path: &Path) -> Chat {
    let (seed, me) = identity(b);
    let client = Client::connect_as(addr, &server_pub, &seed).await.unwrap();
    let store = Store::open(&seed, Some(store_path)).unwrap();
    let mut chat = Chat::new(client, seed, me, PubKey::new(server_pub), store);
    chat.top_up_prekeys().await.unwrap();
    chat
}

fn said(timeline: &Timeline) -> Vec<String> {
    timeline
        .messages()
        .filter(|m| m.is_visible())
        .filter_map(|m| m.post.body_text().map(|t| t.to_string()))
        .collect()
}

// ---- the spike --------------------------------------------------------------

/// Two identities, each holding *two* connections — one for chat, one for voice
/// — with media and messages crossing at the same time.
#[tokio::test]
async fn one_identity_holds_a_chat_and_a_voice_connection_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;

    let (a_seed, a_id) = identity(1);
    let (b_seed, b_id) = identity(2);

    // Connection 1 of 2 for each identity: chat.
    let mut a_chat = chat_at(addr, server_pub, 1, &dir.path().join("a.db")).await;
    let mut b_chat = chat_at(addr, server_pub, 2, &dir.path().join("b.db")).await;

    // Connection 2 of 2 for each identity: voice, as the *same* identity.
    // If an exchange refused a second connection per identity, this is where it
    // would show.
    let mut a_voice = Client::connect_as(addr, &server_pub, &a_seed)
        .await
        .unwrap();
    let mut b_voice = Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .unwrap();
    assert!(
        a_voice.max_datagram_size().is_some(),
        "the voice connection must carry datagrams"
    );

    // Establish a SIP-12 session over the voice connections only.
    let (a_eph, a_eph_pub) = ephemeral();
    let (b_eph, b_eph_pub) = ephemeral();
    let first = open_session(&mut a_voice, b_id, a_eph_pub).await;
    assert_eq!(
        first.state,
        OpenState::Waiting,
        "one side asking should disclose nothing"
    );
    let b_ack = open_session(&mut b_voice, a_id, b_eph_pub).await;
    assert_eq!(b_ack.state, OpenState::Established, "both sides have asked");
    let a_ack = open_session(&mut a_voice, b_id, a_eph_pub).await;
    assert_eq!(a_ack.state, OpenState::Established);
    let sid = a_ack.session_id;
    assert_eq!(sid, b_ack.session_id);

    let a_sess = Session::derive(&a_seed, &a_eph, &b_id, &a_ack.peer_ephemeral).unwrap();
    let b_sess = Session::derive(&b_seed, &b_eph, &a_id, &b_ack.peer_ephemeral).unwrap();

    // --- the actual question: both at once -----------------------------------
    // Alice sends a chat message on her chat connection while sending media on
    // her voice connection. Bob reads both, on his two.
    let channel = a_chat.dm_with(&b_id);
    a_chat.open_dm(&b_id).await.unwrap();
    a_chat.ensure_epoch(&channel).await.unwrap();

    // Media out on the voice connection.
    for seq in 0..5u64 {
        let sealed = a_sess.seal_datagram(seq, b"twenty milliseconds").unwrap();
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
    }

    // A message out on the chat connection, *between* the datagrams and the read.
    a_chat
        .send(&channel, "and a message at the same time")
        .await
        .unwrap();

    // More media, after the chat request.
    for seq in 5..10u64 {
        let sealed = a_sess.seal_datagram(seq, b"twenty milliseconds").unwrap();
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
    }

    // Bob reads media on his voice connection.
    let mut heard = 0;
    for _ in 0..10 {
        let Ok(Ok(bytes)) =
            tokio::time::timeout(Duration::from_secs(2), b_voice.read_datagram()).await
        else {
            break;
        };
        let frame = DatagramFrame::decode(&bytes).unwrap();
        assert_eq!(frame.session_id, sid);
        let plain = b_sess.open(frame.seq, &frame.ciphertext).unwrap();
        assert_eq!(&plain, b"twenty milliseconds");
        heard += 1;
    }
    assert_eq!(heard, 10, "every frame sent should have been opened");

    // Bob reads the message on his chat connection.
    let mut timeline = Timeline::default();
    b_chat.open_dm(&a_id).await.unwrap();
    b_chat.poll(&channel, &mut timeline, 0).await.unwrap();
    assert_eq!(
        said(&timeline),
        vec!["and a message at the same time".to_string()],
        "the chat connection kept working while media flowed on the other"
    );

    // And the voice connection still works *after* the chat traffic, which is
    // what a real session does for as long as the call lasts.
    let sealed = a_sess.seal_datagram(10, b"still here").unwrap();
    a_voice
        .send_datagram(
            DatagramFrame {
                session_id: sid,
                seq: 10,
                ciphertext: sealed,
            }
            .encode(),
        )
        .unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(2), b_voice.read_datagram())
        .await
        .expect("a datagram should still arrive after chat traffic")
        .unwrap();
    let frame = DatagramFrame::decode(&bytes).unwrap();
    assert_eq!(
        b_sess.open(frame.seq, &frame.ciphertext).unwrap(),
        b"still here"
    );
}

/// The cost of holding two connections, measured rather than assumed.
///
/// `sqexd` fans a relayed datagram to *every* live connection the recipient
/// identity holds. So a voice frame is also written to the connection chat is
/// using, where nothing reads it. This test pins that behaviour down: it is the
/// price of the two-connection design, and if it ever changes, the reasoning in
/// the plan changes with it.
#[tokio::test]
async fn a_voice_datagram_reaches_every_connection_the_identity_holds() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;

    let (a_seed, a_id) = identity(1);
    let (b_seed, b_id) = identity(2);

    let mut a_voice = Client::connect_as(addr, &server_pub, &a_seed)
        .await
        .unwrap();
    let mut b_voice = Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .unwrap();
    // A second connection for Bob that never opens a session and never reads —
    // standing in for the one chat would be using.
    let b_other = Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .unwrap();

    let (a_eph, a_eph_pub) = ephemeral();
    let (b_eph, b_eph_pub) = ephemeral();
    open_session(&mut a_voice, b_id, a_eph_pub).await;
    let b_ack = open_session(&mut b_voice, a_id, b_eph_pub).await;
    let a_ack = open_session(&mut a_voice, b_id, a_eph_pub).await;
    assert_eq!(a_ack.state, OpenState::Established);
    let sid = a_ack.session_id;

    let a_sess = Session::derive(&a_seed, &a_eph, &b_id, &a_ack.peer_ephemeral).unwrap();
    let b_sess = Session::derive(&b_seed, &b_eph, &a_id, &b_ack.peer_ephemeral).unwrap();

    let sealed = a_sess.seal_datagram(0, b"one frame").unwrap();
    a_voice
        .send_datagram(
            DatagramFrame {
                session_id: sid,
                seq: 0,
                ciphertext: sealed,
            }
            .encode(),
        )
        .unwrap();

    // The connection that holds the session opens it, as expected.
    let bytes = tokio::time::timeout(Duration::from_secs(2), b_voice.read_datagram())
        .await
        .expect("the session's own connection receives the frame")
        .unwrap();
    let frame = DatagramFrame::decode(&bytes).unwrap();
    assert_eq!(
        b_sess.open(frame.seq, &frame.ciphertext).unwrap(),
        b"one frame"
    );

    // And so does the other one — same bytes, and it has no use for them.
    let also = tokio::time::timeout(Duration::from_secs(2), b_other.read_datagram()).await;
    match also {
        Ok(Ok(other_bytes)) => {
            assert_eq!(
                other_bytes, bytes,
                "the fanout delivers the identical frame to every connection"
            );
        }
        _ => panic!(
            "expected the frame on the second connection too: the two-connection \
             design's cost is this duplicate delivery, and the plan says so"
        ),
    }
}
