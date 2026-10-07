//! SIP-88 feeds and SIP-89 citations, as this client holds them.
//!
//! A feed is an account's public output: append-only, signed by the author's
//! own devices, numbered by the author rather than by the exchange, and
//! served to anybody who asks. Nothing comes in — there is no reaction, no
//! reply and no comment — so everything here is reading, publishing, and the
//! one list the exchange never sees.
//!
//! # Why this is not `session::Line`
//!
//! A [`Line`](crate::Line) carries a SIP-16 `seq`, receipts, reactions and
//! SIP-31 standing. A feed post has none of those and has a `serial` instead,
//! and SIP-89 §Two spaces, one word is explicit about what happens when the
//! two are rendered through one path: both are small integers that usually
//! exist, so one resolves against the other "silently and plausibly". The
//! requirement is to keep them distinct in the types. That is [`Serial`], and
//! it is why `Posted` is its own struct rather than a `Line` with some fields
//! left empty.

use sqnr_core::PubKey;

/// A post's position in its author's feed, from 1.
///
/// **A newtype because SIP-89 §Two spaces, one word requires one.** SIP-19's
/// `Edit`, `Redact`, `Reply` and `Reaction` all carry a `target: u64`; in a
/// channel it is a SIP-16 sequence number and in a feed it is one of these,
/// and the word is the same. A client that lets them share a type will one
/// day apply a feed's edit to the seventh message of a conversation and show
/// the result to somebody, having never been wrong in a way a compiler could
/// see.
///
/// The cost is a `.0` at the boundary. The benefit is that the mistake the
/// specification warns about cannot be written here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Serial(pub u64);

impl std::fmt::Display for Serial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How far apart the author's clock and the exchange's have to be before a
/// reader is told the time is a claim.
///
/// Five minutes: enough that an ordinary unsynchronised phone is not accused
/// of back-dating, little enough that a post claiming yesterday is marked.
/// SIP-88 says to mark a difference that is *material* and leaves the number
/// to the client, which is right -- it is a judgement about how much skew is
/// ordinary, and that is not a protocol fact.
pub const CLAIM_SLACK: u64 = 300;

/// When a post is shown and sorted by: **the lesser** of the author's claim
/// and the exchange's observation.
///
/// SIP-88 §What the clocks are requires this and says why in one sentence:
/// "Without the clamp an author sets `issued_at` far in the future and pins
/// themselves to the top of every reader's timeline for as long as they
/// like." Back-dating stays possible and buries the post, which is
/// self-defeating and so needs no rule.
///
/// `received` is the exchange's observation and nothing signs it, which is
/// why SIP-88 §Security considerations says an implementation "MUST NOT
/// present `issued_at` as established" -- see [`claimed_at`].
pub fn shown_at(issued_at: u64, received: u64) -> u64 {
    issued_at.min(received)
}

/// The author's own claim, where it is far enough from the clamp to be worth
/// marking as one. `None` where the two agree, so an ordinary post carries no
/// second time to explain.
pub fn claimed_at(issued_at: u64, received: u64) -> Option<u64> {
    let apart = issued_at.abs_diff(received);
    (apart >= CLAIM_SLACK).then_some(issued_at)
}

