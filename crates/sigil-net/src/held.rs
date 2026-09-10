//! The connection an identity holds, and who may borrow it.
//!
//! # Why a slot rather than a connection
//!
//! One identity should reach one exchange over one connection, whatever part
//! of the program is doing the reaching. That is not tidiness: an exchange
//! writes a relayed datagram to *every* connection an identity holds, so a
//! second connection carries a duplicate of every audio frame for the length of
//! every call, and costs a handshake at the moment somebody presses answer.
//!
//! But a connection is not a stable thing to hand out. It drops, and the chat
//! session that owns it redials; anyone holding the old one would be holding
//! something closed, with no way to notice. So what is lent is a **slot** —
//! whatever is live now — and the owner rewrites it. A borrower that wants to
//! keep working across a reconnection reads it again; one that only needs a
//! connection once, like a call, takes what is there when it starts.
//!
//! The endpoint travels with it because a connection does not carry one: a
//! signed admin command is bound to the exchange's key (SIP-31), and there is
//! nothing in a `sqnr::Client` to ask.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqnr_core::PubKey;

use crate::Endpoint;

/// A connection somebody else owns, borrowed.
///
/// Cloning gives another view of the same slot, not a copy of its contents.
#[derive(Clone, Default)]
pub struct Held(Arc<Mutex<Option<(sqnr::Client, Endpoint)>>>);

impl Held {
    /// A slot with nothing in it yet — a session that has not connected.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Put the live connection in, or clear it. The owner's call, and the only
    /// thing that should ever call it.
    pub fn set(&self, live: Option<(sqnr::Client, Endpoint)>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = live;
        }
    }

    /// What is live **now**, which is the only question worth asking: a
    /// connection taken a minute ago may have been redialled since.
    pub fn now(&self) -> Option<(sqnr::Client, Endpoint)> {
        self.0.lock().ok()?.clone()
    }

    /// Where the connection goes, when there is one.
    pub fn at(&self) -> Option<Endpoint> {
        self.now().map(|(_, at)| at)
    }

    /// Whether there is anything to borrow.
    pub fn is_live(&self) -> bool {
        self.0.lock().map(|s| s.is_some()).unwrap_or(false)
    }
}

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.at() {
            Some(at) => write!(f, "Held(at {})", at.server),
            None => write!(f, "Held(nothing)"),
        }
    }
}

/// Every connection this process holds, by identity and exchange.
///
/// The chat session for an identity is what dials, holds and redials; this is
/// how anything else in the same window — an administrative console, a call —
/// finds it instead of dialling its own. Keyed by the exchange's **name** as
/// the interface knows it, because that is what a session is keyed on before
/// anything has resolved a key.
///
/// Not a global. It is passed to apps, so two of them in one process — two
/// tests, chiefly — cannot see each other's connections.
#[derive(Clone, Default)]
pub struct Connections(Arc<Mutex<HashMap<(PubKey, String), Held>>>);

impl Connections {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer this identity's connection at this exchange for others to use.
    pub fn lend(&self, identity: PubKey, exchange: &str, held: Held) {
        if let Ok(mut all) = self.0.lock() {
            all.insert((identity, exchange.to_string()), held);
        }
    }

    /// What this identity holds at this exchange, if a session is holding one.
    pub fn of(&self, identity: PubKey, exchange: &str) -> Option<Held> {
        self.0
            .lock()
            .ok()?
            .get(&(identity, exchange.to_string()))
            .cloned()
    }

    /// The session is over; stop offering what it held.
    pub fn forget(&self, identity: PubKey, exchange: &str) {
        if let Ok(mut all) = self.0.lock() {
            all.remove(&(identity, exchange.to_string()));
        }
    }
}
