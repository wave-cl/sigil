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
    server_with(dir, "off").await
}

/// The same, with a SIP-38 registration policy of the caller's choosing.
///
/// `off` is the default and does not carry the route at all, which is right
/// for every other test here and useless for the one about claiming a name.
async fn server_with(
    dir: &Path,
    names: &str,
) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    server_peering(dir, names, &[]).await
}

/// The same, seeded with exchanges it federates with (SIP-39's list, which
/// SIP-39 §The peer directory reads back).
async fn server_peering(
    dir: &Path,
    names: &str,
    peers: &[PubKey],
) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let seeded: Vec<String> = peers.iter().map(|k| format!("\"{k}\"")).collect();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nlimits = {{ posts = [0, 0], signals = [0, 0], joins = [0, 0], creates = [0, 0], uploads = [0, 0] }}\nname_registration = \"{names}\"\nseed_relay_peers = [{}]\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
        seeded.join(", "),
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

/// Two people with a conversation open do not talk to the exchange about
/// nothing.
///
/// A read mark used to be written on every tick while a conversation was open.
/// The exchange stores it unconditionally and publishes a `Cursor` event to
/// every other member whether or not the value moved; the other client marks
/// the channel dirty, fetches it, and writes its own mark. Two windows open on
/// one conversation kept that going between them at 1.4 rounds a second, for
/// as long as both were open, with nobody typing.
///
/// **Counted at the exchange**, in its own `/status`, because the traffic is
/// the thing being fixed and the interface cannot see it: none of this
/// produced a repaint, an unread count, or anything else a client-side
/// assertion could have noticed.
#[tokio::test]
async fn an_open_conversation_is_quiet_when_nothing_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(27);
    let (b_signer, b_id) = signer(28);
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
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    alice.send(Cmd::Send("one thing, and then quiet".into()));
    assert!(
        until(
            || bob
                .state()
                .lines
                .iter()
                .any(|l| l.text == "one thing, and then quiet"),
            20
        )
        .await,
        "the message should arrive before the quiet starts"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    let asked = || async {
        let mut probe = sqnr::Client::connect(addr, &server_pub)
            .await
            .expect("the exchange answers a status");
        let (code, body) = probe.get("/status").await.expect("status");
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        json["requests"].as_u64().expect("a request count")
    };

    let before = asked().await;
    tokio::time::sleep(Duration::from_secs(12)).await;
    let after = asked().await;

    // **Two, and they are both a fetch being left waiting.** Nothing is polled
    // any more: each client holds one parked `/channel/fetch` on the
    // conversation it has open, which the exchange answers when something
    // happens and otherwise lets expire at `MAX_WAIT` — 25 seconds. So in a
    // twelve-second window each client re-parks at most once, whatever the
    // phase, and nothing else asks anything at all.
    //
    // It was forty in a three-second window before any of this: a read mark and
    // a cursor fetch per
    // tick per client, the events those provoked at the other end, and — the
    // larger half, found by logging every request rather than by reasoning
    // about it — a `/channel/info` and one `/device/list` per member inside
    // every poll, asked whether or not anything had arrived to attribute. Then
    // eight, on a 700ms tick. Then two, once the timer became a five-second
    // backstop. Then none, once the open conversation stopped being fetched
    // for being open. This is that same nothing, with one standing question
    // each: 0.08 requests a second, against 13 before any of this.
    //
    // The window is twelve seconds rather than three so the arithmetic is a
    // ceiling rather than a probability — and it stays clear of the
    // 28-second rebuild, which would add its own.
    let spent = after - before - 1; // the probe's own /status
    assert!(
        spent <= 2,
        "two idle clients made {spent} requests in twelve seconds with nobody \
         saying anything; the only thing either should send is one parked fetch"
    );

    // **And at least one, once the window is longer than a wait.** The ceiling
    // above is satisfied by a client that asks nothing at all, which is not
    // what this is: a parked fetch is a standing question, and one that expires
    // is asked again. Past `MAX_WAIT` both clients must therefore have spoken.
    //
    // Neither half means much alone — the ceiling rules out polling, the floor
    // rules out silence, and only together do they say "one question each,
    // left standing".
    tokio::time::sleep(Duration::from_secs(15)).await;
    let later = asked().await;
    let standing = later - after - 1;
    assert!(
        standing >= 2,
        "in fifteen seconds past a twelve-second window neither client renewed \
         a parked fetch ({standing} requests); nothing is waiting at the exchange"
    );

    alice.stop();
    bob.stop();
}

/// A message is on screen before the timer would have asked for it.
///
/// The session used to learn about everything on its own 700ms tick: an event
/// had crossed the world in ninety milliseconds and then sat in a queue for up
/// to seven times that before anything looked at it. The exchange knocks now,
/// and the tick is a backstop.
///
/// **Measured against the tick, not against a number I chose.** Anything at or
/// past 700ms is what the old behaviour did; the assertion is that it beats
/// that, with room for a slow machine.
///
/// **Bob does not open the conversation, and that is what keeps this about the
/// knock.** A conversation on screen has a fetch parked on it, which answers
/// the moment anything is said — so with the conversation open this passed
/// with the knock taken out, testing the long poll under the name of the
/// event stream. Where nobody is looking there is no parked fetch and the
/// event is the only thing that can be quick.
#[tokio::test]
async fn a_message_arrives_without_waiting_for_the_tick() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(25);
    let (b_signer, b_id) = signer(26);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    // **Bob's backstop is ten seconds**, so nothing can arrive on the timer
    // within the deadline below. Everything else runs exactly as it does in
    // the application. With the ordinary 700ms tick the two ways a message can
    // arrive are indistinguishable, which is how the first version of this
    // test passed with the knock taken out.
    let bob = session::start_every(
        endpoint,
        b_signer,
        Some(dir.path().join("b.db")),
        || {},
        Duration::from_secs(10),
    );
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up: {:?}",
        alice.state().trouble
    );
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(|| alice.state().open.is_some(), 15).await,
        "the sender should have the conversation open"
    );
    // Bob has to know the conversation exists before he can be told it moved,
    // but he does not look at it: see the note above about the parked fetch.
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id)),
            15
        )
        .await,
        "the conversation should reach the list: {:?}",
        bob.state().conversations
    );
    assert_eq!(bob.state().open, None, "and nobody is looking at it");
    // Everything that was going to happen, before the clock starts.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let sent = tokio::time::Instant::now();
    alice.send(Cmd::Send("no waiting".into()));
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id) && c.unread > 0),
            20
        )
        .await,
        "the message never arrived at all: {:?}",
        bob.state().trouble
    );
    let took = sent.elapsed();

    // Bob's timer is ten seconds away. Two round trips to a loopback exchange
    // are milliseconds, so anything in this region came from the knock.
    assert!(
        took < Duration::from_secs(2),
        "a message took {took:?} to reach the screen, which is the backstop \
         rather than the exchange"
    );
}

/// A session with nothing happening wakes nobody.
///
/// eframe is reactive: with nothing asking for a repaint it sleeps. The tick
/// used to wake the interface unconditionally, so an idle account held the
/// window at 1.4 frames a second for ever — each of those frames cloning the
/// state several times and laying out every message in view, for an account
/// where nothing at all had happened.
///
/// Counted, not timed: the wake is a callback, so a test can hold the counter.
#[tokio::test]
async fn an_idle_session_stops_waking_the_interface() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(23);
    let (b_signer, b_id) = signer(24);

    let woke = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = woke.clone();
    let alice = session::start(
        endpoint,
        a_signer,
        Some(dir.path().join("a.db")),
        move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        },
    );
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
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open"
    );

    // Let everything that was going to happen happen.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let settled = woke.load(std::sync::atomic::Ordering::Relaxed);

    // Four ticks of nothing at all.
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    let idle = woke.load(std::sync::atomic::Ordering::Relaxed) - settled;
    assert_eq!(
        idle, 0,
        "an idle session woke the interface {idle} times in three seconds"
    );

    // And it still wakes when there is something to say.
    bob.send(Cmd::Send("something to wake for".into()));
    assert!(
        until(
            || woke.load(std::sync::atomic::Ordering::Relaxed) > settled,
            20
        )
        .await,
        "a message arrived and the interface was never told"
    );

    alice.stop();
    bob.stop();
}

/// The interface is handed the picture, not a copy of it.
///
/// The state is cloned four or five times a frame -- to draw, to badge the tray
/// and the rail, to see whether anything is ringing, to read one field about
/// another identity's exchange. A fetched image used to be copied into every
/// published state and again into every one of those clones: a two-megabyte
/// photograph on screen was ten megabytes of memcpy per frame before a pixel
/// was drawn.
///
/// Two snapshots taken a moment apart must point at the same bytes. That is
/// what says `publish` hands out what the session already holds, rather than
/// building a fresh copy every 700 milliseconds for every reader.
#[tokio::test]
async fn a_published_picture_is_shared_and_not_copied() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(21);
    let (b_signer, b_id) = signer(22);
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
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open"
    );

    // A `.png`, because only an image is fetched without being asked for --
    // which is the path that puts bytes into the published state at all. The
    // content need not decode: the kind comes from the name (`kind_of`).
    let picture = dir.path().join("a-picture.png");
    std::fs::write(&picture, vec![9u8; 32 * 1024]).unwrap();
    alice.send(Cmd::SendFile(picture));

    let arrived = until(
        || {
            bob.state()
                .lines
                .iter()
                .any(|l| l.attachments.iter().any(|a| a.bytes.is_some()))
        },
        30,
    )
    .await;
    assert!(
        arrived,
        "the picture should be fetched and published: {:?}",
        bob.state().trouble
    );

    let held = |h: &ChatHandle| {
        h.state()
            .lines
            .into_iter()
            .find_map(|l| l.attachments.into_iter().find_map(|a| a.bytes))
            .expect("the picture in the published state")
    };
    let first = held(&bob);

    // **Across a publish, not within one.** Two snapshots taken back to back
    // read the same published value and would share their bytes however
    // `publish` built them -- the first version of this test passed with the
    // copy put back. Something has to be published in between, so the message
    // below is what makes the second snapshot a new one.
    alice.send(Cmd::Send("and a word after it".into()));
    assert!(
        until(
            || bob
                .state()
                .lines
                .iter()
                .any(|l| l.text == "and a word after it"),
            30
        )
        .await,
        "the second message should arrive, or nothing was republished"
    );

    let again = held(&bob);
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "every published state carries its own copy of the picture, so every \
         clone of one copies it again"
    );

    alice.stop();
    bob.stop();
}

/// A conversation somebody left does not come back at the next launch.
///
/// The list is folded from this machine's own copy before the exchange is
/// asked anything, so anything the store still calls a conversation is one --
/// including channels left, closed, or belonging to an exchange whose log was
/// wiped and rebuilt. That showed as **two conversations with the same name**,
/// one of them ten days stale, for the second before the exchange answered.
///
/// The messages stay on the disc; it is the row that makes it a conversation
/// that goes.
#[tokio::test]
async fn a_conversation_that_was_left_does_not_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(11);
    let store_path = dir.path().join("a.db");

    let alice = start_at(endpoint, a_signer, &store_path);
    assert!(
        until(|| alice.state().me == Some(a_id), 15).await,
        "the session should come up: {:?}",
        alice.state().trouble
    );
    alice.send(Cmd::NewGroup("a room to leave".into()));
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.label == "a room to leave"),
            15
        )
        .await,
        "the group should appear: {:?}",
        alice.state().conversations
    );
    alice.send(Cmd::Leave);
    assert!(
        until(
            || !alice
                .state()
                .conversations
                .iter()
                .any(|c| c.label == "a room to leave"),
            15
        )
        .await,
        "leaving should take it off the list: {:?}",
        alice.state().conversations
    );
    alice.stop();

    // **Asked of the disc, not of the next session.** What the fold would find
    // is what is written down, and a flicker on the way in is over in less
    // time than any polling loop can see -- a test that watched for it would
    // pass whether or not the row was still there.
    let (signer, _) = signer(11);
    let seed = signer.seed();
    let mut store = None;
    // The flock goes when the session's task actually finishes, which is not
    // the instant `stop` returns.
    for _ in 0..50 {
        match sqex_chat::store::Store::open(&seed, Some(&store_path)) {
            Ok(open) => {
                store = Some(open);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut store = store.expect("the store, once the session has let go of it");
    store.scope_to(&PubKey::new(server_pub)).unwrap();
    let left: Vec<String> = store
        .channels()
        .unwrap()
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert!(
        !left.iter().any(|l| l == "a room to leave"),
        "the disc still calls it a conversation, so the next launch will list \
         it and the one after that: {left:?}"
    );
}

/// A conversation is drawn from this machine's own copy, not from the
/// exchange.""
///
/// The exchange is **stopped** before the conversation is reopened, so
/// anything that appears afterwards can only have come off the disc. That is
/// also the only place it could come from in earnest: opening an epoch key
/// spends the prekey it was sealed against, so what this client wrote down is
/// the only copy of these messages that can ever be read.
///
/// # What this does not pin
///
/// **Not the speed.** Publishing on `Cmd::Show` puts the history up the
/// instant a conversation is opened rather than at the end of the next
/// refresh, and this test passes either way -- the refresh republishes from
/// the same folded history a tick later. Distinguishing them means winning a
/// race against a 700ms tick, and a test that has to win a race is a test that
/// fails on a slow morning. What is pinned here is the property underneath
/// both: the transcript does not need the exchange.
#[tokio::test]
async fn a_conversation_is_read_from_the_disc_with_the_exchange_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, server) = server_in(dir.path()).await;
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
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    alice.send(Cmd::Send("written down here".into()));
    assert!(
        until(
            || alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "written down here"),
            20
        )
        .await,
        "the message should be in the transcript first: {:?}",
        alice.state().trouble
    );
    let channel = alice.state().open.expect("a conversation is open");

    // Away from it, so the transcript is genuinely empty...
    alice.send(Cmd::Close);
    assert!(
        until(|| alice.state().lines.is_empty(), 10).await,
        "closing should empty the transcript"
    );
    // ...and the exchange is gone, so nothing can be fetched back.
    server.abort();
    bob.stop();

    alice.send(Cmd::Show(channel));
    let back = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "written down here")
        },
        20,
    )
    .await;
    assert!(
        back,
        "opening a conversation showed nothing while the exchange was unreachable, \
         though every message in it is on this disc: {:?}",
        alice.state().trouble
    );

    alice.stop();
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

