//! Which of the exchange's routes sigil actually reaches.
//!
//! # Why this is enumerated from the source and not from a list
//!
//! "Every endpoint is implemented" is a claim, and a hand-kept list of
//! endpoints is a claim about a claim: it drifts, and it drifts green, because
//! a route added upstream simply never appears in it. So the set of routes is
//! **scanned out of `sqexd`'s own dispatch** every time this runs, and a route
//! that is served and unlisted here fails the test.
//!
//! That is the same instrument `sqexd/tests/suite/route_coverage.rs` uses on
//! its own side, for the same reason: coverage counted from the served side is
//! the only kind that can notice something it has never heard of.
//!
//! # What the verdicts mean
//!
//! Each route says what reaches it, and a route nothing reaches has to say
//! **why** — which turns "we have not done that" from an absence into a
//! sentence somebody wrote on purpose.

use std::collections::BTreeMap;

/// What reaches a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// sigil's chat app reaches it.
    Chat,
    /// sigil's voice app or `sigil-net` reaches it.
    Voice,
    /// The operator console reaches it.
    Admin,
    /// Reached indirectly: a client never calls it by name, but the code paths
    /// above go through it.
    Beneath,
    /// Exchange-to-exchange. A client has no business here at all.
    NotAClientRoute,
    /// Nothing in sigil reaches it yet, and this says why.
    NotYet(&'static str),
}

use Reached::*;

/// Every route this build of `sqexd` serves, and what reaches it.
const COVERAGE: &[(&str, &str, Reached)] = &[
    // ---- chat: channels -------------------------------------------------
    ("POST", "/channel/create", Chat),
    ("POST", "/channel/join", Chat),
    ("POST", "/channel/leave", Chat),
    ("POST", "/channel/close", Chat),
    ("POST", "/channel/post", Chat),
    ("POST", "/channel/fetch", Chat),
    ("POST", "/channel/info", Chat),
    ("POST", "/channel/mine", Chat),
    ("POST", "/channel/list", Chat),
    ("POST", "/channel/invite", Chat),
    ("POST", "/channel/remove", Chat),
    ("POST", "/channel/retain", Chat),
    ("POST", "/channel/redact", Chat),
    ("POST", "/channel/signal", Chat),
    ("POST", "/channel/cursor", Chat),
    ("POST", "/channel/cursors", Chat),
    ("POST", "/channel/directory", Chat),
    ("POST", "/channel/replicate", Chat),
    ("POST", "/channel/unreplicate", Chat),
    ("POST", "/channel/key/put", Chat),
    ("POST", "/channel/key/get", Chat),
    ("POST", "/channel/key/missing", Chat),
    (
        "POST",
        "/channel/equivocation",
        // SIP-31 evidence that the exchange signed two histories for one
        // position. `Chat::poll` fetches it when a fetch is refused as
        // equivocated, so it is reached — but sigil does not yet *render* the
        // proof, which `Trouble` will want when it grows a `forked` field.
        Beneath,
    ),
    // ---- chat: keys, blobs, people --------------------------------------
    ("POST", "/prekey/publish", Chat),
    ("POST", "/prekey/count", Chat),
    ("POST", "/prekey/clear", Chat),
    ("POST", "/prekey/take", Beneath),
    ("POST", "/blob/limits", Chat),
    ("POST", "/blob/begin", Chat),
    ("POST", "/blob/put", Chat),
    ("POST", "/blob/commit", Chat),
    ("POST", "/blob/abort", Beneath),
    ("POST", "/blob/get", Chat),
    ("POST", "/blob/attach", Chat),
    ("POST", "/blob/detach", Beneath),
    (
        "POST",
        "/blob/head",
        NotYet("nothing asks whether a file is still there before fetching it"),
    ),
    ("POST", "/profile/put", Chat),
    ("POST", "/profile/get", Chat),
    ("POST", "/block/set", Chat),
    ("POST", "/block/list", Chat),
    ("POST", "/device/list", Chat),
    ("POST", "/device/revoke", Chat),
    (
        "POST",
        "/device/register",
        NotYet(
            "sigil is always the first device of its account, which registers itself. \
             Registering *with* a credential is how a second one is enrolled, and that \
             is the other half of the Devices screen",
        ),
    ),
    ("POST", "/admission/request", Chat),
    ("POST", "/name/resolve", Chat),
    ("POST", "/name/reverse", Beneath),
    ("POST", "/events", Chat),
    // ---- names, written side --------------------------------------------
    ("POST", "/name/claim", Chat),
    (
        "POST",
        "/name/release",
        NotYet("the other half of claiming one"),
    ),
    // ---- voice -----------------------------------------------------------
    ("POST", "/session/open", Voice),
    ("POST", "/session/send", Voice),
    ("POST", "/session/recv", Voice),
    ("POST", "/session/close", Voice),
    ("POST", "/room/join", Voice),
    ("POST", "/room/leave", Voice),
    (
        "POST",
        "/session/call",
        NotYet("SIP-39 cross-exchange calling, which needs one identity on two exchanges"),
    ),
    (
        "POST",
        "/session/decline",
        NotYet("the cross-exchange refusal, and it arrives with /session/call"),
    ),
    // ---- mailbox: retired ------------------------------------------------
    //
    // sigil rang over the SIP-5 mailbox until SIP-36 gave calls an event of
    // their own. Nothing reaches these now, and that is the point.
    (
        "POST",
        "/mailbox/send",
        NotYet("the mailbox ring was retired when SIP-36 ringing replaced it"),
    ),
    (
        "POST",
        "/mailbox/list",
        NotYet("retired with the mailbox ring"),
    ),
    (
        "POST",
        "/mailbox/fetch",
        NotYet("retired with the mailbox ring"),
    ),
    (
        "POST",
        "/mailbox/delete",
        NotYet("retired with the mailbox ring"),
    ),
    (
        "POST",
        "/mailbox/status",
        NotYet("retired with the mailbox ring"),
    ),
    // ---- the operator's side ---------------------------------------------
    // The SIP-10 authority protocol: a nonce, then a batch signed against it.
    // All seventeen operations go through these two.
    ("GET", "/admin/challenge", Admin),
    ("POST", "/admin/command", Admin),
    ("GET", "/status", Admin),
    ("GET", "/health", Admin),
    (
        "GET",
        "/exchange/ping",
        NotYet(
            "the one whitelist-gated route. Reaching it would say whether *this* client \
             is admitted, which is a different question from whether the exchange is up",
        ),
    ),
    // ---- other services --------------------------------------------------
    (
        "POST",
        "/beacon/beat",
        NotYet("SIP-4 presence, which sigil does not publish"),
    ),
    ("POST", "/beacon/read", NotYet("SIP-4 presence")),
    (
        "POST",
        "/rendezvous/introduce",
        NotYet("SIP-25 introductions"),
    ),
    ("POST", "/attest/lodge", NotYet("SIP-27 attestation")),
    ("POST", "/attest/read", NotYet("SIP-27 attestation")),
    ("POST", "/resolve/publish", NotYet("SIP-28 resolution")),
    ("POST", "/resolve/get", NotYet("SIP-28 resolution")),
    ("POST", "/resolve/successor", NotYet("SIP-28 resolution")),
    // ---- exchange to exchange --------------------------------------------
    ("POST", "/peer/hello", NotAClientRoute),
    ("POST", "/peer/pull", NotAClientRoute),
    ("POST", "/peer/envelopes", NotAClientRoute),
    ("POST", "/peer/blobs", NotAClientRoute),
    ("POST", "/peer/records", NotAClientRoute),
];

