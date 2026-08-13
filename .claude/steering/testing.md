# Testing

## `tests/shell_behavior_test.py` — run this before any PTY/executor change

```bash
cargo build --release
python3 tests/shell_behavior_test.py
```

This is the committed, persistent test suite for the shell-behavior
matrix (pipes, redirects, quoting, functions, `$?` propagation, control
structures, long-running/streaming commands, batch execution). It's not
`cargo test` — see below for why — but it's a real, repeatable,
CI-gateable suite (non-zero exit on any failure), not the one-off
throwaway scripts used for most feature development before it existed.
**One of its 18 cases (`test_dollar_question_propagates`) exists
specifically because it caught a real, previously-undetected bug** —
`$?` wasn't surviving to a separately-run next line — see
`gotchas.md` #7. That's the bar for what belongs in this suite: real
shell semantics a plausible script would depend on, not incidental
implementation details.

Add a new case here whenever you fix a bug that a future refactor could
plausibly reintroduce, not just for new features — regression coverage
is the main point of this file existing at all.

## Why `cargo test` doesn't cover most of this codebase

`cargo test` only covers `editor/buffer.rs`, `editor/editor.rs`, and
`editor/blocks.rs` — pure, PTY-free logic. Everything else (the PTY
protocol, async execution, cancellation, rendering, keybindings)
fundamentally requires a real bash process behind a real PTY and a real
terminal size to mean anything. There's no mocking layer for this,
deliberately — a mocked PTY would not have caught any of the bugs in
`gotchas.md`, all of which were properties of *real* bash/terminal
behavior that looked correct in code review.

The methodology below (spawn the compiled binary inside a Python-driven
pty, send real key bytes, read real output) is what `shell_behavior_test.py`
is built on, and is how every feature in this codebase from v0.1.1
onward was actually verified during development. Treat it as required,
not optional, for anything touching `pty/`, `executor/`, or
`shortcuts/keyboard.rs` — either extend the committed suite, or at
minimum drive the binary through this pattern manually before calling a
change done.

## The core pattern

```python
import pty, os, time, select, signal, struct, fcntl, termios, re

pid, fd = pty.fork()
if pid == 0:
    os.execvp('./target/release/shdev', ['./target/release/shdev', '/path/to/script.sh'])
else:
    winsize = struct.pack('HHHH', 30, 100, 0, 0)  # rows, cols
    fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)

    all_data = b''
    def drain(dur):
        global all_data
        deadline = time.time() + dur
        while time.time() < deadline:
            if select.select([fd], [], [], 0.1)[0]:
                all_data += os.read(fd, 40000)

    drain(0.8)  # let the app start and draw its first frame
    os.write(fd, bytes([0x0A]))  # Ctrl+J: run the current line
    drain(1.0)

    # Decode ONCE, at the end, over the FULL accumulated buffer —
    # never decode per-chunk (see the UTF-8-splitting note below).
    clean = re.sub(rb'\x1b\[[0-9;?]*[a-zA-Z]', b'', all_data).decode(errors='replace')
    print('exit 0' in clean)

    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
```

## Key bytes that matter for this codebase

| Key | Byte(s) |
|---|---|
| Ctrl+Enter (fallback) | `\x0A` (same as Ctrl+J — see `gotchas.md` #4) |
| F5 (run) | `\x1b[15~` |
| F6 (history) | `\x1b[17~` |
| Ctrl+C (cancel) | `\x03` |
| Ctrl+R (history) | `\x12` (18) |
| Ctrl+E (run-above) | `\x05` (5) |
| Down arrow | `\x1b[B` |
| Up arrow | `\x1b[A` |

## Rules that avoid wasted debugging time

1. **Decode the full accumulated buffer once at the end, never per-chunk.**
   `os.read()` boundaries don't respect UTF-8 character boundaries or ANSI
   escape sequence boundaries. Decoding each chunk independently (with
   `errors='replace'`) can mangle text near a chunk split even when the
   underlying byte stream is completely correct. This produced at least
   one false "bug" during development that wasn't real.

2. **Strip ANSI escapes before substring-matching, but still expect
   occasional false negatives on exact text.** Use
   `re.sub(rb'\x1b\[[0-9;?]*[a-zA-Z]', b'', data)`. Even after stripping,
   ratatui's diff-based redraws (only changed cells get retransmitted) can
   fragment adjacent words across cursor-repositioning sequences the regex
   doesn't fully normalize away. If a check fails, **dump the full
   stripped text and look at it** before concluding there's a real bug —
   several apparent failures during development were this, confirmed by
   re-running with a longer settle window or just eyeballing the dump.

3. **Prefer indirect verification over parsing exact rendered text where
   possible.** The single most reliable pattern used throughout
   development: navigate the cursor N times, then run whichever line it
   lands on, and check that *that specific command's output* appears. This
   sidesteps rendering fragmentation entirely and tests the thing that
   actually matters (did the cursor/state logic put us in the right
   place), not the rendering pipeline. Example: to verify scrolling
   actually works past line 9 on a 20-row terminal, don't try to parse the
   scroll indicator text — send 12 Down-arrow presses, then Ctrl+J, and
   check the output contains the *13th* line's command text.

4. **Give the app time to settle before capturing a "final" frame,
   especially after rapid key sequences.** Sending many keys with only
   ~20ms between them and then immediately reading can capture a stream of
   overlapping in-progress redraws that's much harder to interpret than
   one settled frame. A `drain()` of 0.3–0.5s after the last input,
   before the final read, avoids this.

5. **For anything involving `sleep` or timing (streaming output,
   cancellation), use a real multi-second loop, not a single fast
   command.** A script like:
   ```bash
   for i in 1 2 3 4 5; do echo "tick $i"; sleep 1; done
   ```
   is what actually exercises streaming (you can observe `tick 1` arriving
   before `tick 2`) and cancellation (you can send Ctrl+C partway through
   and confirm it stops early, rather than racing a command that's already
   finished by the time you send the signal).

6. **After a `pty.fork()` test, always `SIGKILL` the child in a
   `try/except ProcessLookupError` — never leave shdev processes running.**
   Each test spawns a fresh bash session; forgotten children accumulate.

7. **Debugging a genuine ambiguity (state looks wrong, unclear if it's
   real):** add temporary `eprintln!`-style logging that writes to a file
   *outside* the pty (e.g. `/tmp/shdev_debug.log` via
   `std::fs::OpenOptions`), not to stdout/stderr — the app's own stderr is
   connected to the same pty as everything else and will get mixed into
   (and confuse) whatever you're trying to read. This is exactly how
   gotchas #2 and #3 were actually diagnosed: raw-dump inspection alone
   was ambiguous; a clean out-of-band log made the root cause obvious in
   one pass. Always remove the instrumentation before considering the fix
   done — it's for diagnosis, not to ship.

## What to test for any change touching `pty/` or `executor/`

At minimum, re-verify these still hold (all previously confirmed working
as of v0.1.1 — a regression in any of them is a real problem):

- A simple command runs and shows the correct exit code and output.
- Environment persists across two separate runs (`export X=1` then
  `echo $X` in a second run shows `1`).
- A multi-second command streams output incrementally (not all at once at
  completion).
- Ctrl+C mid-command stops it, and the bash session is still usable
  immediately afterward (run something else, confirm it works).
- Ctrl+E's batch mode continues past a failing command but stops
  entirely on Ctrl+C.
