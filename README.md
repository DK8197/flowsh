# shdev v0.1.1

A terminal-based editor where every line of a shell script is independently
runnable against a **persistent** bash session — edit a line, hit
Ctrl+Enter/Ctrl+J/F5, watch stdout/stderr stream in live while it runs, and
any environment changes (`cd`, `export`, etc.) carry over to the next line
you run, exactly like a real interactive shell. Ctrl+C interrupts a running
command without touching the shell session itself, and the editor scrolls
properly on scripts taller than your terminal.

## Build & run

Requires a Rust toolchain (1.75+; tested against stable). No special
setup beyond `cargo`:

```bash
cargo build --release
./target/release/shdev path/to/script.sh   # opens (or creates) a file
./target/release/shdev                     # opens an empty, unnamed buffer
```

Run the unit tests (pure editor/block-detection logic):

```bash
cargo test
```

Run the shell-behavior test suite (pipes, redirects, quoting, `$?`
propagation, control structures, streaming — drives the real compiled
binary through a real PTY, since none of this is mockable):

```bash
cargo build --release
python3 tests/shell_behavior_test.py
```

## Keybindings

| Key                     | Action                                              |
|--------------------------|------------------------------------------------------|
| Ctrl+Enter / Ctrl+J / F5 | Run the current line, asynchronously                |
| Ctrl+E                  | Run every non-blank line *above* the cursor, in order |
| Ctrl+C                  | Interrupt the running command, or stop a Ctrl+E batch (bash session stays alive) |
| Ctrl+R / F6              | Toggle the execution history browser                 |
| Ctrl+S                  | Save to the opened file                              |
| Ctrl+Q                  | Quit (shuts down the bash session)                   |
| Ctrl+O                  | Toggle focus between editor and output pane          |
| Arrows / Home / End      | Move the cursor (editor auto-scrolls to follow it) — while history is open, Up/Down browse the list instead |
| Backspace / Delete       | Edit                                                 |
| Enter                    | Insert a newline (splits the line) — auto-inserts the matching `done`/`fi` if this line opens a `for`/`while`/`until`/`if` block |
| Ctrl+Z / Ctrl+Y          | Undo / redo                                          |

**Note on Ctrl+Enter / Ctrl+J:** most terminals can't send Ctrl+Enter as a
distinct signal — there's no universal escape sequence for it. shdev opts
into the Kitty keyboard protocol where supported (kitty, WezTerm, foot,
iTerm2), and additionally binds **Ctrl+J**, since that's the byte
(`0x0A`) terminals conventionally substitute for Ctrl+Enter — Windows
Terminal and the default WSL console both do this. **F5** always works
too, everywhere, regardless of terminal or protocol support.

**Note on Ctrl+R (history), not Ctrl+H:** some terminals send byte `0x08`
for the physical Backspace key, which crossterm's raw-mode parser decodes
as **Ctrl+H** — binding history there would have silently hijacked
Backspace on those terminals, the exact same class of bug as the
Ctrl+Enter/Ctrl+J collision above. History uses Ctrl+R (echoing bash's
own "reverse history search") and F6 instead, both unambiguous.

**Note on Ctrl+Z / Ctrl+Y, not Ctrl+Shift+Z:** most terminals can't
distinguish Ctrl+Shift+Z from plain Ctrl+Z either — Shift doesn't change
the byte a non-Kitty-protocol terminal sends for Ctrl+`<letter>`. Redo
uses the unambiguous Ctrl+Y instead (both are also unaffected by raw
mode's ISIG being off, so Ctrl+Z doesn't suspend the process the way it
would at a normal shell prompt — it's just a keystroke here).

**Note on Ctrl+C:** this mirrors real shell behavior — it interrupts
whatever's currently running (via `SIGINT`, exactly like a human pressing
Ctrl+C at a normal prompt), not the whole app. The persistent bash
session survives; run something else right after and it'll pick up right
where the interrupted command's environment left off.

## Architecture

