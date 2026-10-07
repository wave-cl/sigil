//! SIP-88 feeds, through a real session against a real exchange.
//!
//! The unit tests in `sigil_chat::feed` cover the rules a timeline is built
//! by — the clock clamp, the merge, what a tombstone means. None of them
//! crosses the wire, and a client that got every rule right and never asked
//! an exchange anything would pass all of them.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::feed::Serial;
use sigil_chat::{ChatHandle, Cmd, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};

async fn server_in(dir: &Path) -> (SocketAddr, [u8; 32], tokio::task::JoinHandle<()>) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nlimits = {{ posts = [0, 0], signals = [0, 0], joins = [0, 0], creates = [0, 0], uploads = [0, 0] }}\n",
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
    let key = PubKey::new(sk.verifying_key().to_bytes());
    (SoftwareSigner::new(sk), key)
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

/// **A post reaches somebody who follows it.**
///
/// This is the whole of SIP-88 from a client's side in one path: an author
/// signs at `newest + 1` and appends; a reader keeps the author's key in a
/// list no exchange is told about; `/feed/since` says the feed moved; the
/// reader reads forward from what it held. Every unit test in
/// `sigil_chat::feed` is about what happens *after* those bytes arrive, so
/// all of them pass on a client that never asked.
#[tokio::test]
async fn a_post_reaches_somebody_who_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, author) = signer(81);
    let (b_signer, reader_key) = signer(82);
    let author_app = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let reader = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || author_app.state().me == Some(author) && reader.state().me == Some(reader_key),
            15
        )
        .await,
        "both sessions should come up: {:?}",
        author_app.state().trouble
    );

    author_app.send(Cmd::Publish {
        text: "the first thing anybody said here".into(),
        cites: None,
    });
    assert!(
        until(|| author_app.state().my_feed > Serial(0), 20).await,
        "the author's own feed never moved: {:?}",
        author_app.state().trouble
    );

    // **The control.** Before following, the reader's timeline is empty — so
    // what the assertion after it sees is the follow working, and not a
    // client that shows everything it can reach.
    reader.send(Cmd::RefreshFeeds);
    assert!(
        !until(|| !reader.state().timeline.is_empty(), 3).await,
        "a timeline with posts in it before anybody was followed"
    );

    reader.send(Cmd::Follow(author));
    assert!(
        until(|| !reader.state().timeline.is_empty(), 20).await,
        "the post never reached the reader: {:?}",
        reader.state().trouble
    );
    let seen = reader.state().timeline;
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].who, author, "somebody else's post");
    assert_eq!(seen[0].text, "the first thing anybody said here");
    assert_eq!(seen[0].serial, Serial(1), "the author's own first number");
    assert!(!seen[0].mine, "the reader's own, apparently");
    assert_eq!(seen[0].gone, None);

    // And the follow is the reader's: the exchange was told nothing, which is
    // the property SIP-88 §The follow list exists for. What can be asserted
    // from here is the half that is this client's — the list is in this
    // store, and `follows` is what the interface draws from.
    let follows = reader.state().follows;
    assert_eq!(
        follows.iter().map(|f| f.account).collect::<Vec<_>>(),
        vec![author],
        "the follow list is not what was followed"
    );

    // A second post arrives on the poll, without the reader asking again.
    author_app.send(Cmd::Publish {
        text: "and a second".into(),
        cites: None,
    });
    assert!(
        until(|| author_app.state().my_feed >= Serial(2), 20).await,
        "the second post was not appended: {:?}",
        author_app.state().trouble
    );
    reader.send(Cmd::RefreshFeeds);
    assert!(
        until(|| reader.state().timeline.len() == 2, 20).await,
        "the second post never arrived: {:?}",
        reader.state().timeline
    );
    // Newest first, which is how a feed is read.
    let seen = reader.state().timeline;
    assert_eq!(
        seen.iter().map(|p| p.serial).collect::<Vec<_>>(),
        vec![Serial(2), Serial(1)],
        "a timeline in the wrong order"
    );
}

/// **Unfollowing takes the posts off the screen.**
///
/// A person who stops following somebody and goes on seeing them has no way
/// to tell whether the act worked — and the act is purely local, so there is
/// no exchange to blame for it.
#[tokio::test]
async fn unfollowing_takes_the_posts_off_the_timeline() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, author) = signer(83);
    let (b_signer, reader_key) = signer(84);
    let author_app = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    let reader = start_at(endpoint, b_signer, &dir.path().join("b.db"));
    assert!(
        until(
            || author_app.state().me == Some(author) && reader.state().me == Some(reader_key),
            15
        )
        .await,
        "both sessions should come up"
    );
    author_app.send(Cmd::Publish {
        text: "something to stop following".into(),
        cites: None,
    });
    assert!(
        until(|| author_app.state().my_feed > Serial(0), 20).await,
        "nothing was published: {:?}",
        author_app.state().trouble
    );
    reader.send(Cmd::Follow(author));
    assert!(
        until(|| !reader.state().timeline.is_empty(), 20).await,
        "the post never arrived, so this says nothing about unfollowing"
    );

    reader.send(Cmd::Unfollow(author));
    assert!(
        until(|| reader.state().timeline.is_empty(), 20).await,
        "the posts are still on the timeline after unfollowing: {:?}",
        reader.state().timeline
    );
    assert!(
        reader.state().follows.is_empty(),
        "and the follow list still names them"
    );
}

/// **A post the author withdrew stops showing its words.**
///
/// SIP-88 §Withdrawal: the body goes and `body_hash` and `sig` stay, so the
/// tombstone still verifies. What a reader must see is that the post was
/// there and is not any more — not a gap, and not the words.
#[tokio::test]
async fn a_withdrawn_post_keeps_its_place_and_loses_its_words() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub, _h) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, author) = signer(85);
    let app = start_at(endpoint, a_signer, &dir.path().join("a.db"));
    assert!(
        until(|| app.state().me == Some(author), 15).await,
        "the session should come up"
    );
    app.send(Cmd::Publish {
        text: "said in haste".into(),
        cites: None,
    });
    assert!(
        until(|| app.state().my_feed > Serial(0), 20).await,
        "nothing was published: {:?}",
        app.state().trouble
    );
    app.send(Cmd::Follow(author));
    assert!(
        until(
            || app
                .state()
                .timeline
                .iter()
                .any(|p| p.text == "said in haste"),
            20
        )
        .await,
        "the author cannot see their own post: {:?}",
        app.state().timeline
    );

    app.send(Cmd::Withdraw(Serial(1)));
    assert!(
        until(
            || app
                .state()
                .timeline
                .iter()
                .any(|p| p.serial == Serial(1) && p.gone.is_some()),
            20
        )
        .await,
        "the post is still showing its words after being withdrawn: {:?}",
        app.state().timeline
    );
    let seen = app.state().timeline;
    let post = seen.iter().find(|p| p.serial == Serial(1)).expect("a post");
    assert!(
        post.text.is_empty(),
        "the words survived the withdrawal: {post:?}"
    );
    assert!(
        post.mine,
        "the author's own post is not marked as theirs, so no control to \
         withdraw it would be drawn"
    );
}
