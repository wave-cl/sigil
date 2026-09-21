//! SIP-48 from the interface's commands: Alice makes a backup key, backs the
//! store up, and a second store of hers -- a fresh install -- gets the
//! conversation back with the 24 words; wrong words are refused with why,
//! and a dropped backup is gone for the next store.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil_chat::{ChatHandle, Cmd, LinkState, session};
use sigil_net::Endpoint;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, SoftwareSigner};

async fn server_in(dir: &Path) -> (SocketAddr, [u8; 32]) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\nwelcome_channel = \"\"\n",
        key_path.to_string_lossy(),
        dir.join("sqex.state").to_string_lossy(),
    );
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _pub) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind(config, None, signing_key).await.unwrap();
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

#[tokio::test]
async fn a_backup_made_here_restores_a_fresh_store_with_the_words() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, server_pub) = server_in(dir.path()).await;
    let endpoint = Endpoint {
        address: addr,
        server: PubKey::new(server_pub),
    };
    let (a_signer, alice) = signer(0x48);
    let (b_signer, bob) = signer(0x49);
    let a1 = start_at(endpoint, a_signer, &dir.path().join("alice-1.db"));
    let bobs = start_at(endpoint, b_signer, &dir.path().join("bob.db"));
    up(&a1, alice).await;
    up(&bobs, bob).await;

    // A conversation with something in it.
    bobs.send(Cmd::OpenDm(alice));
    a1.send(Cmd::OpenDm(bob));
    assert!(
        until(
            || a1.state().open.is_some() && bobs.state().open.is_some(),
            15
        )
        .await
    );
    for text in ["one", "two", "three"] {
        a1.send(Cmd::Send(text.into()));
        assert!(
            until(|| bobs.state().lines.iter().any(|l| l.text == text), 20).await,
            "{text} never arrived: {:?}",
            bobs.state().trouble
        );
    }

    // Nothing backed up yet; no key yet.
    a1.send(Cmd::BackupStatus);
    assert!(
        until(|| a1.state().backup.is_some(), 10).await,
        "{:?}",
        a1.state().trouble
    );
    let b = a1.state().backup.unwrap();
    assert!(!b.has_key && b.held.is_none());

    // The key, as words; then the backup.
    a1.send(Cmd::BackupKey);
    assert!(
        until(
            || a1
                .state()
                .backup
                .as_ref()
                .is_some_and(|b| b.words.is_some()),
            10
        )
        .await,
        "{:?}",
        a1.state().trouble
    );
    let words = a1.state().backup.unwrap().words.unwrap();
    assert_eq!(words.len(), 24);
    a1.send(Cmd::HideBackupKey);
    assert!(
        until(
            || a1
                .state()
                .backup
                .as_ref()
                .is_some_and(|b| b.words.is_none()),
            5
        )
        .await
    );
    a1.send(Cmd::BackupNow);
    assert!(
        until(
            || a1
                .state()
                .backup
                .as_ref()
                .is_some_and(|b| b.held.as_ref().is_some_and(|h| h.generation == 1)),
            15
        )
        .await,
        "the exchange should hold generation 1: {:?} {:?}",
        a1.state().trouble,
        a1.state().backup
    );
    assert!(
        a1.state()
            .note
            .as_ref()
            .is_some_and(|n| n.said.starts_with("Backed up")),
        "{:?}",
        a1.state().note
    );

    // A fresh install: the same identity, an empty store.
    let (a_signer2, _) = signer(0x48);
    let a2 = start_at(endpoint, a_signer2, &dir.path().join("alice-2.db"));
    up(&a2, alice).await;
    assert!(
        a2.state().conversations.is_empty(),
        "{:?}",
        a2.state().conversations
    );

    // Wrong words are refused with why.
    let mut wrong = words.clone();
    wrong[0] = "zzzz".into();
    // Her identity file: a restore records the home beside it (SIP-60), and
    // a refused one records nothing.
    let identity = dir.path().join("identity-alice");
    std::fs::write(&identity, "x").unwrap();
    a2.send(Cmd::Restore {
        words: wrong.join(" "),
        identity: Some(identity.clone()),
    });
    assert!(
        until(
            || a2
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("not a word")),
            10
        )
        .await,
        "{:?}",
        a2.state().trouble
    );

    // The right ones bring the conversation back, readable.
    assert_eq!(
        sqex_proto::home_file::load(&identity),
        None,
        "a refused restore recorded a home"
    );
    a2.send(Cmd::Restore {
        words: words.join(" "),
        identity: Some(identity.clone()),
    });
    assert!(
        until(
            || a2
                .state()
                .note
                .as_ref()
                .is_some_and(|n| n.said.starts_with("Restored generation 1")),
            15
        )
        .await,
        "{:?}",
        a2.state().trouble
    );
    // The exchange the backup was read from is recorded as her home.
    let recorded = sqex_proto::home_file::load(&identity).expect("the restore recorded no home");
    assert_eq!(recorded.key, Some(endpoint.server));
    let dm = a1.state().open.unwrap();
    assert!(
        until(
            || a2.state().conversations.iter().any(|c| c.channel == dm),
            15
        )
        .await,
        "the restored conversation is not in the list: {:?}",
        a2.state().conversations
    );
    a2.send(Cmd::Show(dm));
    assert!(
        until(
            || ["one", "two", "three"].iter().all(|t| a2
                .state()
                .lines
                .iter()
                .any(|l| l.text == *t)),
            15
        )
        .await,
        "the restored lines are not readable: {:?}",
        a2.state()
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
    );
    // And the second store can write the next generation itself.
    a2.send(Cmd::BackupStatus);
    assert!(
        until(|| a2.state().backup.as_ref().is_some_and(|b| b.has_key), 10).await,
        "the words that opened it should be this store's key now"
    );

    // Dropped: the next fresh store finds nothing.
    a1.send(Cmd::DropBackup);
    assert!(
        until(
            || a1.state().backup.as_ref().is_some_and(|b| b.held.is_none()),
            15
        )
        .await,
        "{:?}",
        a1.state().trouble
    );
    let (a_signer3, _) = signer(0x48);
    let a3 = start_at(endpoint, a_signer3, &dir.path().join("alice-3.db"));
    up(&a3, alice).await;
    a3.send(Cmd::Restore {
        words: words.join(" "),
        identity: None,
    });
    assert!(
        until(
            || a3
                .state()
                .trouble
                .as_deref()
                .is_some_and(|t| t.contains("no backup")),
            10
        )
        .await,
        "{:?}",
        a3.state().trouble
    );
    a1.stop();
    a2.stop();
    a3.stop();
    bobs.stop();
}