/// A message from somebody who is not already a contact must appear.
///
/// # What this is really testing
///
/// The conversation list used to be built from `store().contacts()` — a purely
/// local note-to-self about who exists. A stranger is by definition not in it,
/// so their conversation had no row, and a message in it was invisible: not
/// late, not an error, simply absent, with nothing on screen suggesting
/// anything had happened.
///
/// The fix is that the list comes from `Chat::mine()`, which is the exchange's
/// answer to "what am I in" and **the only way to learn about a channel nobody
/// told us about**. Bob never adds Alice here, deliberately: adding her is
/// exactly the step that used to be required and must not be.
#[tokio::test]
async fn a_message_from_a_stranger_appears_without_adding_them_first() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(3);
    let (b_signer, b_id) = signer(4);

    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));

    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    // Bob opens nothing and adds nobody. He publishes prekeys by existing,
    // which is all SIP-23 needs for Alice to be able to seal to him.
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(|| alice.state().open.is_some(), 15).await,
        "Alice should have the conversation open: {:?}",
        alice.state().trouble
    );
    alice.send(Cmd::Send("out of the blue".into()));

    // Bob's list has to grow a row for a person he has never heard of, and it
    // has to happen promptly rather than when the periodic rebuild comes
    // round. Those are different mechanisms and only one of them is fast
    // enough to be a chat client, so the wait is pinned well inside the
    // backstop -- and asserted against it, so that lowering the backstop
    // cannot quietly turn this back into a test of the backstop.
    //
    // **Which of the two prompt paths carries it is a race, and both are
    // required.** If Bob's subscription was open when Alice created the
    // channel, SIP-30's `Membership` event says so; if it was not -- and at
    // startup it usually is not, because the conversation is created in the
    // same second the two sessions come up -- the row arrives from `mine()`
    // and the message from the sweep that asks about anything the exchange has
    // never answered for. Measured, not assumed: with that sweep removed this
    // fails, having received nothing but a `Cursor` and a heartbeat.
    const WAIT: u64 = 10;
    assert!(
        std::time::Duration::from_secs(WAIT) * 2 < sigil_chat::session::BACKSTOP,
        "this test no longer proves anything but the backstop: it waits {WAIT}s against a backstop of {:?}",
        sigil_chat::session::BACKSTOP
    );
    let listed = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id))
        },
        WAIT,
    )
    .await;
    assert!(
        listed,
        "a conversation with a stranger must appear in the list: {:?}",
        bob.state().conversations
    );

    // And it has to be counted, or the badge that brings somebody back to the
    // window is permanently zero — which it was.
    let counted = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id) && c.unread > 0)
        },
        25,
    )
    .await;
    assert!(
        counted,
        "and be counted unread while it is not on screen: {:?}",
        bob.state().conversations
    );

    // Opening it is what clears the count.
    let channel = bob
        .state()
        .conversations
        .iter()
        .find(|c| c.peer == Some(a_id))
        .map(|c| c.channel)
        .unwrap();
    bob.send(Cmd::Show(channel));
    let read = until(
        || {
            let s = bob.state();
            s.open == Some(channel)
                && s.lines.iter().any(|l| l.text == "out of the blue")
                && s.conversations
                    .iter()
                    .all(|c| c.peer != Some(a_id) || c.unread == 0)
        },
        25,
    )
    .await;
    assert!(read, "opening it shows it and clears it: {:?}", bob.state());

    alice.stop();
    bob.stop();
}

/// What was said while the client was shut down is on screen when it starts.
///
/// **SIP-30's stream has no replay.** It carries what happens from the moment
/// it is opened, so everything said while this account was not running is news
/// no event will ever mention. Nothing else asked either: conversations were
/// fetched when an event named them or when somebody opened them, and a client
/// that had been closed overnight therefore showed yesterday's last word and an
/// unread count of nothing until each conversation was clicked into.
///
/// So the list is asked about once per session — see `ask_about_unfetched` —
/// and this is that, from the far side: Bob reads a message, is closed, misses
/// one, and comes back to find it counted without opening anything.
#[tokio::test]
async fn what_was_said_while_it_was_closed_is_counted_when_it_starts() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(51);
    let (b_signer, b_id) = signer(52);
    let bobs_store = dir.path().join("b.db");

    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &bobs_store);
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(|| alice.state().open.is_some(), 15).await,
        "Alice should have the conversation open: {:?}",
        alice.state().trouble
    );
    alice.send(Cmd::Send("while you were in".into()));

    // Bob has to have the conversation on his disc before he can be closed
    // with it there, which is what the second half of this tests. Counted
    // rather than merely listed: a count means it was fetched and stored.
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id) && c.unread > 0),
            15
        )
        .await,
        "Bob should have been told the first message: {:?}",
        bob.state().conversations
    );

    // Closed, and *finished* closing: the store's lock is released when the
    // task's future is dropped, not when it is asked to stop, and the session
    // started below would be refused it.
    let closing = bob.close();
    assert!(
        until(|| closing.is_finished(), 15).await,
        "Bob's session should stop"
    );

    alice.send(Cmd::Send("while you were out".into()));
    assert!(
        until(|| alice.state().lines.len() == 2, 15).await,
        "Alice should have said both: {:?}",
        alice.state().lines
    );

    // Nothing will announce this one: it was said to a client with no stream
    // open, and the stream Bob opens now begins where it is opened.
    let (b_signer, _) = signer(52);
    let bob = start_at(endpoint, b_signer, &bobs_store);

    // Pinned well inside the backstop, or this proves only that the periodic
    // rebuild eventually comes round -- which it did, half a minute later,
    // and which is not a chat client.
    const WAIT: u64 = 10;
    assert!(
        std::time::Duration::from_secs(WAIT) * 2 < sigil_chat::session::BACKSTOP,
        "this test no longer proves the startup sweep: it waits {WAIT}s against a backstop of {:?}",
        sigil_chat::session::BACKSTOP
    );
    let counted = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id) && c.unread > 0)
        },
        WAIT,
    )
    .await;
    assert!(
        counted,
        "the message said while Bob was closed must be counted without opening it: {:?}",
        bob.state().conversations
    );

    alice.stop();
    bob.stop();
}

/// A private group: created, invited to, and read by the person invited.
///
/// A group's identifier is **random rather than derived**, so there is nothing
/// to compute and nothing to guess — the only way the invitee learns it exists
/// is `Chat::mine()`, reached because SIP-30 said a membership changed. This
/// is the same path as the stranger test and a different shape of it: there,
/// the channel was derivable and we chose not to rely on that; here it is not
/// derivable at all.
#[tokio::test]
async fn a_group_is_created_invited_to_and_read() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(5);
    let (b_signer, b_id) = signer(6);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::NewGroup("release check".into()));
    let made = until(
        || {
            alice
                .state()
                .conversations
                .iter()
                .any(|c| c.group && c.public == Some(false))
        },
        15,
    )
    .await;
    assert!(
        made,
        "the group should appear in its creator's list: {:?}",
        alice.state().conversations
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.group && c.public == Some(false))
        .map(|c| c.channel)
        .unwrap();

    // Alice is its admin, which is what lets her invite. The exchange attests
    // this, unlike anything in a profile.
    assert!(
        until(
            || alice.state().open == Some(channel) && alice.state().i_am_admin,
            15
        )
        .await,
        "its creator administers it: {:?}",
        alice.state().members
    );

    alice.send(Cmd::Invite(b_id));
    let joined = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.channel == channel)
        },
        15,
    )
    .await;
    assert!(
        joined,
        "the invitee learns of a group they could not have guessed: {:?}",
        bob.state().conversations
    );

    // And can read what is said in it. Inviting seals them the epoch in force,
    // which is what grants the history rather than only the future.
    alice.send(Cmd::Send("in the group".into()));
    bob.send(Cmd::Show(channel));
    let read = until(
        || bob.state().lines.iter().any(|l| l.text == "in the group"),
        20,
    )
    .await;
    assert!(read, "and can read it: {:?}", bob.state().lines);

    // A direct message shows none of this. The membership *is* the channel --
    // made on the first word, both of you added, key rotated -- and putting
    // three lines of machinery at the head of every one-to-one conversation
    // buries what was actually said.
    //
    // Asserted here rather than in a test of its own, because the same run has
    // both kinds open and the interesting property is the **difference**: a
    // filter that suppressed everything, or nothing, would pass one half.
    bob.send(Cmd::OpenDm(a_id));
    let dm = until(
        || {
            bob.state().open.is_some_and(|c| {
                bob.state()
                    .conversations
                    .iter()
                    .any(|s| s.channel == c && s.peer == Some(a_id))
            })
        },
        15,
    )
    .await;
    // Not an `if let`: without this the assertion below would be skipped and
    // the test would pass having checked nothing at all.
    assert!(dm, "the direct message should open: {:?}", bob.state().open);
    assert!(
        !bob.state()
            .events
            .iter()
            .any(|e| e.said.contains("made this channel") || e.said.contains("added")),
        "a direct message opens with its own plumbing on screen: {:?}",
        bob.state().events
    );
    bob.send(Cmd::Show(channel));
    assert!(until(|| bob.state().open == Some(channel), 15).await);

    // And the group's own history: the exchange signs an entry for creating it
    // and for every member added, and both belong in the conversation.
    //
    // **Against a real exchange rather than a fixture.** The whole chain has
    // to hold -- sqexd writes the entry, the client decodes SIP-16's `System`
    // layout instead of dropping it, and the session puts it into words -- and
    // a test that built the event itself would prove only the last step.
    let told = until(
        || {
            let s = alice.state();
            s.events
                .iter()
                .any(|e| e.said.contains("made this channel"))
                && s.events.iter().any(|e| e.said.contains("added"))
        },
        20,
    )
    .await;
    assert!(
        told,
        "the channel's own record is missing: {:?}",
        alice.state().events
    );
    // Named, and with the account it names carried alongside -- a name is an
    // assertion (SIP-21) and the key is what the exchange actually recorded.
    let added = alice
        .state()
        .events
        .iter()
        .find(|e| e.said.contains("added"))
        .cloned()
        .expect("checked above");
    assert_eq!(added.subject, b_id, "{added:?}");
    assert!(
        added.said.contains("you"),
        "the actor is us, and reading our own name back is a puzzle: {added:?}"
    );

    alice.stop();
    bob.stop();
}

/// A busy conversation opens on its last page, not on all of it.
///
/// Opening a channel used to build a drawable line for **every message it had
/// ever carried**, again on every poll, and the interface then cloned the
/// whole vector twice a frame. On a public channel a few thousand deep that is
/// seconds of work to show a screenful, and it was the first thing anybody
/// noticed about a busy room.
///
/// Nothing is dropped: the fold still folds all of it and the store still
/// holds it, which is what `earlier` counts and what asking for more reaches.
#[tokio::test]
async fn a_long_conversation_opens_on_a_page_and_reaches_back() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(21);
    let (b_signer, b_id) = signer(22);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::OpenDm(b_id));
    bob.send(Cmd::OpenDm(a_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open"
    );

    // One more than a page, so the window is doing something and the number
    // behind it is exactly known.
    let sent = sigil_chat::session::PAGE + 10;
    for i in 0..sent {
        alice.send(Cmd::Send(format!("message {i}")));
    }
    let all_there = until(
        || alice.state().lines.len() + alice.state().earlier >= sent,
        60,
    )
    .await;
    assert!(
        all_there,
        "the exchange should have taken all of them: {} shown, {} behind",
        alice.state().lines.len(),
        alice.state().earlier
    );

    let s = alice.state();
    assert!(
        s.lines.len() <= sigil_chat::session::PAGE,
        "a conversation opens on a page, not on all of it: {} lines",
        s.lines.len()
    );
    assert!(
        s.earlier > 0,
        "and says how many are behind it: {:?}",
        s.earlier
    );
    // The newest are the ones on screen. A page taken off the *front* would
    // open a busy channel on its oldest messages, which is nobody's idea of
    // opening a conversation.
    assert!(
        s.lines
            .last()
            .is_some_and(|l| l.text == format!("message {}", sent - 1)),
        "the page is the end of the conversation: {:?}",
        s.lines.last().map(|l| l.text.clone())
    );

    // And the rest is reachable.
    let behind = s.earlier;
    alice.send(Cmd::Earlier);
    let reached = until(|| alice.state().earlier < behind, 20).await;
    assert!(
        reached,
        "asking for earlier messages produced none: still {} behind",
        alice.state().earlier
    );

    // A search result reaches its message in one hop. Back to one page
    // first, so there is something to reach past.
    let channel = alice.state().open.unwrap();
    alice.send(Cmd::Show(channel));
    assert!(
        until(
            || alice.state().earlier == sent - sigil_chat::session::PAGE,
            20
        )
        .await,
        "reopening should put the window back to a page: {} behind",
        alice.state().earlier
    );
    alice.send(Cmd::Search("MESSAGE 5".into()));
    assert!(
        until(|| alice.state().searched_messages, 20).await,
        "the search should answer"
    );
    let hits = alice.state().hits.clone();
    let hit = hits
        .iter()
        .find(|h| h.text == "message 5")
        .unwrap_or_else(|| panic!("the fifth message is held and should match: {hits:?}"))
        .clone();
    assert_eq!(hit.who, "You", "our own message is ours");
    assert_eq!(&hit.text[hit.found.clone()], "message 5", "case aside");
    assert!(
        !alice.state().lines.iter().any(|l| l.seq == hit.seq),
        "the fifth message must be behind the page for this to test anything"
    );
    alice.send(Cmd::ShowAt {
        channel: hit.channel,
        seq: hit.seq,
    });
    let reached = until(|| alice.state().lines.iter().any(|l| l.seq == hit.seq), 20).await;
    assert!(
        reached,
        "going to a search result should widen the window to it, without \
         being asked for pages: {} behind",
        alice.state().earlier
    );
    assert_eq!(
        alice.state().open,
        Some(channel),
        "and it is the same conversation, still open"
    );

    alice.stop();
    bob.stop();
}

/// A public channel is findable by somebody who was never told about it.
///
/// The directory is the **only** channel route open to anybody, because every
/// other one names an identifier and an identifier is not an authorisation.
#[tokio::test]
async fn a_public_channel_is_found_in_the_directory_and_joined() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(7);
    let (b_signer, b_id) = signer(8);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::NewPublic {
        name: "the square".into(),
        topic: "anybody at all".into(),
    });
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.public == Some(true)),
            15
        )
        .await,
        "the public channel should appear for its creator: {:?}",
        alice.state().conversations
    );

    // Bob has never heard of it and holds no identifier for it.
    bob.send(Cmd::Find("square".into()));
    let listed = until(
        || bob.state().found.iter().any(|f| f.name == "the square"),
        15,
    )
    .await;
    assert!(
        listed,
        "the directory is how somebody finds a room nobody told them about: {:?}",
        bob.state().found
    );

    let found = bob
        .state()
        .found
        .into_iter()
        .find(|f| f.name == "the square")
        .unwrap();
    // The incarnation comes from the directory row and has to: SIP-31 binds it
    // into the signature, and `Info` requires the membership being acquired.
    bob.send(Cmd::Join {
        channel: found.channel,
        instance: found.instance,
    });
    let joined = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.channel == found.channel && c.public == Some(true))
        },
        15,
    )
    .await;
    assert!(
        joined,
        "and joining it is what lets it be read: {:?}",
        bob.state().conversations
    );

    alice.stop();
    bob.stop();
}

/// A private channel is not merely un-joinable, it is unmentionable.
///
/// Answering "not public" would reopen, on the join route, exactly the
/// existence oracle every read path closes. So a stranger searching the
/// directory must not find a group at all — not as a refusal, not as a row
/// they cannot use.
#[tokio::test]
async fn a_private_group_never_appears_in_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(9);
    let (b_signer, b_id) = signer(10);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::NewGroup("private business".into()));
    // A public one beside it, so this test can tell "the directory does not
    // carry private channels" from "the search returned nothing at all".
    // Without it the assertion below passes when the search is simply broken,
    // which is the shape a security test fails in silently.
    alice.send(Cmd::NewPublic {
        name: "open house".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || {
                let c = alice.state().conversations;
                c.iter().any(|c| c.group && c.public == Some(false))
                    && c.iter().any(|c| c.public == Some(true))
            },
            15
        )
        .await,
        "both exist: {:?}",
        alice.state().conversations
    );

    // An empty query lists everything the directory carries (SIP-16).
    bob.send(Cmd::Find(String::new()));
    assert!(
        until(
            || bob.state().found.iter().any(|f| f.name == "open house"),
            15
        )
        .await,
        "the search works at all -- without this the assertion below is vacuous: {:?}",
        bob.state().found
    );
    // Given a moment in case it were to arrive late, which would be worse than
    // never: a leak that is merely slow is still a leak.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !bob.state()
            .found
            .iter()
            .any(|f| f.name == "private business"),
        "a private group must not be in the public directory: {:?}",
        bob.state().found
    );

    alice.stop();
    bob.stop();
}

