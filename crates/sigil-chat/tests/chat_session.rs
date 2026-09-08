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
    // has to happen because SIP-30 said so rather than because the periodic
    // rebuild came round. Those are different mechanisms and only one of them
    // is fast enough to be a chat client, so the wait is pinned well inside
    // the backstop -- and asserted against it, so that lowering the backstop
    // cannot quietly turn this back into a test of the backstop.
    const WAIT: u64 = 10;
    assert!(
        std::time::Duration::from_secs(WAIT) * 2 < sigil_chat::session::BACKSTOP,
        "this test no longer proves the event path: it waits {WAIT}s against a          backstop of {:?}",
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
                .any(|c| c.group && !c.public)
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
        .find(|c| c.group && !c.public)
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
        until(|| alice.state().conversations.iter().any(|c| c.public), 15).await,
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
                .any(|c| c.channel == found.channel && c.public)
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
                c.iter().any(|c| c.group && !c.public) && c.iter().any(|c| c.public)
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
    // **Bob opens nothing.** That is the whole point: the open conversation is
    // polled every tick whatever happens, so a test where the callee is
    // already looking at the conversation proves only that polling works. It
    // passed with every event handler disabled, which is how I found out.
    // Ringing has to reach somebody who is looking somewhere else.
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
                .is_some_and(|n| n.contains("ada is yours here")),
            20
        )
        .await,
        "the claim was not granted, or not said: {:?}",
        alice.state().note
    );

    bob.send(Cmd::ClaimName("ada".into()));
    assert!(
        until(
            || bob
                .state()
                .note
                .is_some_and(|n| n.contains("already somebody else")),
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
