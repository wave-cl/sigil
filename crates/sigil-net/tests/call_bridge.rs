//! A call run the way sigil will run one: on a task, watched from outside.
//!
//! `sqex-voice`'s own tests prove the loop carries audio. This proves the
//! *bridge* — that a caller who never blocks on the network can still see the
//! call progress, and is told when it ends.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_net::{CallOpts, Endpoint, Phase, spawn_call};
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

#[tokio::test]
async fn a_call_on_a_task_reports_its_progress_and_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(1);
    let (b_signer, b_id) = signer(2);
    let a_wav = dir.path().join("a.wav");
    let b_wav = dir.path().join("b.wav");

    // Count the wakes, because an interface that is never woken never redraws,
    // and one that is woken constantly is the reason a laptop gets warm.
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();

    let mut a = spawn_call(
        endpoint,
        a_signer,
        b_id,
        20,
        tone_to(&a_wav, 1),
        move || {
            counter.fetch_add(1, Ordering::Relaxed);
        },
    );
    let b = spawn_call(endpoint, b_signer, a_id, 20, tone_to(&b_wav, 1), || {});

    // The peer is known before anything has connected -- the interface can name
    // who is being called while it is still ringing.
    assert_eq!(a.state().peer, Some(b_id));

    // Wait for the call to go live, without polling and without blocking on the
    // network, exactly as the interface will.
    let live = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let s = a.state();
            if s.phase == Phase::Live || s.phase == Phase::Ended {
                return s;
            }
            a.changed().await.unwrap();
        }
    })
    .await
    .expect("the call should reach a decision within twenty seconds");

    assert_eq!(live.phase, Phase::Live, "trouble: {:?}", live.trouble);
    assert_eq!(live.me, Some(a_id), "the snapshot says who we are");
    assert!(live.session.is_some(), "and which session is carrying it");

    a.finished().await.expect("A's call");
    b.finished().await.expect("B's call");

    assert!(
        wakes.load(Ordering::Relaxed) > 0,
        "an interface that is never woken never redraws"
    );

    // And it really was a call, not just a state machine.
    let mut reader = hound::WavReader::open(&a_wav).unwrap();
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    let hz = dominant_hz(&samples);
    assert!(
        (hz - TONE_HZ).abs() < 30.0,
        "heard {hz:.0} Hz, wanted {TONE_HZ:.0}"
    );
}

/// A call nobody answers must end by itself and say why, rather than sitting on
/// screen looking like it is still connecting.
#[tokio::test]
async fn a_call_that_fails_ends_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, _) = signer(1);
    let (_, b_id) = signer(2);

    let mut a = spawn_call(endpoint, a_signer, b_id, 1, CallOpts::default(), || {});

    let ended = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let s = a.state();
            if s.is_over() {
                return s;
            }
            a.changed().await.unwrap();
        }
    })
    .await
    .expect("it must give up on its own");

    assert_eq!(ended.phase, Phase::Ended);
    let trouble = ended.trouble.expect("a failed call says why");
    assert!(
        trouble.contains("did not join in time"),
        "unexpected: {trouble}"
    );

    // The narrative is still there to read, separately from the state.
    let said: Vec<String> = a.drain().iter().map(|e| e.describe()).collect();
    assert!(
        said.iter().any(|l| l.contains("waiting for")),
        "the log says what it was waiting for: {said:?}"
    );
}

/// Hanging up ends the call promptly. It is the one way a call stops that is
/// somebody's decision rather than an outcome.
#[tokio::test]
async fn hanging_up_ends_the_call() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, _) = signer(1);
    let (_, b_id) = signer(2);

    // Nobody will answer, so this would otherwise wait the full sixty seconds.
    let a = spawn_call(endpoint, a_signer, b_id, 60, CallOpts::default(), || {});
    tokio::time::sleep(Duration::from_millis(200)).await;
    a.hang_up();

    tokio::time::timeout(Duration::from_secs(5), a.finished())
        .await
        .expect("hanging up must not wait for the dial to time out")
        .expect("a cancelled call is not a failed one");
}
