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

If you're about to add a dependency: check whether `crossbeam-channel`
already gives you what you need (it's doing a lot of work in this
codebase — MPSC channels *and* the `Select` primitive that makes Ctrl+C
interrupt a running command without a busy-poll loop).

## A toolchain constraint you may hit in a sandboxed/offline environment

If you're working in an environment without `rustup` access (only `apt`'s
older Rust), `serde`/`toml`'s current transitive dependency chain
(`hashbrown`, `indexmap`, `unicode-segmentation`) requires `edition2024`,
which isn't stable on older `rustc`. This is why `serde`/`toml` aren't in
`Cargo.toml` — config file support was deferred, not designed around. On a
real `rustup`-installed current stable toolchain this constraint doesn't
exist; feel free to add them back if implementing config support.

If you do hit dependency-resolution failures against an old pinned
toolchain, the fix that worked before: `cargo update -p <crate> --precise
<older-version>` for the specific offending transitive dep, not pinning
your own direct dependencies to old versions.

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