/// Where sqexd's dispatch lives.
///
/// Anchored on `CARGO_MANIFEST_DIR` rather than the working directory, which
/// for an integration test is the crate and not the workspace. The sqex crates
/// are path dependencies beside this tree, and CI checks the two out as
/// siblings, so this resolves the same in both places.
fn server_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sqex-sigil/crates/sqexd/src/server.rs")
}

/// Every route `sqexd` serves, read out of its dispatch.
///
/// Two shapes, because there are two: the `match (method, path)` arms, and
/// `/events`, which is answered before the match because its reply never
/// finishes.
fn served() -> Vec<(String, String)> {
    let path = server_path();
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}. Is the sqex worktree beside this one?",
            path.display()
        )
    });
    let mut out = Vec::new();
    let mut push = |m: &str, p: &str| out.push((m.to_string(), p.to_string()));

    for method in ["GET", "POST"] {
        // The dispatch arms.
        let needle = format!("(\"{method}\", \"");
        let mut from = 0;
        while let Some(i) = src[from..].find(&needle) {
            let open = from + i + needle.len();
            let Some(len) = src[open..].find('"') else {
                break;
            };
            let path = &src[open..open + len];
            if path.starts_with('/') {
                push(method, path);
            }
            from = open + len;
        }
        // Anything answered before the match.
        let early = format!("method == http::Method::{method} && path == \"");
        let mut from = 0;
        while let Some(i) = src[from..].find(&early) {
            let open = from + i + early.len();
            let Some(len) = src[open..].find('"') else {
                break;
            };
            push(method, &src[open..open + len]);
            from = open + len;
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn every_route_the_exchange_serves_is_accounted_for() {
    let served = served();
    assert!(
        served.len() > 50,
        "only {} routes found — the scan has stopped matching the dispatch, \
         which would make this test pass by finding nothing",
        served.len()
    );

    let listed: BTreeMap<(String, String), Reached> = COVERAGE
        .iter()
        .map(|(m, p, r)| ((m.to_string(), p.to_string()), *r))
        .collect();

    let missing: Vec<_> = served.iter().filter(|r| !listed.contains_key(r)).collect();
    assert!(
        missing.is_empty(),
        "these routes are served and this file does not say what reaches them.\n\
         Add each with what reaches it, or NotYet(\"why not\"):\n{missing:#?}"
    );

    let stale: Vec<_> = listed.keys().filter(|r| !served.contains(r)).collect();
    assert!(
        stale.is_empty(),
        "these are listed here and no longer served — the exchange dropped them:\n{stale:#?}"
    );
}

/// What sigil reaches, said out loud, so a change to it is visible in a diff.
#[test]
fn the_coverage_is_what_it_says_it_is() {
    let total = COVERAGE.len();
    let peer = COVERAGE
        .iter()
        .filter(|(_, _, r)| *r == NotAClientRoute)
        .count();
    let reached = COVERAGE
        .iter()
        .filter(|(_, _, r)| matches!(r, Chat | Voice | Admin | Beneath))
        .count();
    let client = total - peer;

    // Pinned, so growth is deliberate and a regression is a failure rather
    // than a number nobody looked at.
    assert_eq!(
        total, 80,
        "the exchange serves a different number of routes"
    );
    assert_eq!(peer, 5, "SIP-35 peering routes, which no client calls");
    assert_eq!(
        client, 75,
        "client-reachable routes: everything but exchange-to-exchange"
    );
    assert_eq!(
        reached, 56,
        "routes sigil reaches. Raise this when a stage lands; it is the only \
         honest measure of \"every endpoint implemented\""
    );
}
