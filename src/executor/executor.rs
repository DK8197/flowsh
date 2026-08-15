//! The Execution Engine, now asynchronous.
//!
//! `ExecutionEngine` spawns a single background thread that owns the
//! `PtyManager` for the lifetime of the app. The UI thread never blocks
//! on it: it fires `run_line()` (a non-blocking send) and then drains
//! `try_recv_event()` once per frame to pick up streamed output chunks
//! and the final result. `cancel()` delivers Ctrl+C to whatever is
//! currently running, without disturbing the persistent bash session.

use crate::executor::command::CancelReason;
use crate::models::Output;
use crate::pty::{PtyEvent, PtyManager};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Select, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Safety-net ceiling on any single command's runtime, if not overridden
/// by config (`Config::command_timeout_secs`). Manual Ctrl+C
/// cancellation is the primary mechanism; this only guards against a
/// forgotten `sleep 999999` eating the session forever.
pub const DEFAULT_MAX_RUNTIME: Duration = Duration::from_secs(15 * 60);

/// Events streamed back from the executor thread to the UI thread.
pub enum ExecEvent {
    OutputChunk { line_id: u64, chunk: String },
    Finished { line_id: u64, output: Output },
    Cancelled { line_id: u64, reason: CancelReason },
    Failed { line_id: u64, message: String },
}

enum ExecCommand {
    Run { line_id: u64, command: String },
    Cancel,
    Shutdown,
}

pub struct ExecutionEngine {
    cmd_tx: Sender<ExecCommand>,
    event_rx: Receiver<ExecEvent>,
    handle: Option<JoinHandle<()>>,
}

