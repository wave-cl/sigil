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
    // SIP-47 §Catching up in one round trip: the session's first reconcile, and the phone's wake window.
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
    // SIP-18: a fetch that failed asks whether the blob is still there, which
    // is what tells a file past its retention window from a radio that
    // dropped -- both of which arrive at the client as the same error.
    ("POST", "/blob/head", Chat),
    ("POST", "/profile/put", Chat),
    ("POST", "/profile/get", Chat),
    ("POST", "/block/set", Chat),
    ("POST", "/block/list", Chat),
    ("POST", "/device/list", Chat),
    ("POST", "/device/revoke", Chat),
    ("POST", "/device/register", Chat),
    // SIP-44 §Which account a device is: which account this transport identity
    // is registered to. Asked once a connection, because the registry is the
    // one party that knows after the account changed its mind: a device
    // handed over, or revoked, has a store that still names the account it
    // was cut off from. `a_revoked_device_comes_back_up_as_itself_again`.
    ("GET", "/device/account", Chat),
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
    // SIP-39 §Pairs across exchanges: a call placed here for `name@domain`,
    // carried to their home, which rings them. `sigil_net::spawn_cross_call`,
    // from the Calls tab whenever what was typed is a name rather than a key
    // -- which it refused as "that is not a key" until now.
    ("POST", "/session/call", Voice),
    // The other half: a call carried *here* arrives as a `CrossCall` on the
    // event stream the chat session already holds, rings in the interface,
    // and is answered on that connection (`sigil_net::spawn_cross_answer`)
    // or refused on it -- `decline_cross`, which is this route. Both ends
    // are proven against two federated exchanges in `reaching_session`.
    ("POST", "/session/decline", Voice),
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
    // SIP-40 §Lineage: the exchange's earlier keys, so a client that pinned an old one
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
    // SIP-25, since sigil-net got direct calls: `sqex_voice::direct::connect`
    // → `sqex_proto::direct::introduce` posts this, and the reply is the
    // address and the moment both sides were told to begin punching. The
    // verdict here said NotYet for as long as it has worked; four direct
    // calls between a desktop and a phone on 2026-09-18 went through it.
    ("POST", "/rendezvous/introduce", Voice),
    // SIP-48, since v0.1.39: the Backup section of the Devices view -- the
    // key as 24 words, Back up now, Restore with the words, Drop.
    ("POST", "/backup/write", Chat),
    ("POST", "/backup/read", Chat),
    ("POST", "/backup/drop", Chat),
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
    // SIP-16 §Federated directory: sigil's finder reads /channel/list, this exchange's own
    // directory; the federated search is not surfaced yet.
    // SIP-16 §Federated directory: the directory pane, since v0.1.35.
    ("POST", "/channel/search", Chat),
    // SIP-56: no moderation surface in sigil yet.
    // SIP-56, since v0.1.36: mute/unmute from the members view, report from a
    // message's menu or the members view, reports and dismiss for admins.
    ("POST", "/channel/mute", Chat),
    ("POST", "/channel/unmute", Chat),
    ("POST", "/channel/report", Chat),
    ("POST", "/channel/reports", Chat),
    ("POST", "/channel/dismiss", Chat),
    // SIP-59, 60, 62: moving home, reaching somebody at another exchange
    // and rotating the account key are `sqex-chat move`, `^N name@domain`
    // and `sqex-chat handover`; sigil has none of the three yet.
    // SIP-60, since v0.1.35: writing to somebody at another exchange from
    // the home session -- `ensure_home` (move/home), `locate`, and the
    // conversation created at the lower key's home.
    ("POST", "/account/move", Chat),
    ("POST", "/account/home", Chat),
    ("POST", "/account/locate", Chat),
    ("POST", "/channel/create_at", Chat),
    (
        "POST",
        "/account/handover",
        NotYet("SIP-44 §The handover, from sqex-chat"),
    ),
    // SIP-60 §A device hints its home. Not sigil's to call: `sqex-chat`
    // posts it itself, from `open_dm` where a direct message is created at
    // the other party's home, and from `home` whenever it learns a channel's
    // origin is not this exchange. So it is reached in production the moment
    // a conversation lives somewhere else -- and it is still counted as
    // unreached here, because nothing in these tests produces that shape:
    // a direct message needs the other key to be the lower one *and* their
    // home already located, and a public room elsewhere cannot be joined at
    // all (`a_room_that_lives_at_another_exchange_is_not_joinable_from_here`
    // in `reaching_session`). Raise it when a test makes one.
    (
        "POST",
        "/account/hint",
        NotYet("reached through sqex-chat's open_dm and home; no test here makes the shape"),
    ),
    (
        "POST",
        "/channel/chain",
        NotYet(
            "SIP-43 §The heads by position: this device's chain heads as the exchange holds them",
        ),
    ),
    (
        "POST",
        "/channel/folded",
        NotYet("SIP-60 §Reading the folded log, from sqex-chat"),
    ),
    // SIP-27, both ways, from SIP-41's dialog: "Say at the exchange that we
    // compared them" lodges the one claim sigil makes, and opening the
    // dialog reads who else has made it about that key -- their word, shown
    // to be read and not acted on. The lodge had been reached for as long as
    // the checkbox existed, and was listed here as unreached the whole time.
    ("POST", "/attest/lodge", Chat),
    ("POST", "/attest/read", Chat),
    // SIP-28: an identity publishes where it can be reached -- host:port,
    // and what it speaks -- for others to look up. That is an identity that
    // runs something at a stable address. A chat client on a phone has no
    // address worth publishing (it changes with every network, sits behind
    // NAT, and advertising it would advertise the phone), reaches people
    // through the exchange, and finds a direct path for a call by SIP-25's
    // introduction, not by a published address. Deliberately not surfaced;
    // the CLI has it for the identities it is for.
    (
        "POST",
        "/resolve/publish",
        NotYet("SIP-28 is for an identity with a stable address to publish; a phone has none"),
    ),
    (
        "POST",
        "/resolve/get",
        NotYet(
            "nothing in sigil reaches a key by a published address; a call is introduced (SIP-25)",
        ),
    ),
    (
        "POST",
        "/resolve/successor",
        NotYet("SIP-28's move notice; sigil's successions are SIP-44's, which move the account"),
    ),
    // SIP-44, from the Devices pane since sqex 0.104.3 gave `Chat` the
    // methods: a will and guardians written by the account, a vouch as a
    // guardian, and the successor's claim -- `WriteWill`, `NameGuardians`,
    // `Vouch`, `Succeed`. The lodged policy is read for the pane and for a
    // successor's claim.
    ("POST", "/account/succeed", Chat),
    ("POST", "/account/lodge", Chat),
    ("POST", "/account/lodged", Chat),
    // What the exchange recorded of a succession: asked once when a direct
    // message opens (`Cmd::SuccessionOf`), the proof checked on the way in,
    // and said in the conversation with the old key. A transcript says so
    // only where the move was written into a room both were in.
    ("POST", "/account/succession", Chat),
    // SIP-45: the endpoint the platform offers is left with the exchange
    // (`Cmd::WakeEndpoint`), on every connect, and taken back when the
    // distributor goes -- `tests/wake_session.rs`, against an exchange that
    // then wakes a loopback distributor. A desktop offers no endpoint and
    // holds its stream open.
    ("POST", "/wake/register", Chat),
    ("POST", "/wake/forget", Chat),
    // ---- exchange to exchange --------------------------------------------
    ("POST", "/peer/hello", NotAClientRoute),
    ("POST", "/peer/pull", NotAClientRoute),
    ("POST", "/peer/envelopes", NotAClientRoute),
    ("POST", "/peer/blobs", NotAClientRoute),
    ("POST", "/peer/records", NotAClientRoute),
    ("POST", "/peer/channel", NotAClientRoute),
    ("POST", "/peer/forward", NotAClientRoute),
    ("POST", "/peer/standing", NotAClientRoute),
    // SIP-59 §Collecting mail: the account's home collects its waiting mail from a former
    // home, and says what it took. Exchange to exchange, and refused to
    // anyone the exchange is not peering with.
    ("POST", "/peer/mailbox", NotAClientRoute),
    ("POST", "/peer/mailbox/took", NotAClientRoute),
    // SIP-60 §The home learns of a stray: a stray told across the peering, with
    // the caller having to be that identifier's home by its own signed
    // Move. Exchange to exchange like the rest of `/peer/`.
    ("POST", "/peer/folded", NotAClientRoute),
    // SIP-59 §Collecting the backup, §Collecting wakes: an account's backup and its wake registrations
    // follow it home, collected by the home over the peering as its mail
    // is. Exchange to exchange.
    ("POST", "/peer/backup", NotAClientRoute),
    ("POST", "/peer/backup/blob", NotAClientRoute),
    ("POST", "/peer/backup/took", NotAClientRoute),
    ("POST", "/peer/wakes", NotAClientRoute),
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
        total, 130,
        "the exchange serves a different number of routes"
    );
    assert_eq!(
        peer, 23,
        "SIP-35, 43, 53, 54, 57, 59, 60 and 61 peering routes, which no client calls"
    );
    assert_eq!(
        client, 107,
        "client-reachable routes: everything but exchange-to-exchange"
    );
    assert_eq!(
        reached, 89,
        "routes sigil reaches. Raise this when a stage lands; it is the only \
         honest measure of \"every endpoint implemented\""
    );
}