/// A call rings in the conversation, and refusing it is recorded.
///
/// # What replaced what
///
/// sigil used to ring over the SIP-5 mailbox, polled every two seconds,
/// because when that was written SIP-30 had **no event kind for a call**. It
/// has one now, and this is the ring arriving on the stream chat already holds
/// open, with no polling and no second mechanism. The mailbox listener is
/// gone, so there is exactly one way a call can ring and no way to ring twice.
///
/// Measured rather than assumed: this passes with `Event::Ringing` ignored and
/// with `Event::Channel` ignored, and fails only when **both** are — an
/// invitation is also an ordinary entry, so two kinds announce it and either
/// one is enough.
///
/// The wait is pinned well inside `BACKSTOP` for the same reason the stranger
/// test is: a ring that only arrived when the periodic rebuild came round
/// would be a ring nobody answered.
#[tokio::test]
async fn a_call_rings_in_the_conversation_and_declining_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(11);
    let (b_signer, b_id) = signer(12);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    // A conversation to ring in. SIP-36 rings *in a channel*, which is the one
    // thing the mailbox did not need — and the reason retiring it is safe is
    // that both parties here are chat clients and so have published prekeys.
    //
    // **Bob opens nothing.** That is the whole point: the open conversation
    // was polled every tick whatever happened, so a test where the callee is
    // already looking at the conversation proved only that polling works. It
    // passed with every event handler disabled, which is how I found out.
    // Ringing has to reach somebody who is looking somewhere else. (Nothing is
    // polled on a timer any more, which makes the distinction moot and the
    // test no worse.)
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(|| alice.state().open.is_some(), 15).await,
        "the caller should have the conversation open"
    );
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id)),
            15
        )
        .await,
        "and the callee should know the conversation exists: {:?}",
        bob.state().conversations
    );
    assert_eq!(bob.state().open, None, "while having nothing open");

    const WAIT: u64 = 10;
    assert!(
        std::time::Duration::from_secs(WAIT) * 2 < sigil_chat::session::BACKSTOP,
        "this test no longer proves the event path"
    );

    alice.send(Cmd::Call { direct: true });
    let rang = until(
        || {
            bob.state()
                .ringing
                .iter()
                .any(|r| r.from == a_id && !r.mine)
        },
        WAIT,
    )
    .await;
    assert!(
        rang,
        "the call should ring for the person called: {:?}",
        bob.state().ringing
    );

    let ring = bob
        .state()
        .ringing
        .into_iter()
        .find(|r| r.from == a_id)
        .unwrap();
    // The invitation carries the room secret, and that is the whole of what
    // joining the audio needs — which is also why it is a bearer capability.
    assert_ne!(ring.secret, [0u8; 32], "the invitation carries a room");
    // And that the caller will ask for an introduction, and who the other
    // person is -- which is who a direct connection would be made to.
    assert!(
        ring.direct,
        "the invitation says the caller will ask to be introduced"
    );
    assert_eq!(
        ring.peer,
        Some(a_id),
        "a direct message names its other party"
    );

    // Alice sees her own as outgoing rather than as something to answer.
    assert!(
        until(|| alice.state().ringing.iter().any(|r| r.mine), WAIT).await,
        "the caller sees it as their own: {:?}",
        alice.state().ringing
    );

    bob.send(Cmd::Decline {
        channel: ring.channel,
        seq: ring.seq,
    });

    // A refusal is a durable entry, not only a signal: a caller who was not
    // listening at that instant still learns the call was refused. Once it is
    // written the call has an outcome, so it stops ringing for both.
    let settled = until(
        || alice.state().ringing.is_empty() && bob.state().ringing.is_empty(),
        WAIT,
    )
    .await;
    assert!(
        settled,
        "a declined call stops ringing for both sides: alice={:?} bob={:?}",
        alice.state().ringing,
        bob.state().ringing
    );

    // And it stays in the conversation. `Timeline` folds an invitation into a
    // `CallRecord` rather than a message, so a call that is over leaves the
    // transcript with nothing in it -- somebody scrolling back finds a silence
    // where a conversation was, and cannot tell a call that was refused from
    // one that never happened.
    let recorded = until(
        || {
            alice
                .state()
                .events
                .iter()
                .any(|e| e.said.contains("declined"))
        },
        WAIT,
    )
    .await;
    assert!(
        recorded,
        "the caller keeps a record of the refusal: {:?}",
        alice.state().events
    );

    alice.stop();
    bob.stop();
}

/// "Typing…" stops, with nothing sent to say that it has.
///
/// Nothing is polled on a timer any more: every kind of news arrives as a
/// SIP-30 event, and a stream that stops carrying them is noticed and
/// resubscribed. A typing signal is the exception that proves it. SIP-19
/// relays it and stores it nowhere, the exchange lets it lapse, and **a lapse
/// has no event** — so a client that stopped asking would latch the indicator
/// on and leave a conversation nobody had touched for an hour saying somebody
/// was writing in it.
///
/// So the one conversation that says somebody is typing is asked about until
/// it stops saying so, and only until then. See `still_live`.
///
/// Alice never sends the *stopped* signal here, deliberately: a client that
/// says so is not the case worth testing — a window that was closed, a laptop
/// that was shut, and a process that was killed all say nothing at all.
#[tokio::test]
async fn typing_stops_looking_like_typing() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(53);
    let (b_signer, b_id) = signer(54);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

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

    alice.send(Cmd::Typing(true));
    assert!(
        until(|| bob.state().typing, 10).await,
        "the other side should see that somebody is writing"
    );

    // And then nothing: no further signal, and above all no `Typing(false)`.
    assert!(
        until(|| !bob.state().typing, 10).await,
        "the indicator must go out on its own when the signals stop"
    );

    alice.stop();
    bob.stop();
}

/// A call that has been answered stops being drawn as one still ringing.
///
/// **Answering writes nothing.** SIP-36 is deliberate that a durable outcome
/// must not come from a signal, and taking a call is not an outcome — the
/// entry comes when it ends. So the only thing that ever says a call was
/// picked up is `Conversation::accepted`, which arrives with a fetch.
///
/// Without it the caller shows a ringing phone for the whole ring window and
/// then writes *missed* of a call that is being spoken on.
#[tokio::test]
async fn an_answered_call_stops_ringing_for_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(55);
    let (b_signer, b_id) = signer(56);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(|| alice.state().open.is_some(), 15).await,
        "the caller should have the conversation open"
    );
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(a_id)),
            15
        )
        .await,
        "and the callee should know the conversation exists"
    );

    alice.send(Cmd::Call { direct: false });
    assert!(
        until(
            || bob
                .state()
                .ringing
                .iter()
                .any(|r| r.from == a_id && !r.mine),
            10
        )
        .await,
        "the call should ring for the person called: {:?}",
        bob.state().ringing
    );
    let ring = bob
        .state()
        .ringing
        .into_iter()
        .find(|r| r.from == a_id)
        .unwrap();
    // A caller that will not ask to be introduced says so by saying nothing.
    assert!(!ring.direct, "no introduction was offered");

    bob.send(Cmd::Answer {
        channel: ring.channel,
        seq: ring.seq,
    });

    // Pinned inside the ring window: a caller told after it has passed has
    // already drawn the call as missed, which is the failure this prevents.
    let answered = until(
        || alice.state().ringing.iter().any(|r| r.mine && r.answered),
        20,
    )
    .await;
    assert!(
        answered,
        "the caller should be told the call was taken: {:?}",
        alice.state().ringing
    );

    // The other side hangs up, and the caller's state says so by the
    // conversation and the ring -- which is what lets the caller's window
    // leave a call the path has not yet noticed is over.
    assert!(
        !alice.state().over.contains(&(ring.channel, ring.seq)),
        "the call is not over yet"
    );
    bob.send(Cmd::Hangup {
        channel: ring.channel,
        seq: ring.seq,
        seconds: 3,
    });
    assert!(
        until(
            || alice.state().over.contains(&(ring.channel, ring.seq)),
            20
        )
        .await,
        "the caller should learn the call is over: {:?}",
        alice.state().over
    );

    alice.stop();
    bob.stop();
}

/// A file sent through a real exchange, and read back on the other side.
///
/// The blob is sealed with a key of its own before it leaves, and its name is
/// the SHA-256 of the **ciphertext** — so the exchange verifies a name for
/// bytes it cannot read, and `download` checks that name before decrypting
/// anything. What arrives is what was named, or nothing.
#[tokio::test]
async fn a_file_is_sent_and_arrives_intact() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(13);
    let (b_signer, b_id) = signer(14);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open"
    );

    // Deliberately not an image: this is about the blob path, and a file that
    // needs no decoder cannot fail for a decoder's reasons.
    let payload: Vec<u8> = (0..5000u32).map(|n| (n % 251) as u8).collect();
    let path = dir.path().join("notes.bin");
    std::fs::write(&path, &payload).unwrap();
    alice.send(Cmd::SendFile(path));

    let arrived = until(
        || bob.state().lines.iter().any(|l| !l.attachments.is_empty()),
        25,
    )
    .await;
    assert!(
        arrived,
        "the file should reach the other side: {:?}",
        bob.state().lines
    );

    let line = bob
        .state()
        .lines
        .into_iter()
        .find(|l| !l.attachments.is_empty())
        .unwrap();
    let file = &line.attachments[0];
    assert_eq!(file.size, payload.len() as u64, "the size is carried");
    assert!(
        file.described.contains("kB") || file.described.contains("B"),
        "and it is described in words a reader can use: {}",
        file.described
    );

    // And the bytes themselves come back exactly, through the same path the
    // Save control uses.
    let out = dir.path().join("out.bin");
    bob.send(Cmd::SaveFile {
        seq: line.seq,
        index: 0,
        to: out.clone(),
    });
    let written = until(|| out.exists(), 25).await;
    assert!(
        written,
        "saving it writes it out: {:?}",
        bob.state().trouble
    );
    assert_eq!(
        std::fs::read(&out).unwrap(),
        payload,
        "and what comes back is byte for byte what was sent"
    );

    alice.stop();
    bob.stop();
}

/// Claiming a SIP-38 name from the client, and being told what came back.
///
/// **A refusal is an answer.** Whether anybody may take a free name is the
/// operator's policy, and "somebody else has it" and "this exchange assigns
/// them itself" want opposite things from whoever asked — so each comes back
/// in its own words rather than as one failure.
#[tokio::test]
async fn a_name_is_claimed_and_a_taken_one_is_said_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_with(dir.path(), "open").await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    let (a_signer, a_id) = signer(31);
    let (b_signer, b_id) = signer(32);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::ClaimName("ada".into()));
    assert!(
        until(
            || alice
                .state()
                .note
                .is_some_and(|n| n.said.contains("ada is yours here")),
            20
        )
        .await,
        "the claim was not granted, or not said: {:?}",
        alice.state().note
    );

    // The handle that follows a claim is **not** asserted here, and cannot be:
    // this dials an address, and `name@203.0.113.1` is not a handle. Production
    // always resolves a domain first, which is where `set_domain` gets its
    // answer; `domain_of` in `sigil-net` covers picking it out of the layers,
    // and `a_claimed_name_becomes_a_handle` in `sqex-chat` covers the rest of
    // the chain with a domain in hand.

    // A confirmation is about something just done, so it stops being true.
    let gone = until(|| alice.state().note.is_none(), 20).await;
    assert!(
        gone,
        "the note is still on screen: {:?}",
        alice.state().note
    );

    bob.send(Cmd::ClaimName("ada".into()));
    assert!(
        until(
            || bob
                .state()
                .note
                .is_some_and(|n| n.said.contains("already somebody else")),
            20
        )
        .await,
        "a taken name reads as a failure rather than as taken: {:?} / {:?}",
        bob.state().note,
        bob.state().trouble
    );

    alice.stop();
    bob.stop();
}

/// A second device is named by one client and enrolled by the other.
///
/// The Devices screen could write a credential and had nowhere to present
/// one, so an account's second device could be named and never enrolled — and
/// a linked device is the only backup an epoch key can have, because opening
/// one spends the prekey it arrived under.
///
/// Both halves in one run, against a real exchange: writing it proves nothing
/// on its own, since the exchange is what decides whether it is honoured.
#[tokio::test]
async fn a_credential_written_by_one_device_enrols_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };

    // One account, two devices. `start_at` takes a signer, and a device *is* a
    // key — so the second session is simply a second key, which is what a
    // second machine would be.
    let (first_signer, first) = signer(41);
    let (second_signer, second) = signer(42);
    let one = start_at(endpoint, first_signer, &dir.path().join("one.db"));
    let two = start_at(endpoint, second_signer, &dir.path().join("two.db"));
    assert!(
        until(
            || one.state().me == Some(first) && two.state().me == Some(second),
            15
        )
        .await,
        "both should come up"
    );

    // The first writes a credential naming the second.
    one.send(Cmd::LinkDevice {
        device: second,
        days: 90,
    });
    let written = until(|| one.state().credential.is_some(), 20).await;
    assert!(
        written,
        "no credential was written: {:?}",
        one.state().trouble
    );
    let credential = one.state().credential.expect("checked above");

    // The second presents it, on its own connection.
    two.send(Cmd::RegisterSelf(credential.clone()));
    let enrolled = until(
        || {
            two.state()
                .note
                .is_some_and(|n| n.said.contains("acts for the account"))
        },
        20,
    )
    .await;
    assert!(
        enrolled,
        "the credential was refused: {:?} / {:?}",
        two.state().note,
        two.state().trouble
    );

    // And the exchange says so, which is the only word that counts.
    one.send(Cmd::Devices);
    let listed = until(
        || one.state().devices.iter().any(|d| d.device == second),
        20,
    )
    .await;
    assert!(
        listed,
        "the exchange does not hold the new device: {:?}",
        one.state().devices
    );

    one.stop();
    two.stop();
}

/// **SIP-47's pairing, both halves, and the day the claim went green.**
///
/// The Devices pane offers two ways in for a new device. One is "Use a
/// credential": the other device writes one, somebody carries it across,
/// and this device presents it -- `RegisterSelf`, which the test above
/// covers. The other is "Where the other device sent you": an `sqx-pair:`
/// string, and `claim_listed`, which finds *this* device already in the
/// account's list and takes the credential the registration carries.
///
/// The exchange always supported it -- `/device/register` takes a posting
/// from "the delegate itself, **or an already-registered device of the same
/// account**" -- but for as long as this test existed nothing posted the
/// second kind. `register_self` was the only caller anywhere, and it
/// registers the *caller*, so a sibling was never registered and the claim
/// could only succeed for a device that had already registered, which by
/// then did not need to claim. This test recorded that, asserted the
/// refusal, and said it would go green the day something registered the
/// sibling.
///
/// That is `Chat::register_device` (sqex 0.104.2), and `LinkDevice` calls
/// it after writing the credential. So now: the first device names the
/// second, the second is in the list before anybody carries anything, and
/// the second's claim -- told only where to go -- finds itself and takes the
/// account. The pairing somebody expects of a QR.
#[tokio::test]
async fn a_device_named_by_its_sibling_claims_the_account_by_name_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (first_signer, first) = signer(51);
    let (second_signer, second) = signer(52);
    let one = start_at(endpoint, first_signer, &dir.path().join("one.db"));
    let two = start_at(endpoint, second_signer, &dir.path().join("two.db"));
    assert!(
        until(
            || one.state().me == Some(first) && two.state().me == Some(second),
            15
        )
        .await,
        "both should come up"
    );

    // **The control: before anything is written, the claim is refused**, and
    // says why -- the account has not registered this device. Without this
    // the assertion below would pass for a device that could always claim.
    two.send(Cmd::ClaimAccount(first.to_string()));
    assert!(
        until(|| two.state().trouble.is_some(), 20).await,
        "the claim neither succeeded nor failed: {:?}",
        two.state().note
    );
    let said = two.state().trouble.unwrap_or_default();
    assert!(
        said.contains("has not registered this device"),
        "the refusal does not say what is missing: {said}"
    );

    // The first device names the second: a credential is written *and* the
    // second is registered, from the first's own connection.
    one.send(Cmd::LinkDevice {
        device: second,
        days: 90,
    });
    assert!(
        until(|| one.state().credential.is_some(), 20).await,
        "no credential was written: {:?}",
        one.state().trouble
    );
    assert!(
        until(
            || one.state().devices.iter().any(|d| d.device == second),
            20
        )
        .await,
        "the other device is not in the account's list, so nothing registered \
         it: {:?} / {:?}",
        one.state().devices,
        one.state().trouble
    );

    // **And the phone's half is the whole of it now**: told where to go and
    // nothing else, it finds itself listed and takes the account.
    two.send(Cmd::ClaimAccount(first.to_string()));
    assert!(
        until(|| two.state().linked == Some(true), 20).await,
        "the claim did not succeed: {:?} / {:?}",
        two.state().trouble,
        two.state().note
    );
    assert!(
        until(|| two.state().me == Some(first), 20).await,
        "the device claimed the account and still draws itself as its own key"
    );

    one.stop();
    two.stop();
}

