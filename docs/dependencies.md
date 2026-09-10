# Depending on sqex

sigil is its own cargo workspace and depends on five crates from
[`sqex`](https://github.com/wave-cl/sqex): `sqex-proto`, `sqex-discovery`,
`sqex-voice`, `sqex-chat`, and `sqexd` (tests only).

## Pinned to a tag

The committed manifest pins them, exactly as `sqex` itself pins `squic` and
`sqnr`:

```toml
sqex-voice = { git = "https://github.com/wave-cl/sqex", tag = "v0.46.0" }
```

This is what makes a release possible at all. A tag whose manifest says
`path = "../sqex-sigil"` can be built only on a machine that happens to have a
directory of that name beside it, which is to say: not from a clone, not on a
runner, and not by anybody else. So the rule is simple —

> **A tag must never carry a path dependency.**

and it is enforced rather than remembered: `.github/workflows/release.yml`
greps the manifest before it builds anything, and refuses the tag. The failure
arrives in the first ten seconds with the offending lines printed, instead of
as a cargo error about a missing directory several minutes into four runners.

Pinning also took a step out of CI. While the crates were paths, every job
checked `wave-cl/sqex` out beside sigil under the name the manifest expected;
now cargo fetches them like any other dependency. sqex, sqnr and squic are all
public, so none of it needs a secret.

## Co-developing against sqex

A change that spans both repositories cannot go through a tag — the tag does
not exist yet, and cutting one per iteration is not a workflow. So point the
manifest at the worktree for the duration:

```toml
sqex-proto = { path = "../sqex-sigil/crates/sqex-proto" }
sqex-discovery = { path = "../sqex-sigil/crates/sqex-discovery" }
sqex-voice = { path = "../sqex-sigil/crates/sqex-voice" }
sqex-chat = { path = "../sqex-sigil/crates/sqex-chat" }
sqexd = { path = "../sqex-sigil/crates/sqexd" }
```

and put the tag back when the sqex side has landed and been tagged. Two things
follow from working this way, and both are deliberate:

- **A path dependency is honest** about the fact that the two move together.
  While it is there, this checkout compiles whatever the worktree has, and
  nobody else can build it.
- **Forgetting to put the tag back cannot ship.** The release workflow refuses
  it. That is the whole reason the guard is a grep in CI rather than a line in
  this document.

Also remember `cargo fmt --all` while the paths are in place: it walks into
them and reformats the other repository. `.github/workflows/ci.yml` says what
that cost once.

## The worktree, and why

While the paths are in place, they compile whatever branch `../sqex` happens to
have checked out. That is fine until something else is working in that tree —
another session, or you in a second terminal — at which point there are two
problems:

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

The name `../sqex-sigil` is the contract: it is what the paths above say, and
what the script creates. A fresh clone needs none of this — it builds from the
tag — so this is one command before *co-development*, not before a build.

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
