//! SIP-45: where the exchange may wake this device, handed from the
//! platform to whoever holds a connection.
//!
//! A phone's distributor gives it an endpoint, and takes it away again; the
//! platform learns of both on a Java thread with no session in reach. This
//! is the one slot it writes and the chat app reads on its next pass:
//! the latest word wins, since a registration is idempotent and only the
//! newest address is worth telling an exchange about.
//!
//! **Nothing made the first registration.** The wake window re-registered
//! the endpoint on every wake, and the running app never registered it at
//! all -- so a phone with a distributor installed had an address nothing
//! had ever told an exchange, and no wake could ever arrive to start the
//! window that would have. Found reading `wake::forget`, which existed and
//! had no caller.

use std::sync::Mutex;

/// The endpoint to register, or `None` to forget the one registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub url: Option<String>,
    /// How long the exchange may keep it, in seconds; SIP-45 clamps it.
    pub ttl_secs: u32,
}

static LATEST: Mutex<Option<Endpoint>> = Mutex::new(None);

/// The platform's word: this endpoint now, or none.
pub fn offer(url: Option<String>, ttl_secs: u32) {
    if let Ok(mut slot) = LATEST.lock() {
        *slot = Some(Endpoint { url, ttl_secs });
    }
}

/// What was offered since last taken.
pub fn take() -> Option<Endpoint> {
    LATEST.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_word_wins_and_is_taken_once() {
        // Serialised against nothing: this is the only test on the slot.
        offer(Some("https://a.example/1".into()), 60);
        offer(None, 60);
        assert_eq!(
            take(),
            Some(Endpoint {
                url: None,
                ttl_secs: 60
            })
        );
        assert_eq!(take(), None);
    }
}
