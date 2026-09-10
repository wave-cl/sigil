# sigil

A desktop application for [sqex](https://github.com/wave-cl/sqex): voice calls
and end-to-end encrypted chat over sQUIC, in one window, on one identity. macOS
and Linux.

It brings together what `sqex-voice` (a CLI) and `sqex-chat` (a terminal UI) do
separately today, and adds what a terminal could not: rendered avatars and
images, a call you join by clicking, and a phone that actually rings.

## Status

Early. See [docs/spikes.md](docs/spikes.md) for what has been proven so far and
what has not.

## Install

Built binaries are on the
[releases page](https://github.com/wave-cl/sigil/releases/latest), for macOS
and Linux on both aarch64 and x86_64.

**macOS.** Unzip, move `sigil.app` to `/Applications`, and clear the quarantine
flag — the build is signed ad-hoc rather than notarised, so Gatekeeper refuses
it and blames the file for what is really the flag:

```bash
xattr -dr com.apple.quarantine /Applications/sigil.app
```

Do that because you trust where you fetched it from, not because a message told
you to.

**Linux.** A `.deb` or `.rpm`, which register the `sigil://` link handler:

```bash
sudo apt install ./sigil-vX.Y.Z-x86_64-linux-gnu.deb
xdg-mime query default x-scheme-handler/sigil     # expects sigil.desktop
```

Check the second line. A registration nothing has read looks exactly like sigil
ignoring the link. There is a `.tar.gz` of the bare binary for distributions
that are neither; it needs a Vulkan driver, and ALSA or PipeWire for a call.

## Building

Needs Rust 1.98.0 (pinned in `rust-toolchain.toml`) and **cmake**, which the
Opus codec builds itself with.

On Linux, also: `pkg-config`, `libasound2-dev`, `libpipewire-0.3-dev`,
GTK3 dev headers and `libxdo-dev` (both for the tray icon), `libpcsclite-dev`
(sqnr links a YubiKey signer even though sigil only uses software identities),
and the X11/Wayland/Vulkan dev packages.

The last two are **link**-time requirements, so `cargo check` passes without
them and only a real build fails.
`cargo build --no-default-features` drops the PipeWire backend if those headers
are not available.

```
./check
```

## Running it

```bash
scripts/macos-app && ./target/sigil.app/Contents/MacOS/sigil
```

The bundle is what makes notifications and the microphone work; running the
binary inside it rather than `open`ing the app keeps its log on the terminal.
`cargo run -p sigil-shell` is fine for iterating, but cannot notify.

The exchange comes from the identity's primary SIP-38 handle, so nothing needs
configuring. To see which layer named it:

```bash
cargo run -p sigil-net --example where
```
