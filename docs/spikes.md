# Spike results

The plan named three spikes to run before building anything, on the grounds
that all three are cheap now and expensive to discover late. Two are done.

## (a) One identity, two connections — PASSES

`crates/sigil-net/tests/one_identity_two_connections.rs`.

Two identities, each holding a chat connection *and* a voice connection against
one in-process `sqexd`, with media and messages crossing at the same time. Both
work. The chat connection kept serving requests while datagrams flowed on the
other, and the voice connection still carried frames after the chat traffic.

This is what the whole design rests on, and it needed proving rather than
reading: `sqex-voice`'s README says "One identity, one client", while
`sqexd/src/server.rs:113-115` says an identity may hold several connections at
once. Both are true — the README is about two *processes* each negotiating
their own SIP-12 session with the same peer, where the peer keeps one and the
other goes deaf. Connections are a separate question, and the answer is yes.

**Negative control run.** Connecting the voice side anonymously
(`Client::connect` rather than `connect_as`) fails the test with
`403 opening a session requires an advertised Ed25519 identity (SIP-3)`, so the
test genuinely exercises the identity-keyed path rather than passing vacuously.

**The cost is real and now pinned by a test.** A relayed datagram is fanned out
to *every* live connection the recipient identity holds, so each voice frame is
also written to the connection chat is using, where nothing reads it. That is
the price of the two-connection design; the second test asserts it, so if the
behaviour ever changes the reasoning changes with it.

## (b) The dependency set — PASSES on macOS

Scratch project, since it is throwaway once answered.

- **One winit.** `eframe` 0.36, `tray-icon` 0.24 and `global-hotkey` 0.8 all
  resolve against **winit 0.30.13**, with a single `raw-window-handle` 0.6.2.
  No duplicate-winit problem, which was the risk.
- **A tray icon builds from inside eframe's creator** and a global hotkey
  registers, in one process, at runtime on macOS. Verified by running it, not
  only by compiling it.
- **`gtk` 0.18 appears in the lock** — `tray-icon`'s Linux path (libappindicator
  over GTK3). So **GTK3 dev headers are a Linux build dependency**, which the
  plan's build-deps list had missed. Added.
- **Correction to the plan:** the hotkey registered on macOS with **no
  Accessibility permission prompt**. `global-hotkey` uses Carbon's
  `RegisterEventHotKey` there, not a `CGEventTap`. Whether *push-to-talk*
  specifically works without it — that needs key-release, not just key-press —
  is still open and must be checked before the Shortcuts settings pane claims
  either way.

### The finding that changes the architecture

**eframe 0.36 replaced `App::update` with a `logic` / `ui` split**, and this is
better than what the plan assumed:

```rust
pub trait App {
    /// Called once before each call to `Self::ui`, and additionally also called
    /// when the UI is hidden, but `request_repaint` was called.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut Frame) { }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);
}
```

> While the window is hidden, `eframe` runs no egui pass at all (so that no ui
> state is disturbed), and calls this via `egui::Context::run_logic` instead.

Notedeck's `update`-for-all-apps / `render`-for-the-visible-one split is an
*app-level* invention on egui 0.31, where no such thing existed. In 0.36 the
framework provides the same shape one layer down, and `logic` keeps running
**while the window is hidden** — which is exactly what sigil needs for
close-to-tray with a live call and a ring listener.

So sigil's `App` trait keeps notedeck's shape, and the host maps it straight
through: `eframe::App::logic` drives every opened app's `update`,
`eframe::App::ui` renders the active one. No workaround needed.

Note the corollary: notedeck's 0.31 patterns cannot be copied verbatim. Their
`CentralPanel::show(ctx, ..)` is now `show(ui, ..)`, and anything reading
`ctx.style()` per frame wants re-checking against 0.36.

## (c) PipeWire capture — NOT YET RUN

Cannot be run from this machine: it is macOS, and PipeWire is Linux-only. This
is the one remaining unproven assumption in the plan, and it is the one that
decides whether the Linux audio arm is a small backend or a project of its own.
It must run on a Linux box (or a container with a PipeWire daemon) before
`sigil-voice` is built out.