```
src/
├── main.rs        entry point: terminal setup/teardown, event loop, polls executor events
├── app/           AppState (single source of truth), App controller, AppEvent bus
├── editor/        Buffer, Cursor, Editor — editing only, never executes
├── executor/       ExecutionEngine — a background thread that owns the bash session and
│                  streams progress back over a channel; never blocks the UI thread
├── pty/           BashProcess (spawn) + PtyManager (thin primitives: send_line,
│                  send_interrupt, event_receiver — the executor composes these into
│                  async, streaming, cancellable execution)
├── ui/            layout, renderer (editor viewport/scrolling), output panel (live +
│                  finished views) — reads AppState, draws
├── storage/       FileManager — open/save/create/reload, nothing else
├── shortcuts/     keyboard.rs — raw key events -> high-level Intents
└── models/        Line, Output — plain data
```

### Async execution

`ExecutionEngine::new()` spawns one long-lived background thread that owns
the `PtyManager` for the app's entire lifetime. The UI thread never
blocks on it:

- `run_line(id, command)` is a non-blocking send into a `crossbeam_channel`
  — it returns immediately.
- The executor thread wraps the command with a unique completion marker
  (same technique as before: `{ <cmd> ; } 2>'<stderr_path>'; printf
  '\n<marker>%d\n' "$?"`), writes it to bash, then uses
  `crossbeam_channel::Select` to wait on *two* channels at once: the PTY's
  raw output, and a control channel carrying `Run` / `Cancel` / `Shutdown`
  requests from the UI thread. This is what makes cancellation possible
  mid-command instead of only between commands.
- As each complete line of output arrives, it's forwarded immediately as
  an `ExecEvent::OutputChunk` — this is the live streaming.
- `App::poll_events()`, called once per frame from the main loop
  (`main.rs`), drains whatever's arrived and applies it to `AppState`
  (appending to `live_output`, finalizing a line's status, etc.), then
  re-emits it as an `AppEvent` onto the app's event bus. The UI never
  waits — it just reflects however much progress exists at redraw time.

### Cancellation (`Ctrl+C`)

Ctrl+C writes the raw byte `0x03` directly into the PTY master — exactly
what a real terminal does. Since local echo (not `ISIG`) is the only tty
flag shdev disables at startup, the kernel's tty driver still turns that
byte into `SIGINT` for whatever currently owns the foreground of the
PTY, precisely like pressing Ctrl+C in a normal terminal.

The subtle part: **interactive bash, on receiving SIGINT, doesn't just
kill the interrupted foreground job and continue to the next
semicolon-separated command on the same line — it unwinds all the way
back to its prompt-read loop, abandoning the rest of the current command
list.** That means shdev's own trailing `printf '...marker...'` (part of
the same wrapped line as the command that got interrupted) never runs,
and waiting on it would hang forever. The fix
(`executor::resynchronize_after_interrupt`) sends a brand-new,
independently-marked no-op probe line right after the interrupt byte,
and watches for *that* marker instead — it sits safely in the kernel's
tty input queue until bash actually gets back to reading input, so this
works regardless of timing. This was found and fixed via direct testing
with a `sleep`-based long-running script, not by inspection — it's the
kind of bug that's invisible until you actually try to cancel something.

### Timeout as a safety net

Every command still has a ceiling (`executor::MAX_RUNTIME`, 15 minutes),
which auto-triggers the same interrupt-and-resync path Ctrl+C uses. This
exists purely as a backstop against a forgotten `sleep 999999`, not as
the primary mechanism — Ctrl+C is.

### Run everything above the cursor (`Ctrl+E`)

Queues every non-blank line above the cursor (not including it) and runs
them one after another, in order — equivalent to pressing Ctrl+Enter on
each of those lines by hand, top to bottom, through the same persistent
bash session. The executor only ever runs one command at a time, so this
is built on top of the existing async machinery rather than as a
separate code path: `AppState.batch_remaining` holds what's left to run,
and `App::advance_batch()` pops and starts the next step each time the
current one's `ExecEvent::Finished` arrives through the normal event-bus
flow in `poll_events()`. A `for`/`while`/`until`/`if`/`case` block is
queued as a single step (see the next section) rather than being broken
into individually-unrunnable fragments.

Two behaviors worth being explicit about, since they weren't obvious
choices:

- **A command's own non-zero exit code does not stop the batch.** This
  matches what actually happens if you run each line by hand — bash
  itself doesn't stop on a plain command failure unless the script says
  `set -e`, and stopping silently changes that expectation. Each line's
  own status glyph (✓/✗) still reflects whether it succeeded.
