//! A session the exchange turns away (SIP-59: the account lives elsewhere)
//! is parked, not restarted every three seconds; and a session that keeps
//! dying is tried again on a clock that doubles. Two identities in a
//! three-second restart loop -- each start a handshake, a fold of the whole
//! store and a round of prekeys -- kept the window from drawing
//! (2026-09-22).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sigil::accounts::Accounts;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::{Account, Silent};
use sigil_chat::ChatApp;
use sqex_proto::home::{Move, Moving};
use sqexd::config::FileConfig;
use sqnr::Client;
use sqnr_core::PubKey;

fn pass(app: &mut ChatApp, accounts: &mut Accounts, egui_ctx: &egui::Context) {
    let mut nav = Navigator::default();
    let connections = sigil_net::Connections::default();
    let mut ctx = AppContext {
        navigator: &mut nav,
        accounts,
        unfocused: true,
        away: false,
        notify: &Silent,
        connections: &connections,
    };
    app.update(&mut ctx, egui_ctx);
}

async fn exchange_in(dir: &Path, domain: &str) -> (SocketAddr, [u8; 32]) {
    let key_path = dir.join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\ndomain = {domain:?}\n",
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

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn until_stopped(app: &ChatApp) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline && !app.stopped_for_test() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    app.stopped_for_test()
}

/// An account that moved away from the exchange sigil is pointed at: the
/// session starts once, is told, publishes where the account lives, and is
/// not started again on the three-second clock.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_told_the_account_moved_is_parked() {
    let f_dir = tempfile::tempdir().unwrap();
    let e_dir = tempfile::tempdir().unwrap();
    let (f_addr, f_pub) = exchange_in(f_dir.path(), "f.test").await;
    let (_e_addr, e_pub) = exchange_in(e_dir.path(), "e.test").await;
    let seed = [0x71u8; 32];
    let me = PubKey::new(SigningKey::from_bytes(&seed).verifying_key().to_bytes());

    // F is told the account lives at E.
    let mut c = Client::connect_as(f_addr, &f_pub, &seed).await.unwrap();
    let (code, _) = c
        .post(
            "/account/move",
            Moving {
                mv: Move::sign(&seed, &PubKey::new(e_pub), now()),
                domain: "e.test".into(),
                origins: vec![],
            }
            .encode(),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
    drop(c);

    let egui_ctx = egui::Context::default();
    let store_root: PathBuf = f_dir.path().join("stores");
    let mut app = ChatApp::new();
    app.set_store_root_for_test(store_root);
    app.set_exchange_for_test(&f_addr.to_string(), &PubKey::new(f_pub).to_string());
    let mut accounts = Accounts::of(vec![Account::unlocked_for_test(seed)]);
    pass(&mut app, &mut accounts, &egui_ctx);
    assert_eq!(app.starts_for_test(), 1);
    assert!(until_stopped(&app).await, "the session should end, parked");
    let state = app.state_of_for_test(&me).expect("the parked session's state");
    assert_eq!(
        state.moved_to.as_ref().map(|(k, d)| (*k, d.as_str())),
        Some((PubKey::new(e_pub), "e.test")),
        "the session did not say where the account lives: {:?}",
        state.trouble
    );
    assert!(
        state
            .trouble
            .as_deref()
            .is_some_and(|t| t.contains("lives at e.test")),
        "{:?}",
        state.trouble
    );

    // Ten seconds of frames: three RETRY clocks, and no second start.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        pass(&mut app, &mut accounts, &egui_ctx);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        app.starts_for_test(),
        1,
        "a parked session was started again inside ten seconds"
    );
}

/// A session that dies at once is tried again after 3 s, then 6 s, then
/// 12 s -- not every 3 s for ever.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_keeps_dying_backs_off() {
    let dir = tempfile::tempdir().unwrap();
    let egui_ctx = egui::Context::default();
    let mut app = ChatApp::new();
    app.set_store_root_for_test(dir.path().to_path_buf());
    // An exchange that is not there: every start dies at the handshake.
    app.set_exchange_for_test("127.0.0.1:1", &PubKey::new([7u8; 32]).to_string());
    let mut accounts = Accounts::of(vec![Account::unlocked_for_test([0x72u8; 32])]);

    let started = std::time::Instant::now();
    let mut seen = Vec::new();
    let mut last = 0;
    // Twenty-five seconds of frames, noting when each start happens.
    while started.elapsed() < Duration::from_secs(25) {
        pass(&mut app, &mut accounts, &egui_ctx);
        if app.starts_for_test() > last {
            last = app.starts_for_test();
            seen.push(started.elapsed().as_secs_f32());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Start, die (a few seconds for the handshake to give up), +3 s, die,
    // +6 s, die, +12 s: four starts at most in twenty-five seconds, and the
    // gaps grow. At three seconds flat there would be more than six.
    assert!(
        (3..=4).contains(&seen.len()),
        "starts at {seen:?}: not the doubling clock"
    );
    let gaps: Vec<f32> = seen.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        gaps.windows(2).all(|g| g[1] > g[0] * 1.5),
        "the gaps do not grow: {gaps:?}"
    );
}
