# Tech

## Toolchain

Standard Rust, edition 2021. Developed and verified against Rust 1.75+ (both
`apt`-installed 1.75.0 in a sandboxed environment, and expected to work
cleanly against a current `rustup`-installed stable toolchain).

```bash
cargo build --release
cargo test
```

No CI. No linter config beyond `cargo build`'s own warnings, which are
treated as build failures in practice — see the "zero warnings" rule in
`CLAUDE.md`.

## Dependencies (`Cargo.toml`)

| Crate | Why |
|---|---|
| `ratatui` 0.26 | TUI rendering |
| `crossterm` 0.27 | terminal backend, raw-mode input, Kitty keyboard protocol support |
| `portable-pty` 0.8 | spawns the persistent bash process inside a real PTY |
| `anyhow` 1.0 | error handling throughout |
| `unicode-width` 0.1 | UTF-8-safe cursor/column math in the editor buffer |
| `crossbeam-channel` 0.5 | the executor thread's control/event channels, and `Select` for the cancellable run loop — this is why cancellation-while-streaming is possible at all |
| `serde` 1.0 (+derive), `toml` 0.7 | config file (`config.rs`) — pinned to `0.7`, not the newer `0.8`, for the sandboxed-toolchain reason below |
| `dirs` 5.0 | platform-appropriate config directory (`~/.config` on Linux, etc.) |

If you're about to add a dependency: check whether `crossbeam-channel`
already gives you what you need (it's doing a lot of work in this
codebase — MPSC channels *and* the `Select` primitive that makes Ctrl+C
interrupt a running command without a busy-poll loop).

## A toolchain constraint you may hit in a sandboxed/offline environment

If you're working in an environment without `rustup` access (only `apt`'s
older Rust, e.g. `1.75.0`), several crates' current transitive dependency
chains pull in `hashbrown` versions that require `edition2024`, which
isn't stable on older `rustc`. This blocked `serde`/`toml` entirely for a
while — **it's since been worked around, not avoided**: `toml = "0.7"`
(not the newer `0.8`, whose `toml_edit` pulls a newer `indexmap` ->
`hashbrown` chain), plus pinning three transitive deps down:

```bash
cargo update -p lru --precise 0.12.3
cargo update -p indexmap --precise 2.2.6
cargo update -p unicode-segmentation --precise 1.11.0
```

`lru` (a `ratatui` dependency) is what actually breaks the chain — its
newer versions pull `hashbrown 0.15` (edition2024-clean on its own), but
`toml_edit`'s `indexmap` requirement independently pulls `hashbrown
0.17` (edition2024-*dirty*) until `indexmap` itself is also pinned down.
Both were needed together; pinning only one still failed. If you hit a
similar wall with a *different* crate, the general pattern is the same:
find what's pulling the offending `hashbrown` version
(`cargo tree -i hashbrown@<version>`, or if that itself fails to
resolve, grep `Cargo.lock` for `hashbrown` and trace backwards) and pin
the nearest transitive dependency that has a choice about which
`hashbrown` it needs — not `hashbrown` itself directly, which is usually
not selectable in isolation once multiple direct deps disagree. On a
real `rustup`-installed current stable toolchain none of this is
necessary; feel free to unpin everything back to natural `cargo update`
defaults there.

## Why bash specifically, and how it's driven

The persistent shell is a real `bash --noprofile --norc --noediting`
process (see `pty/bash.rs`), not a simulated/parsed shell. `--noediting`
disables bash's own readline layer — shdev drives it entirely by writing
lines to the PTY master and reading raw output back, so there's no
interactive line-editing UI from bash itself to fight with or account
for. See `gotchas.md` for the control-byte and SIGINT behaviors this
implies.

The spawned bash process inherits shdev's own environment by default
(standard `fork`/`exec` behavior, and `portable-pty`'s `CommandBuilder`
doesn't change that) — this bit us once already: `HISTFILE` was
unset, so the "isolated" session silently shared the *user's own*
`~/.bash_history` with their real terminal (gotcha #9). If you're adding
anything that should behave as a genuinely sandboxed session property
rather than "whatever the launching terminal happened to have", it needs
an explicit `cmd.env(...)` in `BashProcess::spawn` — don't assume
leaving something unset means it's actually unset from bash's point of
view.

## Platform testing note

Development and verification happened against a native Linux PTY
(`portable-pty`'s Unix backend). It has NOT been verified against
Windows Terminal / ConPTY-over-WSL specifically — a user reported a
scrolling issue that couldn't be reproduced with the Linux-native pty
test harness in `testing.md`, and remains an open question of whether
it's environment-specific PTY/ANSI translation behavior. If you're
debugging something that only reproduces on Windows Terminal/WSL and not
in the standard test harness, that's a real, distinct risk surface worth
suspecting early rather than re-auditing the scroll math again.
