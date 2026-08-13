# Product

## What shdev is for

A terminal editor for developing and iterating on a shell script — the
kind of workflow where you write a `deploy.sh` or a data-pipeline script
incrementally, running each new line as you write it, checking output,
fixing it, running the next line, without losing the shell state (`cd`,
exported vars, background processes) between runs. The persistent bash
session is the entire point: it's what separates this from "run this
file with bash" or a plain text editor with an integrated terminal.

Target session length: 30–60 minutes of real, uninterrupted use — the
bar for every feature decision is "does this survive actually building
something with it," not "does this look complete in a demo."

## What "done" looks like for a feature

A feature is done when it's been exercised against a real, live bash
session through the compiled binary — not when it compiles, and not when
it looks correct on read-through. See `testing.md`. Several real bugs in
this codebase (documented in `gotchas.md`) were invisible at the code
level and only surfaced when actually run.

## Deliberate scope boundaries — don't "fix" these without discussion

These are known gaps, not oversights. Read the reasoning before treating
any of them as a quick win:

- **A real bash parser.** `for`/`while`/`until`/`if`/`case` blocks now run
  as one unit (`editor::blocks` + `App::flatten_block`), which closed the
  biggest correctness gap — but that detector is keyword-based, not a
  real parser: it doesn't tokenize quoting, comments, or heredocs, so a
  `done`/`fi`/`esac` inside a string or after a `#` will confuse it. The
  eventual right fix, if this keeps causing real trouble, is a genuine
  parser-backed `ExecutionUnit` concept. Don't respond to individual
  edge-case reports with more keyword-matching heuristics — that's a trap
  that gets worse with every case someone reports. See
  `.claude/steering/gotchas.md` for two real bugs already found in the
  block-execution path (multi-line input relying on unset PS2 handling;
  `do`/`then` needing a space, not `;`, before the next command) — both
  were invisible on read-through and only found by actually running a
  `for` loop through it.
- **No ShellCheck / linting.** Deliberately sequenced *after* execution
  correctness was solid, not before — linting a script whose execution
  model doesn't match how it's actually run would be misleading. Now
  that block execution works for the common cases, this is more
  reasonable to pick up, but still explicitly not started.
- **Bash only.** No PowerShell/Python/SQL/remote executors. The
  `ExecutionEngine` was kept decoupled enough that this is a real future
  extension point, not a rewrite, but it's explicitly out of scope until
  the bash parser gap above is resolved rather than papered over.
- **Function definitions aren't recognized as blocks.** `editor::blocks`
  covers `for`/`while`/`until`/`if`/`case` but not `name() { ... }` or
  `function name { ... }` — a real, likely-common gap, just not yet
  built. The `{`/`}` closer-matching would need its own careful handling
  since `{`/`}` are used for other things in bash too (parameter
  expansion, brace expansion) that a naive keyword scanner could
  misinterpret.
- **No config file.** `serde`/`toml` were dropped from `Cargo.toml` only
  because of a sandbox toolchain constraint during initial development
  (see `tech.md`) — trivial to add back, just hasn't been prioritized.

## What "steering" means for this project

If a request would add a feature not in the current `README.md` feature
list, check whether it fits the "30–60 minutes of real shell-script work"
bar before building it. Prefer the feature that makes an *existing*
capability more trustworthy (e.g., fixing a PTY edge case) over a new,
separate capability — this tool's core risk is silent incorrectness in
how it talks to bash, not a shortage of surface features.
