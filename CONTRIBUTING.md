# Contributing to shdev

Thanks for considering it. This project has a few conventions that are
stricter than a typical Rust CLI tool, because of what it actually does
(drive a real, persistent shell) — please read this before opening a PR,
it'll save you a review round-trip.

## Before you start

If you're using an AI coding assistant, point it at `CLAUDE.md` and
`.claude/steering/` first — they contain the same information in this
document plus the specific, hard-won bugs in `.claude/steering/gotchas.md`
that are easy to silently reintroduce.

For anything beyond a small fix, please open an issue first describing
what you want to change and why, especially for new features — see
`.claude/steering/product.md` for the project's actual scope boundaries.
Several plausible-sounding features (a bash parser, config files,
ShellCheck integration) are deliberately deferred, not overlooked, and
knowing the reasoning first avoids wasted work on either side.

## Setup

```bash
git clone <this repo>
cd shdev
cargo build --release
cargo test
python3 tests/shell_behavior_test.py
```

Requires Rust 1.75+. No other setup.

## The rules

### 1. Zero warnings

```bash
cargo build --release
```

must produce none. If your change introduces one, fix the actual cause —
an unused field usually means the feature isn't fully wired up yet, not
that it needs `#[allow(dead_code)]`. That attribute is reserved for
genuinely-intentional forward-looking API surface, and even then the
surrounding comment should say why (see existing examples in
`src/app/events.rs`).

### 2. Compiling is not evidence of correctness here

This is the single most important thing to internalize before touching
`src/pty/`, `src/executor/`, or `src/shortcuts/keyboard.rs`. Several real
bugs in this codebase's history looked completely correct on read-through
and compiled cleanly — they were only found by actually running the
binary against a live bash session. Read
`.claude/steering/gotchas.md` for the specific examples (a control byte
silently corrupting commands, a false-positive substring match, bash's
own SIGINT semantics abandoning part of a command line, a keybinding that
would have collided with Backspace on some terminals).

**Any change to PTY handling, execution, or keybindings needs to be
exercised through a real PTY before you open the PR**, not just unit
tested. Run `python3 tests/shell_behavior_test.py` (build with
`cargo build --release` first) — extend it with a new case if you're
fixing a bug or adding behavior it doesn't already cover; that's the
whole point of it existing. `.claude/steering/testing.md` has the
underlying methodology (spawn the compiled binary inside a Python-driven
pty, send real key bytes, read real output) if you need to test
something the committed suite doesn't fit well (e.g. a UI/rendering
change). Describe what you tested in the PR description — see the PR
template.

### 3. New `Ctrl+<letter>` keybindings need a collision check

Before adding one, check whether the byte it maps to collides with what
some terminal sends for an unrelated physical key. The two known
landmines are `0x0A` (Enter, aliased as Ctrl+J on many terminals since
there's no way to encode Ctrl+Enter distinctly without the Kitty keyboard
protocol) and `0x08` (Backspace, on some terminals). See
`.claude/steering/gotchas.md` #4 for the full explanation and how to
check crossterm's source for a given byte. When in doubt, prefer a
function key or an uncommon `Ctrl+<letter>` combination, and consider
giving it a redundant, unambiguous fallback binding the way Ctrl+Enter's
run action also has F5.

### 4. Module boundaries are intentional

`src/editor/` doesn't know execution exists. `src/pty/` doesn't know what
a "command" is (that framing lives in `src/executor/`). `src/executor/`
doesn't know `AppState` exists. See `.claude/steering/structure.md` for
the full breakdown and a quick lookup table for "where does X go". A PR
that reaches across one of these boundaries without a good reason will
get asked to move the code.

### 5. Undo/redo, buffer edits: operation-based, not full-snapshot

If you're touching the editor's undo system, don't add a code path that
snapshots the entire buffer — the existing design uses small,
range-scoped patches (replace N old lines with M new lines at a given
row) specifically so undo scales with the size of an edit, not the size
of the file. See the doc comments in `src/editor/editor.rs` for the
pattern.

## Commit / PR conventions

- Keep commits focused; a PR that mixes an unrelated refactor with a
  feature is harder to review and to `git bisect` later.
- Reference the issue you're addressing, if there is one.
- Fill out the PR template's "how was this tested" section honestly —
  for anything touching PTY/execution/keybindings, "it compiles" is not
  an acceptable answer (see rule 2).
- Update `README.md` if you're adding or changing user-facing behavior
  (a keybinding, a panel, a command's semantics). Update the relevant
  `.claude/steering/*.md` if you're changing an architectural boundary,
  discovering a new gotcha, or adding a new testing pattern worth
  documenting for the next person.

## Reporting bugs

Use the bug report issue template. For anything involving PTY/execution
behavior, please include:
- The exact keys pressed and the script content, if reproducible
- Your terminal emulator and, if relevant, whether you're on native
  Linux, macOS, or Windows Terminal/WSL — see the open note in
  `.claude/steering/tech.md` about a reported scrolling issue that
  couldn't be reproduced against a native Linux PTY and may be
  environment-specific ANSI/PTY translation behavior worth ruling in or
  out early.

## Security

Please see `SECURITY.md` — this tool executes arbitrary shell code by
design, so "is this a vulnerability" has a narrower, specific scope than
you might assume from a typical project.

## License

By contributing, you agree your contributions are licensed under the
same dual MIT/Apache-2.0 terms as the rest of the project (see `LICENSE-MIT`
and `LICENSE-APACHE`).
