//! SIP-42: the other devices of this account, and when to meet them.
//!
//! A device keeps an `open` outstanding toward each sibling and syncs
//! whenever one appears. Consent is SIP-12's, mutual by construction: a
//! sibling that is not there is not synced with, and one that is gets a
//! session the exchange relays and cannot read. This module is the
//! bookkeeping -- who the siblings are, which is being talked to, when
//! each was last -- kept apart from the session loop so it can be reasoned
//! about without one.

use std::collections::HashMap;
use std::time::Instant;

use sqex_chat::sync::{Progress, Relayed, Sync};
use sqnr_core::PubKey;

/// How often to ask the exchange who the siblings are. A device linked or
/// revoked is also learned at once, by whoever did it.
pub const LIST_SECS: u64 = 5 * 60;
/// How often to renew the `open` toward a sibling that has not appeared.
/// The exchange keeps an open for an hour, so this is not about staying
/// listed: it is how soon this side hears that the other has opened too,
/// since the exchange says so only in answer to an open. One small request
/// per sibling.
pub const OPEN_SECS: u64 = 10;
/// How long after a sync with a device before another with the same one.
/// A sync is idempotent, and history only grows between two devices that
/// both read the same exchange when one has been away; once an hour is
/// plenty, and a relink lists afresh.
pub const AGAIN_SECS: u64 = 60 * 60;

/// A sync running with one sibling, on the link it runs on.
pub struct Live {
    pub peer: PubKey,
    pub sync: Sync,
    pub link: Relayed,
    pub since: Instant,
}

/// Who the siblings are and where each stands.
#[derive(Default)]
pub struct Siblings {
    /// The other devices, as the exchange last listed them.
    pub known: Vec<PubKey>,
    listed_at: Option<Instant>,
    /// The ephemeral offered toward each, and when it was last offered.
    /// Kept while the open is outstanding: the session key is derived from
    /// it, so a fresh one per attempt would derive a key the far side does
    /// not hold.
    opens: HashMap<PubKey, (x25519_dalek::StaticSecret, Instant)>,
    /// When each was last synced with, or last failed.
    synced_at: HashMap<PubKey, Instant>,
    pub live: Option<Live>,
}

impl Siblings {
    /// Whether the exchange should be asked again who the siblings are.
    pub fn list_due(&self, now: Instant) -> bool {
        self.listed_at
            .is_none_or(|at| now.duration_since(at).as_secs() >= LIST_SECS)
    }

    /// Ask again at the next chance: a device was just linked or revoked.
    pub fn relist(&mut self) {
        self.listed_at = None;
    }

    /// The exchange said who the siblings are. Opens toward anybody no
    /// longer listed are dropped; a revoked device is not met.
    pub fn listed(&mut self, siblings: Vec<PubKey>, now: Instant) {
        self.opens.retain(|k, _| siblings.contains(k));
        self.synced_at.retain(|k, _| siblings.contains(k));
        self.known = siblings;
        self.listed_at = Some(now);
    }

    /// Which siblings to open toward now, with the ephemeral to offer each:
    /// those not synced with lately, whose open is not fresh. A first open
    /// goes at once.
    pub fn to_open(&mut self, now: Instant) -> Vec<(PubKey, x25519_dalek::StaticSecret)> {
        if self.live.is_some() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for sibling in &self.known {
            if self
                .synced_at
                .get(sibling)
                .is_some_and(|at| now.duration_since(*at).as_secs() < AGAIN_SECS)
            {
                continue;
            }
            let (eph, at) = self.opens.entry(*sibling).or_insert_with(|| {
                (
                    x25519_dalek::StaticSecret::random_from_rng(rand_core::OsRng),
                    now - std::time::Duration::from_secs(OPEN_SECS),
                )
            });
            if now.duration_since(*at).as_secs() >= OPEN_SECS {
                *at = now;
                out.push((*sibling, eph.clone()));
            }
        }
        out
    }

    /// A sibling answered: the session is live, and the open is spent.
    pub fn met(&mut self, peer: PubKey, sync: Sync, link: Relayed, now: Instant) {
        self.opens.remove(&peer);
        self.live = Some(Live {
            peer,
            sync,
            link,
            since: now,
        });
    }