- **Ctrl+C during a batch stops the *whole* batch, not just the current
  line.** This is a deliberate difference from a lone Ctrl+C's "stop
  this one thing" semantics — `App::abort_batch()` clears the remaining
  queue on `ExecEvent::Cancelled` (and on `ExecEvent::Failed`, an
  infrastructure error), so pressing cancel means "stop", not "skip to
  the next line". The bash session itself is unaffected either way —
  it's still the same guarantee as a regular Ctrl+C.

The status bar and output panel both show batch progress (`running
3/7`) while one is in flight.

### Compound blocks (`for`/`while`/`until`/`if`/`case`) run as one unit

Running any line that's part of a multi-line `for`/`while`/`until`/`if`/
`case` construct — the opener, an interior body line, or the closer —
now runs the *whole* construct as one unit, instead of just that single
line (which would either do nothing useful or hang, e.g. running just a
`for ... do` line on its own). This is the fix for the "line-based
execution" gap called out as the project's biggest known correctness
limitation.

`editor::blocks` finds the block: a lightweight, keyword-based scanner
(**not a full bash parser** — see its module doc comment for exactly
what it does and doesn't handle) that tracks nested `for`/`while`/
`until`/`if`/`case` openers and their closers (`done`/`fi`/`esac`) via a
stack, honoring arbitrary nesting as long as the script itself nests
them correctly (a requirement of valid bash anyway).

Two things worth knowing if you're touching this code:

- **The block's lines are flattened into one physical line for
  execution**, joined with `;` (or a plain space right after `do`/
  `then`/`else`/`elif`, which already introduce the next command
  directly — `do; echo hi` is a syntax error). This was **not** the
  first approach tried: sending the block as raw multi-line text (joined
  with `\n`) relies on bash's *interactive* multi-line continuation
  handling (PS2 prompts, etc.), which shdev never accounts for (only
  `PS1` is blanked at startup) — found via actually testing a `for` loop
  this way, which silently only ran its first iteration. See
  `App::flatten_block`'s doc comment for the full story and the known
  edge cases (a `#` comment mid-block, `case` bodies) this simpler
  approach doesn't handle perfectly.
- **All lines in the block get marked Running together and finalized
  with the same result together** (`AppState.running_extra_line_ids`
  tracks the "other" lines beyond the representative one used for the
  executor's event system) — so the status glyph on every line in the
  block updates in sync, not just the opener's.

### `$?` propagates correctly across separate runs

A subtlety in the completion-marker wrapper: every command runs as
`{ <cmd> ; } 2>'<path>'; printf '...'  "$?"`, and until recently, bash's
own `$?` *after that whole line* reflected the `printf` call's exit
status (almost always `0`) — not the user's command. So a later,
*separately run* `echo $?` always showed `0`, regardless of whether the
previous command actually succeeded. Found by
`tests/shell_behavior_test.py`, fixed by capturing the exit code and
explicitly re-establishing it as the wrapped line's last action:
`__shdev_ec=$?; printf '...' "$__shdev_ec"; ( exit $__shdev_ec )`. See
`.claude/steering/gotchas.md` #7 for the full story — it's exactly the
kind of bug that looks completely correct on inspection (the command's
*own* reported exit code in shdev's UI was always right) while silently
breaking `$?` for whatever runs next.

### Keeping bash's own `history` clean

Every command shdev runs goes through the marker-wrapping protocol
above — without any mitigation, every single execution showed up
verbatim in bash's own `history` (stderr-redirect path, `printf` marker
syntax, and all).

The deeper root cause wasn't just wrapper noise: `HISTFILE` was never
explicitly set for the internal bash session, so it inherited whatever
`HISTFILE` the *launching, outer terminal* happened to have — typically
the user's own real `~/.bash_history`. shdev's internal session and the
user's actual terminal ended up reading from and writing to the exact
same file, interleaving shdev's wrapped commands with the user's own,
in both directions. An `HISTCONTROL=ignorespace` fix alone (hiding new
entries within the session) doesn't solve that — it does nothing about
the file already being shared.

The real fix is `HISTFILE=/dev/null` for the internal bash process
(`pty::bash::BashProcess::spawn`): reading `/dev/null` returns EOF
immediately (empty history at startup, so it never loads the user's
real history), and writes to it are discarded (nothing persists, so it
never pollutes the user's real history file either). This fully
isolates the two in both directions, rather than just suppressing
symptoms within one session. `HISTCONTROL=ignorespace` plus a leading
space on every internal command (`executor.rs`) is kept as a second,
complementary layer — it's what keeps the *live* `history` command
usable if you run it from inside a shdev session, since `/dev/null`
alone wouldn't stop entries from existing in the current session's
in-memory list, just from persisting to disk. One line — the startup
setup command itself — still shows up once per session in that
in-memory list, since bash decides whether to record a line using
`HISTCONTROL`'s value from *before* that line runs, not after (confirmed
by testing, not assumed); a command can't hide itself this way. That's
an accepted, minor, one-time exception, not the repeated per-command
pollution — or the cross-session leakage — this was meant to fix.

### Undo / redo (`Ctrl+Z` / `Ctrl+Y`)

Every edit is recorded as a small range patch — "replace `N` lines
starting at row `R` with these `M` new lines" — not a full-buffer
snapshot. This one representation covers a character insert/delete (1
line → 1 line), a newline split (1 → 2), a backspace-triggered line
merge (2 → 1), and the auto-close feature's block insertion (1 → 3)
uniformly. A patch's cost is proportional to the size of the edit, not
the size of the file. See the module doc comment at the top of
`editor::editor` for the full design.

Consecutive character inserts (typing) and consecutive in-line
backspaces (deleting) are coalesced into a single undo step each, so
undo removes "the word you just typed", not one character at a time.
Cursor movement finalizes whatever's currently coalescing (so moving
away and typing elsewhere starts a fresh undo step) without touching
redo history — only an actual edit invalidates redo, consistent with
how undo/redo works in essentially every other editor.

### Auto-completion of block closers

Pressing Enter at the end of a line that opens a `for`/`while`/`until`/
`if` block and doesn't already have a matching closer later in the file
auto-inserts the closer (`done`/`fi`) on its own indented line, with the
cursor landing on a fresh, indented blank line in between, ready for the
block's body. Built on the same `editor::blocks` detector used for
execution — see `editor::blocks::detect_auto_close`.

### Editor scrolling

The renderer computes which buffer lines are visible each frame based on
the current terminal size (only known at draw time) and adjusts
`AppState.editor_viewport_top` to keep the cursor on screen — this is
the one place the renderer mutates state, and it has to be, since
viewport height isn't knowable anywhere else. A small indicator (`↑`/`↓`/
`↕` plus a `12-20/61`-style range) appears in the editor's title bar
whenever the buffer doesn't fit on one screen.

