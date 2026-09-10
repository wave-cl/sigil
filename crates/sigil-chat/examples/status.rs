//! Print an exchange's `/status`, whole. Throwaway, like `say.rs`.
//!
//! `sqex status` summarises and leaves out the two numbers a call is visible
//! in: `sessions` and `rooms`.
//!
//!   cargo run -p sigil-chat --example status -- <host:port> <server-key-base58>
use std::net::ToSocketAddrs;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let host = args.next().expect("host:port");
    let key: sqnr_core::PubKey = args.next().expect("server key").parse().expect("a key");
    let addr = host
        .to_socket_addrs()
        .expect("resolvable")
        .next()
        .expect("an address");
    let client = sqnr::Client::connect(addr, key.as_bytes())
        .await
        .expect("connect");
    let (code, body) = client.requests().get("/status").await.expect("status");
    assert_eq!(code, 200);
    println!("{}", String::from_utf8_lossy(&body));
}
