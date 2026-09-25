//! A conversation is folded when it is needed, and not before.
//!
//! Listing what this machine holds used to mean folding every conversation in
//! it -- every row read, every sealed body opened -- before the window was
//! drawn, for conversations nobody was going to open. The list now comes off
//! three cheap store reads and a twenty-row tail for the preview line, and a
//! conversation is folded whole the moment it is opened, polled or searched.
//!
//! `ChatState::folds` is the readout that can tell the two apart: nothing else
//! observable distinguishes a list drawn from a fold from one drawn without.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};

async fn exchange_in(dir: &Path) -> (SocketAddr, [u8; 32]) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        // **SIP-56's post limit turned off, and only here.** It is 30 a
        // minute *per channel*, which is generous for a conversation and not
        // for a test that has to put more than a page into one room in a few
        // seconds. Zero means unlimited; what is being tested is folding, not
        // the limiter, and sqexd has its own tests for that.
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\ndomain = \"e.test\"\n\
         [limits]\nposts = [0, 0]\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
    );
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind_with(
        config,
        None,
        signing_key,
        sqexd::relay::Find::Fixed(Default::default()),
    )
    .await
    .unwrap();
    let addr = bound.local_addr;
    let server_pub = bound.public_key.to_bytes();
    tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });
    (addr, server_pub)
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

/// How many conversations the store is given, and what is said in each.
const ROOMS: [&str; 6] = ["one", "two", "three", "four", "five", "six"];

/// What an ordinary room holds. Small, because every one of these is a round
/// trip to the exchange and the point of the test is elsewhere.
const LINES: usize = 12;

/// The room the needle is said in, and how much is said on top of it.
///
/// Deeper than a preview tail is folded from **and** deeper than a page is
/// drawn from: the first so that the needle is a word only a whole fold can
/// find, the second so that there is something behind the window to reach
/// for. One room is this big, not all of them.
///
/// The first version of this put the needle in the *last* line, which the
/// preview tail holds -- so the search found it with the search's own fold
/// taken out, and the control passed. A tail makes a search partly work,
/// which is worse than not working at all.
const DEEP: &str = "two";
const DEEP_LINES: usize = sigil_chat::session::PAGE + 5;
const NEEDLE: &str = "pomegranate";

/// Fill a store with six rooms, each with something said in it, and hand back
/// the path it lives at.
async fn a_store_with_six_rooms(dir: &Path, live: Endpoint, who: u8) -> std::path::PathBuf {
    let store = dir.join("alice.db");
    let (a_signer, alice) = signer(who);
    let chat = start_at(live, a_signer, &store);
    assert!(
        until(
            || chat.state().me == Some(alice) && chat.state().link == LinkState::Up,
            15
        )
        .await,
        "the session should come up: {:?}",
        chat.state().trouble
    );
    for room in ROOMS {
        // Public, so that the exchange holds the name in the clear and the
        // list is right from the first frame -- and so each room has a
        // *topic*, which is one of the facts the store now remembers.
        chat.send(Cmd::NewPublic {
            name: room.to_string(),
            topic: format!("about {room}"),
        });
        assert!(
            until(
                || chat.state().conversations.iter().any(|c| c.label == room),
                20
            )
            .await,
            "the room {room} was not created: {:?}",
            chat.state()
                .conversations
                .iter()
                .map(|c| c.label.clone())
                .collect::<Vec<_>>()
        );
        let channel = chat
            .state()
            .conversations
            .iter()
            .find(|c| c.label == room)
            .map(|c| c.channel)
            .unwrap();
        chat.send(Cmd::Show(channel));
        // Said first, and then buried under more lines than the preview tail
        // reaches back through.
        let lines = if room == DEEP {
            chat.send(Cmd::Send(NEEDLE.to_string()));
            DEEP_LINES
        } else {
            LINES
        };
        for i in 0..lines {
            chat.send(Cmd::Send(format!("{room} line {i}")));
        }
        // Drawn plus behind the window: the transcript opens on a page, so
        // counting only what is on screen would wait for a number it can
        // never reach.
        assert!(
            until(
                || chat.state().lines.len() + chat.state().earlier >= lines,
                60
            )
            .await,
            "the exchange did not take the lines for {room}: {} shown, {} behind",
            chat.state().lines.len(),
            chat.state().earlier
        );
    }
    chat.stop();
    tokio::time::sleep(Duration::from_millis(300)).await;
    store
}

