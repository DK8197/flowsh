# Security Policy

## Please read this before filing — shdev's threat model is unusual

shdev's entire purpose is to execute shell commands you write, against a
real, persistent bash session, exactly as if you'd typed them at a
terminal yourself. **"shdev can run arbitrary shell commands" is the
intended, documented design, not a vulnerability.** If you open a script
in shdev and it runs the commands in that script, that's the product
working correctly, in the same way a text editor opening a file and a
terminal running whatever you type in it are both working correctly.

Please do **not** file a security report for:

- shdev executing whatever command is on the current line, a Ctrl+E
  batch, or a detected `for`/`while`/`if` block — this is the feature.
- The persistent bash session retaining `cd`/`export`/environment state
  between runs — this is the feature.
- A malicious or careless script doing something destructive when run —
  the same is true of running that script with `bash script.sh`
  directly; shdev doesn't add or remove any capability bash itself
  doesn't already have.

## What *is* in scope

Things that would represent shdev doing something **beyond** what the
person at the keyboard asked for, or beyond what a normal terminal
emulator + bash would do:

- Command injection into the PTY that the user did not type or that
  didn't come from the buffer they're editing — e.g. a bug in the
  marker-wrapping protocol (`executor::run_one`) that could cause
  attacker-controlled *file content* (not the person's own keystrokes)
  to execute unintended commands. Note the existing wrapping already
  takes the whole line literally as shell text by design — the concern
  here is specifically injection *outside* that intended surface, for
  example via a crafted filename or environment variable content
  processed somewhere shdev doesn't already treat as "shell text to
  execute."
- Any way for a `.sh` file to affect shdev's own process, memory safety,
  or the host beyond what opening it as plain text should permit (e.g.
  a path-handling bug in `storage::FileManager` that escapes the
  intended file, a buffer-handling bug that panics or corrupts memory —
  Rust's safety guarantees should make the latter unlikely outside
  `unsafe` code, of which there is currently none in this codebase).
- A way for Ctrl+C / the interrupt/resync mechanism
  (`pty::manager::PtyManager::send_interrupt`,
  `executor::resynchronize_after_interrupt`) to signal or affect a
  process *other than* the one currently in the foreground of shdev's
  own PTY.
- Credential or secret exposure that's specific to shdev's handling
  (e.g. temp files at predictable, world-readable paths — see the
  `/tmp/shdev_stderr_<pid>_<id>.log` pattern in `executor::run_one`; a
  report that these are created with overly permissive permissions or
  are predictable in a way that enables a symlink attack would be in
  scope).
- Terminal escape sequence injection: if command *output* (not the
  editor buffer, which is rendered as plain text by ratatui, not raw
  terminal bytes) could somehow cause shdev's own rendering to execute
  unintended terminal escape sequences beyond what ratatui's own
  rendering pipeline already sanitizes.

## Reporting

Please **do not** open a public GitHub issue for a security report in
scope above. Instead, use GitHub's private vulnerability reporting
(Security tab → "Report a vulnerability") on this repository, or email
the maintainers listed in `MAINTAINERS.md` if that feature isn't
available.

Please include:
- A clear description of the issue and why it's beyond "shdev ran a
  command," per the scope above
- Steps to reproduce, ideally as a minimal script + exact keys pressed
- Which platform/terminal you tested on (see the open note in
  `.claude/steering/tech.md` about Windows Terminal/WSL vs. native Linux
  PTY behavior not being fully cross-verified yet)

We'll acknowledge reports within a few days and aim to have a fix or a
clear response within two weeks for anything confirmed in scope.

## Supported versions

This project is pre-1.0 (currently v0.1.1). Security fixes land on the
latest release; there is no separate LTS/backport policy yet.