/// A credential that is not one is refused where it was typed.
#[tokio::test]
async fn something_that_is_not_a_credential_is_refused_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(43);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(until(|| alice.state().me == Some(a_id), 15).await);

    alice.send(Cmd::RegisterSelf("not a credential".into()));
    let said = until(
        || {
            alice
                .state()
                .trouble
                .is_some_and(|t| t.contains("not a credential"))
        },
        20,
    )
    .await;
    assert!(said, "refused silently: {:?}", alice.state().trouble);
}

/// What kind of channel it is gets written down, so the next start knows.
///
/// # What this is for
///
/// A row says which kind of conversation it is -- a public channel anybody may
/// join and where nothing is encrypted, against a private group where the
/// opposite holds. That came only from the exchange, because the store
/// recorded group-or-not and nothing else, so **neither mark was drawn for the
/// first sweep of every start**: a wait, every time, for a fact that never
/// changes. sqex-chat v0.47 records it, and this is the half sigil owns --
/// putting the answer on the disc the moment the exchange gives one.
///
/// # Why it asks the disc rather than a second session
///
/// A second session would answer with whatever it had at the moment it was
/// asked, and the sweep fills the same field in within a second of connecting
/// -- so the test would pass whether or not anything had been written down. A
/// second path to the outcome steals the first path's test.
///
/// Restarting *without* the exchange would settle it, and cannot be done: a
/// session is built around a live connection (`Chat::new` takes one), so it
/// does not start at all while the exchange is unreachable, and nothing is
/// restored from the disc to look at.
#[tokio::test]
async fn what_kind_of_channel_it_is_is_written_down() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let store_path = dir.path().join("a.db");

    let (a_signer, a_id) = signer(9);
    let seed = a_signer.seed();
    let alice = start_at(endpoint, a_signer, &store_path);
    assert!(
        until(|| alice.state().me == Some(a_id), 15).await,
        "the session should come up: {:?}",
        alice.state().trouble
    );

    alice.send(Cmd::NewPublic {
        name: "the square".into(),
        topic: "anybody at all".into(),
    });
    alice.send(Cmd::NewGroup("the back room".into()));
    let named = |label: &'static str, public: Option<bool>| {
        alice
            .state()
            .conversations
            .iter()
            .any(|c| c.label == label && c.public == public && c.group)
    };
    assert!(
        until(
            || named("the square", Some(true)) && named("the back room", Some(false)),
            20
        )
        .await,
        "both channels should exist and the exchange should have said which is \
         which: {:?}",
        alice.state().conversations
    );
    alice.stop();

    // The flock goes when the session's task actually finishes, which is not
    // the instant `stop` returns.
    let mut store = None;
    for _ in 0..50 {
        match sqex_chat::store::Store::open(&seed, Some(&store_path)) {
            Ok(open) => {
                store = Some(open);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut store = store.expect("the store, once the session has let go of it");
    store.scope_to(&PubKey::new(server_pub)).unwrap();
    let on_disc = store.channels().unwrap();
    let of = |label: &str| {
        on_disc
            .iter()
            .find(|c| c.label == label)
            .unwrap_or_else(|| panic!("{label} is not on the disc at all: {on_disc:?}"))
            .clone()
    };

    assert_eq!(
        of("the square").public,
        Some(true),
        "the disc does not know the square is public, so the next start draws \
         no mark on it and somebody cannot tell a room anybody may read from \
         an encrypted group"
    );
    assert_eq!(
        of("the back room").public,
        Some(false),
        "nor that the back room is not"
    );
}

/// A picture sent into a public channel is fetched and published like any
/// other.
///
/// Reported from the desktop: an image sent to a public channel never
/// displayed. The library does this fine end to end, so whatever is wrong is
/// in this session -- and this is the session, driven the way the interface
/// drives it: create the channel, send the file, wait for bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_sent_to_a_public_channel_is_fetched_and_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(23);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(
        until(|| alice.state().me == Some(a_id), 15).await,
        "the session should come up: {:?}",
        alice.state().trouble
    );

    alice.send(Cmd::NewPublic {
        name: "pictures".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.label == "pictures" && c.public == Some(true)),
            15
        )
        .await,
        "the public channel should exist: {:?}",
        alice.state().conversations
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.label == "pictures")
        .map(|c| c.channel)
        .unwrap();
    alice.send(Cmd::Show(channel));
    assert!(
        until(|| alice.state().open == Some(channel), 10).await,
        "the channel should be open"
    );

    // A **real, busy** picture. A real one gets a thumbnail made of it and
    // carried inside the message, and a run of bytes that will not decode
    // does not -- the first version of this test sent the latter and passed.
    // And a *busy* one, because a smooth gradient's thumbnail is a few
    // kilobytes and passed too, while the pictures that failed in `general`
    // were a dithered GIF frame and a screenshot: their lossless previews were
    // over SIP-18's cap, and every reader refused the whole message.
    let picture = dir.path().join("a-picture.png");
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    image::RgbImage::from_fn(400, 300, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b = seed.to_le_bytes();
        image::Rgb([b[0], b[1], b[2]])
    })
    .save(&picture)
    .unwrap();
    alice.send(Cmd::SendFile(picture));

    // First the message with the attachment on it at all...
    let posted = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| !l.attachments.is_empty())
        },
        20,
    )
    .await;
    assert!(
        posted,
        "the message carrying the picture never appeared in the public channel: \
         lines {:?}, trouble {:?}",
        alice.state().lines.len(),
        alice.state().trouble
    );
    // ...and then its bytes, which is what draws it.
    let shown = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| l.attachments.iter().any(|a| a.bytes.is_some()))
        },
        30,
    )
    .await;
    assert!(
        shown,
        "the picture is on the message and its bytes never arrived, so it is \
         drawn as nothing: {:?}",
        alice.state().trouble
    );
    // And nothing in the channel is being called sealed: "not opened yet --
    // their key may still arrive" is what the report said, of a channel in
    // which nothing is ever sealed.
    let trouble = alice.state().trouble_with;
    assert_eq!(
        trouble.unreadable, 0,
        "a public channel says {} of its messages are waiting for a key",
        trouble.unreadable
    );
    alice.stop();
}

/// A video goes out with its poster frame, its shape and its length, and
/// comes in as a thumbnail that is fetched when -- and only when -- play is
/// pressed.
///
/// All three were missing: the sender made a thumbnail for pictures only,
/// and the session fetched pictures only, so a pressed video said
/// "fetching" for ever and was never kept on the disc. The fixture is
/// `sigil-video`'s two-second clip.
#[tokio::test(flavor = "multi_thread")]
async fn a_video_travels_with_a_poster_frame_and_is_fetched_when_pressed() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(35);
    let (b_signer, b_id) = signer(36);
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

    let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sigil-video/tests/fixtures/two_seconds.mp4");
    alice.send(Cmd::SendFile(clip));

    let video = |h: &ChatHandle| {
        h.state()
            .lines
            .iter()
            .flat_map(|l| l.attachments.clone())
            .find(|a| a.kind == sigil_ui::attachment::VIDEO)
    };
    assert!(
        until(|| video(&bob).is_some(), 20).await,
        "the video never reached Bob: {:?}",
        bob.state().trouble
    );
    let a = video(&bob).unwrap();
    assert!(!a.preview.is_empty(), "no poster frame travelled with it");
    assert!(
        a.preview.len() <= 8 * 1024,
        "the poster frame is {} bytes, over SIP-18's cap",
        a.preview.len()
    );
    assert_eq!(a.shape, Some((96, 64)), "the shape did not travel");
    assert!(
        a.duration_ms
            .is_some_and(|ms| (1_900..=2_100).contains(&ms)),
        "the length did not travel: {:?}",
        a.duration_ms
    );
    assert!(a.described.starts_with("[video 2s"), "{}", a.described);

    // Not fetched for being small, and not for being on the sender's disc:
    // the reader has to ask.
    assert!(
        !until(|| video(&bob).is_some_and(|a| a.bytes.is_some()), 3).await,
        "a video was fetched without play being pressed"
    );
    let seq = bob
        .state()
        .lines
        .iter()
        .find(|l| !l.attachments.is_empty())
        .map(|l| l.seq)
        .unwrap();
    bob.send(Cmd::Fetch { seq, index: 0 });
    assert!(
        until(|| video(&bob).is_some_and(|a| a.bytes.is_some()), 20).await,
        "pressed, the video should arrive: {:?}",
        bob.state().trouble
    );
    alice.stop();
    bob.stop();
}

/// A picture too big to fetch unasked is held as its thumbnail until the
/// reader asks; the sender, who has it on the disc already, sees it whole.
///
/// **A gif** is how this was found: an ordinary one is over the old cap, so
/// every reader's client kept its thumbnail under a caption that said
/// "fetching", and the sender's own client did the same with the whole file
/// sitting in its store. The reader's side is what `held` is for; the
/// sender's side is the store being asked before the cap is.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_over_the_cap_is_held_until_asked_for_and_the_sender_sees_it_anyway() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(31);
    let (b_signer, b_id) = signer(32);
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

    // Noise, so it will not compress: 3000 by 3000 of it is twenty-six
    // megabytes as a PNG, which is over the cap and under what the store
    // keeps.
    let picture = dir.path().join("big.png");
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    image::RgbImage::from_fn(3000, 3000, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b = seed.to_le_bytes();
        image::Rgb([b[0], b[1], b[2]])
    })
    .save(&picture)
    .unwrap();
    let size = std::fs::metadata(&picture).unwrap().len();
    assert!(
        size > 25 * 1024 * 1024 && size < 32 * 1024 * 1024,
        "the fixture must be over the fetch cap and under the keep cap: {size}"
    );
    alice.send(Cmd::SendFile(picture));

    // Alice sees her own picture whole: the store kept it on the way up.
    let hers = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| l.attachments.iter().any(|a| a.bytes.is_some()))
        },
        60,
    )
    .await;
    assert!(
        hers,
        "the sender should see the picture she sent: {:?}",
        alice.state().trouble
    );

    // Bob sees the thumbnail, marked as held, and nothing more arrives.
    let held = until(
        || {
            bob.state().lines.iter().any(|l| {
                l.attachments
                    .iter()
                    .any(|a| a.held && !a.preview.is_empty())
            })
        },
        30,
    )
    .await;
    assert!(
        held,
        "the reader should have the thumbnail and be told the rest is held: {:?}",
        bob.state().lines
    );
    let fetched = || {
        bob.state()
            .lines
            .iter()
            .any(|l| l.attachments.iter().any(|a| a.bytes.is_some()))
    };
    assert!(
        !until(fetched, 3).await,
        "a picture over the cap was fetched without being asked for"
    );

    // Then he asks, and it comes.
    let seq = bob
        .state()
        .lines
        .iter()
        .find(|l| !l.attachments.is_empty())
        .map(|l| l.seq)
        .unwrap();
    bob.send(Cmd::Fetch { seq, index: 0 });
    assert!(
        until(fetched, 60).await,
        "asked for, the picture should arrive: {:?}",
        bob.state().trouble
    );
    assert!(
        bob.state()
            .lines
            .iter()
            .flat_map(|l| l.attachments.iter())
            .all(|a| !a.held),
        "a fetched picture is no longer held"
    );
    alice.stop();
    bob.stop();
}

/// A message that will never open can be taken down from the notice, and the
/// notice then goes.
///
/// The whole loop through the real fold: somebody posts a message every
/// reader refuses -- a preview over SIP-18's cap, which is what happened in
/// `general` -- the session reports it as unreadable *and* as something this
/// identity may delete, and deleting it clears the count. The poster is a
/// plain library client, because sigil's own sender now sizes its previews
/// and cannot produce the fault.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_message_can_be_deleted_from_the_notice() {
    use sqex_chat::Chat;
    use sqex_chat::store::Store;
    use sqex_proto::message::{Part, Post as SipPost};

    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    // Alice runs sigil and founds the channel, so she administers it.
    let (a_signer, a_id) = signer(24);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(
        until(|| alice.state().me == Some(a_id), 15).await,
        "the session should come up: {:?}",
        alice.state().trouble
    );
    alice.send(Cmd::NewPublic {
        name: "pictures".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.label == "pictures"),
            15
        )
        .await
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.label == "pictures")
        .map(|c| c.channel)
        .unwrap();
    alice.send(Cmd::Show(channel));
    assert!(until(|| alice.state().open == Some(channel), 10).await);

    // Bob is a library client, joins, and posts what no reader will open.
    let sk = SigningKey::from_bytes(&[25u8; 32]);
    let (b_seed, b_id) = (sk.to_bytes(), PubKey::new(sk.verifying_key().to_bytes()));
    let client = sqnr::Client::connect_as(addr, &server_pub, &b_seed)
        .await
        .expect("bob connects");
    let store = Store::open(&b_seed, Some(&dir.path().join("b.db"))).unwrap();
    let mut bob = Chat::new(client, b_seed, b_id, PubKey::new(server_pub), store);
    let listing = bob.find("", 0).await.unwrap();
    let instance = listing
        .channels
        .iter()
        .find(|c| c.channel == channel)
        .map(|c| c.instance)
        .expect("the channel is in the directory");
    bob.join(&channel, instance).await.unwrap();
    let mut post = SipPost::text("look at this");
    post.parts
        .push(Part::Attachment(sqex_proto::blob::Attachment {
            kind: sqex_proto::blob::KIND_IMAGE,
            blob: [1u8; 32],
            key: [2u8; 32],
            size: 10,
            chunks: 1,
            mime: "image/png".into(),
            meta: Vec::new(),
            preview: vec![7u8; sqex_proto::blob::MAX_PREVIEW + 1],
        }));
    let seq = bob.send_post(&channel, post).await.unwrap().seq;

    // Alice's session reports it, and offers it.
    let offered = until(
        || {
            let t = alice.state().trouble_with;
            t.unreadable == 1 && t.redactable == vec![seq]
        },
        20,
    )
    .await;
    assert!(
        offered,
        "the unreadable message was not reported as something the admin may \
         delete: {:?}",
        alice.state().trouble_with
    );

    alice.send(Cmd::Redact(seq));
    let cleared = until(|| alice.state().trouble_with.unreadable == 0, 20).await;
    assert!(
        cleared,
        "deleting the unreadable message did not clear the notice: {:?}",
        alice.state().trouble_with
    );
    alice.stop();
}