/// A post in somebody's feed, as this client draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    /// Whose feed this is. Also the key the post verifies under, which is
    /// what lets a citation name an account and a number and nothing else.
    pub who: PubKey,
    /// Their display name, if a profile has been seen. Self-declared and
    /// attested by nobody (SIP-21), so it is drawn as ordinary text with the
    /// key reachable beside it.
    pub name: Option<String>,
    /// Ours, so it can carry the controls only an author has.
    pub mine: bool,
    pub serial: Serial,
    /// See [`shown_at`].
    pub at: u64,
    /// See [`claimed_at`]. Drawn as a claim, never as the time.
    pub claimed: Option<u64>,
    pub text: String,
    /// The exchange no longer holds the body: withdrawn by its author,
    /// expired, or removed by the exchange. Which of those it is, is
    /// [`Gone`].
    pub gone: Option<Gone>,
    /// SIP-89: the account and serial this post cites, and **nothing of what
    /// they name**. Resolution is a separate act with its own states, so that
    /// nothing of the cited author's is ever drawn before it is checked.
    pub cites: Option<(PubKey, Serial)>,
    /// Parts of a kind this reader does not know. SIP-19: a post with none it
    /// understood is not an empty post, "and a client should say so rather
    /// than showing a blank".
    pub unknown: usize,
    /// A later `Edit` replaced these words. Marked, because SIP-19 says
    /// presenting an edit as though it were the original hides that the text
    /// changed after it was read.
    pub edited: bool,
}

/// Why a post's body is not here.
///
/// **Three states and not one**, because SIP-88 §Withdrawal requires a reader
/// be able to tell the author's act from the exchange's: "SIP-32 requires a
/// reader be able to see that rather than have it pass as an ordinary
/// deletion."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// A tombstone with a SIP-19 `Redact` from this account behind it.
    ///
    /// **A feed's corroboration is stronger than a channel's**: a feed has
    /// exactly one authorised party and prunes only from the oldest end, so
    /// the absence of corroboration means what it says.
    Withdrawn,
    /// A tombstone with nothing behind it, which is the exchange's own act.
    Removed,
}

/// A feed this client follows, and where it has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Follow {
    pub account: PubKey,
    /// What this client has read to. Never moves backwards.
    pub held: Serial,
    /// The newest serial the exchange reported, if it has been asked.
    pub newest: Serial,
}

impl Follow {
    /// How many posts are waiting. Saturating, because an exchange reporting
    /// a `newest` below what we hold is a fault rather than a negative
    /// number -- SIP-88 calls that `state: 0x04`, "which is always a fault".
    pub fn behind(&self) -> u64 {
        self.newest.0.saturating_sub(self.held.0)
    }
}

/// A reader's merged timeline, newest first.
///
/// **There is no global order across feeds and there cannot be.** SIP-88 says
/// so plainly: a timeline is one reader's merge of several independent logs
/// by clocks nobody shares, and two readers may order the same two posts
/// differently without either being wrong. What this function guarantees is
/// only the part that is a rule -- the clamp of [`shown_at`] has already been
/// applied, so nothing sorts above a post that arrived after it.
///
/// Ties break on the serial and then the key, so that one reader's timeline
/// does not reshuffle between two draws of the same posts.
pub fn timeline(mut posts: Vec<Posted>) -> Vec<Posted> {
    posts.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then(b.serial.cmp(&a.serial))
            .then(a.who.as_bytes().cmp(b.who.as_bytes()))
    });
    posts
}

/// One post found by searching what this client holds.
///
/// **Its own type and not `session::Hit`.** A `Hit` names a channel and a
/// `seq`; this names an account and a [`Serial`]. Putting a serial into a
/// channel hit's `seq` is the precise mistake SIP-89 §Two spaces, one word
/// forbids, and a search that mixed them would hand the transcript a number
/// from the wrong space to open — silently and plausibly, because both are
/// small integers that usually exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub who: PubKey,
    /// Whoever's feed it is, as this client names them.
    pub whose: String,
    pub serial: Serial,
    pub text: String,
    /// Where in `text` the match is, so the row can show that part.
    pub found: std::ops::Range<usize>,
    pub at: u64,
}

