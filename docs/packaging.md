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