/// A mention reaches the person named: counted with the unread on their
/// side, announced as live, drawn on the line with the key, and cleared by
/// reading -- and a mention of somebody else does none of that to them.
///
/// Against a real exchange, because the part has to survive the whole way:
/// sealed by Alice's client, stored, fetched and folded by Bob's, and read
/// out of the post rather than out of the text.
#[tokio::test]
async fn a_mention_reaches_the_person_named_and_reading_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(21);
    let (b_signer, b_id) = signer(22);
    let (c_signer, c_id) = signer(23);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    let carol = start_at(endpoint, c_signer, &dir.path().join("c.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id)
                && bob.state().me == Some(b_id)
                && carol.state().me == Some(c_id),
            15
        )
        .await
    );

    alice.send(Cmd::NewGroup("the room".into()));
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.group && c.public == Some(false)),
            15
        )
        .await
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.group && c.public == Some(false))
        .map(|c| c.channel)
        .unwrap();
    assert!(
        until(
            || alice.state().open == Some(channel) && alice.state().i_am_admin,
            15
        )
        .await
    );
    alice.send(Cmd::Invite(b_id));
    alice.send(Cmd::Invite(c_id));
    let both = || {
        [&bob, &carol]
            .iter()
            .all(|s| s.state().conversations.iter().any(|c| c.channel == channel))
    };
    assert!(until(both, 15).await, "both invitees learn of the room");
    // Each has the room in the list and answered for, so what comes next is
    // live rather than history.
    assert!(
        until(
            || [&bob, &carol].iter().all(|s| s
                .state()
                .conversations
                .iter()
                .any(|c| c.channel == channel && !c.waiting)),
            15
        )
        .await
    );

    // Alice mentions Bob. Neither Bob nor Carol has the room open.
    alice.send(Cmd::Post(session::Draft {
        text: "@Bob look at this".into(),
        mentions: vec![b_id],
        ..Default::default()
    }));
    let counted = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.channel == channel && c.unread == 1 && c.mentioned == 1)
        },
        20,
    )
    .await;
    assert!(
        counted,
        "the mention is counted with the unread on Bob's side: {:?}",
        bob.state().conversations
    );
    let announced: Vec<_> = bob
        .state()
        .arrivals
        .into_iter()
        .filter(|a| a.mentions_me)
        .collect();
    assert_eq!(announced.len(), 1, "{announced:?}");
    assert_eq!(announced[0].from, a_id);
    assert_eq!(announced[0].channel, channel);
    assert!(!announced[0].in_open);
    assert!(announced[0].said.contains("look at this"), "{announced:?}");

    // Carol was in the room and read the same message: unread, not mentioned.
    assert!(
        until(
            || carol
                .state()
                .conversations
                .iter()
                .any(|c| c.channel == channel && c.unread == 1),
            20
        )
        .await
    );
    let carols = carol
        .state()
        .conversations
        .iter()
        .find(|c| c.channel == channel)
        .map(|c| c.mentioned);
    assert_eq!(
        carols,
        Some(0),
        "a mention of Bob is not a mention of Carol"
    );
    // Muted, the room's waiting message is left off the count the icon
    // carries; unmuted it is on it. The row's own count is the summary's
    // and is untouched by either.
    assert_eq!(carol.unread(), 1);
    assert_eq!(
        carol.unread_but(|c| *c == channel),
        0,
        "muted: off the icon"
    );
    assert_eq!(
        carol.unread_but(|c| *c != channel),
        1,
        "another muted: still on"
    );

    // Carol got the message as an arrival all the same -- what is said
    // out loud while she is away -- naming who, where, and what.
    let carols_arrivals = carol.state().arrivals;
    assert_eq!(carols_arrivals.len(), 1, "{carols_arrivals:?}");
    assert!(!carols_arrivals[0].mentions_me);
    assert!(!carols_arrivals[0].direct, "a room, not a direct message");
    assert_eq!(carols_arrivals[0].channel, channel);
    assert!(
        carols_arrivals[0].said.contains("look at this"),
        "{carols_arrivals:?}"
    );
    // And Alice, who wrote it, has nothing arriving.
    assert!(
        alice.state().arrivals.is_empty(),
        "{:?}",
        alice.state().arrivals
    );

    // Reading clears the count; the line says who was mentioned, with the
    // key the part carried, and that it was us.
    bob.send(Cmd::Show(channel));
    let read = until(
        || {
            let s = bob.state();
            s.open == Some(channel)
                && s.lines.iter().any(|l| l.me_mentioned)
                && s.conversations
                    .iter()
                    .any(|c| c.channel == channel && c.mentioned == 0 && c.unread == 0)
        },
        20,
    )
    .await;
    assert!(
        read,
        "{:?} / {:?}",
        bob.state().lines,
        bob.state().conversations
    );
    let line = bob
        .state()
        .lines
        .iter()
        .find(|l| l.me_mentioned)
        .cloned()
        .unwrap();
    assert_eq!(line.mentions.len(), 1);
    assert_eq!(line.mentions[0].key, b_id);
    // And on Carol's screen the same line mentions Bob, not her.
    carol.send(Cmd::Show(channel));
    assert!(
        until(
            || carol
                .state()
                .lines
                .iter()
                .any(|l| !l.me_mentioned && l.mentions.iter().any(|m| m.key == b_id)),
            20
        )
        .await,
        "{:?}",
        carol.state().lines
    );
}

/// Words and several files go as one message: the reader's line has the
/// text and every attachment, in the order they were given, and each is
/// fetched. Not four messages for four files.
#[tokio::test]
async fn words_and_several_files_are_one_message() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(31);
    let (b_signer, b_id) = signer(32);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await
    );
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await
    );

    // Three pictures, told apart by their bytes.
    let mut files = Vec::new();
    for (n, fill) in [1u8, 2, 3].into_iter().enumerate() {
        let path = dir.path().join(format!("picture-{n}.png"));
        std::fs::write(&path, vec![fill; 8 * 1024]).unwrap();
        files.push(path);
    }
    alice.send(Cmd::Post(session::Draft {
        text: "three from the walk".into(),
        files: files.clone(),
        ..Default::default()
    }));
    let landed = until(
        || {
            bob.state().lines.iter().any(|l| {
                l.text == "three from the walk"
                    && l.attachments.len() == 3
                    && l.attachments.iter().all(|a| a.bytes.is_some())
            })
        },
        30,
    )
    .await;
    assert!(landed, "{:?}", bob.state().lines);
    let line = bob
        .state()
        .lines
        .iter()
        .find(|l| l.text == "three from the walk")
        .cloned()
        .unwrap();
    for (n, a) in line.attachments.iter().enumerate() {
        let bytes = a.bytes.as_ref().unwrap();
        assert_eq!(
            bytes[0],
            (n + 1) as u8,
            "the files arrive in the order given"
        );
        assert_eq!(bytes.len(), 8 * 1024);
    }
    assert_eq!(
        bob.state()
            .lines
            .iter()
            .filter(|l| !l.attachments.is_empty())
            .count(),
        1,
        "one message, not one per file: {:?}",
        bob.state().lines
    );

    // A reply to a picture quotes the picture: a real one this time, so
    // there is a thumbnail to carry, and no words, so the quote has to say
    // what it is.
    let real = dir.path().join("real.png");
    let img = image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 30, 30, 255]));
    image::DynamicImage::ImageRgba8(img).save(&real).unwrap();
    alice.send(Cmd::Post(session::Draft {
        files: vec![real],
        ..Default::default()
    }));
    let picture = until(
        || {
            bob.state().lines.iter().any(|l| {
                l.text.is_empty()
                    && l.attachments.len() == 1
                    && !l.attachments[0].preview.is_empty()
            })
        },
        30,
    )
    .await;
    assert!(
        picture,
        "the picture arrives with its thumbnail: {:?}",
        bob.state().lines
    );
    let target = bob
        .state()
        .lines
        .iter()
        .find(|l| l.text.is_empty() && l.attachments.len() == 1)
        .map(|l| l.seq)
        .unwrap();
    bob.send(Cmd::Post(session::Draft {
        text: "lovely".into(),
        reply: Some(target),
        ..Default::default()
    }));
    // The thumbnail is named by the blob it is of, so two quotes of two
    // pictures are never one picture.
    let blob = bob
        .state()
        .lines
        .iter()
        .find(|l| l.seq == target)
        .map(|l| l.attachments[0].id.clone())
        .unwrap();
    let quoted = until(
        || {
            bob.state().lines.iter().any(|l| {
                l.text == "lovely"
                    && l.reply_to.as_ref().is_some_and(|q| {
                        q.said == "a picture" && q.preview.as_ref().is_some_and(|t| t.id == blob)
                    })
            })
        },
        30,
    )
    .await;
    assert!(
        quoted,
        "the reply quotes the picture, by thumbnail and by name: {:?}",
        bob.state()
            .lines
            .iter()
            .find(|l| l.text == "lovely")
            .map(|l| &l.reply_to)
    );
}

/// A rewrite is a whole post, and the composer holds only the words: the
/// reply the message made and the files it carried have to come back from
/// the original, or correcting one letter unthreads the message and drops
/// its pictures. Both sides see the rewrite with the quote and the picture
/// still on it.
#[tokio::test]
async fn an_edit_keeps_the_reply_and_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(33);
    let (b_signer, b_id) = signer(34);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await
    );
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await
    );

    // A picture with a word on it, from Alice.
    let real = dir.path().join("real.png");
    let img = image::RgbaImage::from_pixel(64, 48, image::Rgba([30, 200, 30, 255]));
    image::DynamicImage::ImageRgba8(img).save(&real).unwrap();
    alice.send(Cmd::Post(session::Draft {
        text: "look".into(),
        files: vec![real],
        ..Default::default()
    }));
    let seen =
        |who: &ChatHandle, text: &str| who.state().lines.iter().find(|l| l.text == text).cloned();
    assert!(
        until(
            || seen(&bob, "look").is_some_and(|l| l.attachments.len() == 1),
            30
        )
        .await,
        "{:?}",
        bob.state().lines
    );
    let picture = seen(&bob, "look").unwrap().seq;

    // Bob answers it, then corrects his answer -- with a draft that says
    // nothing about the reply, as the composer's does.
    bob.send(Cmd::Post(session::Draft {
        text: "lovley".into(),
        reply: Some(picture),
        ..Default::default()
    }));
    assert!(
        until(
            || seen(&alice, "lovley").is_some_and(|l| l.reply_to.is_some()),
            30
        )
        .await,
        "{:?}",
        alice.state().lines
    );
    let answer = seen(&bob, "lovley").unwrap().seq;
    bob.send(Cmd::Post(session::Draft {
        text: "lovely".into(),
        edit: Some(answer),
        ..Default::default()
    }));
    let threaded = |l: &session::Line| {
        l.edited
            && l.reply_to
                .as_ref()
                .is_some_and(|q| q.seq == picture && q.said == "look" && q.preview.is_some())
    };
    assert!(
        until(
            || seen(&alice, "lovely").is_some_and(|l| threaded(&l))
                && seen(&bob, "lovely").is_some_and(|l| threaded(&l)),
            30
        )
        .await,
        "the rewrite still quotes the picture: alice {:?}, bob {:?}",
        seen(&alice, "lovely").map(|l| l.reply_to),
        seen(&bob, "lovely").map(|l| l.reply_to)
    );

    // And Alice corrects her caption, keeping the picture: it stays on
    // the message.
    let kept = seen(&alice, "look").unwrap().attachments[0].id.clone();
    alice.send(Cmd::Post(session::Draft {
        text: "look here".into(),
        edit: Some(picture),
        keep: vec![kept],
        ..Default::default()
    }));
    let captioned = |l: &session::Line| {
        l.edited && l.attachments.len() == 1 && l.attachments[0].bytes.is_some()
    };
    assert!(
        until(
            || seen(&bob, "look here").is_some_and(|l| captioned(&l))
                && seen(&alice, "look here").is_some_and(|l| captioned(&l)),
            30
        )
        .await,
        "the rewrite still carries the picture: {:?}",
        seen(&bob, "look here").map(|l| l.attachments)
    );
    assert!(
        seen(&bob, "look").is_none(),
        "the old words are gone: {:?}",
        bob.state().lines
    );

    // A rewrite that leaves the picture out takes it off the message: the
    // composer showed it as a tile and it was removed.
    alice.send(Cmd::Post(session::Draft {
        text: "never mind the picture".into(),
        edit: Some(picture),
        ..Default::default()
    }));
    assert!(
        until(
            || seen(&bob, "never mind the picture").is_some_and(|l| l.attachments.is_empty()),
            30
        )
        .await,
        "the picture is taken off: {:?}",
        seen(&bob, "never mind the picture").map(|l| l.attachments)
    );
}

/// What became of a draft is answered under its token: a message that could
/// not go says so and why, and one that went says nothing is wrong. This is
/// what the composer puts a refused message back from.
#[tokio::test]
async fn what_became_of_a_draft_is_answered_under_its_token() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(35);
    let (b_signer, b_id) = signer(36);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await
    );
    alice.send(Cmd::OpenDm(b_id));
    assert!(until(|| alice.state().open.is_some(), 15).await);

    // A file that is not there: the message cannot go.
    alice.send(Cmd::Post(session::Draft {
        text: "with a picture".into(),
        files: vec![dir.path().join("never-made.png")],
        token: 7,
        ..Default::default()
    }));
    assert!(
        until(
            || alice
                .state()
                .posted
                .as_ref()
                .is_some_and(|p| p.token == 7 && p.trouble.is_some()),
            15
        )
        .await,
        "{:?}",
        alice.state().posted
    );
    assert!(
        alice
            .state()
            .lines
            .iter()
            .all(|l| l.text != "with a picture"),
        "nothing was posted: {:?}",
        alice.state().lines
    );

    // And one that goes.
    alice.send(Cmd::Post(session::Draft {
        text: "just words".into(),
        token: 8,
        ..Default::default()
    }));
    assert!(
        until(
            || alice
                .state()
                .posted
                .as_ref()
                .is_some_and(|p| p.token == 8 && p.trouble.is_none()),
            15
        )
        .await,
        "{:?}",
        alice.state().posted
    );
}

