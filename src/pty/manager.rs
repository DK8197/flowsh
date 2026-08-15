//! `PtyManager` — the most important module in shdev.
//!
//! It starts a persistent bash process and exposes low-level primitives
//! that the executor thread composes into async, streaming, cancellable
//! command execution:
//!
//! - [`PtyManager::send_line`] — write a line to bash's stdin
//! - [`PtyManager::send_interrupt`] — deliver Ctrl+C (SIGINT) to whatever
//!   is currently in the foreground, without touching the bash session
//!   itself
//! - [`PtyManager::event_receiver`] — a cheap clone of the channel that
//!   the background PTY-reader thread streams raw output chunks into
//!
//! `PtyManager` itself does not know what "a command" or "a marker" is —
//! that framing lives in `executor`, which is what actually needs to
//! stream partial output and react to cancellation mid-command.

use crate::pty::bash::BashProcess;
use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Raw events coming off the PTY reader thread.
pub enum PtyEvent {
    /// A chunk of raw output text (may or may not end on a line boundary).
    Chunk(String),
    /// The bash process itself has exited / the PTY closed.
    Closed,
}

pub struct PtyManager {
    bash: BashProcess,
    rx: Receiver<PtyEvent>,
    marker_counter: AtomicUsize,
}

/// Purely printable ASCII on purpose: bash reads our writes through
/// readline, so any control byte (e.g. \x01) would be interpreted as a
/// keybinding (like "move to start of line") instead of literal text.
pub(crate) const MARKER_PREFIX: &str = "___SHDEV_DONE___";

/// Ctrl+C's raw byte (ASCII ETX). Writing this into the PTY master is
/// exactly what a real terminal does when you press Ctrl+C — the kernel's
/// tty line discipline turns it into SIGINT for whatever process group
/// currently owns the foreground of this PTY. Since we never disabled
/// ISIG (only local echo), this works even though the session is being
/// driven programmatically rather than by a human typing into it.
const INTERRUPT_BYTE: u8 = 0x03;

impl PtyManager {
    pub fn new(shell: &str, shell_args: &[String]) -> Result<Self> {
        let mut bash = BashProcess::spawn(shell, shell_args)?;

        let (tx, rx): (Sender<PtyEvent>, Receiver<PtyEvent>) = unbounded();
        spawn_reader_thread(bash.take_reader(), tx);

        let mut manager = Self {
            bash,
            rx,
            marker_counter: AtomicUsize::new(0),
        };

        // Disable the kernel tty echo (so our own writes aren't reflected
        // back into future output) and blank the prompt. Run this through
        // the same marker-drain machinery used for real commands so any
        // echoed setup text is fully consumed before the first real
        // command runs — this is a one-shot blocking wait, which is fine
        // only because it happens during startup, before any UI exists.
        // HISTCONTROL=ignorespace means a line starting with a space is
        // excluded from bash's own command history. Every internal
        // command shdev sends from here on (the execution wrapper, the
        // interrupt-resync probe) is prefixed with exactly one leading
        // space for this reason — without it, every single line you run
        // would also show up verbatim (marker, stderr-redirect path,
        // and all) in `history`, making it useless for anything else.
        // This one setup line still shows up in bash's own `history`
        // despite HISTCONTROL=ignorespace being set within it — bash
        // decides whether to record a line using HISTCONTROL's value
        // from *before* that line runs, not after, so a command can't
        // hide itself this way (confirmed by testing, not assumed).
        // Every subsequent internal command can and does hide itself,
        // since ignorespace is active by then — see the leading space
        // on every `pty.send_line` call in `executor.rs`. This one
        // leftover line per session is an accepted, minor exception,
        // not the repeated per-command pollution this was meant to fix.
        manager.blocking_setup("stty -echo 2>/dev/null; PS1=''; HISTCONTROL=ignorespace; true")?;

        Ok(manager)
    }

    /// Allocate the next unique marker id for a command execution.
    pub fn next_marker_id(&self) -> usize {
        self.marker_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Write a line to bash's stdin, terminated with `\n`.
    pub fn send_line(&mut self, line: &str) -> Result<()> {
        self.bash.write_line(line)
    }

    /// Deliver Ctrl+C (SIGINT) to whatever is currently running in the
    /// foreground of this PTY. The persistent bash session itself is not
    /// affected — only the currently-running child (or bash's own
    /// prompt-read, if nothing is running) receives it, exactly like a
    /// real terminal.
    pub fn send_interrupt(&mut self) -> Result<()> {
        use std::io::Write;
        self.bash.writer.write_all(&[INTERRUPT_BYTE])?;
        self.bash.writer.flush()?;
        Ok(())
    }

    /// A cheap clone of the receiving end of the PTY reader channel.
    /// Safe to hand out freely — `crossbeam_channel::Receiver` is a thin,
    /// reference-counted handle.
    pub fn event_receiver(&self) -> Receiver<PtyEvent> {
        self.rx.clone()
    }

    /// One-shot blocking helper used only during [`PtyManager::new`] to
    /// run the initial `stty -echo` setup and fully drain its echoed
    /// output before returning. Real command execution never uses this —
    /// see `executor::run_one` for the async, streaming, cancellable path.
    fn blocking_setup(&mut self, command: &str) -> Result<()> {
        let id = self.next_marker_id();
        let marker = format!("{}{}=", MARKER_PREFIX, id);
        let wrapped = format!("{command}; printf '\\n{marker}%d\\n' \"$?\"");
        self.send_line(&wrapped)?;

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut pending = String::new();
        loop {
            if Instant::now() > deadline {
                return Err(anyhow!("startup setup command timed out"));
            }
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PtyEvent::Chunk(chunk)) => {
                    pending.push_str(&chunk);
                    // Match the marker only as the start of a complete
                    // line, exactly like the real per-command execution
                    // path does. A plain substring search would also
                    // match the terminal's own echo of what we just
                    // typed (echo is still on until `stty -echo` has
                    // actually run) — that echoed line contains the
                    // marker text too, just embedded mid-line inside the
                    // printf format string, not printed by bash as real
                    // output yet.
                    while let Some(pos) = pending.find('\n') {
                        let line: String = pending.drain(..=pos).collect();
                        let line_clean = line.replace('\r', "");
                        let line_clean = line_clean.trim_end_matches('\n');
                        if line_clean.starts_with(&marker) {
                            return Ok(());
                        }
                    }
                }
                Ok(PtyEvent::Closed) => return Err(anyhow!("bash session closed during startup")),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("pty reader thread disconnected during startup"))
                }
            }
        }
    }

    pub fn shutdown(&mut self) -> Result<()> {
        let _ = self.bash.write_line("exit");
        self.bash.kill()
    }
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, tx: Sender<PtyEvent>) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(PtyEvent::Closed);
                    break;
                }
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if tx.send(PtyEvent::Chunk(text)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(PtyEvent::Closed);
                    break;
                }
            }
        }
    });
}