### Execution history

`AppState.output_history` already recorded every run (it's what powers
the "last result" view) — the history browser just adds a UI for it.
Ctrl+R/F6 toggles a list view (most recent first, ✓/✗ glyph, exit code,
runtime) with the full stdout/stderr of the selected entry shown below
it. Selection is tracked as *distance from the most recent entry* rather
than a raw index, so if you're looking at an older run and something new
executes elsewhere, the entry you're looking at doesn't shift out from
under you. Starting a new run automatically closes history, so live
streaming output is never hidden behind it.

### Threading

- **Main thread**: terminal setup, the crossterm event loop, `poll_events()`,
  drawing. Polls at 50ms while something's running (for smooth live
  output/elapsed-time updates) and 100ms while idle.
- **Executor thread**: owns the `PtyManager` and the bash session for the
  app's entire lifetime; processes one command at a time, using
  `crossbeam_channel::Select` to stay responsive to cancel/shutdown
  requests *while* a command is running.
- **PTY reader thread**: blocks on `read()` from the PTY master and
  forwards raw chunks to the executor thread over a channel.
- **Bash itself**: a single long-lived child process, persistent across
  every execution and every cancellation.

## What's implemented

- [x] Cargo project, module layout, AppState, event loop, terminal UI
- [x] Load/save files, text editing (insert/delete/split/merge, UTF-8 safe), cursor movement
- [x] Persistent bash via PTY, stdout/stderr capture, exit codes
- [x] Status bar, per-line status glyphs (idle / running / ✓ / ✗ / ⊘ cancelled)
- [x] **Editor viewport scrolling** — auto-scrolls to keep the cursor visible, with a position indicator
- [x] **Fully asynchronous execution** — the UI never blocks on a running command
- [x] **Live stdout/stderr streaming** — output appears line-by-line as it's produced, not all at once at the end
- [x] **Cancellation via Ctrl+C** — interrupts only the running command; the persistent bash session survives
- [x] **Timeout handling** — a 15-minute safety-net auto-cancel, using the same interrupt/resync path as manual Ctrl+C
- [x] **`AppEvent` bus wired into the full execution lifecycle** — every status message, execution start/chunk/finish/cancel/fail, file save, and file load routes through it
- [x] **Execution history browser** (Ctrl+R/F6) — list of every past run with exit code and runtime, browsable, with full output detail for the selected entry
- [x] **Run everything above the cursor** (Ctrl+E) — sequential batch execution through the same async/streaming/cancellable machinery, stoppable as a whole via Ctrl+C, block-aware
- [x] Explicit terminal resize handling in the event loop
- [x] **`for`/`while`/`until`/`if`/`case` blocks run as one unit** — a lightweight, keyword-based block detector (not a full parser) plus flattening to a single valid command line
- [x] **Undo/redo** (Ctrl+Z/Ctrl+Y) — range-patch based, with typing/deleting coalesced into single steps
- [x] **Auto-completion of block closers** — Enter after a `for`/`while`/`until`/`if` opener auto-inserts `done`/`fi`
- [x] **`$?` propagates correctly across separate runs** — fixed a real bug where it always reset to 0 regardless of the previous command's actual exit status
- [x] **Bash's own `history` stays clean** — `HISTCONTROL=ignorespace` plus a leading space on every internal command hides shdev's marker-wrapping protocol from it (one setup line per session is an accepted exception)
- [x] **Expanded shell-behavior test suite** (`tests/shell_behavior_test.py`) — pipes, redirects, quoting, `$?` propagation, functions, control structures, streaming, batch execution; 18 cases, all passing, drives the real compiled binary through a real PTY

