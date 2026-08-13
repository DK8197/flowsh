# Gotchas

Every one of these was found by actually running the compiled binary
against a live bash session, not by code review. Several of the
underlying bugs *looked correct* on read-through — that's exactly why
they're documented here instead of just fixed silently. If you're
touching `pty/`, `executor/`, `shortcuts/keyboard.rs`, `editor/blocks.rs`,
or `App::flatten_block`/block execution in `app/app.rs`, read this
first.

## 1. Control bytes in a marker string get interpreted as keybindings, not text

**What happened:** the very first version of the completion-marker
protocol used `\x01` (Ctrl-A / SOH) as a marker prefix. Every command
sent through it came out scrambled.

**Why:** bash reads its input through readline (unless launched with
`--noediting` — see below). Readline doesn't see our writes to the PTY
master as "data," it sees them as **keystrokes**, byte by byte, exactly
as if a human typed them. `\x01` is readline's default binding for
"move to beginning of line." Every subsequent character we sent got
inserted at the wrong position, silently corrupting the command.

**The fix, and the rule going forward:** the marker
(`pty::manager::MARKER_PREFIX`) is purely printable ASCII
(`___SHDEV_DONE___`). **Never put a control byte in anything written to
bash's stdin as "data"** — if it needs to be unambiguous and safe, it
needs to be printable. (Separately, `--noediting` is also passed to bash
at spawn time specifically to reduce this whole class of risk — see
gotcha 4 for why that alone wasn't sufficient.)

## 2. Substring marker-matching gets a false positive from the terminal's own echo

**What happened:** the very first real command after startup showed the
literal wrapped bash command (braces, `2>` redirect, the `printf` marker
syntax, all of it) as if it were program output.

**Why:** the startup setup command (`stty -echo; ...`) was itself sent
*before* echo was disabled — meaning the terminal echoed back what we'd
just typed, including the setup command's own `printf` argument, which
*contains the marker text as a substring* (since the marker is embedded
in the format string we're sending). The original detection logic was
`pending.contains(&marker)` — a substring search anywhere in the
accumulated buffer — so it matched the **echo of our own input**, not
bash's actual output, and returned "done" long before `stty -echo` had
actually run.

**The fix, and the rule going forward:** marker detection must check that
the marker is a **line prefix** (`line.starts_with(&marker)`), not a
substring anywhere in accumulated text. `PtyManager::blocking_setup` and
`executor::run_one` both do this correctly now — if you add a third place
that waits for a marker, copy this pattern, not a substring search.

## 3. Interactive bash's SIGINT handling abandons the rest of the command line, not just the interrupted job

**What happened:** after implementing Ctrl+C cancellation (write `0x03`
to the PTY master, exactly what a real terminal does), cancelling a
running `sleep`-based loop left the app permanently stuck thinking a
command was still running.

**Why:** the (very reasonable-sounding, and wrong) assumption was that
SIGINT kills the current foreground job and bash then continues to the
next `;`-separated command on the same line — so our trailing
`printf '...marker...'` (part of the same wrapped line as the interrupted
command) would still run and we'd still see completion. **That's not
what interactive bash actually does.** On SIGINT, interactive bash
unwinds all the way back to its prompt-read loop, abandoning the rest of
the current command list entirely — the trailing `printf` never runs.
Waiting on the original marker after an interrupt hangs forever.

**The fix, and the rule going forward:**
`executor::resynchronize_after_interrupt` sends the raw interrupt byte,
then immediately queues a **brand-new, independently-marked** no-op probe
line (`printf '\n<fresh-marker>%d\n' "$?"`) and repoints the "what am I
waiting for" state at that new marker instead. This works regardless of
timing because the probe line sits safely in the kernel's tty input queue
until bash actually gets back to reading input. **If you ever add a
second interrupt-and-wait path (e.g., a different cancellation trigger),
it needs to go through this same resync, not a bare `send_interrupt()`
followed by waiting on the original marker.**

## 4. `Ctrl+<letter>` bindings can collide with a legacy control byte for an unrelated physical key

Two separate real instances of this:

- **Ctrl+Enter vs. Ctrl+J:** most terminals have no way to encode
  Ctrl+Enter distinctly from plain Enter — there's no dedicated escape
  sequence for it outside the Kitty keyboard protocol. Convention on many
  terminals (confirmed: Windows Terminal, the default WSL console) is to
  send the same byte as Ctrl+J (`0x0A`) for Ctrl+Enter. shdev's keyboard
  handler originally only bound `Ctrl+Enter` explicitly; `Ctrl+J` fell
  through to the generic "insert this character" branch, so pressing what
  the user experienced as Ctrl+Enter silently typed a literal `j`.
- **Ctrl+H vs. Backspace:** while adding the history browser, `Ctrl+H` was
  the first, obvious choice. Before shipping it, a check of crossterm's
  source (`crossterm-0.27.0/src/event/sys/unix/parse.rs`) confirmed: bytes
  `0x01`–`0x1A` decode to `Char(<letter>) + CONTROL` in the legacy
  (non-Kitty-protocol) raw parser, and `0x08` (which some terminals send
  for the physical Backspace key) falls in that range, decoding to
  `Ctrl+H`. Binding history there would have silently hijacked Backspace
  on those terminals. History uses `Ctrl+R` / `F6` instead.

**The rule going forward:** before binding any new `Ctrl+<letter>`
shortcut, check whether that byte is what some terminal sends for an
unrelated key. The two known landmines are `0x0A` (Enter/Ctrl+J) and
`0x08` (Backspace/Ctrl+H) — avoid rebinding either. When in doubt, grep
`crossterm-<version>/src/event/sys/unix/parse.rs` for the byte's mapping,
or prefer a function key (`F5`, `F6`, ...) or a rare `Ctrl+<letter>`
combo (`Ctrl+E`, `Ctrl+R` used so far) that isn't a conventional
"physical key equivalent" anywhere. Every existing binding that has this
kind of risk (`Ctrl+Enter`/`Ctrl+J`) ships with a redundant, unambiguous
fallback (`F5`) specifically because of gotcha instances 1 and 2 in this
list — new risky bindings should probably get the same treatment.

## 5. Multi-line command text sent as raw newlines hits bash's *interactive* line-continuation handling

**What happened:** the first implementation of compound-block execution
(running a `for`/`while`/`if` block as one unit) joined the block's
lines with literal `\n` and sent that as one write. A three-iteration
`for` loop silently only ran its first iteration.

**Why:** a write containing embedded `\n` bytes doesn't arrive at bash as
one command — the kernel's line-buffered (canonical-mode) tty delivers
it as several distinct physical input lines, exactly as if a human had
typed each one and pressed Enter. Bash's *interactive* multi-line
continuation machinery (PS2 prompts, etc.) then applies — and shdev only
ever blanks `PS1` at startup, never `PS2`. The exact failure mode wasn't
"stray prompt text corrupts output" (as you might expect from gotcha #2)
so much as the block simply not completing as intended.

**The fix, and the rule going forward:** flatten a block's lines into a
**single physical line** before sending it (`App::flatten_block`),
joining with `;`. Newline and `;` are equivalent statement separators in
bash grammar in most positions, so this sidesteps interactive
line-continuation handling entirely. **Never send a write containing an
embedded `\n` as "one command" and assume bash treats it atomically** —
if you need multi-statement execution, flatten to one line first.

## 6. `do`/`then`/`else`/`elif` already introduce the next command — don't add a separator after them

**What happened:** immediately after fixing gotcha #5, the naive fix
(join every line with `; `, unconditionally) produced
`for i in 1 2 3; do; echo hi; done` — a syntax error
(`` bash: syntax error near unexpected token `;' ``).

**Why:** `do` (and `then`/`else`/`elif`) already introduce the next
command directly — `do echo hi` is correct, `do; echo hi` puts an empty
statement between `do` and the real command, which bash rejects.

**The fix, and the rule going forward:** `App::flatten_block` joins with
`;` **except** immediately after a line whose last token is `do`,
`then`, `else`, or `elif`, where it joins with a plain space instead.
**If you ever change how blocks get flattened, this specific
keyword-adjacent case needs its own test** — it's exactly the kind of
thing that looks fine for the "happy path" single-statement-body case
(`for i in 1; do echo hi; done` has no visible problem since there's
only one join point to get right) and only breaks on multi-statement
bodies once you actually run one.

## 7. `$?` doesn't survive across separate invocations unless the wrapper explicitly re-exits with it

**What happened:** found by the expanded shell-behavior test suite
(`tests/shell_behavior_test.py`), specifically the case it exists to
catch: run `false` on one line, then `echo $?` on a *separate* line —
real bash shows `1`; shdev showed `0`.

**Why:** the completion-marker wrapper for every command is
`{ <cmd> ; } 2>'<stderr_path>'; printf '\n<marker>%d\n' "$?"`. The `"$?"`
*inside* that line correctly captures the user's command's exit status
(that's how shdev reports the right exit code in its own UI — already
correct). But after the whole wrapped line finishes, bash's own `$?` —
the one a *later, separately-run* line would see — reflects the exit
status of the **last command in that line**, which is the `printf` call,
not the user's original command. `printf` almost always succeeds, so
every command looked like it left `$?` at `0` for the next line,
regardless of its real exit status.

**The fix, and the rule going forward:** capture the exit code into a
variable and explicitly re-exit with it as the wrapped line's own final
action: `__shdev_ec=$?; printf '...' "$__shdev_ec"; ( exit $__shdev_ec )`.
**Any wrapper that appends commands after the user's command on the same
input line changes what `$?` means for whatever runs next, unless its
very last action re-establishes the original exit code.** This applies
to `executor::run_one`'s main wrapper and
`executor::resynchronize_after_interrupt`'s probe alike — both were
fixed together. If you add a third place that wraps a command with
trailing bookkeeping, it needs the same treatment, and it's exactly the
kind of thing that will look completely correct (the wrapped command's
*own* reported exit code will be right) while silently breaking the
*next* command's view of `$?` — worth specifically testing, not just
inspecting.

## 8. `HISTCONTROL=ignorespace` can't hide the line that sets it

**What happened:** while fixing history pollution (every wrapped command
showing up verbatim in bash's own `history`), the natural instinct was
to also prefix the *startup* `HISTCONTROL=ignorespace; ...` line with a
leading space, expecting it to hide itself too, symmetrically with every
line after it.

**Why it doesn't:** bash decides whether to record a line into history
using `HISTCONTROL`'s value from **before** that line executes, not
after — confirmed by testing (built, ran, checked `history` with an
isolated `HISTFILE`), not assumed from documentation. A line can set
`HISTCONTROL=ignorespace` for every *subsequent* line, but it can't make
itself retroactively exempt.

**The fix, and the rule going forward:** accept it — this is exactly one
line, once per session, not the repeated per-command pollution the fix
was for. **Don't spend more effort trying to also hide the setup line
itself** (e.g. a follow-up `history -d` call) unless it becomes an
actual reported problem; the cost/benefit doesn't currently justify the
added fragility (`history -d` needs the right index, which depends on
whatever history the user's `HISTFILE` already had loaded before shdev
even started).

## 9. A spawned child process inherits the parent's environment by default — including things you didn't think to isolate

**What happened:** after shipping the `HISTCONTROL=ignorespace` fix for
history pollution (gotcha #8's sibling problem), a user still reported
messy history — and their raw `history` dump showed *their own typed
commands* (`cd /mnt/c/Users/...`, `flowsh test3.sh`) interleaved with
shdev's wrapped commands, in the same listing.

**Why:** `pty::bash::BashProcess::spawn` never explicitly set
`HISTFILE`. `portable-pty`'s `CommandBuilder` inherits the parent
process's environment by default (like `fork`/`exec` normally do) unless
told otherwise — so the internal bash session's `HISTFILE` was whatever
the *user's own outer terminal* happened to have set, typically their
real `~/.bash_history`. The internal session and the user's actual
terminal were reading from and writing to the **same file**, in both
directions. `ignorespace` only stops entries from being added to *this
session's in-memory history list* — it does nothing about the file being
shared in the first place, so it looked like a fix but only addressed
half the problem.

**The fix, and the rule going forward:** `HISTFILE=/dev/null` on the
spawned bash process, isolating it completely (reads return EOF
immediately; writes are discarded) — not just suppressing symptoms
within one session's memory. **When spawning a persistent, long-lived
child process meant to be its own sandboxed environment, don't assume
"we didn't set X" means "X is unset" — it means "X is inherited from
whatever process happened to launch us," which for a genuinely isolated
session is almost never what you want.** Explicitly set every
environment variable whose *source* matters (not just its absence), and
specifically test for it with a *populated*, realistic value in the
parent's environment — an empty/default test environment won't catch
this class of bug, which is exactly why the first fix's tests
(`test_history_stays_clean`, using an isolated but *empty* `HISTFILE`)
passed while the real bug was still present. `test_history_fully_isolated_from_outer_terminal`
was added specifically to close that gap: it pre-populates a fake
"outer terminal" `HISTFILE` with realistic content and checks isolation
in both directions.

## 10. Raw terminal byte-dumps are an unreliable test oracle — strip ANSI, and still expect noise

Not a shdev bug, but a real trap in *verifying* shdev: ratatui's
diff-based rendering, combined with rapid successive redraws (e.g. while
live output is streaming), can fragment text across `read()` call
boundaries in ways that break naive substring checks even after stripping
recognized ANSI escape sequences. Several apparent test failures during
development turned out to be this, not real regressions — confirmed by
re-capturing with a longer settle window and/or dumping the full
ANSI-stripped output for visual inspection instead of trusting a single
`substring in text` check. See `testing.md` for the pattern that
minimizes this (decode once at the end, use generous settle/drain
windows, prefer *indirect* verification — e.g. "run whichever line the
cursor lands on" — over parsing exact rendered text where possible).
