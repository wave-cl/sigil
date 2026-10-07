//! What one SIP-88 post looks like, including every way it can be absent.
//!
//! The states here are not presentation. SIP-88 §Withdrawal requires a reader
//! be able to tell the author's act from the exchange's, and SIP-89 §When it
//! cannot be resolved says in as many words that a client MUST NOT collapse
//! the reasons a citation did not resolve. So each of these tests is about a
//! distinction the specification asks for, and each would pass on a client
//! that drew one sentence for all of them if it only checked that *something*
//! was said.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::theme;
use sigil_ui::{Absent, Cited, FeedPost};

const PHONE: f32 = 360.0;

fn said(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

/// Draw one post in a phone-wide pane, and say what it said and how wide it
/// drew.
fn drawn(post: FeedPost<'static>) -> (String, f32) {
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let took = width.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::feed_post(ui, &post);
            took.set(ui.min_rect().width());
        });
    h.run();
    h.run();
    (said(&h), width.get())
}

fn plain() -> FeedPost<'static> {
    FeedPost {
        key: "AKnLQ8ZxRSZ9",
        picture: None,
        named: Some("Ada"),
        mine: false,
        at: "11:00",
        claimed: None,
        text: "the thing I said",
        edited: false,
        absent: None,
        unknown: 0,
        cites: None,
        files: &[],
        regard: None,
        after_seam: None,
    }
}

/// An ordinary post says who, when and what.
#[test]
fn a_post_says_who_said_it_and_what_they_said() {
    let (words, wide) = drawn(plain());
    assert!(words.contains("Ada"), "{words}");
    assert!(words.contains("the thing I said"), "{words}");
    assert!(words.contains("11:00"), "{words}");
    assert!(
        wide <= PHONE + 1.0,
        "a post draws {wide} points wide in a {PHONE}-point pane"
    );
}

/// **The author's act and the exchange's are not the same sentence.**
///
/// SIP-88 §Withdrawal: a tombstone with a `Redact` behind it is the author
/// taking their post down; one with nothing behind it is the exchange's own
/// act, and "SIP-32 requires a reader be able to see that rather than have it
/// pass as an ordinary deletion".
#[test]
fn a_withdrawal_and_a_removal_do_not_read_the_same() {
    let (withdrawn, _) = drawn(FeedPost {
        absent: Some(Absent::Withdrawn),
        text: "",
        ..plain()
    });
    let (removed, _) = drawn(FeedPost {
        absent: Some(Absent::Removed),
        text: "",
        ..plain()
    });
    assert_ne!(
        withdrawn, removed,
        "the author taking a post down and the exchange dropping it say the same thing"
    );
    assert!(
        withdrawn.contains("author took this post off"),
        "a withdrawal does not say who did it: {withdrawn}"
    );
    assert!(
        removed.contains("its author did not say"),
        "a removal is presented as an ordinary deletion: {removed}"
    );
}

/// And neither of them shows the words. The body is gone; what survives is
/// the signature over its hash, which is what lets the tombstone verify.
#[test]
fn an_absent_post_does_not_show_the_words_it_no_longer_has() {
    for absent in [Absent::Withdrawn, Absent::Removed] {
        let (words, _) = drawn(FeedPost {
            absent: Some(absent),
            text: "the thing I said",
            ..plain()
        });
        assert!(
            !words.contains("the thing I said"),
            "{absent:?} still draws the body: {words}"
        );
    }
}

/// **The author's clock is drawn as a claim and never as the time.**
///
/// SIP-88 §Security considerations: `received` is the exchange's word and
/// nothing signs it, `issued_at` is the author's, and "an implementation MUST
/// NOT present `issued_at` as established".
#[test]
fn an_authors_own_time_is_marked_as_theirs() {
    let (words, _) = drawn(FeedPost {
        at: "11:00",
        claimed: Some("yesterday"),
        ..plain()
    });
    assert!(
        words.contains("author says yesterday"),
        "the author's claimed time is not attributed to them: {words}"
    );
    assert!(
        words.contains("11:00"),
        "and the time it actually arrived is gone: {words}"
    );
}

/// An ordinary post carries no second time, so the mark means something when
/// it does appear.
#[test]
fn an_ordinary_post_carries_no_second_time() {
    let (words, _) = drawn(plain());
    assert!(!words.contains("author says"), "{words}");
}

/// **A citation that did not resolve names its own unavailability.**
///
/// SIP-89 §When it cannot be resolved gives eleven rows and one rule: the
/// reader is told which, and "none of them is silence". The widget draws
/// whichever sentence the caller resolved to; what is tested here is that it
/// draws it at all, rather than leaving a quote looking like a post with
/// nothing attached.
#[test]
fn a_citation_that_did_not_resolve_says_so() {
    let (words, _) = drawn(FeedPost {
        cites: Some(Cited {
            named: None,
            text: None,
            instead: Some("That post was taken down by its author."),
        }),
        ..plain()
    });
    assert!(
        words.contains("taken down by its author"),
        "a citation that could not be shown said nothing at all: {words}"
    );
}