/// A session says it is here (SIP-4): it beats within its interval of
/// connecting, at once when told nobody is at the machine and again when
/// somebody is, and it reads the beacons of the people it talks to into
/// its state -- active, then away when they say so.
#[tokio::test]
async fn a_session_beats_and_reads_the_beacons_of_the_people_it_talks_to() {
    use sqex_proto::beacon::{Read, Reply};

    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(61);
    let (b_signer, b_id) = signer(62);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    // Somebody who is neither: reads what the exchange says of Alice.
    let mut reader = sqnr::Client::connect(addr, &server_pub).await.unwrap();
    async fn read_of(c: &mut sqnr::Client, who: PubKey) -> Reply {
        let (code, body) = c
            .post("/beacon/read", Read { key: who }.encode())
            .await
            .unwrap();
        assert_eq!(code, 200);
        Reply::decode(&body).unwrap()
    }
    async fn read_until(
        c: &mut sqnr::Client,
        who: PubKey,
        secs: u64,
        want: impl Fn(&Reply) -> bool,
    ) -> Option<Reply> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let r = read_of(c, who).await;
            if want(&r) {
                return Some(r);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
    let r = read_until(&mut reader, a_id, 15, |r| r.found)
        .await
        .expect("Alice's session did not beat within its interval of connecting");
    assert_eq!(
        r.interval_secs,
        sigil_chat::presence::BEAT_SECS,
        "the interval the session promises"
    );
    assert!(!r.away, "somebody is here to begin with");

    // Told nobody is here: said at once, not at the next beat.
    alice.send(Cmd::Away(true));
    assert!(
        read_until(&mut reader, a_id, 5, |r| r.away).await.is_some(),
        "away was not beaten at once"
    );
    alice.send(Cmd::Away(false));
    assert!(
        read_until(&mut reader, a_id, 5, |r| !r.away)
            .await
            .is_some(),
        "back was not beaten at once"
    );

    // And what Bob sees of Alice, once they talk: active, then away.
    alice.send(Cmd::OpenDm(b_id));
    bob.send(Cmd::OpenDm(a_id));
    let seen = until(
        || {
            bob.state()
                .presence
                .get(&a_id)
                .is_some_and(|p| p.seen == sigil_chat::presence::Seen::Active)
        },
        20,
    )
    .await;
    assert!(
        seen,
        "Bob does not see Alice as active: {:?}",
        bob.state().presence
    );
    alice.send(Cmd::Away(true));
    let seen = until(
        || {
            bob.state()
                .presence
                .get(&a_id)
                .is_some_and(|p| p.seen == sigil_chat::presence::Seen::Away)
        },
        // Bob asks again after his read interval.
        sigil_chat::presence::READ_SECS + 10,
    )
    .await;
    assert!(
        seen,
        "Bob does not see Alice as away: {:?}",
        bob.state().presence
    );
    // Bob never reads himself.
    assert!(!bob.state().presence.contains_key(&b_id));

    alice.stop();
    bob.stop();
}

/// A mark made is in the state and survives a restart of the session; taken
/// back, it is gone; and saying so lodges a SIP-27 claim of the right kind
/// that anybody can read -- and only when asked.
#[tokio::test]
async fn a_verified_mark_is_kept_here_and_said_only_when_asked() {
    use sqex_proto::attest::{CLAIM_VERIFIED_IN_PERSON, Held, Query};

    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(71);
    let (_, b_id) = signer(72);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(until(|| alice.state().me == Some(a_id), 15).await);
    assert!(alice.state().verified.is_empty());

    alice.send(Cmd::Verify(b_id));
    assert!(
        until(|| alice.state().verified.contains_key(&b_id), 10).await,
        "the mark is not in the state: {:?}",
        alice.state().verified
    );
    // Nothing was said at the exchange for it.
    let mut reader = sqnr::Client::connect(addr, &server_pub).await.unwrap();
    let query = Query {
        subject: b_id,
        issuer: Some(a_id),
    };
    let (code, body) = reader.post("/attest/read", query.encode()).await.unwrap();
    assert_eq!(code, 200);
    let held = Held::decode(&body).unwrap();
    assert!(held.attestations.is_empty(), "a mark is local: {held:?}");

    // Restarted, the mark is still there: it is this machine's.
    alice.stop();
    let (a_signer, _) = signer(71);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(
        until(|| alice.state().verified.contains_key(&b_id), 15).await,
        "the mark did not survive a restart"
    );

    // Said, when asked to be.
    alice.send(Cmd::Attest(b_id));
    assert!(
        until(
            || alice
                .state()
                .note
                .as_ref()
                .is_some_and(|n| n.said.contains("compared the words")),
            10
        )
        .await,
        "{:?} {:?}",
        alice.state().note,
        alice.state().trouble
    );
    let (code, body) = reader.post("/attest/read", query.encode()).await.unwrap();
    assert_eq!(code, 200);
    let held = Held::decode(&body).unwrap();
    assert_eq!(held.attestations.len(), 1);
    let a = &held.attestations[0];
    assert_eq!(a.claim, CLAIM_VERIFIED_IN_PERSON);
    assert!(a.body.is_empty());
    assert!(
        a.readable(),
        "the claim is one this build knows, empty-bodied"
    );
    assert_eq!((a.issuer, a.subject), (a_id, b_id));

    alice.send(Cmd::Unverify(b_id));
    assert!(
        until(|| !alice.state().verified.contains_key(&b_id), 10).await,
        "the mark was not taken back"
    );
    alice.stop();
}

/// What the exchange federates with is in the state once connected (SIP-39 §The peer directory):
/// the seeded peer by key, with no domain since a seed carries none.
#[tokio::test]
async fn the_exchanges_this_one_federates_with_are_in_the_state() {
    let dir = tempfile::tempdir().unwrap();
    let (_, trunk) = signer(81);
    let (addr, server_pub, _h) = server_peering(dir.path(), "off", &[trunk]).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(82);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(until(|| alice.state().me == Some(a_id), 15).await);
    assert!(
        until(|| alice.state().peers.iter().any(|(k, _)| *k == trunk), 15).await,
        "the peer is not in the state: {:?}",
        alice.state().peers
    );
    let (_, domain) = alice
        .state()
        .peers
        .into_iter()
        .find(|(k, _)| *k == trunk)
        .unwrap();
    assert_eq!(domain, "", "a seeded peer has no domain");
    alice.stop();
}

/// SIP-42: a device linked to an account gets the history its sibling holds,
/// from the sibling, and is told how much came.
///
/// The phone reads a conversation; the laptop is linked afterwards with an
/// empty store and nobody reseals a key to it. Both keep an open toward
/// each other, meet through the exchange, and the laptop ends up with the
/// messages -- read from its own store, since it holds the key now -- and a
/// note saying so.
#[tokio::test]
async fn a_linked_device_is_handed_the_history_its_sibling_holds() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (phone_signer, phone_key) = signer(44);
    let (laptop_signer, laptop_key) = signer(45);
    let (bob_signer, bob_key) = signer(46);
    let phone = start_at(endpoint, phone_signer, &dir.path().join("phone.db"));
    let bob = start_at(endpoint, bob_signer, &dir.path().join("bob.db"));
    assert!(
        until(
            || phone.state().me == Some(phone_key) && bob.state().me == Some(bob_key),
            15
        )
        .await
    );
    bob.send(Cmd::OpenDm(phone_key));
    phone.send(Cmd::OpenDm(bob_key));
    assert!(
        until(
            || phone.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await
    );
    for text in ["one", "two", "three"] {
        phone.send(Cmd::Send(text.into()));
        assert!(
            until(|| bob.state().lines.iter().any(|l| l.text == text), 20).await,
            "{text} never reached bob"
        );
    }
    assert!(
        until(
            || phone.state().lines.iter().filter(|l| l.mine).count() == 3,
            20
        )
        .await,
        "the phone has not read its own conversation back"
    );
    // The exchange forgets all but its newest entry. From here the phone's
    // disk is the only copy of the conversation, which is the case SIP-42
    // exists for -- and the control: a laptop that reads the messages got
    // them from its sibling, because nowhere else has them.
    phone.send(Cmd::SetRetention {
        secs: sqex_proto::channel::MIN_RETENTION,
        max_entries: 1,
    });
    assert!(
        until(
            || phone
                .state()
                .note
                .is_some_and(|n| n.said.starts_with("Retention set")),
            20
        )
        .await,
        "{:?}",
        phone.state().trouble
    );

    // The laptop comes up as its own key, then presents the credential.
    let laptop = start_at(endpoint, laptop_signer, &dir.path().join("laptop.db"));
    assert!(until(|| laptop.state().me == Some(laptop_key), 15).await);
    phone.send(Cmd::LinkDevice {
        device: laptop_key,
        days: 90,
    });
    assert!(until(|| phone.state().credential.is_some(), 20).await);
    let credential = phone.state().credential.expect("checked above");
    laptop.send(Cmd::RegisterSelf(credential));
    assert!(
        until(
            || laptop
                .state()
                .note
                .is_some_and(|n| n.said.contains("acts for the account")),
            20
        )
        .await,
        "{:?}",
        laptop.state().trouble
    );
    // Linking registered the phone itself first: the listed set is both.
    phone.send(Cmd::Devices);
    assert!(
        until(
            || {
                let d = phone.state().devices;
                d.iter().any(|d| d.device == phone_key) && d.iter().any(|d| d.device == laptop_key)
            },
            20
        )
        .await,
        "the phone is not among its own account's devices: {:?}",
        phone.state().devices
    );

    // Neither is told to do anything. They find each other.
    let synced = until(
        || {
            laptop
                .state()
                .note
                .is_some_and(|n| n.said.starts_with("Synced 3 messages"))
        },
        60,
    )
    .await;
    assert!(
        synced,
        "the laptop was not handed the history: note {:?}, trouble {:?}",
        laptop.state().note,
        laptop.state().trouble
    );
    // The phone had nothing to learn, so it was told nothing.
    assert!(
        phone
            .state()
            .note
            .is_none_or(|n| !n.said.starts_with("Synced")),
        "{:?}",
        phone.state().note
    );

    // And the conversation reads on the laptop.
    let channel = phone.state().open.expect("the phone has it open");
    laptop.send(Cmd::Show(channel));
    let readable = until(
        || {
            let lines = laptop.state().lines;
            ["one", "two", "three"]
                .iter()
                .all(|t| lines.iter().any(|l| l.text == *t && l.mine))
        },
        30,
    )
    .await;
    assert!(readable, "{:?}", laptop.state().lines);

    phone.stop();
    laptop.stop();
    bob.stop();
}

/// A second exchange that replicates `channel` from the origin every second
/// and serves it, naming the origin's domain.
async fn server_replicating(
    dir: &Path,
    origin: PubKey,
    origin_addr: SocketAddr,
    channel: [u8; 32],
) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    let key_path = dir.join("host_key");
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nlimits = {{ posts = [0, 0], signals = [0, 0], joins = [0, 0], creates = [0, 0], uploads = [0, 0] }}\n\n[[replicate]]\norigin = {:?}\naddr = {:?}\n\
         channels = [{:?}]\ninterval_secs = 1\ndomain = \"origin.example\"\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
        origin.to_string(),
        origin_addr.to_string(),
        bs58::encode(channel).into_string(),
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

/// SIP-43: a member posts at the exchange they are connected to, and the
/// channel lives at another. Bob joins the square at the origin, then moves
/// home to an exchange that only replicates it; what he says there is
/// ordered at the origin and read by Alice, who never left it -- and his
/// bar says where the conversation lives.
#[tokio::test]
async fn a_member_posts_from_an_exchange_that_only_holds_a_copy() {
    let origin_dir = tempfile::tempdir().unwrap();
    let replica_dir = tempfile::tempdir().unwrap();
    let (replica_sk, replica_pub) = squic::generate_keypair();
    std::fs::write(
        replica_dir.path().join("host_key"),
        hex::encode(replica_sk.to_bytes()),
    )
    .unwrap();
    let replica_key = PubKey::new(replica_pub);

    // The origin serves the replica as a peer.
    let key_path = origin_dir.path().join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nlimits = {{ posts = [0, 0], signals = [0, 0], joins = [0, 0], creates = [0, 0], uploads = [0, 0] }}\nreplication_peers = [{:?}]\n",
        key_path.to_string_lossy(),
        origin_dir.path().join("sqex.state").to_string_lossy(),
        replica_key.to_string(),
    );
    let config_path = origin_dir.path().join("sqexd.toml");
    std::fs::write(&config_path, &config_toml).unwrap();
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _pub) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind(config, Some(config_path), signing_key)
        .await
        .unwrap();
    let origin_addr = bound.local_addr;
    let origin_pub = bound.public_key.to_bytes();
    let origin = PubKey::new(origin_pub);
    tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });
    let at_origin = Endpoint {
        address: origin_addr,
        server: origin,
    };

    let (alice_signer, alice_key) = signer(47);
    let (_, bob_key) = signer(48);
    let alice = start_at(at_origin, alice_signer, &origin_dir.path().join("alice.db"));
    assert!(until(|| alice.state().me == Some(alice_key), 15).await);
    alice.send(Cmd::NewPublic {
        name: "the square".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.public == Some(true)),
            15
        )
        .await
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.public == Some(true))
        .unwrap()
        .channel;
    alice.send(Cmd::Show(channel));
    assert!(until(|| alice.state().open == Some(channel), 15).await);
    alice.send(Cmd::Send("welcome".into()));
    assert!(
        until(
            || alice.state().lines.iter().any(|l| l.text == "welcome"),
            20
        )
        .await
    );

    // Bob joins at the origin -- a join is not forwarded -- and leaves.
    let bob_store = origin_dir.path().join("bob.db");
    {
        let bob = start_at(at_origin, signer(48).0, &bob_store);
        assert!(until(|| bob.state().me == Some(bob_key), 15).await);
        bob.send(Cmd::Find("square".into()));
        assert!(
            until(
                || bob.state().found.iter().any(|f| f.name == "the square"),
                15
            )
            .await
        );
        let found = bob
            .state()
            .found
            .into_iter()
            .find(|f| f.name == "the square")
            .unwrap();
        bob.send(Cmd::Join {
            channel: found.channel,
            instance: found.instance,
        });
        assert!(
            until(
                || bob
                    .state()
                    .conversations
                    .iter()
                    .any(|c| c.channel == channel),
                15
            )
            .await
        );
        bob.stop();
    }

    // Alice, an admin, authorises the replica; it comes up and catches up.
    alice.send(Cmd::Replicate {
        exchange: replica_key,
        on: true,
    });
    assert!(
        until(
            || alice
                .state()
                .note
                .as_ref()
                .is_some_and(|n| n.said.contains("carry a copy")),
            20
        )
        .await,
        "{:?} / {:?}",
        alice.state().note,
        alice.state().trouble
    );
    let (replica_addr, _, _rh) =
        server_replicating(replica_dir.path(), origin, origin_addr, channel).await;
    let at_replica = Endpoint {
        address: replica_addr,
        server: replica_key,
    };

    // Bob comes home to the replica, with the store he joined with.
    let bob = start_at(at_replica, signer(48).0, &bob_store);
    assert!(until(|| bob.state().me == Some(bob_key), 15).await);
    let listed = until(
        || {
            bob.state()
                .conversations
                .iter()
                .any(|c| c.channel == channel)
        },
        30,
    )
    .await;
    assert!(
        listed,
        "the replica never listed the square for its member: {:?}",
        bob.state().conversations
    );
    bob.send(Cmd::Show(channel));
    assert!(
        until(|| bob.state().lines.iter().any(|l| l.text == "welcome"), 30).await,
        "the copy did not read at the replica: {:?}",
        bob.state().trouble
    );
    // His bar says where it lives.
    assert!(
        until(
            || bob
                .state()
                .home
                .as_ref()
                .is_some_and(|(k, d)| *k == origin && d == "origin.example"),
            15
        )
        .await,
        "{:?}",
        bob.state().home
    );
    // Alice's does not: it lives with her.
    assert_eq!(alice.state().home, None);
    // And the two are told apart by name: hers is "the square", his is
    // "the square@origin.example".
    let label_at = |h: &ChatHandle| {
        h.state()
            .conversations
            .iter()
            .find(|c| c.channel == channel)
            .map(|c| c.label.clone())
    };
    assert!(
        until(
            || label_at(&bob).as_deref() == Some("the square@origin.example"),
            15
        )
        .await,
        "{:?}",
        label_at(&bob)
    );
    assert_eq!(label_at(&alice).as_deref(), Some("the square"));

    // What Bob says at the replica is ordered at the origin and read there.
    bob.send(Cmd::Send("hello from the other side".into()));
    let heard = until(
        || {
            alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "hello from the other side" && l.who == bob_key)
        },
        30,
    )
    .await;
    assert!(
        heard,
        "the origin never got Bob's post: bob {:?} / alice {:?}",
        bob.state().trouble,
        alice.state().trouble
    );
    // And Bob reads it back where he wrote it, as his own.
    assert!(
        until(
            || bob
                .state()
                .lines
                .iter()
                .any(|l| l.text == "hello from the other side" && l.mine),
            30
        )
        .await,
        "{:?}",
        bob.state().lines
    );
    // Bob's message says which exchange carried it -- his own word, in the
    // body -- and Alice's, made where the room lives, says nothing. The
    // replica was dialled by address, so it is named by the head of its key.
    let via_of = |h: &ChatHandle, text: &str| {
        h.state()
            .lines
            .iter()
            .find(|l| l.text == text)
            .map(|l| l.via.clone())
    };
    let replica_head = format!("{}…", &replica_key.to_string()[..8]);
    assert_eq!(
        via_of(&alice, "hello from the other side"),
        Some(Some(replica_head.clone()))
    );
    assert_eq!(via_of(&alice, "welcome"), Some(None));
    assert_eq!(
        via_of(&bob, "hello from the other side"),
        Some(Some(replica_head.clone()))
    );
    assert_eq!(via_of(&bob, "welcome"), Some(None));

    // Carol has never been to the origin. She finds the square in the
    // replica's directory, joins it *there* -- the join is carried to the
    // origin as a post is -- reads it, and speaks.
    let (_, carol_key) = signer(49);
    let carol = start_at(
        at_replica,
        signer(49).0,
        &replica_dir.path().join("carol.db"),
    );
    assert!(until(|| carol.state().me == Some(carol_key), 15).await);
    carol.send(Cmd::Find("square".into()));
    assert!(
        until(
            || carol
                .state()
                .found
                .iter()
                .any(|f| f.name.starts_with("the square")),
            20
        )
        .await,
        "the replica's directory does not list the square: {:?}",
        carol.state().found
    );
    let found = carol
        .state()
        .found
        .into_iter()
        .find(|f| f.name == "the square@origin.example")
        .expect("the directory at a copy names the room with where it lives");
    carol.send(Cmd::Join {
        channel: found.channel,
        instance: found.instance,
    });
    assert!(
        until(
            || carol
                .state()
                .conversations
                .iter()
                .any(|c| c.channel == channel),
            30
        )
        .await,
        "the join was not carried to the origin: {:?}",
        carol.state().trouble
    );
    carol.send(Cmd::Show(channel));
    assert!(
        until(
            || carol.state().lines.iter().any(|l| l.text == "welcome"),
            30
        )
        .await
    );
    carol.send(Cmd::Send("and a third, who joined from the copy".into()));
    assert!(
        until(
            || alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "and a third, who joined from the copy" && l.who == carol_key),
            30
        )
        .await,
        "carol {:?} / alice {:?}",
        carol.state().trouble,
        alice.state().trouble
    );
    assert_eq!(
        via_of(&alice, "and a third, who joined from the copy"),
        Some(Some(replica_head))
    );
    carol.stop();

    alice.stop();
    bob.stop();
}

