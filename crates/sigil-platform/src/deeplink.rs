//! `sigil://` links, as the desktop and the phone hand them over.
//!
//! What a link *is* lives in [`sigil::deeplink`], which is where the `App`
//! trait can see it; parsing needs nothing from a platform. What is here is
//! the other half: however a link arrives -- Android's activity, the command
//! line, a Dock open -- it arrives on somebody else's thread, so it is left
//! here and the interface is asked to look. The tray does the same, for the
//! same reason.
//!
//! # A link is an offer, never an action
//!
//! Nothing here acts on one, and nothing here decides anything: it is a
//! queue. The interface drains it, asks [`confirmation`] out loud, and waits
//! for somebody to say yes.

use std::sync::Mutex;

pub use sigil::deeplink::{Link, confirmation, parse};

/// Links offered and not yet collected.
static OFFERED: Mutex<Vec<Link>> = Mutex::new(Vec::new());

/// Offer a link to the interface. Returns whether it parsed; a link that did
/// not is not queued, and the reason is the error.
pub fn offer(url: &str) -> Result<Link, String> {
    let link = parse(url)?;
    if let Ok(mut offered) = OFFERED.lock() {
        // **Bounded.** Whatever hands these over is outside sigil, and a
        // window that is not running cannot answer any of them: a program
        // opening a thousand links would otherwise queue a thousand
        // questions for whoever comes back to the machine.
        if offered.len() < 8 {
            offered.push(link.clone());
        }
    }
    crate::wake();
    Ok(link)
}

/// The links offered since last asked.
pub fn offered() -> Vec<Link> {
    OFFERED
        .lock()
        .map(|mut o| std::mem::take(&mut *o))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One at a time and once each: a question answered twice is a room
    /// joined twice.
    #[test]
    fn a_link_is_handed_over_once() {
        let _ = offered();
        let key = sigil::Account::unlocked_for_test([5u8; 32])
            .unlocked()
            .expect("unlocked")
            .me()
            .to_string();
        offer(&format!("sigil://contact/{key}")).expect("a good link");
        assert_eq!(offered().len(), 1);
        assert!(offered().is_empty(), "the same link came back twice");
    }

    /// A link that is not one is refused where it arrived, and queues
    /// nothing: the interface never sees a question it cannot ask.
    #[test]
    fn a_link_that_is_not_one_queues_nothing() {
        let _ = offered();
        assert!(offer("https://example.org").is_err());
        assert!(offered().is_empty());
    }
}
