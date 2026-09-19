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
    // SIP-52: the session's first reconcile, and the phone's wake window.
    ("POST", "/channel/catchup", Chat),
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
    ("POST", "/device/register", Chat),
    // SIP-67: which account this transport identity is registered to. A
    // linked device learns that from the pairing claim it was given, and a
    // desktop opens an identity that is its own account, so nothing asks
    // yet. The phone will want it after a handover, when its store still
    // names the key that was retired.
    (
        "GET",
        "/device/account",
        NotYet("a device learns its account from the pairing claim, not from the registry"),
    ),
    ("POST", "/admission/request", Chat),
    ("POST", "/name/resolve", Chat),
    ("POST", "/name/reverse", Beneath),
    ("POST", "/events", Chat),
    // ---- names, written side --------------------------------------------
    ("POST", "/name/claim", Chat),
    ("POST", "/name/release", Chat),
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
    ("GET", "/exchange/peers", Chat),
    // SIP-64: the exchange's earlier keys, so a client that pinned an old one
    // can follow a handover it did not see. sigil pins through sqnr and
    // follows SIP-40's signed handover instead, and writing a lineage is an
    // operator's act, not a chat client's.
    (
        "GET",
        "/exchange/lineage",
        NotYet("sigil follows SIP-40's signed handover; it does not read the lineage"),
    ),
    (
        "POST",
        "/exchange/lineage",
        NotYet("an operator writes an exchange's lineage, through sqnr's admin protocol"),
    ),
    // SIP-43: asked with a channel's first `info`, so the session signs and
    // verifies under the exchange that orders it.
    ("POST", "/channel/home", Chat),
    // ---- other services --------------------------------------------------
    ("POST", "/beacon/beat", Chat),
    ("POST", "/beacon/read", Chat),
    (
        "POST",
        "/rendezvous/introduce",
        NotYet("SIP-25 introductions"),
    ),
    // SIP-48: the sealed backup is written and restored from sqex-chat's
    // command line; sigil has no backup surface yet.
    (
        "POST",
        "/backup/write",
        NotYet("SIP-48 sealed backup, from sqex-chat"),
    ),
    (
        "POST",
        "/backup/read",
        NotYet("SIP-48 sealed backup, from sqex-chat"),
    ),
    (
        "POST",
        "/backup/drop",
        NotYet("SIP-48 sealed backup, from sqex-chat"),
    ),
    // SIP-53: a channel's origin moves; sigil reads across a move through
    // sqex-chat's client, which verifies under former origins, but does
    // not post one.
    (
        "POST",
        "/channel/rehome",
        NotYet("SIP-53 origin succession, from sqex-chat's /rehome"),
    ),
    (
        "POST",
        "/channel/rehomed",
        NotYet("SIP-53 origin succession, from sqex-chat's /rehome"),
    ),
    (
        "POST",
        "/channel/stranded",
        NotYet("SIP-53 origin succession, from sqex-chat's /rehome"),
    ),
    // SIP-55: sigil's finder reads /channel/list, this exchange's own
    // directory; the federated search is not surfaced yet.
    (
        "POST",
        "/channel/search",
        NotYet("SIP-55 federated directory; sigil's finder is local"),
    ),
    // SIP-56: no moderation surface in sigil yet.
    (
        "POST",
        "/channel/mute",
        NotYet("SIP-56 abuse controls, from sqex-chat"),
    ),
    (
        "POST",
        "/channel/unmute",
        NotYet("SIP-56 abuse controls, from sqex-chat"),
    ),
    (
        "POST",
        "/channel/report",
        NotYet("SIP-56 abuse controls, from sqex-chat"),
    ),
    (
        "POST",
        "/channel/reports",
        NotYet("SIP-56 abuse controls, from sqex-chat"),
    ),
    (
        "POST",
        "/channel/dismiss",
        NotYet("SIP-56 abuse controls, from sqex-chat"),
    ),
    // SIP-59, 60, 62: moving home, reaching somebody at another exchange
    // and rotating the account key are `sqex-chat move`, `^N name@domain`
    // and `sqex-chat handover`; sigil has none of the three yet.
    (
        "POST",
        "/account/move",
        NotYet("SIP-59 moving home, from sqex-chat"),
    ),
    (
        "POST",
        "/account/home",
        NotYet("SIP-59 moving home, from sqex-chat"),
    ),
    (
        "POST",
        "/account/locate",
        NotYet("SIP-60 reaching another exchange, from sqex-chat"),
    ),
    (
        "POST",
        "/channel/create_at",
        NotYet("SIP-60 reaching another exchange, from sqex-chat"),
    ),
    (
        "POST",
        "/account/handover",
        NotYet("SIP-62 key handover, from sqex-chat"),
    ),
    (
        "POST",
        "/account/hint",
        NotYet("SIP-76: a device tells its home which origin to pull from"),
    ),
    (
        "POST",
        "/channel/chain",
        NotYet("SIP-77: this device's chain heads as the exchange holds them"),
    ),
    (
        "POST",
        "/channel/folded",
        NotYet("SIP-71: the folded log of a direct message the exchange ended"),
    ),
    ("POST", "/attest/lodge", NotYet("SIP-27 attestation")),
    ("POST", "/attest/read", NotYet("SIP-27 attestation")),
    ("POST", "/resolve/publish", NotYet("SIP-28 resolution")),
    ("POST", "/resolve/get", NotYet("SIP-28 resolution")),
    ("POST", "/resolve/successor", NotYet("SIP-28 resolution")),
    // SIP-44: signed with `sqex succession`; sigil shows the result in the
    // transcript and tells a succeeded key where its account went.
    (
        "POST",
        "/account/succeed",
        NotYet("SIP-44 succession, from the CLI"),
    ),
    (
        "POST",
        "/account/succession",
        NotYet("SIP-44 succession, from the CLI"),
    ),
    (
        "POST",
        "/account/lodge",
        NotYet("SIP-44 succession, from the CLI"),
    ),
    (
        "POST",
        "/account/lodged",
        NotYet("SIP-44 succession, from the CLI"),
    ),
    // SIP-45: a desktop holds its stream and needs no waking; the phone
    // client that will register an endpoint does not exist yet.
    (
        "POST",
        "/wake/register",
        NotYet("SIP-45 wake-up, for a phone client that does not exist yet"),
    ),
    (
        "POST",
        "/wake/forget",
        NotYet("SIP-45 wake-up, for a phone client that does not exist yet"),
    ),
    // ---- exchange to exchange --------------------------------------------
    ("POST", "/peer/hello", NotAClientRoute),
    ("POST", "/peer/pull", NotAClientRoute),
    ("POST", "/peer/envelopes", NotAClientRoute),
    ("POST", "/peer/blobs", NotAClientRoute),
    ("POST", "/peer/records", NotAClientRoute),
    ("POST", "/peer/channel", NotAClientRoute),
    ("POST", "/peer/forward", NotAClientRoute),
    ("POST", "/peer/standing", NotAClientRoute),
    // SIP-68: the account's home collects its waiting mail from a former
    // home, and says what it took. Exchange to exchange, and refused to
    // anyone the exchange is not peering with.
    ("POST", "/peer/mailbox", NotAClientRoute),
    ("POST", "/peer/mailbox/took", NotAClientRoute),
    // SIP-71: a folded direct message asked for across the peering, with
    // the caller having to be that identifier's home by its own signed
    // Move. Exchange to exchange like the rest of `/peer/`.
    ("POST", "/peer/folded", NotAClientRoute),
    // SIP-53, 54, 57, 59, 60, 61: more of the same, between exchanges.
    ("POST", "/peer/rehomed", NotAClientRoute),
    ("POST", "/peer/cursors", NotAClientRoute),
    ("POST", "/peer/signals", NotAClientRoute),
    ("POST", "/peer/tombstones", NotAClientRoute),
    ("POST", "/peer/mine", NotAClientRoute),
    ("POST", "/peer/moved", NotAClientRoute),
    ("POST", "/peer/invited", NotAClientRoute),
    ("POST", "/peer/wait", NotAClientRoute),
];

