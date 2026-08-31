# sigil

A desktop application for [sqex](../sqex): voice calls and end-to-end encrypted
chat over sQUIC, in one window, on one identity. macOS and Linux.

It brings together what `sqex-voice` (a CLI) and `sqex-chat` (a terminal UI) do
separately today, and adds what a terminal could not: rendered avatars and
images, a call you join by clicking, and a phone that actually rings.

## Status

Early. See [docs/spikes.md](docs/spikes.md) for what has been proven so far and
what has not.

## Building

Needs Rust 1.98.0 (pinned in `rust-toolchain.toml`) and **cmake**, which the
Opus codec builds itself with.

On Linux, also: `pkg-config`, `libasound2-dev`, `libpipewire-0.3-dev`,
GTK3 dev headers (for the tray icon), `libpcsclite-dev` (sqnr links a YubiKey
signer even though sigil only uses software identities), and the
X11/Wayland/Vulkan dev packages.
`cargo build --no-default-features` drops the PipeWire backend if those headers
are not available.

```
./check
```
