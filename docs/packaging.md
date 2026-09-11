# Packaging

## macOS

```bash
scripts/macos-app          # target/sigil.app, ad-hoc signed
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
> run `sigil.app`. Once the bundle has run **once**, macOS knows
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
install -m 755 target/debug/examples/notify_probe target/sigil.app/Contents/MacOS/
./target/sigil.app/Contents/MacOS/notify_probe
```

### Distributing

The script signs ad-hoc, which is enough to run locally and not enough for
anyone else. A distributable build needs a Developer ID, hardened runtime,
notarisation and stapling:

```bash
codesign --force --deep --options runtime --timestamp \
    -s "Developer ID Application: NAME (TEAMID)" target/sigil.app
xcrun notarytool submit --wait --apple-id ... --team-id ... --password ... sigil.zip
xcrun stapler staple target/sigil.app
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

`packaging/icon.py` draws it at any size, with no dependencies. It is generated
rather than checked in so it cannot drift from the disc the tray draws, and so
there is no binary blob in the repository that nobody can diff.

## Releasing

`.github/workflows/release.yml`, on a pushed `v*` tag. Four artefacts, and every
one of them built **natively**:

| | runner |
|---|---|
| `x86_64-linux-gnu` | `ubuntu-latest` |
| `aarch64-linux-gnu` | `ubuntu-24.04-arm` |
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
  complete and is missing an architecture.

### What is not signed

macOS is ad-hoc signed, so Gatekeeper refuses it on first launch and blames the
file ("damaged and can't be opened") for what is really the quarantine flag.
The release notes say how to clear it. A Developer ID and notarisation are what
remove the step; see **Distributing** above.

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