Not yet built: a *real* bash parser (the current block detection is
keyword-based, not a parser — see Known Limitations), ShellCheck
integration, config file support, and executors for shells other than
bash.

## Known limitations

- **Block detection is keyword-based, not a real parser.** `editor::blocks`
  tracks `for`/`while`/`until`/`if`/`case` openers and closers via a
  stack, which handles arbitrary correct nesting well, but doesn't
  tokenize quoting/comments/heredocs — a `done`/`fi`/`esac` keyword
  appearing inside a string or after a `#` on the same line will confuse
  it. This is a deliberate, bounded scope (see
  `.claude/steering/product.md`); the eventual right fix is a real
  parser-backed `ExecutionUnit` concept, not more special-casing here.
- **Block execution flattens multi-line bodies to one line**, which
  doesn't handle a `#` comment on a body line (it swallows the rest of
  the flattened line) or complex multi-pattern `case` bodies perfectly —
  see `App::flatten_block`'s doc comment for specifics.
- If a running command ignores or catches SIGINT, Ctrl+C won't stop it
  (same as a real terminal — this isn't a shdev-specific gap).
- Horizontal scrolling isn't implemented — very long single lines will
  overflow the pane width rather than wrap or scroll sideways.
- **Reported: editor viewport appears stuck around line 21 in some
  environments.** This was investigated but not reproduced: the
  underlying scroll math was verified correct via two independent
  automated tests (arrow-key navigation through a pre-loaded 60-line
  file, and actually typing 40+ lines via real keystrokes), both
  confirming `editor_viewport_top` advances correctly and the cursor
  lands on the right line after scrolling well past the initial
  viewport. The one related gap found and fixed: `Event::Resize` was
  being silently discarded by the event loop (only `Event::Key` was
  matched), which could leave a stale frame on screen until the next
  input event after a terminal resize — now handled explicitly. If this
  still reproduces, the terminal emulator, its exact size in rows, and
  whether it's a native Linux terminal vs. Windows Terminal/ConPTY over
  WSL would help narrow it down; the discrepancy between clean automated
  tests and a real report suggests something environment-specific
  (rendering/PTY translation) rather than the scroll logic itself.
