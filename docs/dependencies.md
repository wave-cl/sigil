# Depending on sqex

sigil is its own cargo workspace and depends on five crates from
[`sqex`](../../sqex): `sqex-proto`, `sqex-discovery`, `sqex-voice`,
`sqex-chat`, and `sqexd` (tests only).

## Path now, git tags later

The committed manifest uses **path dependencies** into `../sqex`:

```toml
sqex-voice = { path = "../sqex/crates/sqex-voice" }
```

This is local co-development, and it is how sqex itself works against `sqnr`
during a change that spans both. A release pins them to git tags instead,
exactly as `sqex` pins `squic` and `sqnr`:

```toml
sqex-voice = { git = "https://github.com/wave-cl/sqex", tag = "v0.25.0" }
```

Switch when the sqex side of a change has landed and been tagged. Until then a
path dependency is honest about the fact that the two move together.

`sqexd` is a **dev-dependency only**. Tests drive a real exchange in-process
rather than a mock, on the principle sqex states plainly: a client that was only
ever tested against a mock has tested the mock.

## The worktree, and why

A path dependency compiles whatever branch `../sqex` happens to have checked
out. That is fine until something else is working in that tree — another
session, or you in a second terminal — at which point there are two problems:

- **The branch moves underneath the build.** A `git checkout` over there changes
  what sigil compiles over here, with no warning.
- **Cargo's target-directory lock is per directory.** Two cargo processes in one
  tree do not run in parallel; the second blocks on the lock, and a blocked
  cargo looks exactly like a very slow compile. This cost thirteen minutes
  before anyone noticed it was not compiling at all.

Both were real. So the sqex-side work for sigil lives in a **git worktree** —
the same repository, a separate branch, a separate working copy, a separate
target directory:

```bash
scripts/dev-worktree            # create it and wire sigil up
scripts/dev-worktree --status   # show what is wired where
scripts/dev-worktree --remove   # unwire and remove it
```

The manifest names `../sqex-sigil` directly, and the script creates a worktree
there. So a fresh clone needs one command before it builds, and in exchange the
crate graph is unambiguous.

### Why not a `.cargo/config.toml` override

The obvious alternative is to keep the manifest pointing at `../sqex` and
redirect with a gitignored `paths` override. That was tried first, and cargo
refuses it:

> path override for crate `sqex-voice` has altered the original list of
> dependencies … This is currently allowed but is known to produce buggy
> behavior with spurious recompiles and changes to the crate graph … In the
> future, however, this message will become a hard error.

The reason is structural rather than incidental: the sqex crates depend on each
other by path, so overriding one changes the resolved dependencies of the
others, and `paths` was never meant to reshape a graph. Naming the worktree in
the manifest keeps exactly one copy of each crate in the graph.

Verify what is actually being compiled — resolution is easy to assume:

```bash
scripts/dev-worktree --status
```