/// SIP-44: a member sees a succession in the transcript, worded and backed by
/// the old key's own signature; the old key's session is told where the
/// account went.
#[tokio::test]
async fn a_succession_is_said_in_the_room_and_to_the_old_key() {
    use sqex_proto::succession::{Claim, Proof, Will};

    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (alice_signer, alice_key) = signer(51);
    let (bob_signer, bob_key) = signer(52);
    let carol_seed = [53u8; 32];
    let carol_key = PubKey::new(
        SigningKey::from_bytes(&carol_seed)
            .verifying_key()
            .to_bytes(),
    );
    let alice = start_at(endpoint, alice_signer, &dir.path().join("alice.db"));
    let bob = start_at(endpoint, bob_signer, &dir.path().join("bob.db"));
    assert!(
        until(
            || alice.state().me == Some(alice_key) && bob.state().me == Some(bob_key),
            15
        )
        .await
    );
    alice.send(Cmd::NewPublic {
        name: "the square".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || alice
                .state()
                .conversations
                .iter()
                .any(|c| c.public == Some(true)),
            15
        )
        .await
    );
    let channel = alice
        .state()
        .conversations
        .iter()
        .find(|c| c.public == Some(true))
        .unwrap()
        .channel;
    alice.send(Cmd::Show(channel));
    assert!(until(|| alice.state().open == Some(channel), 15).await);
    bob.send(Cmd::Find("square".into()));
    assert!(
        until(
            || bob.state().found.iter().any(|f| f.name == "the square"),
            15
        )
        .await
    );
    let found = bob
        .state()
        .found
        .into_iter()
        .find(|f| f.name == "the square")
        .unwrap();
    bob.send(Cmd::Join {
        channel: found.channel,
        instance: found.instance,
    });
    assert!(until(|| bob.state().open == Some(channel), 15).await);

    // Alice's will, presented by Carol as herself, on a bare connection.
    let will = Will::sign(&[51u8; 32], &carol_key, 1000);
    let mut carol = sqnr::Client::connect_as(addr, &server_pub, &carol_seed)
        .await
        .unwrap();
    let (code, _) = carol
        .post(
            "/account/succeed",
            Claim {
                proof: Proof::Will(will),
            }
            .encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);

    // Bob's transcript says so, in words that name both keys.
    let said = until(
        || {
            bob.state().events.iter().any(|e| {
                e.said.contains("account is now") && e.actor == alice_key && e.subject == carol_key
            })
        },
        30,
    )
    .await;
    assert!(
        said,
        "the succession was not said: {:?}",
        bob.state()
            .events
            .iter()
            .map(|e| e.said.clone())
            .collect::<Vec<_>>()
    );
    // Alice's session is told where the account went, rather than left
    // failing at everything.
    let told =
        until(
            || {
                alice.state().trouble.as_deref().is_some_and(|t| {
                    t.contains("succeeded by") && t.contains(&carol_key.to_string())
                })
            },
            30,
        )
        .await;
    assert!(told, "{:?}", alice.state().trouble);

    alice.stop();
    bob.stop();
}

/// SIP-56 from the interface's commands: an admin mutes a member, whose
/// next message is refused and said as a mute; the transcript says who muted
/// whom and the roster shows it; unmuting lets them write again. A member
/// reports a message; the admin reads the report, with who reported it,
/// and dismisses it; a member asking for the reports is told it is an
/// admin's to do.
#[tokio::test]
async fn an_admin_mutes_and_reads_reports_and_a_member_reports() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(0x56);
    let (b_signer, b_id) = signer(0x57);
    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up"
    );

    alice.send(Cmd::NewPublic {
        name: "moderated".into(),
        topic: String::new(),
    });
    assert!(
        until(
            || alice.state().open.is_some() && alice.state().i_am_admin,
            15
        )
        .await,
        "{:?}",
        alice.state().trouble
    );
    let channel = alice.state().open.unwrap();
    bob.send(Cmd::Find("moderated".into()));
    assert!(
        until(
            || bob.state().found.iter().any(|f| f.name == "moderated"),
            15
        )
        .await
    );
    let found = bob
        .state()
        .found
        .into_iter()
        .find(|f| f.name == "moderated")
        .unwrap();
    bob.send(Cmd::Join {
        channel: found.channel,
        instance: found.instance,
    });
    assert!(
        until(|| bob.state().open == Some(channel), 15).await,
        "{:?}",
        bob.state().trouble
    );
    bob.send(Cmd::Send("before the mute".into()));
    assert!(
        until(
            || alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "before the mute"),
            20
        )
        .await
    );
    assert!(
        until(
            || alice.state().members.iter().any(|m| m.account == b_id),
            15
        )
        .await,
        "Bob is on Alice's roster: {:?}",
        alice.state().members
    );

    // Muted: refused, and said as what it is.
    alice.send(Cmd::Mute {
        who: b_id,
        on: true,
    });
    assert!(
        until(
            || alice
                .state()
                .members
                .iter()
                .any(|m| m.account == b_id && m.muted),
            15
        )
        .await,
        "the roster shows the mute: {:?}",
        alice.state().members
    );
    assert!(
        until(
            || bob
                .state()
                .events
                .iter()
                .any(|e| e.said.contains("muted you")),
            15
        )
        .await,
        "the transcript says so: {:?}",
        bob.state().events
    );
    bob.send(Cmd::Send("while muted".into()));
    assert!(
        until(
            || bob
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("You are muted here")),
            15
        )
        .await,
        "a muted member is told: {:?}",
        bob.state().trouble
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !alice.state().lines.iter().any(|l| l.text == "while muted"),
        "nothing was written"
    );

    // Unmuted: writes again.
    alice.send(Cmd::Mute {
        who: b_id,
        on: false,
    });
    assert!(
        until(
            || alice
                .state()
                .members
                .iter()
                .any(|m| m.account == b_id && !m.muted),
            15
        )
        .await
    );
    bob.send(Cmd::Send("after the mute".into()));
    assert!(
        until(
            || alice
                .state()
                .lines
                .iter()
                .any(|l| l.text == "after the mute"),
            20
        )
        .await,
        "{:?}",
        bob.state().trouble
    );

    // A report, read by the admin with who made it, then dismissed.
    let seq = alice
        .state()
        .lines
        .iter()
        .find(|l| l.text == "after the mute")
        .map(|l| l.seq)
        .unwrap();
    bob.send(Cmd::Report {
        target: seq,
        reason: 2,
        note: "unkind".into(),
    });
    assert!(
        until(
            || bob
                .state()
                .note
                .as_ref()
                .is_some_and(|n| n.said.contains("Reported to the admins")),
            15
        )
        .await,
        "{:?}",
        bob.state().trouble
    );
    alice.send(Cmd::LoadReports);
    assert!(
        until(|| !alice.state().reports.is_empty(), 15).await,
        "the admin sees it: {:?}",
        alice.state().trouble
    );
    let report = alice.state().reports[0].clone();
    assert_eq!(report.reporter, b_id);
    assert_eq!(report.target, seq);
    assert_eq!(report.reason, "harassment");
    assert_eq!(report.note, "unkind");
    alice.send(Cmd::Dismiss(report.id));
    assert!(
        until(|| alice.state().reports.is_empty(), 15).await,
        "dismissed"
    );

    // Not for members.
    bob.send(Cmd::LoadReports);
    assert!(
        until(
            || bob
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("admin")),
            15
        )
        .await,
        "{:?}",
        bob.state().trouble
    );
    alice.stop();
    bob.stop();
}

/// **A device that acts for an account says so it is the account.**
///
/// `Chat` keeps two keys: the device's, which it seals under and counts
/// messages with, and the account's, which is what everything drawn is
/// relative to -- the display name, the handle, whether a message is one's
/// own, which admin is somebody else. They are the same key until this
/// device is linked to an account, and from then on they are not.
///
/// The session took the account from the signer once, at the top, and never
/// looked again. So a linked device drew itself as its own key: the identity
/// in the app bar was the device, the account's name and handle were looked
/// up under a key that has neither, and every message from the account's
/// other device read as somebody else's.
///
/// Both halves are checked, because they fail separately: the session that
/// presented the credential, and the next one to open the same store.
#[tokio::test]
async fn a_linked_device_acts_as_the_account_it_was_linked_to() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (first_signer, first) = signer(61);
    let (second_signer, second) = signer(62);
    let store = dir.path().join("two.db");
    let one = start_at(endpoint, first_signer, &dir.path().join("one.db"));
    let two = start_at(endpoint, second_signer, &store);
    assert!(
        until(
            || one.state().me == Some(first) && two.state().me == Some(second),
            15
        )
        .await,
        "both should come up"
    );

    one.send(Cmd::LinkDevice {
        device: second,
        days: 90,
    });
    assert!(
        until(|| one.state().credential.is_some(), 20).await,
        "no credential was written: {:?}",
        one.state().trouble
    );
    let credential = one.state().credential.expect("checked above");

    two.send(Cmd::RegisterSelf(credential));
    assert!(
        until(
            || two
                .state()
                .note
                .is_some_and(|n| n.said.contains("acts for the account")),
            20
        )
        .await,
        "the credential was refused: {:?} / {:?}",
        two.state().note,
        two.state().trouble
    );

    // The session that presented it.
    assert!(
        until(|| two.state().me == Some(first), 20).await,
        "after enrolling, this device still draws itself as its own key \
         ({:?}) rather than as the account it acts for ({first})",
        two.state().me
    );
    two.stop();

    // And the next one to open the same store, which learns it from the
    // store rather than from the command.
    let (second_again, _) = signer(62);
    let again = start_at(endpoint, second_again, &store);
    assert!(
        until(|| again.state().me.is_some(), 15).await,
        "the reopened session never came up"
    );
    assert_eq!(
        again.state().me,
        Some(first),
        "a session opened on a linked device's store acts as the device \
         again: the account is in the store and nothing read it"
    );

    one.stop();
    again.stop();
}

/// **SIP-44 §The handover: a revoked device stops acting for the account.**
///
/// The account this client acts for lives in two places: this device's store,
/// which is what it has believed since the credential was presented, and the
/// exchange's registry, which is the one party that knows after the account
/// changed its mind. Nothing reconciled them, so a device that had been
/// revoked came back up still calling itself the account -- drawing the
/// account's name, sealing as it, reading the account's other device as
/// itself -- and only the exchange's refusals said otherwise, one operation
/// at a time.
///
/// `follow_account` asks once a connection and the store follows. A revoked
/// registration is the producible half of the same mechanism a succession
/// uses: the registry answers the device its own key back, which is what
/// every key is until a registration says otherwise (SIP-22).
#[tokio::test]
async fn a_revoked_device_comes_back_up_as_itself_again() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (first_signer, first) = signer(63);
    let (second_signer, second) = signer(64);
    let store = dir.path().join("two.db");
    let one = start_at(endpoint, first_signer, &dir.path().join("one.db"));
    let two = start_at(endpoint, second_signer, &store);
    assert!(
        until(
            || one.state().me == Some(first) && two.state().me == Some(second),
            15
        )
        .await,
        "both should come up"
    );

    one.send(Cmd::LinkDevice {
        device: second,
        days: 90,
    });
    assert!(
        until(|| one.state().credential.is_some(), 20).await,
        "no credential was written: {:?}",
        one.state().trouble
    );
    let credential = one.state().credential.expect("checked above");
    two.send(Cmd::RegisterSelf(credential));
    assert!(
        until(|| two.state().me == Some(first), 20).await,
        "the device never took the account on: {:?}",
        two.state().trouble
    );
    two.stop();

    // The account changes its mind. The exchange's word is the only one that
    // counts, so this is checked from its own listing rather than from the
    // command's answer.
    one.send(Cmd::RevokeDevice(second));
    one.send(Cmd::Devices);
    assert!(
        until(
            || !one.state().devices.is_empty()
                && !one.state().devices.iter().any(|d| d.device == second),
            20
        )
        .await,
        "the exchange still lists the device, so there is nothing to follow: {:?}",
        one.state().devices
    );

    // The device knows nothing of this: its store still says it is the
    // account. It finds out by asking.
    let (second_again, _) = signer(64);
    let again = start_at(endpoint, second_again, &store);
    assert!(
        until(|| again.state().me == Some(second), 20).await,
        "a revoked device came back up still acting as the account it was \
         cut off from: {:?}",
        again.state().me
    );

    one.stop();
    again.stop();
}

/// **SIP-44 from the interface's commands, both ways.**
///
/// The CLI has had `succession will | policy | vouch | claim` since SIP-44
/// landed, and sigil's own documentation said "done from a terminal today".
/// A phone has no terminal, and it is the device most likely to be the one
/// that is lost. So: Alice writes a will for Carol from the Devices pane;
/// Carol pastes it and takes the account; Bob's transcript says so and
/// Alice's session is told where the account went. Then, separately, Dave
/// names two guardians, they vouch from their own panes, and Erin pastes
/// Dave's key and the two vouches.
///
/// What is asserted is what the CLI test already asserts of the exchange's
/// side -- the room line and the old key's notice -- reached through
/// `Cmd`s alone, plus the words each pane shows for what it just wrote.
#[tokio::test]
async fn an_account_arranges_its_succession_from_the_pane_and_the_successor_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (alice_signer, alice_key) = signer(71);
    let (bob_signer, bob_key) = signer(72);
    let (carol_signer, carol_key) = signer(73);
    let alice = start_at(endpoint, alice_signer, &dir.path().join("alice.db"));
    let bob = start_at(endpoint, bob_signer, &dir.path().join("bob.db"));
    let carol = start_at(endpoint, carol_signer, &dir.path().join("carol.db"));
    for (h, k) in [(&alice, alice_key), (&bob, bob_key), (&carol, carol_key)] {
        assert!(
            until(|| h.state().me == Some(k), 15).await,
            "should come up"
        );
    }
    // A room Alice and Bob share, so there is somewhere for the succession
    // to be said.
    alice.send(Cmd::NewPublic {
        name: "the room".into(),
        topic: String::new(),
    });
    assert!(until(|| alice.state().open.is_some(), 15).await);
    let channel = alice.state().open.unwrap();
    alice.send(Cmd::Invite(bob_key));
    assert!(
        until(
            || bob
                .state()
                .conversations
                .iter()
                .any(|c| c.channel == channel),
            20
        )
        .await
    );
    bob.send(Cmd::Show(channel));
    assert!(until(|| bob.state().open == Some(channel), 15).await);

    // **The pane asks what is arranged, and nothing is.**
    alice.send(Cmd::SuccessionStatus);
    assert!(
        until(|| alice.state().succession.is_some(), 15).await,
        "the pane was never told what is arranged"
    );
    let su = alice.state().succession.unwrap();
    assert!(
        su.is_account,
        "Alice is her own account and the pane says otherwise"
    );
    assert!(su.lodged.is_none());

    // **A will**, written from the pane and shown to be copied.
    alice.send(Cmd::WriteWill(carol_key));
    assert!(
        until(
            || alice
                .state()
                .succession
                .as_ref()
                .is_some_and(|s| s.will.is_some()),
            15
        )
        .await,
        "no will was shown: {:?}",
        alice.state().trouble
    );
    let will = alice.state().succession.unwrap().will.unwrap();
    assert!(
        alice
            .state()
            .note
            .is_some_and(|n| n.said.contains("Keep it apart")),
        "the pane does not say what a will has to be kept apart from"
    );
    // Put away, as somebody would once it is copied.
    alice.send(Cmd::HideSuccession);
    assert!(
        until(
            || alice
                .state()
                .succession
                .as_ref()
                .is_some_and(|s| s.will.is_none()),
            10
        )
        .await
    );

    // **Before the claim, the registry says Alice was not succeeded** --
    // the answer a direct message with her would draw, and the control for
    // the answer after.
    bob.send(Cmd::SuccessionOf(alice_key));
    assert!(
        until(|| bob.state().succeeded.contains_key(&alice_key), 15).await,
        "the registry was never asked: {:?}",
        bob.state().trouble
    );
    assert_eq!(bob.state().succeeded.get(&alice_key), Some(&None));

    // **Carol takes it**, by pasting the will and nothing else.
    carol.send(Cmd::Succeed(will));
    assert!(
        until(
            || carol
                .state()
                .note
                .is_some_and(|n| n.said.contains("is yours")),
            20
        )
        .await,
        "Carol's claim did not go through: {:?}",
        carol.state().trouble
    );
    // Bob's transcript says so, in words that name both keys.
    assert!(
        until(
            || bob.state().events.iter().any(|e| {
                e.said.contains("account is now") && e.actor == alice_key && e.subject == carol_key
            }),
            30
        )
        .await,
        "the succession was not said in the room"
    );
    // And Alice's session is told where the account went.
    assert!(
        until(
            || alice
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("succeeded by")),
            30
        )
        .await,
        "{:?}",
        alice.state().trouble
    );

    // **And the registry, asked again, names Carol** -- with the proof
    // checked on the way in, which is what a direct message with Alice
    // opened tomorrow, by somebody who was in no room with her, has to go
    // on. A key nobody succeeded still reads as not succeeded.
    bob.send(Cmd::SuccessionOf(alice_key));
    assert!(
        until(
            || bob.state().succeeded.get(&alice_key) == Some(&Some(carol_key)),
            15
        )
        .await,
        "the registry did not name the successor: {:?} / {:?}",
        bob.state().succeeded.get(&alice_key),
        bob.state().trouble
    );
    bob.send(Cmd::SuccessionOf(bob_key));
    assert!(
        until(|| bob.state().succeeded.contains_key(&bob_key), 15).await,
        "{:?}",
        bob.state().trouble
    );
    assert_eq!(bob.state().succeeded.get(&bob_key), Some(&None));

    alice.stop();
    bob.stop();
    carol.stop();
}