/// The room a name belongs to, in the list this session published.
fn channel_of(chat: &ChatHandle, room: &str) -> [u8; 32] {
    chat.state()
        .conversations
        .iter()
        .find(|c| c.label == room)
        .unwrap_or_else(|| panic!("no room called {room} in the list"))
        .channel
}

/// **The list, drawn from a store, folding nothing.**
///
/// At an address nobody answers, so that every fold this session does is one
/// it chose: nothing can arrive to provoke one.
#[tokio::test]
async fn the_list_is_drawn_without_folding_a_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub) = exchange_in(dir.path()).await;
    let live = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let store = a_store_with_six_rooms(dir.path(), live, 0x91).await;

    let dead = Endpoint {
        address: "127.0.0.1:1".parse().unwrap(),
        server: PubKey::new(server_pub),
    };
    let (a_signer, _) = signer(0x91);
    let chat = start_at(dead, a_signer, &store);
    assert!(
        until(|| chat.state().conversations.len() >= ROOMS.len(), 5).await,
        "the rooms were not drawn from the disc: {:?}",
        chat.state()
            .conversations
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>()
    );
    let s = chat.state();
    assert_ne!(
        s.link,
        LinkState::Up,
        "the exchange at 127.0.0.1:1 answered?"
    );
    assert_eq!(
        s.folds, 0,
        "drawing the list folded {} conversations; it needs none",
        s.folds
    );
    for room in ROOMS {
        let row = s
            .conversations
            .iter()
            .find(|c| c.label == room)
            .unwrap_or_else(|| panic!("{room} is missing from the list"));
        assert!(
            row.preview
                .as_deref()
                .is_some_and(|p| p.contains(&format!("{room} line"))),
            "{room} has no last line under it: {:?}",
            row.preview
        );
        assert!(row.at.is_some(), "{room} has no time on it");
    }

    // Opening one folds one, and draws the whole of it.
    let one = channel_of(&chat, "one");
    chat.send(Cmd::Show(one));
    assert!(
        until(
            || chat.state().lines.len() + chat.state().earlier >= LINES,
            10
        )
        .await,
        "the transcript was not drawn from the disc: {} lines, {} behind",
        chat.state().lines.len(),
        chat.state().earlier
    );
    assert_eq!(
        chat.state().folds,
        1,
        "opening one conversation folded {}",
        chat.state().folds
    );

    // **A search reaches what was never opened.** Offline, nothing else is
    // going to fold the rest, so the search folds them itself.
    chat.send(Cmd::Search(NEEDLE.to_string()));
    assert!(
        until(|| chat.state().searched_messages, 10).await,
        "the search did not answer"
    );
    let hits = chat.state().hits;
    assert_eq!(
        hits.len(),
        1,
        "the word is at the top of one conversation, which was never opened: {:?}",
        hits.iter().map(|h| h.label.clone()).collect::<Vec<_>>()
    );
    assert_eq!(hits[0].label, DEEP, "the hit is in the wrong conversation");
    assert_eq!(
        chat.state().folds,
        ROOMS.len(),
        "a search must have folded every conversation it searched"
    );
}

/// **And with an exchange answering, nobody has to ask.** The sweep already
/// asks about every conversation once, four a tick, and a poll cannot be
/// handed a tail -- so the folding happens in the background, off the path
/// the window is drawn on.
#[tokio::test]
async fn the_sweep_folds_the_rest_with_nobody_asking() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub) = exchange_in(dir.path()).await;
    let live = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let store = a_store_with_six_rooms(dir.path(), live, 0x92).await;

    let (a_signer, alice) = signer(0x92);
    let chat = start_at(live, a_signer, &store);
    assert!(
        until(|| chat.state().me == Some(alice), 15).await,
        "the session should come up again: {:?}",
        chat.state().trouble
    );
    assert!(
        until(|| chat.state().folds >= ROOMS.len(), 30).await,
        "the sweep folded {} of {} conversations",
        chat.state().folds,
        ROOMS.len()
    );
}

