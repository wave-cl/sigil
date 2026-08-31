//! Post one notification and say whether it went out.
//!
//! The capability probe answers "could this work here"; this answers "did it".
//! They differ on macOS, where the probe can only check that the process is
//! inside a bundle — whether the notification is then actually delivered
//! depends on Notification Centre settings this cannot see.
//!
//! Run it from *inside* a bundle to test the case that matters:
//!
//! ```text
//! cargo build -p sigil-platform --example notify_probe
//! cp target/debug/examples/notify_probe target/sigil.app/Contents/MacOS/
//! ./target/sigil.app/Contents/MacOS/notify_probe
//! ```
fn main() {
    let notifier = sigil_platform::Notifier::new();
    println!("support: {}", notifier.support());
    if !notifier.support().is_yes() {
        std::process::exit(2);
    }
    let sent = notifier.post("sigil", "A test notification. Nobody is calling.");
    println!("posted: {sent}");
    std::process::exit(if sent { 0 } else { 1 });
}
