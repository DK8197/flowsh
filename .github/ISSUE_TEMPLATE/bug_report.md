---
name: Bug report
about: Something in shdev doesn't work as expected
title: ""
labels: bug
assignees: ""
---

**What happened**

A clear description of the incorrect behavior.

**What you expected**

What should have happened instead.

**Steps to reproduce**

1. Script content (paste the relevant lines, or attach the `.sh` file)
2. Exact keys pressed, in order (e.g. "moved cursor down 3 times, pressed
   Ctrl+E, then Ctrl+C after ~2 seconds")

**Environment**

- shdev version / commit:
- OS:
- Terminal emulator (this matters a lot for this project — see below):
- Terminal size (rows x cols), if relevant to a rendering/scrolling issue:

**Native Linux terminal, or Windows Terminal / WSL?**

There's an open, unresolved question about whether some rendering
behavior differs between a native Linux PTY and Windows Terminal's
ConPTY-over-WSL translation (see `.claude/steering/tech.md`). If you're
on Windows Terminal/WSL and the issue might be rendering-related
(scrolling, cursor position, garbled text), please say so explicitly —
it helps narrow down whether this is a logic bug or a PTY/ANSI
translation difference.

**Additional context**

Anything else — screenshots, a copy of the raw terminal output if you
have one, whether it's reproducible every time or intermittent.
