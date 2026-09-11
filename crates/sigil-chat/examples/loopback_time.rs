//! Throughput of a blob over sQUIC on loopback, in release: the transport
//! with no network under it, which is how "the code" and "the path" were
//! told apart (67 MB/s here against under 1 MB/s from ex). A bench tool,
//! not a feature.
//!
//!   cargo run --release -p sigil-chat --example loopback_time
use std::time::Instant;

use sqex_chat::Chat;
use sqex_chat::store::Store;
use sqexd::config::FileConfig;
use sqnr_core::{PubKey, Signer as _, SoftwareSigner};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("host_key");
    let (server_sk, _) = squic::generate_keypair();
    std::fs::write(&key_path, hex::encode(server_sk.to_bytes())).unwrap();
    let config_toml = format!(
        "listen = \"127.0.0.1:0\"\nkey_file = {:?}\nstate_file = {:?}\nadmins = []\n\
         welcome_channel = \"\"\nname_registration = \"off\"\n",
        key_path.to_string_lossy(),
        dir.path().join("sqex.state").to_string_lossy(),
    );
    let config_path = dir.path().join("sqexd.toml");
    std::fs::write(&config_path, &config_toml).unwrap();
    let file: FileConfig = toml::from_str(&config_toml).unwrap();
    let config = file.resolve().unwrap();
    let (signing_key, _) =
        squic::load_keypair(&std::fs::read_to_string(&config.key_file).unwrap()).unwrap();
    let bound = sqexd::bind(config, Some(config_path), signing_key)
        .await
        .unwrap();
    let addr = bound.local_addr;
    let server = PubKey::new(bound.public_key.to_bytes());
    tokio::spawn(async move {
        let _ = sqexd::serve(bound).await;
    });

    let sk = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let signer = SoftwareSigner::new(sk);
    let seed = signer.seed();
    let me = PubKey::new(signer.public());
    let store = Store::open(&seed, Some(&dir.path().join("a.db"))).unwrap();
    let client = sqnr::Client::connect_as(addr, server.as_bytes(), &seed)
        .await
        .unwrap();
    let mut chat = Chat::new(client, seed, me, server, store);

    let sk2 = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let other = PubKey::new(sk2.verifying_key().to_bytes());
    let channel = chat.open_dm(&other).await.unwrap();
    let path = dir.path().join("big.bin");
    let bytes: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let prepared = chat.prepare_file(&path, 256 * 1024).unwrap();
    let t = Instant::now();
    let a = chat.upload(&channel, &prepared).await.unwrap();
    println!(
        "upload {} chunks: {:?} = {:.1} MB/s",
        a.chunks,
        t.elapsed(),
        bytes.len() as f64 / t.elapsed().as_secs_f64() / 1e6
    );
    chat.store().forget_blob(&a.blob).unwrap();
    let t = Instant::now();
    let got = chat.download(&a).await.unwrap();
    println!(
        "download {} chunks: {:?} = {:.1} MB/s",
        a.chunks,
        t.elapsed(),
        got.len() as f64 / t.elapsed().as_secs_f64() / 1e6
    );
}
