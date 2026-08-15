#!/usr/bin/env python3
"""Expanded shell-behavior tests for shdev.

This is the persistent, repeatable test suite for the shell-behavior
matrix called for in the project roadmap (pipes, redirects, quoting,
functions, failure/exit-code propagation, control structures,
long-running commands) -- the one item in that roadmap that had no
committed test coverage; everything else was verified with one-off
scripts during development and then discarded.

Why this isn't `cargo test`: see `.claude/steering/testing.md`. This
suite follows that same methodology (spawn the compiled release binary
inside a real PTY, send real key bytes, read real output) rather than
mocking bash or the terminal, because several real bugs in this
codebase were properties of actual bash/terminal behavior invisible to
any mock.

Usage:
    cargo build --release
    python3 tests/shell_behavior_test.py

Exits non-zero if any test fails, so it's usable as a CI gate.
"""

import os
import re
import select
import signal
import struct
import sys
import tempfile
import termios
import time
import fcntl
import pty

BINARY = os.path.join(os.path.dirname(__file__), "..", "target", "release", "shdev")

CTRL_J = bytes([0x0A])  # run current line -- see testing.md's key byte table
CTRL_E = bytes([0x05])  # run everything above the cursor
DOWN = b"\x1b[B"
ALT_UP = b"\x1b[1;3A"  # command-history recall: previous command (xterm CSI-modifier form; modifier 3 = Alt)
ALT_DOWN = b"\x1b[1;3B"  # command-history recall: next command / restore
ANSI_RE = re.compile(rb"\x1b\[[0-9;?]*[a-zA-Z]")