    /// The live sync is over, however it ended; the next with that device
    /// waits its turn.
    pub fn over(&mut self, now: Instant) -> Option<Live> {
        let live = self.live.take()?;
        self.synced_at.insert(live.peer, now);
        Some(live)
    }
}

/// What to tell the person a sync brought, if anything. Messages and
/// files, not entries: a membership entry is not something they wrote.
/// Nothing arriving is nothing to say: a sync runs on its own and a note
/// about it every hour would be noise.
pub fn said(p: &Progress) -> Option<String> {
    if p.messages_in == 0 && p.blobs_in == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if p.messages_in > 0 {
        parts.push(format!(
            "{} message{}",
            p.messages_in,
            if p.messages_in == 1 { "" } else { "s" }
        ));
    }
    if p.blobs_in > 0 {
        parts.push(format!(
            "{} file{}",
            p.blobs_in,
            if p.blobs_in == 1 { "" } else { "s" }
        ));
    }
    let what = parts.join(" and ");
    let wherein = match p.channels_in.len() {
        0 | 1 => String::new(),
        n => format!(" across {n} conversations"),
    };
    Some(format!("Synced {what}{wherein} from your other device."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// A newly listed sibling is opened toward at once, then not again
    /// until the open has aged; the ephemeral offered stays the same, so
    /// the key derived on either side agrees.
    #[test]
    fn an_open_is_renewed_on_a_cadence_with_the_same_ephemeral() {
        let mut s = Siblings::default();
        let t0 = Instant::now();
        assert!(s.list_due(t0));
        s.listed(vec![key(2)], t0);
        assert!(!s.list_due(t0));
        let first = s.to_open(t0);
        assert_eq!(first.len(), 1, "a first open goes at once");
        assert!(s.to_open(t0 + Duration::from_secs(1)).is_empty());
        let again = s.to_open(t0 + Duration::from_secs(OPEN_SECS));
        assert_eq!(again.len(), 1);
        assert_eq!(
            first[0].1.to_bytes(),
            again[0].1.to_bytes(),
            "the ephemeral changed between opens"
        );
        assert!(s.list_due(t0 + Duration::from_secs(LIST_SECS)));
    }

    /// A device the exchange stopped listing is not opened toward, and one
    /// synced with lately waits its turn.
    #[test]
    fn unlisted_and_lately_synced_siblings_are_left_alone() {
        let mut s = Siblings::default();
        let t0 = Instant::now();
        s.listed(vec![key(2), key(3)], t0);
        assert_eq!(s.to_open(t0).len(), 2);
        s.listed(vec![key(3)], t0 + Duration::from_secs(1));
        let later = t0 + Duration::from_secs(OPEN_SECS);
        let opens = s.to_open(later);
        assert_eq!(opens.len(), 1);
        assert_eq!(opens[0].0, key(3));
        // Pretend a sync with 3 just ended.
        s.synced_at.insert(key(3), later);
        assert!(s.to_open(later + Duration::from_secs(OPEN_SECS)).is_empty());
        assert_eq!(
            s.to_open(later + Duration::from_secs(AGAIN_SECS)).len(),
            1,
            "an hour later it is due again"
        );
    }

    /// The note counts what arrived and says nothing when nothing did.
    #[test]
    fn the_note_counts_what_came() {
        let mut p = Progress::default();
        assert_eq!(said(&p), None);
        p.entries_in = 4;
        assert_eq!(said(&p), None, "entries nobody wrote are not messages");
        p.messages_in = 1;
        p.channels_in.insert([1; 32]);
        assert_eq!(
            said(&p).unwrap(),
            "Synced 1 message from your other device."
        );
        p.messages_in = 12;
        p.blobs_in = 2;
        p.channels_in.insert([2; 32]);
        p.channels_in.insert([3; 32]);
        assert_eq!(
            said(&p).unwrap(),
            "Synced 12 messages and 2 files across 3 conversations from your other device."
        );
        p.messages_in = 0;
        p.blobs_in = 1;
        assert_eq!(
            said(&p).unwrap(),
            "Synced 1 file across 3 conversations from your other device."
        );
    }
}
