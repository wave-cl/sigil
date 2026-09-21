//! SIP-60 from sigil: Alice at A writes to Bob at B by `bob@b.test`. Her
//! session presents her Move at A on the first reach (her home), finds Bob
//! through A, and the conversation is created at the lower key's home; Bob's
//! session at B sees it. A session that is a visitor at its exchange -- a
//! store filed under another one -- does not reach out from there. And the
//! directory: a public room at B is found from A as living at b.test, not
//! held here; from B, as held here.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqex_proto::home::{Move, Moving};
use sqexd::config::FileConfig;
use sqexd::server::Server;
use sqnr_core::{PubKey, SignedTransaction, SoftwareSigner, Transaction};

/// The administrator every exchange here has, for labelling a peer.
const ADMIN: u8 = 0xad;

struct Exchange {
    endpoint: Endpoint,
    key: PubKey,
    #[allow(dead_code)]
    server: Arc<Server>,
    _dir: tempfile::TempDir,
}

fn key_in(dir: &Path) -> PubKey {
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(dir.join("host_key"), hex::encode(server_sk.to_bytes())).unwrap();
    let vk = SigningKey::from_bytes(&server_sk.to_bytes()).verifying_key();
    PubKey::new(vk.to_bytes())
}

fn free_port() -> SocketAddr {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap()
}

/// An exchange that lists `peers` (SIP-35/39) and finds `found` without DNS.
async fn exchange_in(
    dir: tempfile::TempDir,
    listen: SocketAddr,
    domain: &str,
    peers: &[PubKey],
    found: &[(&str, PubKey, SocketAddr)],
) -> Option<Exchange> {
    let list = peers
        .iter()
        .map(|p| format!("{:?}", p.to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    let key_path = dir.path().join("host_key");
    let config_toml = format!(
        "listen = {:?}\nkey_file = {:?}\nstate_file = {:?}\nadmins = [\"{}\"]\n\
         welcome_channel = \"\"\ndomain = {domain:?}\nreplication_peers = [{list}]\n\
         home_secs = 1\ndirectory_secs = 2\n",
        listen.to_string(),
        key_path.to_string_lossy(),
        dir.path().join("sqex.state").to_string_lossy(),
        signer(ADMIN).1,
    );
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _pub) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let map = found
        .iter()
        .map(|(d, k, a)| ((*d).to_string(), (*k, *a)))
        .collect();
    let bound = sqexd::bind_with(config, None, signing_key, sqexd::relay::Find::Fixed(map))
        .await
        .ok()?;
    let ex = Exchange {
        endpoint: Endpoint {
            address: bound.local_addr,
            server: bound.public_key,
        },
        key: bound.public_key,
        server: Arc::clone(&bound.server),
        _dir: dir,
    };
    tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });
    Some(ex)
}

/// Two exchanges that list each other, each finding the other by name.
async fn pair() -> (Exchange, Exchange) {
    for _ in 0..5 {
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a_key = key_in(a_dir.path());
        let b_key = key_in(b_dir.path());
        let (a_at, b_at) = (free_port(), free_port());
        let Some(a) =
            exchange_in(a_dir, a_at, "a.test", &[b_key], &[("b.test", b_key, b_at)]).await
        else {
            continue;
        };
        let Some(b) =
            exchange_in(b_dir, b_at, "b.test", &[a_key], &[("a.test", a_key, a_at)]).await
        else {
            continue;
        };
        assert_eq!((a.key, b.key), (a_key, b_key));
        // SIP-39's peer list, added by the administrator with a domain
        // label: SIP-16 §Federated directory reads the directories of the
        // peers that have one, and a seeded key is not relabelled.
        label_peer(&a, b_key, "b.test").await;
        label_peer(&b, a_key, "a.test").await;
        return (a, b);
    }
    panic!("no free port pair in five tries");
}

