//! How long a picture takes to reach the screen, through the same session
//! sigil runs. A bench tool, not a feature.
//!
//!   cargo run --release -p sigil-chat --example picture_time -- <identity> <domain> <channel-label>
//!
//! Opens the channel in a fresh store, so every picture in it is fetched,
//! and prints when each message with a picture appeared and when its bytes
//! did.
use std::time::{Duration, Instant};

use sigil_chat::{Cmd, session};
use sigil_net::Dial;

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, domain, label) = (&a[1], &a[2], &a[3]);
    let signer = sqnr::identity::load(std::path::Path::new(path), None).expect("identity");
    let layers = vec![sigil_net::Layer {
        server: Some(domain.clone()),
        ..Default::default()
    }];
    // A fourth argument names a store to keep and reuse, for timing the
    // second sight of a picture: off the disc rather than the exchange.
    let (store, keep) = match a.get(4) {
        Some(p) => (std::path::PathBuf::from(p), true),
        None => (
            std::env::temp_dir().join(format!("picture-time-{}.db", std::process::id())),
            false,
        ),
    };
    let t0 = Instant::now();
    let chat = session::start(Dial::Discover(layers), signer, Some(store.clone()), || {});
    macro_rules! wait {
        ($cond:expr, $secs:expr) => {{
            let deadline = Instant::now() + Duration::from_secs($secs);
            let mut got = false;
            while Instant::now() < deadline {
                if $cond {
                    got = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            got
        }};
    }
    assert!(
        wait!(chat.state().me.is_some(), 30),
        "no session: {:?}",
        chat.state().trouble
    );
    println!("{:>7.3}s connected", t0.elapsed().as_secs_f64());
    assert!(
        wait!(
            chat.state().conversations.iter().any(|x| &x.label == label),
            30
        ),
        "no channel {label}: {:?}",
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
        .find(|x| &x.label == label)
        .unwrap()
        .channel;
    chat.send(Cmd::Show(channel));
    println!("{:>7.3}s shown", t0.elapsed().as_secs_f64());
    let mut seen: std::collections::HashMap<u64, Instant> = Default::default();
    let mut done: std::collections::HashSet<u64> = Default::default();
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        let s = chat.state();
        for l in s.lines.iter().filter(|l| !l.attachments.is_empty()) {
            let at = *seen.entry(l.seq).or_insert_with(|| {
                println!(
                    "{:>7.3}s seq {} appeared: {}",
                    t0.elapsed().as_secs_f64(),
                    l.seq,
                    l.attachments[0].described
                );
                Instant::now()
            });
            if !done.contains(&l.seq) && l.attachments.iter().all(|a| a.bytes.is_some()) {
                done.insert(l.seq);
                println!(
                    "{:>7.3}s seq {} bytes in, {:?} after it appeared",
                    t0.elapsed().as_secs_f64(),
                    l.seq,
                    at.elapsed()
                );
            }
        }
        if !seen.is_empty() && seen.len() == done.len() && t0.elapsed() > Duration::from_secs(5) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for l in chat
        .state()
        .lines
        .iter()
        .filter(|l| !l.attachments.is_empty())
    {
        if !done.contains(&l.seq) {
            println!(
                "seq {} never got its bytes: held {} missing {}",
                l.seq, l.attachments[0].held, l.attachments[0].missing
            );
        }
    }
    println!("trouble: {:?}", chat.state().trouble);
    chat.stop();
    if !keep {
        let _ = std::fs::remove_file(&store);
    }
}
