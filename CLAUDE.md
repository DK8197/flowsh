# CLAUDE.md

Guidance for Claude Code (or any AI coding agent) working in this repository.

## What this is

shdev: a terminal editor where every line of a shell script runs independently
against one **persistent** bash session. Edit a line, run it, see live
stdout/stderr/exit code, and `cd`/`export`/env changes carry forward to the
next line — like a real interactive shell, but editable.

Read `.claude/steering/product.md` before making product decisions (what to
build next, how a feature should behave) — it has the actual scope
boundaries and the "why", not just the "what".

## Before touching code

Read these in order. They contain hard-won, non-obvious facts about this
codebase that are easy to silently break:

1. `.claude/steering/tech.md` — toolchain constraints, dependency pins, why
2. `.claude/steering/structure.md` — module boundaries and what belongs where
3. `.claude/steering/gotchas.md` — **read this one especially carefully.**
   Several real, subtle bugs were found and fixed in this codebase (PTY
   control-byte corruption, a Ctrl+H/Backspace collision, bash's SIGINT
   behavior abandoning the rest of a command line). All were invisible until
   actually exercised against a live bash session. If you're touching `pty/`,
   `executor/`, or `shortcuts/keyboard.rs`, this file is not optional reading.
4. `.claude/steering/testing.md` — there is no headless test harness for the
   TUI; verification happens by driving the compiled binary through a real
   PTY. This file has working, copy-pasteable test patterns.

## Build & verify

```bash
cargo build --release   # must be zero warnings — treat any new warning as a bug to fix, not silence
cargo test               # unit tests: editor/buffer logic only; nothing PTY-related is unit-testable
```

There is no CI in this repo. Every feature in the current codebase was
verified by actually spawning the compiled binary in a pty and driving it
with real key bytes — see `.claude/steering/testing.md`. **Do not consider a
change to `pty/`, `executor/`, or `shortcuts/` done until it's been exercised
this way.** Compiling is not evidence of correctness for this codebase —
several of the bugs in `gotchas.md` compiled cleanly and looked correct on
inspection.

## Ground rules for this codebase

- **Zero warnings, always.** `cargo build --release` currently produces none.
  If a change introduces a warning, fix the actual cause (unused field,
  dead code path) rather than reaching for `#[allow(...)]` — that's only
  appropriate for genuinely-intentional future-facing API surface, and even
  then it should say why in a comment.
- **New Ctrl+<letter> keybindings need a collision check before shipping.**
  crossterm's legacy (non-Kitty-protocol) raw parser maps bytes `0x01`–`0x1A`
  to `Char(<letter>) + CONTROL`. Some of those bytes are *also* what a
  terminal sends for an unrelated physical key — `0x08` (Ctrl+H) is what
  some terminals send for Backspace; `0x0A` (Ctrl+J) is what most terminals
  send for Ctrl+Enter, since there's no way to encode Ctrl+Enter distinctly
  without the Kitty keyboard protocol. Before binding a new Ctrl+<letter>,
  grep crossterm's `src/event/sys/unix/parse.rs` for that byte, or check
  `gotchas.md` for the pattern already established.
- **Never trust a raw terminal byte-dump as a test oracle without stripping
  ANSI escapes, and even then expect occasional false negatives.**
  ratatui's diff-based rendering plus rapid redraws can fragment text across
  read() boundaries in ways that break naive substring checks. When a test
  assertion fails, always dump the raw (ANSI-stripped) output before
  concluding there's a real bug — several apparent regressions during
  development turned out to be test-harness artifacts, not app bugs. See
  `testing.md` for the specific pattern that avoids this.
- **The executor thread owns the bash session for the app's entire
  lifetime.** Don't add a second path that writes to the PTY or spawns a
  second `PtyManager` — there is exactly one bash process per app run, and
  `executor::run_one` / `resynchronize_after_interrupt` are the only places
  that write commands to it.
- **Line-based execution is a known, documented limitation, not an
  oversight.** A shell script is not a collection of independent lines —
  `if`/`for`/`while` spanning multiple lines will not behave correctly run
  one line at a time. Don't paper over this with more line-execution
  polish; see `product.md` for why this is deliberately deferred and what
  the real fix looks like (an `ExecutionUnit` concept backed by a parser).

## Current status

v0.1.1. Async execution with live streaming, Ctrl+C cancellation (session
survives), editor scrolling, an execution history browser (Ctrl+R/F6), and
batch execution of everything above the cursor (Ctrl+E) are implemented and
verified. See `README.md` for the full user-facing feature list and
`product.md` for what's intentionally not built yet.