/// `sqex admin peer add <key> --label <domain>`, as the administrator.
async fn label_peer(ex: &Exchange, key: PubKey, label: &str) {
    let (admin, _, admin_seed) = signer(ADMIN);
    let mut c = sqnr::Client::connect_as(
        ex.endpoint.address,
        ex.endpoint.server.as_bytes(),
        &admin_seed,
    )
    .await
    .unwrap();
    let (cs, nonce_bytes) = c.get("/admin/challenge").await.unwrap();
    assert_eq!(cs, 200);
    let mut nonce = [0u8; 32];
    nonce.copy_from_slice(&nonce_bytes);
    let txn = Transaction {
        server: ex.endpoint.server,
        nonce,
        ops: vec![
            sqex_proto::Op::PeerAdd {
                key,
                label: Some(label.into()),
            }
            .to_operation(),
        ],
    };
    let (code, body) = c
        .post(
            "/admin/command",
            SignedTransaction::create(txn, &admin).encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
}

fn signer(b: u8) -> (SoftwareSigner, PubKey, [u8; 32]) {
    let sk = SigningKey::from_bytes(&[b; 32]);
    let public = PubKey::new(sk.verifying_key().to_bytes());
    let seed = sk.to_bytes();
    (SoftwareSigner::new(sk), public, seed)
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

fn start_at(ex: &Exchange, signer: SoftwareSigner, store: &Path) -> ChatHandle {
    session::start(ex.endpoint, signer, Some(store.to_path_buf()), || {})
}

async fn up(chat: &ChatHandle, me: PubKey) {
    assert!(
        until(
            || chat.state().me == Some(me) && chat.state().link == LinkState::Up,
            15
        )
        .await,
        "the session should come up: {:?}",
        chat.state().trouble
    );
}

/// Where an exchange records an account's home, asked as a client would.
async fn home_on_record(ex: &Exchange, seed: &[u8; 32], account: &PubKey) -> Option<PubKey> {
    let mut c = sqnr::Client::connect_as(ex.endpoint.address, ex.endpoint.server.as_bytes(), seed)
        .await
        .unwrap();
    let (code, body) = c
        .post("/account/home", account.as_bytes().to_vec())
        .await
        .unwrap();
    // A home is on record only by a signed Move: `since = 0` is the
    // exchange's own guess, which is not a record.
    (code == 200)
        .then(|| sqex_proto::home::Homed::decode(&body).unwrap())
        .filter(|h| h.since != 0)
        .map(|h| h.home)
}

#[tokio::test]
async fn alice_at_a_writes_to_bob_at_b_by_name_and_domain() {
    let (a, b) = pair().await;
    // Alice's key is the lower one, so the conversation lives at A; either
    // way the point is the reach, not the address.
    let (a_signer, alice, a_seed) = signer(0x60);
    let (b_signer, bob, _b_seed) = signer(0x61);
    let dir = tempfile::tempdir().unwrap();

    let bobs = start_at(&b, b_signer, &dir.path().join("bob.db"));
    up(&bobs, bob).await;
    // Bob lives at B: on record there by his own reach out (to nobody in
    // particular -- a name B cannot find is enough to present the Move).
    bobs.send(Cmd::OpenRemote {
        target: format!("{alice}@a.test"),
        identity: None,
    });
    let mut on_record = false;
    for _ in 0..150 {
        if home_on_record(&b, &_b_seed, &bob).await == Some(b.key) {
            on_record = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(on_record, "Bob's Move should be on record at B");

    let alices = start_at(&a, a_signer, &dir.path().join("alice.db"));
    up(&alices, alice).await;
    assert_eq!(
        home_on_record(&a, &a_seed, &alice).await,
        None,
        "nothing is presented on start"
    );

    // Her identity file, so the home is recorded beside it (SIP-60 §When a
    // client presents a Move unasked, 2026-09-21): nothing there before.
    let identity = dir.path().join("identity-alice");
    std::fs::write(&identity, "x").unwrap();
    assert_eq!(sqex_proto::home_file::load(&identity), None);
    alices.send(Cmd::OpenRemote {
        target: format!("{bob}@b.test"),
        identity: Some(identity.clone()),
    });
    assert!(
        until(|| alices.state().open.is_some(), 20).await,
        "the conversation should open: {:?}",
        alices.state().trouble
    );
    assert_eq!(
        home_on_record(&a, &a_seed, &alice).await,
        Some(a.key),
        "the first reach put Alice's home on record at A"
    );
    // Recorded by key: this harness dials by address (`Dial::At`), so the
    // session has no domain to write; the interface's sessions discover by
    // name and record both.
    let recorded = sqex_proto::home_file::load(&identity).expect("the home was recorded");
    assert_eq!(recorded.key, Some(a.key));
    assert_eq!(recorded.domain, None);
    // Reached: Bob's session at B sees what Alice says from A.
    alices.send(Cmd::Send("hello from a".into()));
    assert!(
        until(
            || bobs.state().lines.iter().any(|l| l.text == "hello from a"),
            20
        )
        .await,
        "Bob should receive it: {:?}",
        bobs.state().trouble
    );
    // And the conversation is in her list, with Bob as the other party.
    assert!(
        until(
            || alices
                .state()
                .conversations
                .iter()
                .any(|c| c.peer == Some(bob)),
            10
        )
        .await,
        "the conversation is in Alice's list: {:?}",
        alices.state().conversations
    );
    alices.stop();
    bobs.stop();
}

/// A store filed under A, connected at B: a visitor, who does not reach out
/// from B -- that would present a Move naming B and move the home there.
#[tokio::test]
async fn a_visitor_does_not_reach_out_from_the_exchange_it_is_visiting() {
    let (a, b) = pair().await;
    let (signer_a, carol, seed) = signer(0x62);
    let dir = tempfile::tempdir().unwrap();
    // Homed at A by a Move presented there.
    let mut c = sqnr::Client::connect_as(a.endpoint.address, a.endpoint.server.as_bytes(), &seed)
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (code, _) = c
        .post(
            "/account/move",
            Moving {
                mv: Move::sign(&seed, &a.key, now),
                domain: "a.test".into(),
                origins: vec![],
            }
            .encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
    // A session at A first, so the store is filed under A.
    let at_a = start_at(&a, signer_a, &dir.path().join("carol.db"));
    up(&at_a, carol).await;
    at_a.stop();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (signer_b, _, _) = signer(0x62);
    let at_b = start_at(&b, signer_b, &dir.path().join("carol.db"));
    up(&at_b, carol).await;
    at_b.send(Cmd::OpenRemote {
        target: format!("{}@a.test", PubKey::new([9u8; 32])),
        identity: None,
    });
    assert!(
        until(
            || at_b
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("write to people at other exchanges from there")),
            15
        )
        .await,
        "expected the visitor refusal, got {:?}",
        at_b.state().trouble
    );
    // What B answers for Carol is what it learned of her home, never itself.
    assert_ne!(
        home_on_record(&b, &seed, &carol).await,
        Some(b.key),
        "B must not have been given a Move naming itself"
    );
    assert_eq!(home_on_record(&a, &seed, &carol).await, Some(a.key));
    at_b.stop();
}

#[tokio::test]
async fn a_room_at_b_is_found_from_a_as_living_there() {
    let (a, b) = pair().await;
    let (signer_b, bob, _) = signer(0x63);
    let (signer_a, alice, _) = signer(0x64);
    let dir = tempfile::tempdir().unwrap();
    let bobs = start_at(&b, signer_b, &dir.path().join("bob.db"));
    up(&bobs, bob).await;
    bobs.send(Cmd::NewPublic {
        name: "lounge".into(),
        topic: "at b".into(),
    });
    assert!(
        until(|| bobs.state().open.is_some(), 15).await,
        "{:?}",
        bobs.state().trouble
    );
    // B's own directory holds it.
    bobs.send(Cmd::Find("lounge".into()));
    assert!(
        until(
            || bobs.state().searched && !bobs.state().found.is_empty(),
            15
        )
        .await,
        "B lists its own room: {:?}",
        bobs.state().trouble
    );
    let row = bobs.state().found[0].clone();
    assert!(row.here, "held at B");
    assert!(row.domain.is_empty() || row.domain == "b.test");

    // From A, after A has read B's directory (SIP-16 §Federated directory,
    // `directory_secs`; the harness sets nothing, so allow the default).
    let alices = start_at(&a, signer_a, &dir.path().join("alice.db"));
    up(&alices, alice).await;
    let mut found = false;
    for _ in 0..60 {
        alices.send(Cmd::Find("lounge".into()));
        tokio::time::sleep(Duration::from_millis(500)).await;
        if alices.state().found.iter().any(|f| !f.here) {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "A should list B's room as living elsewhere: {:?}",
        alices.state().found
    );
    let row = alices.state().found.into_iter().find(|f| !f.here).unwrap();
    assert_eq!(row.domain, "b.test");
    assert_eq!(row.name, "lounge@b.test");
    alices.stop();
    bobs.stop();
}

/// **A room that lives at another exchange cannot be joined from here.**
///
/// The federated directory (SIP-16) lists B's rooms at A, named
/// `lounge@b.test`, and pressing Join on one is refused: A has no copy, so
/// there is nothing at A to join. That is why the directory offers *Add
/// exchange* against a row that lives elsewhere rather than Join -- the way
/// to a room at B is a session at B, which is what the roster and SIP-85's
/// `via` are for.
///
/// It is recorded as a test because the refusal is the reason for the
/// button, and a refusal nobody has written down is indistinguishable from
/// one nobody has noticed. It goes red the day an exchange pulls a room its
/// member asked for, and then the directory should offer Join.
#[tokio::test]
async fn a_room_that_lives_at_another_exchange_is_not_joinable_from_here() {
    let (a, b) = pair().await;
    let (signer_b, bob, _) = signer(0x66);
    let (signer_a, alice, a_seed) = signer(0x67);
    let dir = tempfile::tempdir().unwrap();
    let bobs = start_at(&b, signer_b, &dir.path().join("bob.db"));
    up(&bobs, bob).await;
    bobs.send(Cmd::NewPublic {
        name: "atrium".into(),
        topic: "at b".into(),
    });
    assert!(
        until(|| bobs.state().open.is_some(), 15).await,
        "{:?}",
        bobs.state().trouble
    );
    bobs.send(Cmd::Send("said at b".into()));

    let alices = start_at(&a, signer_a, &dir.path().join("alice.db"));
    up(&alices, alice).await;
    assert!(
        mine_at(&a, &a_seed).await.is_empty(),
        "A already holds channels for Alice before she has joined anything"
    );

    let mut row = None;
    for _ in 0..60 {
        alices.send(Cmd::Find("atrium".into()));
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Some(f) = alices.state().found.into_iter().find(|f| !f.here) {
            row = Some(f);
            break;
        }
    }
    let row = row.expect("A should list B's room as living elsewhere");
    assert_eq!(row.domain, "b.test");
    assert_eq!(row.name, "atrium@b.test");

    alices.send(Cmd::Join {
        channel: row.channel,
        instance: row.instance,
    });
    assert!(
        until(|| alices.state().trouble.is_some(), 20).await,
        "the join neither succeeded nor failed: {:?}",
        alices.state().note
    );
    let said = alices.state().trouble.unwrap_or_default();
    assert!(
        said.contains("no_such_channel"),
        "the join was refused for some other reason, which this test is not \
         about: {said}"
    );
    assert_eq!(
        alices.state().open,
        None,
        "the room opened after all, and this test is about nothing"
    );
    // And A still holds nothing: there was no copy before and the refused
    // join made none.
    assert!(
        mine_at(&a, &a_seed).await.is_empty(),
        "A holds something for Alice after a refused join"
    );

    alices.stop();
    bobs.stop();
}

/// What an exchange holds for the account that `seed` signs as, asked the way
/// a client asks it and on its own connection.
async fn mine_at(ex: &Exchange, seed: &[u8; 32]) -> Vec<[u8; 32]> {
    let Ok(mut c) =
        sqnr::Client::connect_as(ex.endpoint.address, ex.endpoint.server.as_bytes(), seed).await
    else {
        return Vec::new();
    };
    let Ok((code, body)) = c
        .post(
            "/channel/mine",
            sqex_proto::channel::Mine { offset: 0 }.encode(),
        )
        .await
    else {
        return Vec::new();
    };
    if code != 200 {
        return Vec::new();
    }
    sqex_proto::channel::Mines::decode(&body)
        .map(|m| m.channels.into_iter().map(|r| r.channel).collect())
        .unwrap_or_default()
}