impl ExecutionEngine {
    pub fn new(shell: &str, shell_args: &[String], max_runtime: Duration) -> Result<Self> {
        // Spawned on the calling thread so startup errors (bash missing,
        // PTY unavailable, ...) surface synchronously from `App::new`
        // rather than silently failing inside a background thread.
        let pty = PtyManager::new(shell, shell_args)?;

        let (cmd_tx, cmd_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        let handle = thread::spawn(move || executor_thread(pty, cmd_rx, event_tx, max_runtime));

        Ok(Self { cmd_tx, event_rx, handle: Some(handle) })
    }

    /// Fire off a command asynchronously. Returns immediately; results
    /// arrive later via `try_recv_event()`.
    pub fn run_line(&self, line_id: u64, command: String) {
        let _ = self.cmd_tx.send(ExecCommand::Run { line_id, command });
    }

    /// Deliver Ctrl+C to whatever is currently running. A no-op (safely
    /// ignored by the executor thread) if nothing is running.
    pub fn cancel(&self) {
        let _ = self.cmd_tx.send(ExecCommand::Cancel);
    }

    /// Non-blocking poll for the next available event, if any.
    pub fn try_recv_event(&self) -> Option<ExecEvent> {
        match self.event_rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(ExecCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

enum RunOutcome {
    Completed,
    ShutdownRequested,
}

/// Deliver Ctrl+C, then immediately queue a brand-new, uniquely-marked
/// no-op probe (`printf '\n<marker>%d\n' "$?"`) and repoint `marker` at
/// it. This exists because of a real, non-obvious bash behavior: when an
/// interactive bash receives SIGINT while running a foreground job, it
/// doesn't just kill that job and continue on to the next
/// semicolon-separated command on the same input line — it unwinds all
/// the way back to its prompt-read loop, abandoning the rest of the
/// current command list entirely. That means our own trailing
/// `printf '...marker...'` (part of the *same* line as the command that
/// got interrupted) never runs, and waiting on it would hang forever.
/// The probe line is written independently and sits safely in the
/// kernel's tty input queue until bash actually reads it, so this works
/// regardless of exact timing.
fn resynchronize_after_interrupt(pty: &mut PtyManager, marker: &mut String) {
    let _ = pty.send_interrupt();
    let resync_id = pty.next_marker_id();
    *marker = format!("{}{}=", crate::pty::manager::MARKER_PREFIX, resync_id);
    // Same $?-preservation reasoning as the main wrapped command (see
    // its comment below): without capturing and re-exiting with it,
    // bash's own $? after this probe would reflect printf's success,
    // not the interrupted command's real exit status.
    // Leading space: excluded from bash's own history via
    // HISTCONTROL=ignorespace (set at startup) — see the comment on
    // that setup line for why every internal command does this.
    let _ = pty.send_line(&format!(
        " __shdev_ec=$?; printf '\\n{marker}%d\\n' \"$__shdev_ec\"; ( exit $__shdev_ec )",
        marker = marker
    ));
}

fn executor_thread(mut pty: PtyManager, cmd_rx: Receiver<ExecCommand>, event_tx: Sender<ExecEvent>, max_runtime: Duration) {
    loop {
        match cmd_rx.recv() {
            Ok(ExecCommand::Run { line_id, command }) => {
                match run_one(&mut pty, line_id, &command, &cmd_rx, &event_tx, max_runtime) {
                    RunOutcome::Completed => continue,
                    RunOutcome::ShutdownRequested => break,
                }
            }
            // Nothing is running (we're blocked in `recv()`, not inside
            // `run_one`), so there's nothing to cancel.
            Ok(ExecCommand::Cancel) => continue,
            Ok(ExecCommand::Shutdown) | Err(_) => break,
        }
    }
    let _ = pty.shutdown();
}

/// Run a single command to completion (or until cancelled/shut down),
/// streaming output chunks as they arrive and reacting to `Cancel` /
/// `Shutdown` requests that come in *while* the command is running.
fn run_one(
    pty: &mut PtyManager,
    line_id: u64,
    command: &str,
    cmd_rx: &Receiver<ExecCommand>,
    event_tx: &Sender<ExecEvent>,
    max_runtime: Duration,
) -> RunOutcome {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        let _ = event_tx.send(ExecEvent::Finished {
            line_id,
            output: Output::new(command, String::new(), String::new(), 0, 0),
        });
        return RunOutcome::Completed;
    }

    let id = pty.next_marker_id();
    let stderr_path = format!("/tmp/shdev_stderr_{}_{}.log", std::process::id(), id);
    let mut marker = format!("{}{}=", crate::pty::manager::MARKER_PREFIX, id);
    // `__shdev_ec` captures the wrapped command's real exit status so we
    // can both report it (via the marker) *and* re-exit with it as the
    // wrapped line's own last action. Without that final `(exit ...)`,
    // bash's own `$?` — as seen by a later, separately-run `echo $?` —
    // would reflect our internal `printf`'s success (0), not the user's
    // command. Found via tests/shell_behavior_test.py, not inspection:
    // `false` followed by a separate `echo $?` showed 0, not 1.
    // Leading space: excluded from bash's own history via
    // HISTCONTROL=ignorespace (set at startup). This hides the whole
    // line — including the user's own command, which can't be
    // separated from the wrapping around it — not just the marker
    // machinery. shdev's own history browser (Ctrl+R/F6) already
    // tracks every command alongside its output and exit code, so
    // nothing is lost; this just keeps bash's own `history` usable for
    // anything else rather than filling up with wrapper syntax.
    let wrapped = format!(
        " {{ {cmd} ; }} 2>'{stderr_path}'; __shdev_ec=$?; printf '\\n{marker}%d\\n' \"$__shdev_ec\"; ( exit $__shdev_ec )",
        cmd = trimmed,
        stderr_path = stderr_path,
        marker = marker
    );

    if let Err(e) = pty.send_line(&wrapped) {
        let _ = event_tx.send(ExecEvent::Failed { line_id, message: e.to_string() });
        return RunOutcome::Completed;
    }

    let started_at = Instant::now();
    let deadline = started_at + max_runtime;
    let pty_rx = pty.event_receiver();

    let mut sel = Select::new();
    let cmd_idx = sel.recv(cmd_rx);
    let pty_idx = sel.recv(&pty_rx);

    let mut pending = String::new();
    let mut stdout_acc = String::new();
    let mut exit_code: Option<i32> = None;
    let mut cancel_reason: Option<CancelReason> = None;
    let mut timeout_fired = false;

    loop {
        if exit_code.is_some() {
            break;
        }
        if !timeout_fired && Instant::now() > deadline {
            timeout_fired = true;
            cancel_reason = Some(CancelReason::Timeout);
            resynchronize_after_interrupt(pty, &mut marker);
        }

        let oper = match sel.select_timeout(Duration::from_millis(200)) {
            Ok(op) => op,
            Err(_) => continue, // idle tick — loop back to recheck the deadline
        };

        if oper.index() == cmd_idx {
            match oper.recv(cmd_rx) {
                Ok(ExecCommand::Cancel) => {
                    if cancel_reason.is_none() {
                        cancel_reason = Some(CancelReason::UserRequested);
                        resynchronize_after_interrupt(pty, &mut marker);
                    } else {
                        // Already cancelling — resend just the raw
                        // interrupt byte in case the first one didn't
                        // land in time to catch a just-started process.
                        // Don't send a second resync probe: bash can
                        // only be unwinding to one prompt at a time, and
                        // an extra probe would just be a stray line we'd
                        // have to ignore.
                        let _ = pty.send_interrupt();
                    }
                }
                Ok(ExecCommand::Run { .. }) => {
                    // Only one command runs at a time; the App layer is
                    // expected to guard against this, but ignore it here
                    // too rather than corrupting the in-flight command.
                }
                Ok(ExecCommand::Shutdown) | Err(_) => {
                    let _ = pty.send_interrupt();
                    return RunOutcome::ShutdownRequested;
                }
            }
        } else if oper.index() == pty_idx {
            match oper.recv(&pty_rx) {
                Ok(PtyEvent::Chunk(chunk)) => {
                    pending.push_str(&chunk);
                    while let Some(pos) = pending.find('\n') {
                        let line: String = pending.drain(..=pos).collect();
                        let line_clean = line.replace('\r', "");
                        let line_clean = line_clean.trim_end_matches('\n').to_string();
                        if let Some(rest) = line_clean.strip_prefix(&marker) {
                            exit_code = rest.trim().parse::<i32>().ok().or(Some(-1));
                            break;
                        } else {
                            stdout_acc.push_str(&line_clean);
                            stdout_acc.push('\n');
                            let _ = event_tx.send(ExecEvent::OutputChunk {
                                line_id,
                                chunk: format!("{line_clean}\n"),
                            });
                        }
                    }
                }
                Ok(PtyEvent::Closed) => {
                    let _ = event_tx.send(ExecEvent::Failed {
                        line_id,
                        message: "bash session closed unexpectedly".to_string(),
                    });
                    return RunOutcome::Completed;
                }
                Err(_) => {
                    let _ = event_tx.send(ExecEvent::Failed {
                        line_id,
                        message: "pty reader channel disconnected".to_string(),
                    });
                    return RunOutcome::Completed;
                }
            }
        }
    }

    // Our wrapper always emits a leading blank line right before the
    // marker (the literal "\n" in the printf format), which shows up
    // here as one extra trailing empty entry. Strip exactly one.
    if let Some(stripped) = stdout_acc.strip_suffix("\n\n") {
        stdout_acc = format!("{stripped}\n");
    } else if stdout_acc == "\n" {
        stdout_acc.clear();
    }

    let stderr_acc = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = std::fs::remove_file(&stderr_path);
    let runtime_ms = started_at.elapsed().as_millis();
    let code = exit_code.unwrap_or(-1);

    match cancel_reason {
        Some(reason) => {
            let _ = event_tx.send(ExecEvent::Cancelled { line_id, reason });
        }
        None => {
            let output = Output::new(command, stdout_acc, stderr_acc, code, runtime_ms);
            let _ = event_tx.send(ExecEvent::Finished { line_id, output });
        }
    }

    RunOutcome::Completed
}
