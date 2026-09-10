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

    /// The connection this identity holds, when there is no question which.
    ///
    /// For a borrower with no exchange in mind: the voice tab, dialling a key
    /// somebody pasted, and the administrative console, which acts on whatever
    /// exchange this identity is on. Both used to ask for the **default**
    /// exchange by name -- `""` -- and that was wrong for a real
    /// configuration: an identity whose exchange is named explicitly in its
    /// account settings has no default session at all, so both quietly went
    /// back to dialling a second connection to an exchange the identity was
    /// already connected to.
    ///
    /// See [`to_borrow`] for the rule and why it declines to guess.
    pub fn one_of(&self, identity: PubKey) -> Option<Held> {
        let all = self.0.lock().ok()?;
        let names: Vec<&str> = all
            .keys()
            .filter(|(who, _)| *who == identity)
            .map(|(_, at)| at.as_str())
            .collect();
        let chosen = to_borrow(&names)?.to_string();
        all.get(&(identity, chosen)).cloned()
    }

    /// The session is over; stop offering what it held.
    pub fn forget(&self, identity: PubKey, exchange: &str) {
        if let Ok(mut all) = self.0.lock() {
            all.remove(&(identity, exchange.to_string()));
        }
    }
}

/// Which of an identity's exchanges to borrow a connection at, given that the
/// borrower has not said.
///
/// - **The default one** — what `""` names — when there is one. It is the
///   exchange everything else resolves to from `~/.sqnr/config` and the
///   identity's own handle, so it is what the borrower would have dialled.
/// - **The only one**, when there is no default. An identity with a single
///   named exchange is connected to exactly one place, and dialling a second
///   connection to it is the thing this exists to stop.
/// - **Nothing**, when there are several and no default. Picking one would be
///   choosing an exchange on somebody's behalf, and the caller falls back to
///   dialling, which at least does what it is configured to do.
///
/// A free function over plain data because the rule is the part worth testing:
/// a slot cannot be filled without a real connection, so a test of
/// [`Connections::one_of`] alone could not tell which of two it had picked.
fn to_borrow<'a>(names: &[&'a str]) -> Option<&'a str> {
    if names.iter().any(|n| n.is_empty()) {
        return Some("");
    }
    match names {
        [only] => Some(only),
        _ => None,
    }
}

#[cfg(test)]
mod borrow_tests {
    use super::to_borrow;

    /// The default is what the borrower would have dialled, so it wins.
    #[test]
    fn the_default_exchange_is_taken_when_there_is_one() {
        assert_eq!(to_borrow(&["", "indra.org"]), Some(""));
        assert_eq!(to_borrow(&["indra.org", ""]), Some(""));
        assert_eq!(to_borrow(&[""]), Some(""));
    }

    /// The case that sent this back to the drawing board: an identity whose
    /// exchange is named in its account settings has no default session, and
    /// asking for one by name found nothing.
    #[test]
    fn a_single_named_exchange_is_the_one_to_borrow() {
        assert_eq!(to_borrow(&["squic.org"]), Some("squic.org"));
    }

    /// Several, and nothing to say which: dialling is better than guessing.
    #[test]
    fn several_named_exchanges_are_not_chosen_between() {
        assert_eq!(to_borrow(&["squic.org", "indra.org"]), None);
    }

    /// Nothing held is nothing to borrow.
    #[test]
    fn no_sessions_is_nothing() {
        assert_eq!(to_borrow(&[]), None);
    }
}