#[tokio::test]
async fn guardians_named_from_the_pane_vouch_and_the_successor_takes_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (dave_signer, dave_key) = signer(81);
    let (g1_signer, g1_key) = signer(82);
    let (g2_signer, g2_key) = signer(83);
    let (erin_signer, erin_key) = signer(84);
    let dave = start_at(endpoint, dave_signer, &dir.path().join("dave.db"));
    let g1 = start_at(endpoint, g1_signer, &dir.path().join("g1.db"));
    let g2 = start_at(endpoint, g2_signer, &dir.path().join("g2.db"));
    let erin = start_at(endpoint, erin_signer, &dir.path().join("erin.db"));
    for (h, k) in [
        (&dave, dave_key),
        (&g1, g1_key),
        (&g2, g2_key),
        (&erin, erin_key),
    ] {
        assert!(
            until(|| h.state().me == Some(k), 15).await,
            "should come up"
        );
    }

    // Dave names two guardians, both of whom it takes.
    dave.send(Cmd::NameGuardians {
        threshold: 2,
        guardians: vec![g1_key, g2_key],
    });
    assert!(
        until(
            || dave.state().succession.as_ref().is_some_and(|s| s
                .lodged
                .as_ref()
                .is_some_and(|(t, g)| *t == 2 && g.len() == 2)),
            20
        )
        .await,
        "the policy was not lodged, or the pane was not told: {:?}",
        dave.state().trouble
    );

    // The guardians vouch for Erin, each from their own pane.
    let mut vouches = Vec::new();
    for g in [&g1, &g2] {
        g.send(Cmd::Vouch {
            account: dave_key,
            successor: erin_key,
        });
        assert!(
            until(
                || g.state()
                    .succession
                    .as_ref()
                    .is_some_and(|s| s.vouch.is_some()),
                15
            )
            .await,
            "no vouch was shown"
        );
        vouches.push(g.state().succession.unwrap().vouch.unwrap());
    }

    // **One vouch is not enough**, and the pane says so before anything is
    // sent -- the quorum is short.
    erin.send(Cmd::Succeed(format!("{dave_key}\n{}", vouches[0])));
    assert!(
        until(|| erin.state().trouble.is_some(), 20).await,
        "a short quorum was accepted: {:?}",
        erin.state().note
    );
    assert!(
        erin.state().trouble.unwrap_or_default().contains("quorum"),
        "the refusal does not say the quorum is short: {:?}",
        erin.state().trouble
    );

    // Two is: Erin pastes Dave's key and both vouches.
    erin.send(Cmd::Succeed(format!(
        "{dave_key}\n{}\n{}",
        vouches[0], vouches[1]
    )));
    assert!(
        until(
            || erin
                .state()
                .note
                .is_some_and(|n| n.said.contains("is yours")),
            20
        )
        .await,
        "Erin's claim did not go through: {:?}",
        erin.state().trouble
    );

    dave.stop();
    g1.stop();
    g2.stop();
    erin.stop();
}

/// **SIP-27, read back.** sigil has lodged "we compared the words" at the
/// exchange since SIP-41's dialog offered to (`Attest`), and never read what
/// anybody else had lodged. Now the dialog that offers to say it also shows
/// who already has -- their word, to be read and not acted on, and the
/// dialog says so.
///
/// Alice says it of Bob; Carol, opening Bob's dialog, is told one person
/// has, and that it was Alice. Before Alice says it, Carol is told nobody
/// has -- the control -- and Alice's own statement is never shown back to
/// Alice, whose dialog already shows Bob as verified.
#[tokio::test]
async fn who_else_compared_the_words_is_read_back_for_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (alice_signer, alice_key) = signer(91);
    let (_, bob_key) = signer(92);
    let (carol_signer, carol_key) = signer(93);
    let alice = start_at(endpoint, alice_signer, &dir.path().join("alice.db"));
    let carol = start_at(endpoint, carol_signer, &dir.path().join("carol.db"));
    assert!(
        until(
            || alice.state().me == Some(alice_key) && carol.state().me == Some(carol_key),
            15
        )
        .await
    );

    // Nobody has said anything yet, and Carol is told exactly that rather
    // than left with a question in flight.
    carol.send(Cmd::Attested(bob_key));
    assert!(
        until(
            || carol
                .state()
                .attested
                .get(&bob_key)
                .is_some_and(|v| v.is_empty()),
            15
        )
        .await,
        "Carol was not told that nobody has said anything: {:?}",
        carol.state().trouble
    );

    alice.send(Cmd::Attest(bob_key));
    assert!(
        until(
            || alice
                .state()
                .note
                .is_some_and(|n| n.said.contains("Said, at this exchange")),
            15
        )
        .await,
        "Alice's statement was not lodged: {:?}",
        alice.state().trouble
    );

    carol.send(Cmd::Attested(bob_key));
    assert!(
        until(
            || carol.state().attested.get(&bob_key) == Some(&vec![alice_key]),
            15
        )
        .await,
        "Carol was not told Alice said so: {:?}",
        carol.state().attested.get(&bob_key)
    );
    // Alice's own statement is not shown back to her.
    alice.send(Cmd::Attested(bob_key));
    assert!(
        until(
            || alice
                .state()
                .attested
                .get(&bob_key)
                .is_some_and(|v| v.is_empty()),
            15
        )
        .await,
        "Alice was shown her own statement: {:?}",
        alice.state().attested.get(&bob_key)
    );

    alice.stop();
    carol.stop();
}

/// A voice note attached with the paperclip carries its waveform.
///
/// SIP-18: "A voice note's `bars` are its waveform, so it draws before any
/// audio is fetched." The meta is built by the sender, so this is the end
/// that has to get it right — and it is read back here through
/// `sqex_proto`'s own accessors, not by re-parsing the bytes this wrote.
#[test]
fn a_voice_notes_meta_carries_its_length_and_its_waveform() {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join("note.ogg");
    std::fs::write(&path, an_ogg_opus_note(1000)).expect("write");

    let (meta, preview) =
        sigil_chat::session::preview_of(&path, sqex_proto::blob::KIND_VOICE).expect("a voice note");
    // No thumbnail: there is nothing to look at, and the bars are not one.
    assert!(preview.is_empty(), "a voice note got a picture");

    let a = sqex_proto::blob::Attachment {
        kind: sqex_proto::blob::KIND_VOICE,
        blob: [0; 32],
        key: [0; 32],
        size: 1,
        chunks: 1,
        mime: "audio/ogg".into(),
        meta,
        preview: Vec::new(),
    };
    let ms = a.duration_ms().expect("a length");
    assert!((900..=1000).contains(&ms), "{ms} ms");
    let bars = a.waveform().expect("a waveform");
    assert_eq!(bars.len(), 48);
    // A tone is not silence, and SIP-15 spells silence 255.
    assert!(
        bars.iter().any(|&b| b < 200),
        "the whole note measured as silence: {bars:?}"
    );
}

/// The negative control: the bars measure the *audio*. A silent note comes
/// out quieter at its loudest than a tone is at its quietest — on SIP-15's
/// scale, where a bigger number is quieter.
///
/// Compared against each other rather than against a threshold, because a
/// threshold would be a guess about libopus: it reconstructs digital
/// silence as very faint noise rather than as nought, so "silence is
/// exactly 255" is a test that would fail on a true statement.
#[test]
fn the_bars_measure_the_audio_and_not_the_file() {
    let quiet = bars_of(an_ogg_opus_note(0));
    let loud = bars_of(an_ogg_opus_note(50));
    let quietest_of_the_tone = loud.iter().copied().max().expect("bars");
    let loudest_of_the_silence = quiet.iter().copied().min().expect("bars");
    assert!(
        loudest_of_the_silence > quietest_of_the_tone,
        "silence at its loudest ({loudest_of_the_silence}) was not quieter than the \
         tone at its quietest ({quietest_of_the_tone})"
    );
}

fn bars_of(note: Vec<u8>) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join("note.ogg");
    std::fs::write(&path, note).expect("write");
    let (meta, _) =
        sigil_chat::session::preview_of(&path, sqex_proto::blob::KIND_VOICE).expect("a voice note");
    sqex_proto::blob::Attachment {
        kind: sqex_proto::blob::KIND_VOICE,
        blob: [0; 32],
        key: [0; 32],
        size: 1,
        chunks: 1,
        mime: "audio/ogg".into(),
        meta,
        preview: Vec::new(),
    }
    .waveform()
    .expect("a waveform")
    .to_vec()
}

/// A second of Ogg-encapsulated Opus: a 440 Hz tone, or silence when
/// `amplitude` is nought. Built here rather than checked in as a fixture so
/// the bytes are made by libopus and the page layout by this test.
fn an_ogg_opus_note(amplitude_pct: u32) -> Vec<u8> {
    const RATE: u32 = 48_000;
    const FRAME: usize = 960;
    let mut encoder =
        opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).expect("encoder");
    let amplitude = amplitude_pct as f32 / 100.0;
    let mut packets: Vec<Vec<u8>> = Vec::new();
    for f in 0..50 {
        let pcm: Vec<f32> = (0..FRAME)
            .map(|i| {
                let t = (f * FRAME + i) as f32 / RATE as f32;
                (t * 440.0 * std::f32::consts::TAU).sin() * amplitude
            })
            .collect();
        packets.push(encoder.encode_vec_float(&pcm, 4000).expect("encode"));
    }
    let mut head = Vec::new();
    head.extend_from_slice(b"OpusHead");
    head.push(1);
    head.push(1);
    head.extend_from_slice(&0u16.to_le_bytes());
    head.extend_from_slice(&RATE.to_le_bytes());
    head.extend_from_slice(&0u16.to_le_bytes());
    head.push(0);
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&4u32.to_le_bytes());
    tags.extend_from_slice(b"none");
    tags.extend_from_slice(&0u32.to_le_bytes());

    let page = |header_type: u8, granule: u64, seq: u32, packet: &[u8]| -> Vec<u8> {
        let mut segments = Vec::new();
        let mut left = packet.len();
        loop {
            let take = left.min(255);
            segments.push(take as u8);
            left -= take;
            if take < 255 {
                break;
            }
            if left == 0 {
                segments.push(0);
                break;
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"OggS");
        out.push(0);
        out.push(header_type);
        out.extend_from_slice(&granule.to_le_bytes());
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(segments.len() as u8);
        out.extend_from_slice(&segments);
        out.extend_from_slice(packet);
        out
    };
    let mut out = page(0x02, 0, 0, &head);
    out.extend_from_slice(&page(0x00, 0, 1, &tags));
    let mut granule = 0u64;
    for (seq, (i, packet)) in (2u32..).zip(packets.iter().enumerate()) {
        granule += FRAME as u64;
        let last = i + 1 == packets.len();
        out.extend_from_slice(&page(if last { 0x04 } else { 0x00 }, granule, seq, packet));
    }
    out
}

/// **What the store archived comes back as an earlier copy** (SIP-60 §The
/// client keeps what it read).
///
/// A direct message opened twice, or one folded because it turned out to be
/// a stray, ends the channel's sequence space; the client archives what it
/// read of the old incarnation and reads the rest of it from
/// `/channel/folded`. The library has done that since 0.93.1 and sigil
/// called `Chat::earlier` nowhere, so all of it sat on the disc unread
/// while the reader was told the conversation had been destroyed.
///
/// The fold itself takes two exchanges and belongs to the library's own
/// `fold_flow`. What is proved here is sigil's half: a channel whose
/// sequence space restarted has its archived messages on screen, above,
/// marked as an earlier copy.
#[tokio::test]
async fn what_the_store_archived_comes_back_as_an_earlier_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, a_id) = signer(61);
    let (b_signer, b_id) = signer(62);
    let b_store = dir.path().join("b.db");

    let alice = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let bob = start_at(endpoint, b_signer, &b_store);
    assert!(
        until(
            || alice.state().me == Some(a_id) && bob.state().me == Some(b_id),
            15
        )
        .await,
        "both sessions should come up: {:?}",
        alice.state().trouble
    );
    bob.send(Cmd::OpenDm(a_id));
    alice.send(Cmd::OpenDm(b_id));
    assert!(
        until(
            || alice.state().open.is_some() && bob.state().open.is_some(),
            15
        )
        .await,
        "both should have the conversation open"
    );
    alice.send(Cmd::Send("said before the sequence space ended".into()));
    assert!(
        until(
            || {
                bob.state()
                    .lines
                    .iter()
                    .any(|l| l.text == "said before the sequence space ended")
            },
            20,
        )
        .await,
        "Bob should have read it before it is archived: {:?}",
        bob.state().trouble
    );
    let channel = bob.state().open.expect("open");

    // Bob's client stops, and his store is told what a fold tells it: this
    // channel's sequence space has ended. That archives what he read.
    alice.stop();
    let closing = bob.close();
    assert!(
        until(|| closing.is_finished(), 10).await,
        "the store lock should be released"
    );
    {
        let (seed, _) = signer(62);
        let mut store =
            sqex_chat::store::Store::open(&seed.seed(), Some(&b_store)).expect("the store opens");
        store.scope_to(&PubKey::new(server_pub)).expect("scoped");
        store
            .reset_sequence_space(&channel)
            .expect("the sequence space restarts");
        // The control for the whole test: if this archived nothing there is
        // nothing for sigil to fail to draw, and every assertion below would
        // be about an empty list.
        assert!(
            !store
                .message_history(&channel)
                .expect("read back")
                .is_empty(),
            "nothing was archived, so this test cannot say anything"
        );
    }

    // Bob comes back.
    let (b_signer, _) = signer(62);
    let bob = start_at(endpoint, b_signer, &b_store);
    assert!(
        until(|| bob.state().me == Some(b_id), 15).await,
        "Bob's session should come up again"
    );
    bob.send(Cmd::Show(channel));
    let kept = until(
        || {
            bob.state().copies.iter().flatten().any(|l| {
                l.text == "said before the sequence space ended" && l.earlier && l.receipt.is_none()
            })
        },
        20,
    )
    .await;
    assert!(
        kept,
        "what was archived was not offered as an earlier copy: {:?}",
        bob.state().copies
    );
    bob.stop();
}
