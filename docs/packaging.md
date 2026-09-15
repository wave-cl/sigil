# Packaging

## macOS

```bash
scripts/macos-app          # target/Sigil.app, ad-hoc signed
```

The bundle is **not** a distribution nicety. Two things only work inside one,
and both fail silently without it:

- **Notifications.** macOS routes them by `CFBundleIdentifier`. Without one,
  an incoming call is drawn and never announced — so sigil reaches only
  somebody already looking at the window.
- **The microphone.** A bundle without `NSMicrophoneUsageDescription` is denied
  audio input, and the failure presents as a silent call rather than a
  permissions error.

Both keys are written by `scripts/macos-app`, by hand rather than through
`cargo-bundle`, because they are the entire reason the bundle exists and they
should be visible in the script that writes them.

### A trap when testing notifications

`Notifier` asks whether it can bind the bundle id, because that is the operative
question — if it binds, posting works. That is **not** the same as asking
whether the executable sits inside a `.app`, and the two diverge in a way that
will mislead you:

> A bare `cargo run` binary reports *unavailable* on a machine that has never
> run `Sigil.app`. Once the bundle has run **once**, macOS knows
> `org.squic.sigil`, and from then on the bare binary binds it happily and
> really can post.

So on your own machine an unbundled build will start claiming notifications
work. It is telling the truth about your machine and nothing about anyone
else's. **Never take an unbundled build as evidence that shipping one would be
fine.**

`crates/sigil-platform/examples/notify_probe.rs` posts a real one, which is a
stronger check than the probe:

```bash
cargo build -p sigil-platform --example notify_probe
install -m 755 target/debug/examples/notify_probe target/Sigil.app/Contents/MacOS/
./target/Sigil.app/Contents/MacOS/notify_probe
```

### Distributing

The script signs ad-hoc, which is enough to run locally and not enough for
anyone else. A distributable build needs a Developer ID, hardened runtime,
notarisation and stapling:

```bash
codesign --force --deep --options runtime --timestamp \
    -s "Developer ID Application: NAME (TEAMID)" target/Sigil.app
xcrun notarytool submit --wait --apple-id ... --team-id ... --password ... sigil.zip
xcrun stapler staple target/Sigil.app
```

A `.dmg` needs `create-dmg`, which is not installed here; the `.app` is the
deliverable until somebody needs a disk image.

## Linux

```bash
scripts/linux-packages     # deb and rpm
```

deb and rpm rather than AppImage or flatpak — what notedeck ships, and what a
distribution's own tooling expects. A flatpak is worth revisiting later, since
it would make the XDG portal paths (global shortcuts especially) first-class
rather than best-effort.

Build dependencies are listed at the top of the script. Two are easy to miss:
**libgtk-3-dev**, because the tray icon speaks StatusNotifierItem through GTK
even though sigil draws with wgpu; and **libpipewire-0.3-dev**, without which
the build falls back to ALSA through cpal.

`packaging/sigil.desktop` registers `sigil://` links, and the postinst runs
`update-desktop-database` — without it the handler is written to a file nothing
has read, and clicking a link does nothing, which looks exactly like sigil
ignoring it. Check it took:

```bash
xdg-mime query default x-scheme-handler/sigil
```

## The icon

A white hexagon, pointed left and right, on a black rounded square; in it,
four black arms of one width meet around the centre without touching, each
rounded on the corner where it turns towards the next, so the white between
them winds through the middle. `packaging/icon.py` draws it at any size,
with no dependencies, and `crates/sigil-platform/src/mark.rs` draws the same
thing for the tray from the same numbers -- a test there runs the script and
compares the two pixel for pixel, so the dock and the menu bar cannot drift
apart. On macOS the tray gets the emblem alone as a template, which the menu
bar tints to match itself; the running application sets the same icon on
itself at launch, so the dock shows one icon before and after. The icon is
generated rather than checked in so there is no binary blob in the
repository that nobody can diff.

## What the desktop shows while sigil is not in front

**The tray.** The number waiting sits beside the mark (a menu bar title on
macOS, an appindicator label on Linux), the mark carries a dot while there
is anything, and the tooltip says it in words. A left press on the mark
brings the window up on macOS; the library reports no press at all on
Linux, so there the menu is what a press opens, and **Open Sigil** is its
first item. The menu also holds **Do not disturb** and **Quit Sigil**.
Closing the window puts sigil in the tray rather than ending it, where
there is a tray: Quit is how it ends. Without one, a close is a close.

