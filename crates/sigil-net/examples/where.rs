//! Print which exchange sigil would use, and which layer named it.
//!
//! `cargo run -p sigil-net --example where`
//!
//! "sigil will not place a call" has one common cause -- no exchange named
//! anywhere -- and no way to see it from the interface, which can only report
//! the conclusion. This shows the working: every layer in priority order, and
//! which one answered.
fn main() {
    let identity = sqnr::identity::default_identity_path().ok();
    let cfg = sqnr::config::Config::load();
    let layers = sigil_net::discovery::layers(
        sigil_net::discovery::nothing_explicit(),
        &cfg,
        identity.as_deref(),
    );
    println!("identity: {identity:?}");
    for (i, l) in layers.iter().enumerate() {
        let named = l.server.is_some() || l.host.is_some();
        println!(
            "  layer {i}: server={:?} host={:?}{}",
            l.server,
            l.host,
            if named { "   <- names an exchange" } else { "" }
        );
    }
    println!(
        "any_configured: {}",
        sigil_net::discovery::any_configured(&layers)
    );
}
