//! `sigil://` links.
//!
//! # A link is an offer, never an action
//!
//! Every one of these parses to something the interface must *confirm*. A link
//! is a thing somebody else wrote and put where you would click it, and the
//! most dangerous one is the least dramatic: `sigil://room/<secret>` naming a
//! room. Joining silently would put somebody into a conversation they did not
//! choose, and — because a room's membership is holding the secret — there is
//! no owner to remove them and no way to take it back.
//!
//! So parsing yields an intent, and nothing here acts on one.

use sqnr_core::PubKey;

/// What a `sigil://` link is asking for. Every variant needs confirming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Show somebody, so they can be added or called.
    Contact(PubKey),
    /// Offer to call somebody.
    Call(PubKey),
    /// Offer to join a room. **The one that matters most**: see the module note.
    Room(String),
}

/// Parse a `sigil://` link.
///
/// Deliberately strict. A link arrives from outside — a web page, a message,
/// another program — and guessing at a malformed one is how a link that meant
/// nothing becomes a link that meant something.
pub fn parse(url: &str) -> Result<Link, String> {
    let rest = url
        .strip_prefix("sigil://")
        .ok_or_else(|| format!("not a sigil link: {url:?}"))?;
    let (kind, value) = rest
        .split_once('/')
        .ok_or_else(|| format!("a sigil link needs a kind and a value: {url:?}"))?;
    // Anything after a second slash is not something this understands, and
    // ignoring it would let a link say one thing and mean another.
    if value.contains('/') {
        return Err(format!("too many parts in {url:?}"));
    }
    let value = value.trim_end_matches('/');
    if value.is_empty() {
        return Err(format!("a sigil link needs a value: {url:?}"));
    }
    match kind {
        "contact" => value
            .parse::<PubKey>()
            .map(Link::Contact)
            .map_err(|e| format!("bad key in {url:?}: {e}")),
        "call" => value
            .parse::<PubKey>()
            .map(Link::Call)
            .map_err(|e| format!("bad key in {url:?}: {e}")),
        "room" => {
            // Not parsed into a RoomId here. The value is a secret, and a
            // parse failure message is exactly the sort of thing that ends up
            // in a log — so it is carried opaquely and validated where it is
            // used.
            if value.len() < 40 || !value.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(format!("that does not look like a room secret: {url:?}"));
            }
            Ok(Link::Room(value.to_string()))
        }
        other => Err(format!("sigil does not know {other:?} links")),
    }
}

/// What to ask before acting on a link. The interface shows this and waits.
pub fn confirmation(link: &Link) -> String {
    match link {
        Link::Contact(who) => format!("Add {who} as a contact?"),
        Link::Call(who) => format!("Call {who}?"),
        Link::Room(_) => "Join this room? Anyone holding its secret is a member, \
             and membership cannot be taken back."
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_key() -> String {
        let sk = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
        PubKey::new(sk.verifying_key().to_bytes()).to_string()
    }

    #[test]
    fn a_contact_link_carries_a_key() {
        let key = a_key();
        let got = parse(&format!("sigil://contact/{key}")).unwrap();
        assert_eq!(got, Link::Contact(key.parse().unwrap()));
    }

    #[test]
    fn a_call_link_carries_a_key() {
        let key = a_key();
        assert!(matches!(
            parse(&format!("sigil://call/{key}")),
            Ok(Link::Call(_))
        ));
    }

    #[test]
    fn a_room_link_carries_its_secret_opaquely() {
        let secret = "TestRoomSecretNotARea1RoomDoNotUseAAAAAAAAAA";
        assert_eq!(
            parse(&format!("sigil://room/{secret}")).unwrap(),
            Link::Room(secret.to_string())
        );
    }

    /// The one that matters. A room link must never read as anything but an
    /// offer, and the question must say what accepting means -- membership is
    /// holding the secret, and it cannot be taken back.
    #[test]
    fn joining_a_room_is_asked_and_says_what_it_costs() {
        let link = parse("sigil://room/TestRoomSecretNotARea1RoomDoNotUseAAAAAAAAAA").unwrap();
        let asked = confirmation(&link);
        assert!(asked.starts_with("Join this room?"), "{asked}");
        assert!(asked.contains("cannot be taken back"), "{asked}");
    }

    #[test]
    fn every_link_asks_before_acting() {
        let key = a_key();
        for url in [
            format!("sigil://contact/{key}"),
            format!("sigil://call/{key}"),
            "sigil://room/TestRoomSecretNotARea1RoomDoNotUseAAAAAAAAAA".to_string(),
        ] {
            let asked = confirmation(&parse(&url).unwrap());
            // A question, not necessarily the last word: the room one asks and
            // then says what accepting costs, which is the point of asking.
            assert!(asked.contains('?'), "a link is a question: {asked}");
        }
    }

    #[test]
    fn malformed_links_are_refused_rather_than_guessed_at() {
        for bad in [
            "http://example.com",
            "sigil://",
            "sigil://contact",
            "sigil://contact/",
            "sigil://contact/not-a-key",
            "sigil://room/short",
            "sigil://mystery/thing",
            // Extra parts could make a link say one thing and mean another.
            "sigil://call/aaa/bbb",
        ] {
            assert!(parse(bad).is_err(), "should be refused: {bad}");
        }
    }

    /// A refusal has to say which link, so somebody can see what they clicked.
    #[test]
    fn a_refusal_names_the_link() {
        let e = parse("sigil://mystery/thing").unwrap_err();
        assert!(e.contains("mystery"), "{e}");
    }
}