/// Search the posts this client holds.
///
/// **Local, over what is already here, and no exchange is asked.** SIP-88
/// says "there is no search", and means the network has none: there is no
/// route to ask and no index anywhere. This is the same thing sigil's
/// conversation search already is and says of itself — "Searches what this
/// client has opened. The exchange holds ciphertext and cannot search it."
/// Nothing here reaches past this device, so nothing here is the search
/// SIP-88 refuses.
pub fn search(feeds: &Feeds, needle: &str, named: &dyn Fn(&PubKey) -> String) -> Vec<Found> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for post in feeds.timeline() {
        // A withdrawn post has no words to find, and finding one by words it
        // no longer carries would be showing a body the exchange has dropped.
        if post.gone.is_some() {
            continue;
        }
        let Some(found) = crate::session::find_ignoring_case(&post.text, &needle) else {
            continue;
        };
        out.push(Found {
            who: post.who,
            whose: post.name.clone().unwrap_or_else(|| named(&post.who)),
            serial: post.serial,
            text: post.text.clone(),
            found,
            at: post.at,
        });
    }
    // Newest first, as the conversation search is: a word said often wants
    // the last time rather than the first.
    out.sort_by_key(|f| std::cmp::Reverse(f.at));
    out
}

/// What a SIP-89 citation resolved to, in the words a reader is given.
///
/// **Each state is its own variant and a caller cannot collapse them.**
/// SIP-89 §When it cannot be resolved lists eleven outcomes and says a reader
/// MUST NOT collapse them, because "withdrawn by its author", "no longer
/// held" and "could not be reached" are facts about different things and
/// somebody acts differently on each. An error and a comment would have let
/// a caller fold them by accident; variants do not.
///
/// **Seven, where the specification names eleven**, and the shortfall is
/// recorded rather than hidden. `sqex-chat` cannot today distinguish *removed
/// by the exchange* from *withdrawn by its author*, *unverifiable after a
/// revocation* from *forged*, or the successor and depth cases at all — so
/// four of SIP-89's rows are folded into neighbours here. The folding is in
/// the library and closing it belongs there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Citation {
    /// Asked for and not answered yet. Drawn as a labelled reference, never
    /// as absence: a citation nobody has looked up yet is not a citation that
    /// failed.
    Asking,
    /// Fetched, its signature holds, and these are its words.
    Got {
        who: PubKey,
        name: Option<String>,
        text: String,
        serial: Serial,
    },
    /// Its author took it off, or it passed its own timer.
    Withdrawn,
    /// Below the feed's oldest: no longer held, and that is not the same as
    /// deleted by anybody or as never having existed.
    Evicted,
    /// Absent, withheld, or its owner has blocked this reader. **One answer
    /// for all three**, and SIP-21 forbids presenting a guess at which.
    NoFeed,
    /// Fetched and the signature does not hold under the device it names.
    Forged,
    /// The feed lives at an exchange this client is not connected to. Not a
    /// failure: the home was found and named, and resolving it needs a
    /// connection this session does not have.
    Elsewhere { domain: String },
    /// Nothing could be asked: the home is unreachable, or unknown.
    Unresolved,
}

impl Citation {
    /// The sentence a reader is shown where there are no words to show.
    ///
    /// Written to read as facts about the post rather than as faults in the
    /// program, which is what SIP-89's own table does and what its reference
    /// implementation records as never having been tried on anybody.
    pub fn instead(&self) -> Option<&'static str> {
        match self {
            Citation::Got { .. } => None,
            Citation::Asking => Some("Looking for the post this carries…"),
            Citation::Withdrawn => Some("Its author took this post down."),
            Citation::Evicted => Some("That feed no longer holds this post."),
            Citation::NoFeed => Some("That feed could not be found."),
            Citation::Forged => Some("This does not verify as that author's, so it is not shown."),
            Citation::Elsewhere { .. } => {
                Some("That feed lives at another exchange, which this client did not ask.")
            }
            Citation::Unresolved => Some("That feed could not be reached."),
        }
    }
}