**The application's own icon.** On macOS the count goes on the Dock tile
(`NSDockTile`). On Linux there is no standard; sigil sends the
`com.canonical.Unity.LauncherEntry.Update` signal on the session bus,
naming `application://sigil.desktop`, which KDE, Cinnamon and most docks
honour and GNOME ignores. For the desktop to tie the window to that
`.desktop` file the window's app id is set to `sigil` -- Wayland's app id,
X11's class -- which is also what `StartupWMClass=sigil` in the `.desktop`
file matches. Both are `Capability` rows in the Desktop pane, so an absent
badge has a stated reason.

**Notifications.** A message arriving while the window is not in front
is said out loud -- who, where, and what; several arriving together in
one conversation are one notice that counts them; a mention keeps its own
words. A ring is said with a sound (one chime -- sigil has no ringer of
its own yet) and brings the window up. Pressing a notification brings the
window up on the conversation it was about: each notification with
somewhere to lead is watched on a thread of its own until pressed or
dismissed, which is the only shape either desktop's library offers, so no
more than 32 are watched at once and the rest lead nowhere. On Linux the
press is the daemon's `default` action, which not every daemon delivers.

## Releasing

`.github/workflows/release.yml`, on a pushed `v*` tag. Four artefacts, and every
one of them built **natively**:

| | runner |
|---|---|
| `x86_64-linux-gnu` | `ubuntu-22.04` |
| `aarch64-linux-gnu` | `ubuntu-22.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-15-intel` |

Native rather than cross, because sigil links ALSA, PipeWire, GTK, libxdo and
libpcsclite: a cross toolchain would need every one of those headers and
libraries for the other architecture inside the image, and GitHub hands out
machines of both architectures for nothing. It calls the same
`scripts/linux-packages` and `scripts/macos-app` somebody builds with by hand —
a release built by a second recipe is a release nobody has tested.

Three guards, each of which exists because its failure is silent:

- **The manifest must not carry a path dependency.** See
  `docs/dependencies.md`; a tag that does can only be built here.
- **The runner must be the architecture it is named for.** A runner label is a
  promise about a machine and the promise is what names the file, so it asks
  `uname -m`. Otherwise a label that quietly resolves elsewhere ships two
  x86_64 binaries, one of them labelled `aarch64`, and nobody finds out until
  it will not start.
- **All eight files must be present before anything is published.** `needs`
  stops a *failed* build publishing. It does not stop a build that succeeded
  while producing less than it should — a rename that matched nothing, an
  upload glob that found one file — and that publishes a release which looks
  complete and is missing an architecture. After signing, ten and only ten.

### What is not signed

macOS is ad-hoc signed, so Gatekeeper refuses it on first launch and blames the
file ("damaged and can't be opened") for what is really the quarantine flag.
The release notes say how to clear it. A Developer ID and notarisation are what
remove the step; see **Distributing** above.

### The signed manifest, and self-update

What *is* signed is the manifest. After the eight files are in `dist`, the
`release` job checks out the tag's tree and runs `sigil-update-tool` from
`crates/sigil-update` — the same crate an installed sigil verifies with, so
what is signed is byte for byte what is checked:

```
sigil-update-tool manifest --tag v0.1.6 --dir dist --out dist/sigil-v0.1.6-manifest.json
sigil-update-tool sign   dist/sigil-v0.1.6-manifest.json      # SIGIL_UPDATE_KEY in the environment
sigil-update-tool verify dist/sigil-v0.1.6-manifest.json dist  # with the compiled-in key, re-hashing every file
```

The manifest is compact JSON with sorted keys — the tag, the version, and
each asset's `bytes` and `sha256` — and `.sig` beside it is 128 hex
characters: an Ed25519 signature over the **exact bytes of the file**, never a
re-serialisation. Ten files are published, and the job refuses to publish
nine or eleven.

The tool refuses three things, each of which would otherwise be a release
that looks right and installs nowhere: a tag that is not the tree's
`Cargo.toml` version (bump before tagging), a `dist` that is not exactly the
eight builds, and a secret whose public key is not the one in
`crates/sigil-update/src/lib.rs` — `PUBLIC_KEY`, which every sigil is built
with.