/// Where sqexd's dispatch lives -- asked of cargo, never assumed.
///
/// This used to be a sibling path, `../../../sqex-sigil/crates/sqexd/...`,
/// which was true for exactly as long as the sqex crates were path
/// dependencies into a worktree beside this tree. Pinning them to a git tag
/// made it false, and it failed in the worst available way: **green on the
/// machine that happens to have the worktree, red on CI**, which has only what
/// the manifest says. `./check` cannot catch that -- it was reading a
/// directory the runner does not have.
///
/// So ask cargo where the crate it actually compiled came from. That answers
/// the same for a path dependency and a git one, which is what this file
/// needs, because the manifest moves between them whenever somebody
/// co-develops against sqex (see `docs/dependencies.md`).
fn server_path() -> std::path::PathBuf {
    let out = std::process::Command::new(env!("CARGO"))
        // `--locked` so a test can never rewrite Cargo.lock as a side effect
        // of asking a question.
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo should be runnable from a cargo test");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata speaks json");
    let manifest = meta["packages"]
        .as_array()
        .expect("metadata lists packages")
        .iter()
        .find(|p| p["name"] == "sqexd")
        .and_then(|p| p["manifest_path"].as_str())
        .expect(
            "sqexd is a dev-dependency of this crate, so cargo knows where it \
             is -- if it does not, the dependency has been removed and this \
             test has nothing left to read",
        )
        .to_string();
    // .../crates/sqexd/Cargo.toml -> .../crates/sqexd/src/server.rs
    std::path::Path::new(&manifest)
        .with_file_name("src")
        .join("server.rs")
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
        total, 126,
        "the exchange serves a different number of routes"
    );
    assert_eq!(
        peer, 19,
        "SIP-35, 43, 53, 54, 57, 59, 60 and 61 peering routes, which no client calls"
    );
    assert_eq!(
        client, 107,
        "client-reachable routes: everything but exchange-to-exchange"
    );
    assert_eq!(
        reached, 63,
        "routes sigil reaches. Raise this when a stage lands; it is the only \
         honest measure of \"every endpoint implemented\""
    );
}