/// **And it names nobody until it has resolved.**
///
/// SIP-89 carries no author name in the part on purpose: "a quote naming an
/// author is a claim a reader renders before it can check anything, so a
/// citer could name a feed that will never resolve and have every client
/// display 'a post by K, unavailable'."
#[test]
fn an_unresolved_citation_puts_no_name_in_front_of_a_reader() {
    let (words, _) = drawn(FeedPost {
        cites: Some(Cited {
            named: None,
            text: None,
            instead: Some("That feed could not be reached."),
        }),
        ..plain()
    });
    // Only the quoting author is named. "Ada" is the poster; nobody else.
    assert_eq!(
        words.matches("Ada").count(),
        1,
        "a second name appears beside an unresolved citation: {words}"
    );
}

/// A resolved citation shows the cited author and their words, under the post
/// that carried it.
#[test]
fn a_resolved_citation_shows_whose_post_it_was() {
    let (words, wide) = drawn(FeedPost {
        cites: Some(Cited {
            named: Some("Bram"),
            text: Some("the post being carried"),
            instead: None,
        }),
        ..plain()
    });
    assert!(words.contains("Bram"), "{words}");
    assert!(words.contains("the post being carried"), "{words}");
    assert!(
        wide <= PHONE + 1.0,
        "a post with a citation draws {wide} points wide in a {PHONE}-point pane"
    );
}

/// **No control offers something a feed cannot do.** SIP-88 §Nothing comes
/// in: there is no reaction, no reply and no comment, so a button for one
/// would be a button that cannot work.
#[test]
fn a_post_offers_nothing_a_feed_has_no_path_for() {
    let (words, _) = drawn(plain());
    for absent in ["Reply", "React", "Comment"] {
        assert!(
            !words.contains(absent),
            "a feed post offers {absent:?}, which SIP-88 has no inbound path for: {words}"
        );
    }
}

/// Taking a post off is offered on one's own and on nobody else's.
#[test]
fn only_the_author_is_offered_the_way_to_take_a_post_off() {
    let (mine, _) = drawn(FeedPost {
        mine: true,
        ..plain()
    });
    assert!(
        mine.contains("Take it off"),
        "an author is not offered the way to withdraw their own post: {mine}"
    );
    let (theirs, _) = drawn(plain());
    assert!(
        !theirs.contains("Take it off"),
        "somebody else's post offers to withdraw it: {theirs}"
    );
}

/// A post whose parts this version cannot read says so, rather than drawing a
/// blank. SIP-19 asks for exactly this.
#[test]
fn a_post_with_parts_this_version_cannot_read_says_so() {
    let (words, _) = drawn(FeedPost {
        text: "",
        unknown: 1,
        ..plain()
    });
    assert!(
        words.contains("cannot show"),
        "a post this reader could not read drew a blank: {words}"
    );
}

/// **A post says what it carries even with no picture to show.**
///
/// A thumbnail travels inside a feed post, so a picture is usually on the
/// screen the moment the post is. "Usually" is not "always": a sender may
/// omit it, and the attachment is still there to be fetched. SIP-19's rule
/// for a post whose parts could not all be shown applies — say so rather than
/// draw a blank.
#[test]
fn a_file_with_no_thumbnail_is_still_said() {
    // Leaked on purpose: `drawn` builds a harness that outlives this frame,
    // and one array in one test is cheaper than threading a lifetime through
    // the helper for it.
    let files: &'static [sigil_ui::Shown<'static>] = Box::leak(Box::new([sigil_ui::Shown {
        described: "[picture, 2.1 MiB]",
        picture: None,
    }]));
    let (words, wide) = drawn(FeedPost {
        text: "look at this",
        files,
        ..plain()
    });
    assert!(
        words.contains("[picture, 2.1 MiB]"),
        "a post carrying a file said nothing about it: {words}"
    );
    assert!(
        wide <= PHONE + 1.0,
        "a post with a file draws {wide} points wide in a {PHONE}-point pane"
    );
}

/// **A succeeded feed is not drawn as continuous.**
///
/// SIP-88 makes this a MUST and names it the rule most likely to ship
/// missing, because nothing in the log will tell a reader: the chain stays
/// unbroken across a hand-over by design — `prev` links signing inputs and a
/// signing input references no credential. A stolen key is the capture of
/// everything the account ever published, under that unbroken chain.
#[test]
fn a_feed_that_changed_hands_says_so_above_the_post_that_changed_it() {
    let (words, _) = drawn(FeedPost {
        after_seam: Some("Ada's old key"),
        ..plain()
    });
    assert!(
        words.contains("changed hands"),
        "a feed that changed hands reads as one person's: {words}"
    );
    assert!(
        words.contains("Ada's old key"),
        "and it does not say who held it before: {words}"
    );
    // The control: an ordinary post carries no such line, so the warning
    // means something where it appears.
    let (ordinary, _) = drawn(plain());
    assert!(!ordinary.contains("changed hands"), "{ordinary}");
}
