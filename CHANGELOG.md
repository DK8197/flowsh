# Changelog

All notable changes to this project are documented here. Format loosely
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Undo/redo (Ctrl+Z / Ctrl+Y), built on a range-scoped edit-patch model
  rather than full-buffer snapshots. Consecutive character inserts and
  in-line backspaces coalesce into single undo steps.
- Auto-completion of `for`/`while`/`until`/`if` block closers: pressing
  Enter after a line ending in `do`/`then` auto-inserts the matching
  `done`/`fi` and positions the cursor on a fresh, indented body line.
- Multi-line compound statements (`for`/`while`/`until`/`if`/`case`) are
  now detected and executed as a single unit when you run any line that's
  part of one — addressing the previously-documented "line-based
  execution" limitation for the common cases. Ctrl+E's batch mode
  respects block boundaries too, treating a whole block as one queued
  step instead of breaking it into unrunnable fragments.
- A committed, repeatable expanded shell-behavior test suite
  (`tests/shell_behavior_test.py`, 20 cases): pipes, redirects, quoting,
  `$?` propagation, functions, control structures, streaming, batch
  execution, and history cleanliness, driving the real compiled binary
  through a real PTY. Closes the one item from the original roadmap
  that had no persistent test coverage — everything else had only been
  verified with one-off scripts during development.
- `HISTFILE=/dev/null` on the spawned bash process, fully isolating its
  history from the user's own real terminal session. Root cause: the
  internal bash process inherited whatever `HISTFILE` the *launching,
  outer terminal* had set (unset by shdev = inherited, not actually
  unset) — typically the user's real `~/.bash_history` — so the two
  sessions were reading from and writing to the same file, interleaving
  shdev's wrapped commands with the user's own in both directions.
- `HISTCONTROL=ignorespace` plus a leading space on every internal
  command, as a complementary layer on top of the `/dev/null` isolation
  above — keeps the *live* `history` command usable from inside a
  running shdev session, which `/dev/null` alone wouldn't do (it stops
  persistence to disk, not entries existing in the current session's
  in-memory list).

### Fixed
- A `for`-loop block sent for execution as raw multi-line text (joined
  with `\n`) silently only ran its first iteration: bash treats embedded
  newlines as separate physical input lines, subject to its interactive
  multi-line continuation handling (PS2 prompts) that shdev never
  accounts for. Fixed by flattening a block's lines into a single
  physical line, joined with `;`, before sending it.
- That flattening initially produced a syntax error
  (`for i in 1 2 3; do; echo hi; done`) because `do`/`then`/`else`/`elif`
  already introduce the next command directly — inserting `;` right
  after them creates an empty statement. Fixed by joining with a plain
  space in that specific position instead.
- `$?` didn't propagate correctly to a later, separately-run command: it
  always reset to `0` after any run regardless of the actual command's
  exit status, because the completion-marker wrapper's own trailing
  `printf` became the new "last command" bash's `$?` referred to. Found
  by the new shell-behavior test suite. Fixed by capturing the exit code
  and explicitly re-establishing it as the wrapped line's final action.

## [0.1.1]

### Added
- Fully asynchronous command execution — the UI never blocks on a
  running command.
- Live stdout/stderr streaming — output appears as it's produced, not
  all at once when the command finishes.
- Cancellation via Ctrl+C, mirroring real shell semantics: interrupts
  only the running command via `SIGINT`; the persistent bash session
  survives.
- A 15-minute safety-net timeout per command, using the same
  interrupt/resync path as manual cancellation.
- The `AppEvent` bus wired through the full execution lifecycle
  (start/chunk/finish/cancel/fail, file save/load, status messages).
- Editor viewport scrolling — the visible window follows the cursor on
  buffers taller than the terminal, with a position indicator.
- An execution history browser (Ctrl+R / F6): every past run, browsable,
  with full output detail for the selected entry.
- Batch execution of every non-blank line above the cursor (Ctrl+E),
  built on the same async machinery, continuing past a command's own
  non-zero exit but stoppable as a whole via Ctrl+C.
- Explicit terminal resize handling in the event loop.

### Fixed
- A control byte (`\x01`) in the internal completion-marker protocol was
  being interpreted by bash's readline as a keybinding rather than
  literal text, silently corrupting commands. Switched to a
  purely-printable marker.
- A false-positive marker match: the startup `stty -echo` setup command's
  own echoed input (before echo was actually disabled) contained the
  marker text as a substring, causing detection to return before the
  setup command had actually run. Fixed by matching the marker as a line
  prefix instead of a substring anywhere in the buffer.
- Ctrl+C cancellation could hang forever: interactive bash's SIGINT
  handling abandons the rest of the current command list (not just the
  interrupted foreground job), so the trailing completion-marker
  `printf` never ran. Fixed by re-synchronizing with a fresh,
  independently-marked probe after every interrupt.
- Ctrl+Enter silently typed a literal `j` on terminals without Kitty
  keyboard protocol support (Windows Terminal, the default WSL console):
  those terminals send the same byte as Ctrl+J for Ctrl+Enter, which
  wasn't explicitly bound. Ctrl+J is now bound directly, alongside the
  existing F5 fallback.

## [0.1.0]

Initial MVP: persistent bash session via `portable-pty`, line-based
editing and execution, synchronous (blocking) command execution, basic
file open/save, status bar, and per-line status glyphs.
