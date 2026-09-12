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
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nname_registration = \"{names}\"\n",
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

    alice.send(Cmd::Call);
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

    alice.send(Cmd::Call);
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