**The key.** `scripts/update-key` prints a fresh pair: the public half as
Rust for `lib.rs`, the seed as the value of the repository secret
`SIGIL_UPDATE_KEY` (`gh secret set SIGIL_UPDATE_KEY --repo wave-cl/sigil`).
The seed lives in that secret and nowhere else. **Rotation** is a future
concern with a known shape: a build trusts one key, so a new key has to ship
in a release signed by the old one — commit the new `PUBLIC_KEY`, release
with the old secret, then swap the secret. Two releases, in that order.

**What the app does with it.** `sigil-update` checks `/releases/latest` five
seconds after launch and daily; a newer tag with a manifest and signature
that verify, and a build for this install, is `Available`. The Desktop tab
is marked and one notification is posted; nothing is fetched until Update is
pressed. The download streams through SHA-256 into a `.part` file and is
only renamed when the digest matches. Installing never writes over the
running binary: a macOS bundle is unpacked with `ditto` beside the old one,
renamed into place with the old kept as `Sigil.app.previous` (removed at the
next start), and the process keeps running from the moved-aside inode; a
Linux tarball's binary is copied to `<exe>.new` and renamed over; a `.deb` or
`.rpm` is handed to `apt-get`/`dnf` (falling back to `dpkg`/`rpm`) under
`pkexec`. Restart spawns a shell that waits for this pid to release the
instance lock, then `open -n`s the bundle or execs the binary; it gives up
after a minute. The ad-hoc signature is per build, so macOS asks about the
microphone again after an update.

A release with no manifest — everything before v0.1.6 — is shown as "not
signed" and never installed. A copy that is not in a `.app`, or whose
directory it cannot write, says so on the Desktop tab instead of offering a
button.

**Rehearsing an update** without cutting a release: serve a directory as a
release (a `/releases/latest` answer naming the files, and the files under
`/download/<name>`), sign its manifest with the real seed, then either run the
app with `SIGIL_UPDATE_API=http://127.0.0.1:PORT` — it says so loudly in the
log — or run `cargo run -p sigil-update --example rehearsal -- <api base>
<Sigil.app>`, which makes the same calls the buttons do and prints each
state. The manifest for a rehearsal has to be written with the library rather
than the tool, because the tool refuses a tag that is not this tree's
version — which is the point of it.

## What the video player links

`sigil-video` plays an MP4 attachment -- H.264 pictures, AAC sound -- in the
transcript. The H.264 decoder is Cisco's openh264, and the `openh264` crate
**builds it from its own C++ source** in `build.rs`: no system package on any
platform, nothing to add to the apt line above or to a runner, and the same
decoder on all four release builds. It uses `nasm` for its assembly when one
is on the path and compiles the C fallback when not, so a machine without
`nasm` builds a slower decoder rather than no decoder (this laptop has none
and decodes 720p at 347 frames a second). The container reader (`mp4`) and
the AAC decoder (`symphonia`) are Rust. Sound goes out through `cpal`, which
sigil already links for calls.

What it does not play: HEVC (an iPhone's "High Efficiency" default), VP9 and
AV1. Those show as a file with Save, and the box says why. Playing them means
a second decoder, and every candidate for HEVC is a native library with a
system package on each platform -- a packaging decision, not a code one.

## CI

`.github/workflows/ci.yml` is the single source of truth, and `./check` runs its
steps locally by reading it — so the two cannot drift.

sqex, sqnr and squic-rust are all public, so CI needs **no secrets** and runs
unchanged on a fork. It used to need one thing an ordinary repository does not
— a second checkout of `wave-cl/sqex` beside sigil, because the manifest named
`../sqex-sigil` — and pinning the sqex crates to a tag removed it. See
`docs/dependencies.md`.

### Why there is a job called `complete`

A green tick can mean *nothing ran*. A matrix that resolved empty, a path filter
that excluded everything, a job skipped because one it needed was skipped — all
of those are green, and none of them checked anything.

So `complete` names every job that must have run and asserts each one
**succeeded** rather than merely not failing: `skipped` and `cancelled` are
failures there. Branch protection should require `complete`, not the individual
jobs.

### Why there is a test floor

`cargo test` exits 0 when it runs no tests. `scripts/run-tests` counts what
passed and compares it against `scripts/test-floor`, so losing tests fails CI
until somebody lowers the number in a diff. It is set to the exact current
count, not a round number below it — the point is that losing one test is
enough.
