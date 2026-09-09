//! Send one message as an identity, then stop. A bench tool, not a feature.
//!
//!   cargo run -p sigil-chat --example say -- <identity> <to-key> <host:port> <server-key> <text>
use std::time::Duration;

use sigil_chat::{Cmd, session};
use sigil_net::Endpoint;
use sqnr_core::PubKey;

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, to, host, key, text) = (&a[1], &a[2], &a[3], &a[4], a[5..].join(" "));
    let signer = sqnr::identity::load(std::path::Path::new(path), None).expect("identity");
    let endpoint = Endpoint {
        address: {
            use std::net::ToSocketAddrs as _;
            host.to_socket_addrs()
                .expect("host:port")
                .next()
                .expect("an address")
        },
        server: PubKey::from_base58(key).expect("server key"),
    };
    let to = PubKey::from_base58(to).expect("recipient");
    let chat = session::start(endpoint, signer, None, || {});

    macro_rules! until {
        ($cond:expr, $secs:expr, $what:expr) => {{
            let deadline = tokio::time::Instant::now() + Duration::from_secs($secs);
            let mut got = false;
            while tokio::time::Instant::now() < deadline {
                if $cond {
                    got = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if !got {
                eprintln!("gave up waiting for {}", $what);
            }
            got
        }};
    }

    assert!(
        until!(chat.state().me.is_some(), 20, "the session"),
        "no session: {:?}",
        chat.state().trouble
    );
    println!("connected as {}", chat.state().me.unwrap());

    chat.send(Cmd::OpenDm(to));
    assert!(
        until!(chat.state().open.is_some(), 20, "the conversation"),
        "could not open: {:?}",
        chat.state().trouble
    );

    chat.send(Cmd::Send(text.clone()));
    let landed = until!(
        chat.state().lines.iter().any(|l| l.text == text && l.mine),
        30,
        "the message to be posted"
    );
    println!("posted: {landed} — trouble: {:?}", chat.state().trouble);
    tokio::time::sleep(Duration::from_secs(2)).await;
    chat.stop();
}