/// **A page of earlier messages, at an exchange that never answers.**
///
/// The window over a fold is a number and the entries are on the disc, so
/// reaching the top and asking for more is answerable with no connection at
/// all. It was not: the command sat in the deferred queue behind a dial that
/// never completed, and the control did nothing for ever.
#[tokio::test]
async fn earlier_messages_are_reached_without_an_exchange() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub) = exchange_in(dir.path()).await;
    let live = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let store = a_store_with_six_rooms(dir.path(), live, 0x93).await;

    let dead = Endpoint {
        address: "127.0.0.1:1".parse().unwrap(),
        server: PubKey::new(server_pub),
    };
    let (a_signer, _) = signer(0x93);
    let chat = start_at(dead, a_signer, &store);
    assert!(
        until(|| chat.state().conversations.len() >= ROOMS.len(), 5).await,
        "the rooms were not drawn from the disc"
    );
    let deep = channel_of(&chat, DEEP);
    chat.send(Cmd::Show(deep));
    // The room holds one more than a page, so there is something behind the
    // window to reach.
    assert!(
        until(|| chat.state().earlier > 0, 5).await,
        "nothing was behind the window: {} lines, {} earlier",
        chat.state().lines.len(),
        chat.state().earlier
    );
    let behind = chat.state().earlier;
    chat.send(Cmd::Earlier);
    assert!(
        until(|| chat.state().earlier < behind, 5).await,
        "asking for earlier messages with no exchange produced none: still {} behind",
        chat.state().earlier
    );
}

/// **What a large store costs, measured rather than asserted.**
///
/// Ignored: it writes a store far larger than anything on this machine holds
/// (the biggest channel here on 2026-09-25 was 355 messages), and it is a
/// measurement, not a rule. Run it with
///
/// ```text
/// cargo test -p sigil-chat --test lazy_fold_session -- --ignored --nocapture
/// ```
///
/// It prints two numbers: how long the conversation list takes to appear, and
/// how long folding every conversation takes. The second is what the old start
/// paid **before drawing anything**; the first is what it pays now.
#[tokio::test]
#[ignore]
async fn what_a_large_store_costs_to_list() {
    use sqex_chat::store::{Kept, Store};

    // Fourteen times the biggest channel on this machine, twenty times over
    // -- a store larger than anything here holds, which is the point: what is
    // being shown is how the two costs scale, and one of them does not.
    const CHANNELS: usize = 20;
    const EACH: usize = 5_000;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.db");
    let sk = SigningKey::from_bytes(&[0x9f; 32]);
    let me = PubKey::new(sk.verifying_key().to_bytes());
    // A real exchange's key, for an exchange at an address nobody answers.
    // **The key has to be one that could be dialled**: a made-up one fails
    // the dial outright, the session task ends with it, and then nothing is
    // served from the disc at all -- which looked, from outside, exactly like
    // a search that folded nothing.
    let (_addr, server_pub) = exchange_in(dir.path()).await;
    let exchange = PubKey::new(server_pub);

    let built = std::time::Instant::now();
    {
        let mut store = Store::open(&sk.to_bytes(), Some(&path)).unwrap();
        store.scope_to(&exchange).unwrap();
        for c in 0..CHANNELS {
            let mut channel = [0u8; 32];
            channel[0] = c as u8;
            store
                .put_channel(&channel, true, Some(true), &format!("room {c}"), &[me])
                .unwrap();
            for seq in 1..=EACH as u64 {
                let body = sqex_proto::message::Body::Post(sqex_proto::message::Post::text(
                    &format!("room {c} line {seq}"),
                ));
                store
                    .put_message(
                        &channel,
                        Kept {
                            seq,
                            account: me,
                            posted: 1_000 + seq,
                            kind: sqex_proto::channel::KIND_MEMBER,
                            plain: Some(&body.encode()),
                        },
                    )
                    .unwrap();
            }
        }
    }
    let wrote = built.elapsed();
    eprintln!("wrote {} rows in {wrote:?}", CHANNELS * EACH);

    let dead = Endpoint {
        address: "127.0.0.1:1".parse().unwrap(),
        server: exchange,
    };
    let listing = std::time::Instant::now();
    let chat = start_at(dead, SoftwareSigner::new(sk), &path);
    assert!(
        until(|| chat.state().conversations.len() >= CHANNELS, 120).await,
        "the list never appeared"
    );
    let listed = listing.elapsed();
    eprintln!("listed {CHANNELS} conversations in {listed:?}");

    // A search folds every conversation, which is what the start used to do
    // before it drew a single row.
    let folding = std::time::Instant::now();
    chat.send(Cmd::Search("a-word-nobody-said".into()));
    assert!(
        until(|| chat.state().folds >= CHANNELS, 300).await,
        "only {} of {CHANNELS} were folded",
        chat.state().folds
    );
    let folded = folding.elapsed();

    println!(
        "{CHANNELS} channels x {EACH} messages ({} rows, written in {wrote:?}):\n  \
         list: {listed:?}\n  fold every conversation: {folded:?}",
        CHANNELS * EACH
    );
}
