//! Whether the people you talk to are there: SIP-4 beacons, read.
//!
//! An identity beats; the exchange remembers when it last did and the
//! interval it promised; anyone asks. Whether that means *there* is the
//! reader's judgement, and SIP-4 says how to make it: three declared
//! intervals missed is absent, one is not. The beat also says whether
//! anybody is at the keyboard (the *away* bit), which nothing about timing
//! could.
//!
//! All of it over plain data, like [`mention`](crate::mention): the session
//! loop asks and stores, and everything about who to ask, how often, and
//! what an answer means is here where a test can reach it.

use std::collections::HashMap;

use sqex_proto::beacon::Reply;
use sqnr_core::PubKey;

/// How often this client beats. Short enough that a peer sees us go
/// within a minute and a half -- three of these -- and long enough not to
/// be a heartbeat on every tick.
pub const BEAT_SECS: u32 = 30;

/// How often each person is asked about. The same as the beat, since an
/// answer cannot change faster than the beats that feed it.
pub const READ_SECS: u64 = 30;

/// How many people are asked about at most. Beyond it the rest are simply
/// not known -- offline with no record -- rather than a burst of requests
/// every half minute for a roster nobody is looking at.
pub const MOST_READ: usize = 64;

/// How many are asked about on one tick, so the reads are spread over the
/// interval rather than fired together.
pub const READS_PER_TICK: usize = 4;

/// Missed beats before absence: SIP-4's "small multiple, three is a
/// reasonable default".
pub const MISSED_BEATS: u64 = 3;

/// Whether somebody is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Seen {
    /// Beating, and at the keyboard.
    Active,
    /// Beating, and not.
    Away,
    /// Not beating for long enough to say, or never.
    #[default]
    Offline,
}

/// What is known of one person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Presence {
    pub seen: Seen,
    /// When the exchange last saw them, on its clock; nought is never.
    pub last_seen: u64,
    /// The exchange's clock when this was read, so `last_seen` can be
    /// said in words against the right clock.
    pub read_at: u64,
}

impl Presence {
    /// What an exchange's answer means, by SIP-4's rule.
    pub fn of(reply: &Reply) -> Presence {
        let seen = if !reply.found {
            Seen::Offline
        } else {
            let allowed = u64::from(reply.interval_secs).saturating_mul(MISSED_BEATS);
            if reply.staleness() > allowed {
                Seen::Offline
            } else if reply.away {
                Seen::Away
            } else {
                Seen::Active
            }
        };
        Presence {
            seen,
            last_seen: reply.last_seen,
            read_at: reply.now,
        }
    }
}

/// Who to ask about: the other party of every direct message, and every
/// member of the open conversation, but never ourselves -- and no more
/// than [`MOST_READ`], the open conversation's people first, since those
/// are the ones on screen.
pub fn wanted(
    me: Option<PubKey>,
    dm_peers: impl IntoIterator<Item = PubKey>,
    open_members: impl IntoIterator<Item = PubKey>,
) -> Vec<PubKey> {
    let mut out: Vec<PubKey> = Vec::new();
    for k in open_members.into_iter().chain(dm_peers) {
        if Some(k) == me || out.contains(&k) {
            continue;
        }
        out.push(k);
        if out.len() == MOST_READ {
            break;
        }
    }
    out
}

/// Which of `wanted` to ask about now: the ones never asked, then the ones
/// asked longest ago, at most [`READS_PER_TICK`], and none that was asked
/// inside [`READ_SECS`].
pub fn due(
    wanted: &[PubKey],
    asked_at: &HashMap<PubKey, std::time::Instant>,
    now: std::time::Instant,
) -> Vec<PubKey> {
    let mut candidates: Vec<(Option<std::time::Instant>, PubKey)> = wanted
        .iter()
        .map(|k| (asked_at.get(k).copied(), *k))
        .filter(|(at, _)| at.is_none_or(|at| now.duration_since(at).as_secs() >= READ_SECS))
        .collect();
    // Never asked first (`None` sorts first), then the oldest ask.
    candidates.sort_by_key(|(at, _)| *at);
    candidates
        .into_iter()
        .take(READS_PER_TICK)
        .map(|(_, k)| k)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn reply(found: bool, staleness: u64, interval: u32, away: bool) -> Reply {
        Reply {
            found,
            last_seen: 10_000 - staleness,
            interval_secs: interval,
            now: 10_000,
            away,
        }
    }

    /// SIP-4's rule, and the away bit on top of it: fresh is active or
    /// away by the bit; past three intervals is offline whatever the bit
    /// says; not found is offline with nothing to say about when.
    #[test]
    fn an_answer_means_what_sip_4_says_it_means() {
        assert_eq!(Presence::of(&reply(true, 5, 30, false)).seen, Seen::Active);
        assert_eq!(Presence::of(&reply(true, 5, 30, true)).seen, Seen::Away);
        assert_eq!(
            Presence::of(&reply(true, 90, 30, false)).seen,
            Seen::Active,
            "three intervals exactly is not yet absent"
        );
        assert_eq!(
            Presence::of(&reply(true, 91, 30, false)).seen,
            Seen::Offline
        );
        assert_eq!(
            Presence::of(&reply(true, 91, 30, true)).seen,
            Seen::Offline,
            "away is not a way to stay present without beating"
        );
        let gone = Presence::of(&reply(true, 91, 30, false));
        assert_eq!(
            gone.last_seen,
            10_000 - 91,
            "when they were last there is kept"
        );
        let never = Presence::of(&Reply::not_found(10_000));
        assert_eq!(never.seen, Seen::Offline);
        assert_eq!(never.last_seen, 0);
        assert_eq!(never.read_at, 10_000);
    }

    /// The open conversation's people come first, nobody is asked about
    /// twice, we are never asked about, and the list stops at the most.
    #[test]
    fn who_is_asked_about() {
        let k = |b: u8| PubKey::new([b; 32]);
        let got = wanted(Some(k(1)), [k(2), k(3), k(1)], [k(3), k(4)]);
        assert_eq!(got, vec![k(3), k(4), k(2)]);
        let many: Vec<PubKey> = (0..200).map(|i| PubKey::new([i as u8; 32])).collect();
        assert_eq!(wanted(None, many.clone(), []).len(), MOST_READ);
    }

    /// Never asked before the longest ago, a few at a time, and not again
    /// inside the interval.
    #[test]
    fn reads_are_spread_and_not_repeated_too_soon() {
        let k = |b: u8| PubKey::new([b; 32]);
        let now = Instant::now();
        let all: Vec<PubKey> = (1..=6).map(k).collect();
        let mut asked = HashMap::new();
        asked.insert(k(1), now - Duration::from_secs(5)); // just asked
        asked.insert(k(2), now - Duration::from_secs(100)); // long ago
        asked.insert(k(3), now - Duration::from_secs(40));
        let got = due(&all, &asked, now);
        assert_eq!(got.len(), READS_PER_TICK);
        assert!(!got.contains(&k(1)), "asked five seconds ago");
        assert_eq!(&got[..3], &[k(4), k(5), k(6)], "never asked come first");
        assert_eq!(got[3], k(2), "then the longest ago");
        let none: Vec<PubKey> = Vec::new();
        assert!(due(&none, &asked, now).is_empty());
    }
}