class Session:
    """One spawned shdev process driven over a real PTY."""

    def __init__(self, script_text):
        self.script_path = tempfile.NamedTemporaryFile(suffix=".sh", delete=False, mode="w")
        self.script_path.write(script_text)
        self.script_path.close()

        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execvp(BINARY, [BINARY, self.script_path.name])
        else:
            winsize = struct.pack("HHHH", 30, 120, 0, 0)
            fcntl.ioctl(self.fd, termios.TIOCSWINSZ, winsize)
        self.all_data = b""

    def drain(self, dur):
        deadline = time.time() + dur
        while time.time() < deadline:
            if select.select([self.fd], [], [], 0.1)[0]:
                try:
                    self.all_data += os.read(self.fd, 65536)
                except OSError:
                    break

    def send(self, data):
        os.write(self.fd, data)

    def down(self, n=1):
        for _ in range(n):
            self.send(DOWN)
            self.drain(0.3)

    def run_current_line(self, settle=1.8):
        self.send(CTRL_J)
        self.drain(settle)

    def run_all_before(self, settle=2.5):
        self.send(CTRL_E)
        self.drain(settle)

    def text(self):
        """Full ANSI-stripped output captured so far, decoded once over
        the whole buffer (never per-chunk -- see testing.md)."""
        return ANSI_RE.sub(b"", self.all_data).decode(errors="replace")

    def close(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            os.unlink(self.script_path.name)
        except OSError:
            pass


results = []


def check(name, condition, detail=""):
    results.append((name, bool(condition), detail))
    status = "PASS" if condition else "FAIL"
    print(f"[{status}] {name}" + (f" -- {detail}" if detail and not condition else ""))


def run_case(name, script, steps, settle_start=1.5):
    """steps: list of callables taking the Session, run in order after startup."""
    s = Session(script)
    try:
        s.drain(settle_start)
        for step in steps:
            step(s)
        return s.text()
    finally:
        s.close()


# ---------------------------------------------------------------------
# Simple commands
# ---------------------------------------------------------------------

def test_simple():
    text = run_case(
        "simple: echo",
        "echo hello\n",
        [lambda s: s.run_current_line()],
    )
    check("simple: echo hello prints and exits 0", "hello" in text and "exit 0" in text, text[-200:])


def test_pwd():
    text = run_case(
        "simple: pwd",
        "pwd\n",
        [lambda s: s.run_current_line()],
    )
    check("simple: pwd prints a path and exits 0", "/" in text and "exit 0" in text, text[-200:])


# ---------------------------------------------------------------------
# Persistent shell state
# ---------------------------------------------------------------------

def test_cd_persists():
    text = run_case(
        "state: cd persists",
        "cd /tmp\npwd\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check("state: cd /tmp then pwd shows /tmp", "/tmp" in text, text[-300:])


def test_export_persists():
    text = run_case(
        "state: export persists",
        'export SHDEV_TEST_VAR=hello123\necho "$SHDEV_TEST_VAR"\n',
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check("state: exported var visible in a later, separate run", "hello123" in text, text[-300:])


# ---------------------------------------------------------------------
# Pipes
# ---------------------------------------------------------------------

def test_pipe():
    text = run_case(
        "pipes: printf | grep",
        'printf "a\\nb\\nc\\n" | grep b\n',
        [lambda s: s.run_current_line()],
    )
    tail = text[-300:]
    check(
        "pipes: grep b matches only b, not a or c",
        "b" in text and "exit 0" in text and "\na\n" not in text and "\nc\n" not in text,
        tail,
    )


# ---------------------------------------------------------------------
# Redirection
# ---------------------------------------------------------------------

def test_redirection():
    text = run_case(
        "redirection: > then cat",
        "echo hello > /tmp/shdev_test_redirect.txt\ncat /tmp/shdev_test_redirect.txt\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check("redirection: file written and read back", "hello" in text, text[-300:])
    try:
        os.unlink("/tmp/shdev_test_redirect.txt")
    except OSError:
        pass


# ---------------------------------------------------------------------
# Quoting
# ---------------------------------------------------------------------

def test_single_quotes():
    text = run_case(
        "quoting: single quotes, no interpolation",
        "X=nope\necho 'hello $X world'\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check(
        "quoting: single-quoted $X is literal, not expanded",
        "hello $X world" in text,
        text[-300:],
    )


def test_double_quotes_interpolate():
    text = run_case(
        "quoting: double quotes interpolate",
        'NAME=shdev\necho "hello $NAME"\n',
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check(
        "quoting: double-quoted $NAME expands to shdev",
        "hello shdev" in text,
        text[-300:],
    )


# ---------------------------------------------------------------------
# Failure / exit-code propagation
# ---------------------------------------------------------------------

def test_failure_exit_code():
    text = run_case(
        "failure: false reports exit 1",
        "false\n",
        [lambda s: s.run_current_line()],
    )
    check("failure: false shows exit 1 in the output panel", "exit 1" in text, text[-300:])


def test_dollar_question_propagates():
    """The important one: a SEPARATE, later `echo $?` should see the
    real exit status of the PREVIOUS command, exactly like a real
    interactive shell -- not an artifact of how shdev's own
    completion-marker wrapping happens to leave bash's own $? set."""
    text = run_case(
        "failure: $? propagates to the next separate run",
        "false\necho $?\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    # The echoed value renders with variable whitespace padding before
    # "[exit ...]" depending on terminal redraw timing -- match loosely.
    check(
        "failure: echo $? on the next line shows 1, not an artifact of shdev's own wrapping",
        bool(re.search(r"\b1\b\s*\[exit 0", text)),
        text[-400:],
    )


def test_dollar_question_success_case():
    text = run_case(
        "failure: $? is 0 after a successful command",
        "true\necho $?\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check(
        "failure: echo $? shows 0 after `true`",
        bool(re.search(r"\b0\b\s*\[exit 0", text)),
        text[-400:],
    )


# ---------------------------------------------------------------------
# Functions
# ---------------------------------------------------------------------

def test_function_definition_and_call():
    text = run_case(
        "functions: define then call",
        'greet() {\n  echo "hi there"\n}\ngreet\n',
        [
            lambda s: s.run_current_line(),
            lambda s: (s.down(3), s.run_current_line()),
        ],
    )
    check("functions: calling a defined function runs its body", "hi there" in text, text[-400:])


# ---------------------------------------------------------------------
# Control structures (lighter versions of what's covered elsewhere;
# included for completeness of the matrix)
# ---------------------------------------------------------------------

def test_if_true():
    text = run_case(
        "control: if true",
        "if true; then\n  echo yes\nfi\n",
        [lambda s: s.run_current_line()],
    )
    check("control: if-block (cursor on opener) runs and prints yes", "yes" in text and "exit 0" in text, text[-300:])


def test_for_loop():
    text = run_case(
        "control: for loop",
        "for i in 1 2 3; do\n  echo \"v$i\"\ndone\n",
        [lambda s: s.run_current_line()],
    )
    check(
        "control: for-loop runs all 3 iterations",
        "v1" in text and "v2" in text and "v3" in text,
        text[-400:],
    )


def test_while_loop():
    text = run_case(
        "control: while loop",
        "n=0\nwhile [ $n -lt 3 ]; do\n  echo \"w$n\"\n  n=$((n+1))\ndone\n",
        [lambda s: s.run_current_line(), lambda s: (s.down(), s.run_current_line())],
    )
    check(
        "control: while-loop runs 3 iterations with correct values",
        "w0" in text and "w1" in text and "w2" in text,
        text[-400:],
    )


# ---------------------------------------------------------------------
# Long-running / streaming
# ---------------------------------------------------------------------

def test_streaming_output():
    s = Session('for i in 1 2 3; do echo "tick $i"; sleep 0.4; done\n')
    try:
        s.drain(1.5)
        s.send(CTRL_J)
        s.drain(0.7)
        early = s.text()
        s.drain(1.2)
        later = s.text()
        check(
            "streaming: tick 1 visible before the command finishes",
            "tick 1" in early,
            early[-200:],
        )
        check(
            "streaming: tick 3 visible once it's had time to run",
            "tick 3" in later,
            later[-200:],
        )
    finally:
        s.close()


def test_batch_run_above_cursor():
    text = run_case(
        "batch: Ctrl+E runs everything above cursor in order",
        "echo one\necho two\necho three\n",
        [lambda s: (s.down(2), s.run_all_before())],
    )
    # "Batch finished: ran 2" as one contiguous substring is fragile to
    # the known ANSI-strip rendering-fragmentation artifact (gotcha #7)
    # -- check the two halves independently instead of requiring them
    # adjacent with the exact original spacing.
    check(
        "batch: reports finishing 2 lines",
        "Batch finished" in text and "ran 2" in text,
        text[-300:],
    )


def test_history_stays_clean():
    """Every command shdev runs goes through a marker-wrapping protocol
    (redirect, printf, exit-code capture) that would otherwise pollute
    bash's own `history` on every single execution -- HISTCONTROL=
    ignorespace plus a leading space on each internal command hides all
    of it. Uses an isolated HISTFILE so this test doesn't see history
    left over from other sessions or prior test runs."""
    histfile = tempfile.NamedTemporaryFile(suffix=".hist", delete=False)
    histfile.close()
    try:
        os.unlink(histfile.name)
    except OSError:
        pass

    script = tempfile.NamedTemporaryFile(suffix=".sh", delete=False, mode="w")
    script.write("echo hello\npwd\nhistory\n")
    script.close()

    pid, fd = pty.fork()
    if pid == 0:
        env = dict(os.environ)
        env["HISTFILE"] = histfile.name
        os.execvpe(BINARY, [BINARY, script.name], env)
    text = ""
    try:
        winsize = struct.pack("HHHH", 30, 120, 0, 0)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)
        all_data = b""

        def drain(dur):
            nonlocal all_data
            deadline = time.time() + dur
            while time.time() < deadline:
                if select.select([fd], [], [], 0.1)[0]:
                    try:
                        all_data += os.read(fd, 65536)
                    except OSError:
                        break

        drain(1.5)
        os.write(fd, CTRL_J)
        drain(1.5)
        os.write(fd, DOWN)
        drain(0.3)
        os.write(fd, CTRL_J)  # pwd
        drain(1.5)
        os.write(fd, DOWN)
        drain(0.3)
        os.write(fd, CTRL_J)  # history
        drain(1.5)
        text = ANSI_RE.sub(b"", all_data).decode(errors="replace")
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        for path in (script.name, histfile.name):
            try:
                os.unlink(path)
            except OSError:
                pass

    # Only the one, accepted, unavoidable startup line should appear --
    # none of shdev's wrapped `echo hello` / `pwd` invocations, and no
    # marker/stderr-redirect syntax repeated per command.
    check(
        "history: wrapped commands don't pollute bash's own history",
        text.count("SHDEV_DONE") <= 1 and "stty" in text,
        text[-500:],
    )
    check(
        "history: functionality unaffected (echo/pwd still ran)",
        "hello" in text and "/" in text,
        text[-300:],
    )


def test_history_fully_isolated_from_outer_terminal():
    """The deeper fix: shdev's internal bash session must never share a
    HISTFILE with the user's own outer terminal at all -- ignorespace
    alone only stops *new* pollution within a session, but if HISTFILE
    is unset, the internal session inherits whatever HISTFILE the
    launching (outer) terminal uses, so the two end up reading and
    writing the exact same file. Simulates a realistic, pre-populated
    outer HISTFILE and checks isolation in both directions: shdev
    doesn't load the outer history, and doesn't write anything back into
    it either."""
    outer_histfile = tempfile.NamedTemporaryFile(suffix=".hist", delete=False, mode="w")
    outer_histfile.write("cd /some/real/project\nls -la\ngit status\n")
    outer_histfile.close()

    script = tempfile.NamedTemporaryFile(suffix=".sh", delete=False, mode="w")
    script.write("echo hello\npwd\nhistory\n")
    script.close()

    pid, fd = pty.fork()
    if pid == 0:
        env = dict(os.environ)
        env["HISTFILE"] = outer_histfile.name  # simulate the user's real outer terminal
        os.execvpe(BINARY, [BINARY, script.name], env)
    text = ""
    try:
        winsize = struct.pack("HHHH", 30, 120, 0, 0)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)
        all_data = b""

        def drain(dur):
            nonlocal all_data
            deadline = time.time() + dur
            while time.time() < deadline:
                if select.select([fd], [], [], 0.1)[0]:
                    try:
                        all_data += os.read(fd, 65536)
                    except OSError:
                        break

        drain(1.5)
        os.write(fd, CTRL_J)
        drain(1.5)
        os.write(fd, DOWN)
        drain(0.3)
        os.write(fd, CTRL_J)  # pwd
        drain(1.5)
        os.write(fd, DOWN)
        drain(0.3)
        os.write(fd, CTRL_J)  # history
        drain(1.5)
        text = ANSI_RE.sub(b"", all_data).decode(errors="replace")
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            os.unlink(script.name)
        except OSError:
            pass

    check(
        "history: doesn't load the outer terminal's existing history",
        "git status" not in text and "ls -la" not in text,
        text[-400:],
    )

    with open(outer_histfile.name) as f:
        outer_after = f.read()
    try:
        os.unlink(outer_histfile.name)
    except OSError:
        pass
    check(
        "history: doesn't write anything back into the outer terminal's history file",
        outer_after == "cd /some/real/project\nls -la\ngit status\n",
        outer_after,
    )


def test_command_history_recall():
    """Alt+Up/Alt+Down: readline-style recall of previously run commands
    into the current line -- distinct from the Ctrl+R/F6 execution
    history browser. Verifies recall of the most recent command, cycling
    further back to an older one, and restoring unsaved draft text when
    recalling back past the newest entry. Each check uses its own fresh
    session: running a recalled command adds a *new* history entry,
    which would shift what "N presses back" points to if checks were
    chained in one session.
    """
    s = Session("echo one\necho two\n\n")
    try:
        s.drain(1.2)
        s.run_current_line()  # echo one
        s.down()
        s.run_current_line()  # echo two
        s.down()  # blank line 3

        s.send(ALT_UP)
        s.drain(0.4)
        s.run_current_line()
        text = s.text()
        check("recall: Alt+Up recalls the most recent command", "two" in text, text[-300:])
    finally:
        s.close()

    s2 = Session("echo one\necho two\n\n")
    try:
        s2.drain(1.2)
        s2.run_current_line()
        s2.down()
        s2.run_current_line()
        s2.down()

        s2.send(ALT_UP)
        s2.drain(0.4)
        s2.send(ALT_UP)
        s2.drain(0.4)
        s2.run_current_line()
        text2 = s2.text()
        check("recall: a second Alt+Up cycles to an older command", "one" in text2[-400:], text2[-400:])
    finally:
        s2.close()

    # Third session: verify Alt+Down past the newest entry restores
    # whatever draft text was on the line before recall started.
    s3 = Session("echo one\necho two\n\n")
    try:
        s3.drain(1.2)
        s3.run_current_line()
        s3.down()
        s3.run_current_line()
        s3.down()
        s3.send(b"draft_text_marker")
        s3.drain(0.3)
        s3.send(ALT_UP)
        s3.drain(0.3)
        s3.send(ALT_DOWN)
        s3.drain(0.3)
        s3.run_current_line()
        text3 = s3.text()
        check(
            "recall: Alt+Down past newest restores the original unsaved line",
            "draft_text_marker" in text3,
            text3[-400:],
        )
    finally:
        s3.close()


TESTS = [
    test_simple,
    test_pwd,
    test_cd_persists,
    test_export_persists,
    test_pipe,
    test_redirection,
    test_single_quotes,
    test_double_quotes_interpolate,
    test_failure_exit_code,
    test_dollar_question_propagates,
    test_dollar_question_success_case,
    test_function_definition_and_call,
    test_if_true,
    test_for_loop,
    test_while_loop,
    test_streaming_output,
    test_batch_run_above_cursor,
    test_history_stays_clean,
    test_history_fully_isolated_from_outer_terminal,
    test_command_history_recall,
]


def main():
    if not os.path.exists(BINARY):
        print(f"error: {BINARY} not found -- run `cargo build --release` first", file=sys.stderr)
        sys.exit(2)

    for t in TESTS:
        try:
            t()
        except Exception as e:  # noqa: BLE001 -- a test crashing is itself a failure to report
            check(t.__name__, False, f"raised {type(e).__name__}: {e}")

    failed = [name for name, ok, _ in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    if failed:
        print("Failed:")
        for name in failed:
            print(f"  - {name}")
        sys.exit(1)


if __name__ == "__main__":
    main()
