## What this changes

<!-- A clear description of the change and why. Link the issue it addresses, if any. -->

## How was this tested

<!--
For anything touching src/pty/, src/executor/, or src/shortcuts/keyboard.rs:
"it compiles" / "cargo test passes" is not sufficient on its own — see
CONTRIBUTING.md rule 2 and .claude/steering/testing.md. Describe what you
actually ran the compiled binary through (a real PTY test, manual
testing in a real terminal, etc.) and what you confirmed.

For pure editor/model logic changes (src/editor/buffer.rs, src/models/),
a new or updated #[cfg(test)] unit test is the right bar.
-->

- [ ] `cargo build --release` — zero warnings
- [ ] `cargo test` passes
- [ ] `python3 tests/shell_behavior_test.py` passes (build release first);
      added a new case if this fixes a bug or adds behavior it didn't
      already cover
- [ ] For PTY/execution/keybinding changes not well covered by the
      committed suite: exercised through a real PTY manually (describe
      how below)
- [ ] Updated `README.md` if this changes user-facing behavior
- [ ] Updated the relevant `.claude/steering/*.md` if this changes an
      architectural boundary, or added a new gotcha/testing pattern
      worth documenting

**Testing details:**

<!-- What you actually ran, and what it confirmed. -->

## Checklist

- [ ] I've read `CONTRIBUTING.md`
- [ ] If this adds a `Ctrl+<letter>` keybinding, I checked it doesn't
      collide with a legacy control byte for an unrelated key (see
      `.claude/steering/gotchas.md` #4)
- [ ] If this touches undo/redo, it uses the existing range-patch model
      rather than full-buffer snapshots
