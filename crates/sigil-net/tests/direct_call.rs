//! A direct-message call goes straight to the peer when the exchange can
//! introduce them, and through the exchange's room when it cannot.
//!
//! On loopback there is no NAT, so what this proves is that the two paths
//! are wired: the introduction is asked for, taken when both ask, and the
//! room is fallen back to -- within the budget -- when only one does. The
//! hole itself is the two-homes field test's to prove.

use std::path::Path;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use sigil_net::{CallHandle, CallOpts, Endpoint, Path as Way, Phase, RoomId, spawn_dm_call};
use sqex_voice::audio::{Sink, Source, TONE_HZ, dominant_hz};
use sqnr_core::{PubKey, SoftwareSigner};

mod harness;
use harness::server_in;

fn signer(b: u8) -> (SoftwareSigner, PubKey) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    (SoftwareSigner::new(sk), public)
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

/// Wait for the call to go live or end, as the interface does.
async fn settled(h: &mut CallHandle) -> sigil_net::CallState {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let s = h.state();
            if s.phase == Phase::Live || s.phase == Phase::Ended {
                return s;
            }
            h.changed().await.unwrap();
        }
    })
    .await
    .expect("the call should reach a decision")
}

fn heard_tone(path: &Path, who: &str) {
    let mut reader = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{who}: {e}"));
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    let hz = dominant_hz(&samples);
    assert!(
        (hz - TONE_HZ).abs() < 30.0,
        "{who} heard {hz:.0} Hz, wanted {TONE_HZ:.0}"
    );
}

/// Both ask: the call is direct, says so, and carries audio without the
/// room ever being joined.
#[tokio::test]
async fn a_dm_call_is_introduced_and_goes_direct() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(1);
    let (b_signer, b_id) = signer(2);
    let room = RoomId::generate();
    let a_wav = dir.path().join("a.wav");
    let b_wav = dir.path().join("b.wav");

    let mut a = spawn_dm_call(
        endpoint,
        a_signer,
        b_id,
        room,
        true,
        tone_to(&a_wav, 1),
        || {},
    );
    let mut b = spawn_dm_call(
        endpoint,
        b_signer,
        a_id,
        room,
        true,
        tone_to(&b_wav, 1),
        || {},
    );
    assert_eq!(
        a.state().path,
        None,
        "nothing is settled before the introduction"
    );

    let (sa, sb) = (settled(&mut a).await, settled(&mut b).await);
    for (who, s) in [("A", &sa), ("B", &sb)] {
        assert_eq!(s.phase, Phase::Live, "{who}: {:?}", s.trouble);
        assert_eq!(
            s.path,
            Some(Way::Direct),
            "{who} did not go direct: {:?}",
            s.why
        );
        assert_eq!(s.why, None);
        assert_eq!(s.session, Some(sqex_voice::direct::DIRECT_SESSION));
    }
    a.finished().await.expect("A's call");
    b.finished().await.expect("B's call");
    heard_tone(&a_wav, "A");
    heard_tone(&b_wav, "B");
}

/// One side never asks: the other waits its budget, says it is relayed and
/// why, and the two meet in the room -- with audio.
#[tokio::test]
async fn without_a_second_asker_the_call_is_relayed_in_time() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(1);
    let (b_signer, b_id) = signer(2);
    let room = RoomId::generate();
    let a_wav = dir.path().join("a.wav");
    let b_wav = dir.path().join("b.wav");

    let started = Instant::now();
    // B stays in the room long enough for A to arrive after its wait; A's
    // tone is short, so only what A heard is checked.
    let mut a = spawn_dm_call(
        endpoint,
        a_signer,
        b_id,
        room,
        true,
        tone_to(&a_wav, 2),
        || {},
    );
    let mut b = spawn_dm_call(
        endpoint,
        b_signer,
        a_id,
        room,
        false,
        tone_to(&b_wav, 14),
        || {},
    );

    let sb = settled(&mut b).await;
    assert_eq!(sb.phase, Phase::Live, "B: {:?}", sb.trouble);
    assert_eq!(sb.path, Some(Way::Relayed));
    assert!(
        sb.why
            .as_deref()
            .is_some_and(|w| w.contains("no introduction")),
        "{:?}",
        sb.why
    );

    let sa = settled(&mut a).await;
    let took = started.elapsed();
    assert_eq!(sa.phase, Phase::Live, "A: {:?}", sa.trouble);
    assert_eq!(sa.path, Some(Way::Relayed), "A should have fallen back");
    assert!(
        sa.why.as_deref().is_some_and(|w| w.contains("did not ask")),
        "A should say why: {:?}",
        sa.why
    );
    assert!(
        took < Duration::from_secs(15),
        "the fallback took {took:?}; the budget is eight seconds of waiting"
    );
    assert!(
        took >= Duration::from_secs(8),
        "A did not wait for the introduction at all: {took:?}"
    );
    a.finished().await.expect("A's call");
    b.finished().await.expect("B's call");
    heard_tone(&a_wav, "A");
}

/// **Hanging up asks the call to end; it does not kill it.**
///
/// `engine::call` posts `/session/close` as its last act, and `hang_up` used
/// to `abort()` the task — which cancels it at its next await, so the close
/// was never sent. On a dialled connection the connection dropping is the
/// signal and nothing was lost. On a **borrowed** one, a SIP-39 call across
/// exchanges riding the chat session, there is no drop and the far side is
/// told nothing: it goes on sending into a session this end has forgotten.
/// Seen live, for 24 minutes.
///
/// `Event::Draining` is the discriminator. The engine raises it when it
/// starts letting what is in flight arrive, on the one path that reaches
/// the close; an aborted task raises nothing at all, so this test fails on
/// the old behaviour for the right reason rather than by timing.
#[tokio::test(flavor = "multi_thread")]
async fn hanging_up_lets_the_call_close_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(1);
    let (b_signer, _b_id) = signer(2);
    let room = RoomId::generate();

    // Long enough that it cannot end on its own inside the test: what ends
    // it has to be the hang-up.
    let mut a = spawn_dm_call(
        endpoint,
        a_signer,
        PubKey::new([9u8; 32]),
        room,
        false,
        tone_to(&dir.path().join("a.wav"), 600),
        || {},
    );
    let mut b = spawn_dm_call(
        endpoint,
        b_signer,
        a_id,
        room,
        false,
        tone_to(&dir.path().join("b.wav"), 600),
        || {},
    );
    assert_eq!(settled(&mut a).await.phase, Phase::Live, "a is in the call");
    assert_eq!(settled(&mut b).await.phase, Phase::Live, "b is in the call");

    a.hang_up();

    // The engine says it is draining, which only the path that reaches
    // `/session/close` does.
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.drain()
                .iter()
                .any(|e| matches!(e, sigil_net::Event::Draining))
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or(false);
    assert!(
        drained,
        "hanging up never reached the drain, so the session was never closed"
    );
    // And the interface still hears about the ending at once, rather than
    // waiting out the drain.
    assert_eq!(
        a.state().phase,
        Phase::Ended,
        "the screen was left in a call"
    );
}