/// Read a page of a feed into what a reader draws.
///
/// Four things happen here that cannot happen post by post, which is why this
/// takes a page rather than one `Stored`:
///
/// - a **tombstone** is `withdrawn` or `removed` depending on whether a
///   SIP-19 `Redact` from this account stands behind it, and SIP-88 requires
///   a reader be able to tell the author's act from the exchange's;
/// - a `Redact` is a *notification* and not a post, so it is not drawn as
///   one, though it holds a serial of its own;
/// - an `Edit` names a **serial** and is applied to the post it names, marked,
///   because SIP-19 says presenting an edit as the original hides that the
///   text changed after it was read;
/// - `Metadata`, `Reaction`, `Call` and `CallEnd` have no home in a feed and
///   are dropped **without counting as unknown**: SIP-88 is explicit that a
///   client MUST NOT report one as an unknown type, because "that would make
///   a context restriction look like a version skip, and a later reader would
///   conclude it had learned something about the sender's software".
///
/// **Corroboration reaches only as far as the page.** A `Redact` outside what
/// is held leaves its tombstone reading as the exchange's act. SIP-88 prunes
/// only from the oldest end, so the corroborating post outlives the one it
/// corroborates -- but a reader holding a window into the middle of a feed
/// may still have one without the other, and `removed` is the safe direction:
/// it claims less.
pub fn read_page(
    stored: &[sqex_proto::feed::Stored],
    me: &PubKey,
    name: &dyn Fn(&PubKey) -> Option<String>,
) -> Vec<Posted> {
    use sqex_proto::message::{Body, Part};

    let mut out: Vec<Posted> = Vec::new();
    let mut redacted: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut edits: Vec<(u64, String)> = Vec::new();

    for s in stored {
        let post = &s.post;
        let at = shown_at(post.issued_at, s.received);
        let claimed = claimed_at(post.issued_at, s.received);
        // A body the exchange no longer holds. Its signature still verifies,
        // which is the whole reason `body_hash` is a field.
        if post.body.is_empty() {
            out.push(Posted {
                who: post.account,
                name: name(&post.account),
                mine: post.account == *me,
                serial: Serial(post.serial),
                at,
                claimed,
                text: String::new(),
                gone: Some(Gone::Removed),
                cites: None,
                unknown: 0,
                edited: false,
            });
            continue;
        }
        // **Two ways to be unreadable, one way to say so.** `Err` is
        // malformed; `Ok(None)` is a body type from a later SIP-19 than this
        // reader knows, which SIP-19 says to ignore. In a channel ignoring it
        // loses one message among many. In a feed it would leave a serial
        // held by something invisible, in a sequence whose density is the
        // thing a reader checks -- so both are drawn as a post with one part
        // that could not be shown, which is what SIP-19 asks for anyway: "a
        // client SHOULD say that the message contained something it could not
        // display".
        let Ok(Some(body)) = Body::decode(&post.body) else {
            out.push(Posted {
                who: post.account,
                name: name(&post.account),
                mine: post.account == *me,
                serial: Serial(post.serial),
                at,
                claimed,
                text: String::new(),
                gone: None,
                cites: None,
                unknown: 1,
                edited: false,
            });
            continue;
        };
        match body {
            Body::Redact { target } => {
                redacted.insert(target);
            }
            Body::Edit { target, post: p } => {
                let said = p
                    .parts
                    .iter()
                    .find_map(|part| match part {
                        Part::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                edits.push((target, said));
            }
            Body::Post(p) => {
                let mut text = String::new();
                let mut cites = None;
                for part in &p.parts {
                    match part {
                        Part::Text(t) => text = t.clone(),
                        // SIP-89: at most one, and the account key is both
                        // the locator and the verifying key. Nothing of what
                        // it names is carried, and nothing of the cited
                        // author's is drawn until it resolves.
                        Part::Quote(who, serial) => cites = Some((*who, Serial(*serial))),
                        _ => {}
                    }
                }
                out.push(Posted {
                    who: post.account,
                    name: name(&post.account),
                    mine: post.account == *me,
                    serial: Serial(post.serial),
                    at,
                    claimed,
                    text,
                    gone: None,
                    cites,
                    unknown: p.unknown,
                    edited: false,
                });
            }
            // No home in a feed, and **not** reported as unknown.
            _ => {}
        }
    }

    for post in &mut out {
        if post.gone == Some(Gone::Removed) && redacted.contains(&post.serial.0) {
            post.gone = Some(Gone::Withdrawn);
        }
        if let Some((_, said)) = edits.iter().rev().find(|(t, _)| *t == post.serial.0)
            && post.gone.is_none()
        {
            post.text = said.clone();
            post.edited = true;
        }
    }
    out
}

/// How many of one feed's posts this client keeps in memory.
///
/// A feed holds up to ten thousand and a reader wants the last few. Paging
/// further back is a read, not a bigger buffer.
pub const PER_FEED: usize = 50;

/// How many posts a merged timeline is built from.
///
/// Two hundred across every feed followed: enough that scrolling does not run
/// out between polls, little enough that the sort is free. A reader who wants
/// more of one person opens that feed.
pub const TIMELINE: usize = 200;

/// Everything the session holds about feeds between passes.
///
/// Private to the session, like `Desk`: what a reader draws is the published
/// snapshot, and this is what builds it.
#[derive(Debug, Default)]
pub struct Feeds {
    /// Posts fetched, by author, **oldest first** -- the order a feed is read
    /// forward in, and the order the chain is checked in.
    held: std::collections::HashMap<PubKey, Vec<Posted>>,
    /// Where each followed feed stood when it was last asked about.
    standing: std::collections::HashMap<PubKey, Follow>,
    /// This account's own newest serial, so a composer knows where it is.
    pub mine: Serial,
    /// Feeds whose home could not be asked at the last poll.
    ///
    /// **Kept, because it is not the same fact as "nothing new".** SIP-88 is
    /// emphatic: "A batched route that cannot distinguish 'nothing new' from
    /// 'I could not ask' is a route that silently stops delivering, and a
    /// client has no way to notice." A reader is told.
    pub unasked: Vec<PubKey>,
    /// Feeds the exchange will tell us nothing about: absent, withheld, or
    /// whose owner has blocked this reader. One answer for all three, and a
    /// client MUST NOT infer which (SIP-21).
    pub silent: Vec<PubKey>,
    /// Feeds whose oldest is now above what this client held: a gap no amount
    /// of reading closes. SIP-88 requires the reader be told rather than
    /// shown the remainder as though it were the whole.
    pub truncated: Vec<PubKey>,
    /// SIP-89: what each citation seen so far resolved to.
    ///
    /// Kept so that a page of posts does not ask the same exchange the same
    /// question once per frame. SIP-89 §Reference implementation records the
    /// absence of exactly this cache as an open question -- "every citation
    /// on a page is a fresh read, resolved eagerly as the page is printed"
    /// -- and a scrolling client is what makes it due.
    cited: std::collections::HashMap<(PubKey, Serial), Citation>,
}

impl Feeds {
    /// Take the posts of one feed, newest first.
    pub fn of(&self, account: &PubKey) -> Vec<Posted> {
        let mut posts = self.held.get(account).cloned().unwrap_or_default();
        posts.reverse();
        posts
    }

    /// Where a feed stood when last asked.
    pub fn standing_of(&self, account: &PubKey) -> Option<Follow> {
        self.standing.get(account).copied()
    }

    /// The merged timeline, newest first and capped.
    pub fn timeline(&self) -> Vec<Posted> {
        let mut all: Vec<Posted> = self.held.values().flatten().cloned().collect();
        all = timeline(all);
        all.truncate(TIMELINE);
        all
    }

    /// Keep a page of one feed's posts, newest `PER_FEED` kept.
    ///
    /// Merged by serial rather than appended: a page read backwards and a
    /// page read forwards overlap, and a feed read twice would otherwise
    /// draw every post in it twice.
    pub fn keep(&mut self, account: &PubKey, posts: Vec<Posted>) {
        let held = self.held.entry(*account).or_default();
        for post in posts {
            match held.binary_search_by(|p| p.serial.cmp(&post.serial)) {
                Ok(at) => held[at] = post,
                Err(at) => held.insert(at, post),
            }
        }
        if held.len() > PER_FEED {
            held.drain(..held.len() - PER_FEED);
        }
    }

    /// Note where a feed stands, from a `/feed/since` row or a read.
    pub fn stands(&mut self, account: PubKey, newest: Serial, held: Serial) {
        self.standing.insert(
            account,
            Follow {
                account,
                held,
                newest,
            },
        );
    }

    /// Forget a feed entirely, on unfollowing it.
    pub fn drop_feed(&mut self, account: &PubKey) {
        self.held.remove(account);
        self.standing.remove(account);
        self.unasked.retain(|a| a != account);
        self.silent.retain(|a| a != account);
        self.truncated.retain(|a| a != account);
    }

    /// Whether anything is held for a feed, so a pane can tell "read and
    /// empty" from "not read yet".
    pub fn knows(&self, account: &PubKey) -> bool {
        self.held.contains_key(account)
    }

    /// What a citation resolved to, if it has been asked about.
    pub fn cited(&self, who: &PubKey, serial: Serial) -> Option<&Citation> {
        self.cited.get(&(*who, serial))
    }

    /// Note that a citation is being looked up, so the next frame does not
    /// ask again. Returns false where it was already asked.
    pub fn asking(&mut self, who: PubKey, serial: Serial) -> bool {
        self.cited.insert((who, serial), Citation::Asking).is_none()
    }

    /// Note what a citation resolved to.
    pub fn resolved(&mut self, who: PubKey, serial: Serial, what: Citation) {
        self.cited.insert((who, serial), what);
    }

    /// Every citation resolved so far, for the published snapshot.
    pub fn citations(&self) -> Vec<(PubKey, Serial, Citation)> {
        let mut out: Vec<(PubKey, Serial, Citation)> = self
            .cited
            .iter()
            .map(|((who, serial), what)| (*who, *serial, what.clone()))
            .collect();
        out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()).then(a.1.cmp(&b.1)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    fn post(who: u8, serial: u64, issued_at: u64, received: u64) -> Posted {
        Posted {
            who: key(who),
            name: None,
            mine: false,
            serial: Serial(serial),
            at: shown_at(issued_at, received),
            claimed: claimed_at(issued_at, received),
            text: String::new(),
            gone: None,
            cites: None,
            unknown: 0,
            edited: false,
        }
    }

    /// **The rule SIP-88 spends a paragraph on.** An author who sets
    /// `issued_at` far in the future would otherwise sit at the top of every
    /// reader's timeline for as long as they liked.
    #[test]
    fn a_post_dated_in_the_future_does_not_sort_above_one_that_arrived_later() {
        let liar = post(1, 1, 9_000_000, 1_000);
        let honest = post(2, 1, 2_000, 2_000);
        assert!(
            liar.at < honest.at,
            "a post claiming the year 2255 outranks one that actually arrived later: \
             {} against {}",
            liar.at,
            honest.at
        );
        let order = timeline(vec![liar.clone(), honest.clone()]);
        assert_eq!(
            order.first().map(|p| p.who),
            Some(honest.who),
            "the forward-dated post is at the top of the timeline"
        );
    }

    /// And back-dating is left alone, because it buries the post. A rule
    /// against it would be a rule against the only direction that costs its
    /// author something.
    #[test]
    fn a_back_dated_post_is_shown_at_the_time_it_claims() {
        let old = post(1, 1, 1_000, 9_000);
        assert_eq!(old.at, 1_000, "the lesser of the two, which is the claim");
        assert_eq!(
            old.claimed,
            Some(1_000),
            "and the claim is carried, so a reader can be told it is one"
        );
    }

    /// A claim within the slack is not marked: an unsynchronised phone is not
    /// accused of anything, and a second time beside every post is a second
    /// time nobody reads.
    #[test]
    fn an_ordinary_clock_difference_is_not_drawn_as_a_claim() {
        assert_eq!(claimed_at(1_000, 1_030), None, "thirty seconds apart");
        assert_eq!(
            claimed_at(1_000, 1_000 + CLAIM_SLACK),
            Some(1_000),
            "and at the slack it is marked"
        );
    }

    /// One reader's order does not reshuffle between two draws of the same
    /// posts. Two posts sharing a clamped time are ordinary — a feed's clock
    /// is whole seconds and a timeline merges several of them.
    #[test]
    fn two_posts_at_one_moment_keep_an_order_of_their_own() {
        let a = post(1, 7, 5_000, 5_000);
        let b = post(2, 9, 5_000, 5_000);
        let once = timeline(vec![a.clone(), b.clone()]);
        let again = timeline(vec![b, a]);
        assert_eq!(
            once, again,
            "the same posts in a different input order came out differently"
        );
    }

    /// A page read twice does not draw every post in it twice. A reader
    /// scrolling back overlaps the page they already hold, every time.
    #[test]
    fn a_feed_read_twice_holds_each_post_once() {
        let mut feeds = Feeds::default();
        feeds.keep(&key(1), vec![post(1, 1, 10, 10), post(1, 2, 20, 20)]);
        feeds.keep(&key(1), vec![post(1, 2, 20, 20), post(1, 3, 30, 30)]);
        let serials: Vec<u64> = feeds.of(&key(1)).iter().map(|p| p.serial.0).collect();
        assert_eq!(serials, vec![3, 2, 1], "newest first, and each once");
    }

    /// And a post fetched again replaces the one held, so a withdrawal read
    /// on a later pass takes the body off the screen.
    #[test]
    fn a_post_read_again_replaces_the_one_held() {
        let mut feeds = Feeds::default();
        let mut said = post(1, 1, 10, 10);
        said.text = "the words".into();
        feeds.keep(&key(1), vec![said]);
        let mut gone = post(1, 1, 10, 10);
        gone.gone = Some(Gone::Withdrawn);
        feeds.keep(&key(1), vec![gone]);
        let held = feeds.of(&key(1));
        assert_eq!(held.len(), 1, "two copies of one serial");
        assert_eq!(
            held[0].gone,
            Some(Gone::Withdrawn),
            "the withdrawal did not replace the body it withdrew"
        );
    }

    /// Unfollowing takes the feed out of the timeline. A reader who stopped
    /// following somebody and went on seeing them would have no way to tell
    /// whether the act had worked.
    #[test]
    fn unfollowing_takes_the_posts_with_it() {
        let mut feeds = Feeds::default();
        feeds.keep(&key(1), vec![post(1, 1, 10, 10)]);
        feeds.keep(&key(2), vec![post(2, 1, 20, 20)]);
        feeds.drop_feed(&key(1));
        let left: Vec<PubKey> = feeds.timeline().iter().map(|p| p.who).collect();
        assert_eq!(left, vec![key(2)], "the unfollowed feed is still drawn");
    }

    /// A feed's serial is not a channel's sequence number, and the type says
    /// so. This test is the one SIP-89 §Two spaces, one word asks for: it
    /// fails to *compile* rather than at runtime, so what it really pins is
    /// that `Serial` has not been quietly turned back into a `u64` alias.
    #[test]
    fn a_serial_is_not_a_sequence_number() {
        fn takes_a_channel_seq(seq: u64) -> u64 {
            seq
        }
        let serial = Serial(7);
        // takes_a_channel_seq(serial) does not compile, which is the point.
        assert_eq!(takes_a_channel_seq(serial.0), 7);
        assert_ne!(
            std::any::TypeId::of::<Serial>(),
            std::any::TypeId::of::<u64>(),
            "Serial has become an alias for u64, and the two spaces can now be confused"
        );
    }
}
