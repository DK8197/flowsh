# Structure

```
src/
├── main.rs        entry point: terminal setup/teardown, event loop, polls executor events
├── app/           AppState (single source of truth), App controller, AppEvent bus
├── editor/        Buffer, Cursor, Editor — editing only, never executes
├── executor/      ExecutionEngine — a background thread that owns the bash session and
│                  streams progress back over a channel; never blocks the UI thread
├── pty/           BashProcess (spawn) + PtyManager (thin primitives: send_line,
│                  send_interrupt, event_receiver — the executor composes these into
│                  async, streaming, cancellable execution)
├── ui/            layout, renderer (editor viewport/scrolling), output panel (live +
│                  finished + history views) — reads AppState, draws
├── storage/       FileManager — open/save/create/reload, nothing else
├── shortcuts/     keyboard.rs — raw key events -> high-level Intents
└── models/        Line, Output — plain data
```

## The rule each module follows

Every module has exactly one job, stated in its own doc comment at the top
of the file — read that comment before adding a function to a module, and
if what you're adding doesn't fit the one-line job description, it
probably belongs in a different module or a new one, not bolted on.

Concretely, as of v0.1.1:

- **`editor/`** edits text. It has no idea execution exists. If you find
  yourself wanting to import `executor` or `pty` types into `editor/`,
  stop — that's `app/`'s job.
- **`pty/`** knows how to talk to a PTY and a bash process. It does not
  know what "a command" is, does not know about markers, exit codes, or
  streaming — that framing is deliberately kept in `executor/`. `pty/`
  exposes primitives (`send_line`, `send_interrupt`, `event_receiver`);
  `executor/` composes them.
- **`executor/`** knows what a command execution is (the marker-wrapping
  protocol, cancellation, timeout) but has zero knowledge of `AppState`,
  rendering, or keybindings. It communicates *out* via `ExecEvent`, not by
  reaching into app state.
- **`app/`** is the only module allowed to know about both `editor/` and
  `executor/` at once. `App::poll_events()` is the sole place `ExecEvent`s
  get translated into `AppEvent`s and applied to `AppState`. If you need
  execution progress to affect something the editor or UI cares about,
  this is where that translation happens — don't have `executor/` reach
  into `AppState` directly.
- **`ui/`** reads `AppState` and draws. The one sanctioned exception:
  `ui::renderer::draw_editor` mutates `AppState.editor_viewport_top`,
  because visible-row count is only knowable at draw time (it depends on
  the real terminal size). Don't add a second such exception without a
  similarly hard constraint forcing it — prefer computing things in `app/`
  and handing `ui/` a value to render.
- **`shortcuts/`** maps raw `crossterm::KeyEvent`s to `Intent` enum
  values. It makes no judgment about whether an intent is currently valid
  (e.g. it doesn't know if a command is already running) — that's
  `App::dispatch`'s job. Keep it that way; it makes the collision-checking
  discipline in `gotchas.md` tractable because this file is the only place
  raw key bytes get interpreted.
- **`storage/`** is intentionally thin (open/save/create/reload) and has
  no opinion about what "the buffer" is beyond plain text in, plain text
  out.
- **`models/`** is plain data (`Line`, `Output`) with no behavior beyond
  simple constructors/predicates. If a model type starts accumulating
  logic that isn't a pure function of its own fields, that logic
  probably belongs in whichever module owns the lifecycle (`editor/` for
  `Line` edits, `executor/` for `Output` construction).

## Where new code goes — quick lookup

| Adding... | Goes in |
|---|---|
| A new keybinding | `shortcuts/keyboard.rs` (the raw-key → `Intent` mapping) + `app/app.rs` (`App::dispatch`, the actual behavior) |
| A new thing that can happen during a command's lifecycle | `executor::executor::ExecEvent` (new variant) + `app/events.rs` (`AppEvent` equivalent) + `App::poll_events` (the translation) |
| A new editor operation (insert/delete/select) | `editor/buffer.rs` (the mutation) + `editor/editor.rs` (the public API) + a unit test in `buffer.rs`'s `#[cfg(test)] mod tests` |
| A new UI panel or view mode | `ui/output.rs` or a new file in `ui/`, gated by a new `AppState` boolean/enum the same way `history_open` and `is_running()` already work |
| A new piece of persistent app state | `app/state.rs` — and check whether it needs to be reset anywhere (`begin_run`/`end_run` in `state.rs` show the pattern for state that has a clear start/end lifecycle) |

## Tests

Unit tests live inline in the relevant file under `#[cfg(test)] mod
tests` — currently only `editor/buffer.rs` and `editor/editor.rs` have
them, because they're the only pure, PTY-free logic in the codebase.
Everything touching `pty/`, `executor/`, or the rendered TUI is verified
via the external harness described in `testing.md`, not `cargo test`.
