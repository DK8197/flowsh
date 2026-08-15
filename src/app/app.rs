//! The App controller. This is the glue layer: it owns [`AppState`], the
//! [`ExecutionEngine`], and reacts to [`Intent`]s produced by the
//! shortcut handler. It never draws to the screen (that's `ui::renderer`)
//! and never talks to the PTY directly (that's the executor's job).
//!
//! Execution is asynchronous: `dispatch(Intent::RunCurrentLine)` fires a
//! request at the executor thread and returns immediately. The actual
//! progress (streamed output chunks, the final result, a cancellation)
//! arrives later via `poll_events()`, which the main loop calls once per
//! frame — see `main.rs`.

use crate::app::events::AppEvent;
use crate::app::state::{AppState, Focus};
use crate::editor::{blocks, Editor};
use crate::executor::{ExecEvent, ExecutionEngine};
use crate::models::{Line, LineStatus, Output};
use crate::shortcuts::Intent;
use crate::storage::FileManager;
use anyhow::Result;
use std::collections::VecDeque;
use std::path::PathBuf;

/// Flatten a compound block's lines into a single physical line for
/// execution, using `;` as the statement separator instead of literal
/// embedded newlines.
///
/// This matters more than it looks: sending a block as raw multi-line
/// text (joined with `\n`) means bash receives it as several distinct
/// physical input lines, handled through its *interactive* multi-line
/// continuation machinery (PS2 prompts, etc.) — which shdev never
/// accounts for (only `PS1` is blanked at startup, not `PS2`), and which
/// turned out to be fragile in practice: found via testing that a
/// `for`-loop block sent this way silently only ran its first
/// iteration. Newline and `;` are equivalent statement separators in
/// bash grammar in most positions, so flattening to one line sidesteps
/// interactive line-continuation handling entirely — *except* right
/// after `do`/`then`/`else`/`elif`, which already introduce the next
/// command directly; inserting `;` there creates an empty statement
/// (`do; echo hi` is a syntax error — also found via testing, not by
/// inspection). A plain space is used after those instead.
///
/// Blank lines are dropped (an empty statement between two `;`s is also
/// a syntax error). A `#` comment on a body line will swallow the rest
/// of the flattened line, same as it would swallow a following line in
/// the original multi-line form — a known, accepted limitation of the
/// same kind as `editor::blocks`' keyword-based (not fully parsed)
/// detection. `case` block bodies (which use `;;` internally) may not
/// always flatten to a perfectly valid one-liner for complex multi
/// pattern cases — a narrower version of the same limitation.
fn flatten_block(lines: &[Line]) -> String {
    const NO_SEPARATOR_NEEDED_AFTER: [&str; 4] = ["do", "then", "else", "elif"];
    let mut result = String::new();
    let mut prev_last_word: Option<String> = None;
    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !result.is_empty() {
            let joiner = match prev_last_word.as_deref() {
                Some(w) if NO_SEPARATOR_NEEDED_AFTER.contains(&w) => " ",
                _ => "; ",
            };
            result.push_str(joiner);
        }
        result.push_str(trimmed);
        prev_last_word = trimmed
            .rsplit(|c: char| c.is_whitespace())
            .find(|s| !s.is_empty())
            .map(str::to_string);
    }
    result
}

/// Cap on the in-memory event log so a long dev session doesn't grow
/// without bound. Purely for observability (nothing currently reads this
/// back), so a simple ring buffer is enough.
const EVENT_LOG_CAP: usize = 500;

pub struct App {
    pub state: AppState,
    executor: ExecutionEngine,
    pub events: VecDeque<AppEvent>,
}

impl App {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let editor = match &path {
            Some(p) => {
                let text = FileManager::open(p)?;
                Editor::from_text(&text)
            }
            None => Editor::new(),
        };

        let loaded = crate::config::Config::load();
        let timeout = std::time::Duration::from_secs(loaded.config.command_timeout_secs);
        let executor = ExecutionEngine::new(&loaded.config.shell, &loaded.config.shell_args, timeout)?;
        let state = AppState::new(editor, path.clone());
        let mut app = Self { state, executor, events: VecDeque::new() };

        if let Some(p) = &path {
            app.emit(AppEvent::FileLoaded { path: p.display().to_string() });
        }
        if let Some(warning) = loaded.warning {
            app.set_status(warning);
        } else if let Some(p) = &path {
            app.set_status(format!("Opened {}", p.display()));
        } else {
            app.set_status("New buffer (unsaved) — Ctrl+S to save".to_string());
        }

        Ok(app)
    }

    pub fn dispatch(&mut self, intent: Intent) {
        // Any intent other than continuing to recall ends the recall
        // session — the recalled (or restored) text just becomes normal
        // line content from here on, same as pressing any other key at
        // a real shell prompt after using Up/Down to recall a command.
        if !matches!(intent, Intent::RecallPreviousCommand | Intent::RecallNextCommand) {
            self.state.history_recall_index = None;
            self.state.history_recall_saved_line = None;
        }

        match intent {
            Intent::InsertChar(c) if self.state.focus == Focus::Editor => self.state.editor.insert_char(c),
            Intent::InsertNewline if self.state.focus == Focus::Editor => self.state.editor.insert_newline(),
            Intent::Backspace if self.state.focus == Focus::Editor => self.state.editor.backspace(),
            Intent::DeleteForward if self.state.focus == Focus::Editor => self.state.editor.delete_forward(),
            Intent::MoveLeft if self.state.focus == Focus::Editor => self.state.editor.move_left(),
            Intent::MoveRight if self.state.focus == Focus::Editor => self.state.editor.move_right(),
            // While history is open, the arrow keys browse the list
            // instead of moving the (hidden) editor cursor — Up goes to
            // a more recent entry, Down to an older one.
            Intent::MoveUp if self.state.history_open => {
                self.state.history_selected = self.state.history_selected.saturating_sub(1);
                self.state.output_scroll = 0;
            }
            Intent::MoveDown if self.state.history_open => {
                let max = self.state.output_history.len().saturating_sub(1);
                self.state.history_selected = (self.state.history_selected + 1).min(max);
                self.state.output_scroll = 0;
            }
            Intent::MoveUp if self.state.focus == Focus::Editor => self.state.editor.move_up(),
            Intent::MoveDown if self.state.focus == Focus::Editor => self.state.editor.move_down(),
            Intent::MoveHome if self.state.focus == Focus::Editor => self.state.editor.move_home(),
            Intent::MoveEnd if self.state.focus == Focus::Editor => self.state.editor.move_end(),
            Intent::ScrollOutputUp => self.state.output_scroll = self.state.output_scroll.saturating_sub(1),
            Intent::ScrollOutputDown => self.state.output_scroll = self.state.output_scroll.saturating_add(1),
            Intent::ToggleFocus => {
                self.state.focus = match self.state.focus {
                    Focus::Editor => Focus::Output,
                    Focus::Output => Focus::Editor,
                };
            }
            Intent::ToggleHistory => self.toggle_history(),
            Intent::Undo => {
                let status = if self.state.editor.undo() { "Undo" } else { "Nothing to undo" };
                self.set_status(status.to_string());
            }
            Intent::Redo => {
                let status = if self.state.editor.redo() { "Redo" } else { "Nothing to redo" };
                self.set_status(status.to_string());
            }
            Intent::RecallPreviousCommand if self.state.focus == Focus::Editor => self.recall_command(true),
            Intent::RecallNextCommand if self.state.focus == Focus::Editor => self.recall_command(false),
            Intent::RunCurrentLine => self.start_run(),
            Intent::RunAllBefore => self.start_run_all_before(),
            Intent::CancelExecution => self.cancel_run(),
            Intent::Save => self.save(),
            Intent::Quit => self.state.should_quit = true,
            _ => {}
        }
    }

    fn toggle_history(&mut self) {
        self.state.history_open = !self.state.history_open;
        if self.state.history_open {
            if self.state.output_history.is_empty() {
                self.state.history_open = false;
                self.set_status("No commands have run yet — nothing to show in history".to_string());
                return;
            }
            self.state.history_selected = 0;
            self.state.output_scroll = 0;
            self.state.focus = Focus::Output;
            self.set_status(format!(
                "History: {} run(s) — ↑/↓ browse, Ctrl+↑/↓ scroll detail, Ctrl+R/F6 close",
                self.state.output_history.len()
            ));
        }
    }

    /// Alt+Up (`older=true`) / Alt+Down (`older=false`): readline-style
    /// recall of a previously *run* command into the current line, the
    /// same way pressing Up/Down at a real shell prompt cycles through
    /// history. Reuses `output_history` (already tracked for the
    /// execution-history browser) rather than keeping a second, separate
    /// command list — the two features share the same underlying data,
    /// just different UI for browsing it.
    fn recall_command(&mut self, older: bool) {
        if self.state.output_history.is_empty() {
            self.set_status("No command history yet".to_string());
            return;
        }

        let max = self.state.output_history.len() - 1;
        let next_index = match self.state.history_recall_index {
            None if older => {
                self.state.history_recall_saved_line = Some(self.state.editor.current_line().to_string());
                Some(0)
            }
            None => return, // Alt+Down with nothing being recalled: no-op
            Some(idx) if older => {
                if idx >= max {
                    self.set_status("Beginning of command history".to_string());
                    return;
                }
                Some(idx + 1)
            }
            Some(0) => {
                // Recalled past the newest entry: restore what was on
                // the line before recall started, same as a real shell.
                let saved = self.state.history_recall_saved_line.take().unwrap_or_default();
                let row = self.state.editor.cursor.row;
                self.state.editor.set_line_text(row, saved);
                None
            }
            Some(idx) => Some(idx - 1),
        };

        self.state.history_recall_index = next_index;
        let Some(idx) = next_index else { return };
        let source_idx = self.state.output_history.len() - 1 - idx;
        let command = self.state.output_history[source_idx].command.clone();
        let row = self.state.editor.cursor.row;
        self.state.editor.set_line_text(row, command);
    }

    /// Drain every event currently available from the executor thread and
    /// apply it to `AppState`. Called once per frame from the main loop,
    /// *before* drawing, so the UI never blocks waiting for a running
    /// command — it just reflects whatever progress has arrived so far.
    pub fn poll_events(&mut self) {
        while let Some(ev) = self.executor.try_recv_event() {
            match ev {
                ExecEvent::OutputChunk { line_id, chunk } => {
                    if self.state.running_line_id == Some(line_id) {
                        self.state.live_output.push_str(&chunk);
                    }
                    self.emit(AppEvent::ExecutionOutputChunk { line_id, chunk });
                }
                ExecEvent::Finished { line_id, output } => {
                    let status = if output.is_success() { LineStatus::Success } else { LineStatus::Failed };
                    let mut ids = vec![line_id];
                    ids.extend(self.state.running_extra_line_ids.iter().copied());
                    for id in &ids {
                        self.finalize_line(*id, status.clone(), Some(&output));
                    }
                    self.state.record_output(line_id, output.clone());
                    self.set_status(format!("Exit {} in {}ms", output.exit_code, output.runtime_ms));
                    self.emit(AppEvent::ExecutionFinished { line_id, output });
                    self.state.end_run();
                    // A batch run continues past a command's own non-zero
                    // exit code — this mirrors what running each line by
                    // hand, one after another, would do. Only a
                    // cancellation or an infrastructure failure aborts
                    // the rest of the queue (handled below).
                    self.advance_batch();
                }
                ExecEvent::Cancelled { line_id, reason } => {
                    let mut ids = vec![line_id];
                    ids.extend(self.state.running_extra_line_ids.iter().copied());
                    for id in &ids {
                        self.finalize_line(*id, LineStatus::Cancelled, None);
                    }
                    let batch_note = if self.state.is_batch_running() { " — batch stopped" } else { "" };
                    self.set_status(format!("Command {reason}{batch_note} — bash session is still alive"));
                    self.emit(AppEvent::ExecutionCancelled { line_id, reason });
                    self.state.end_run();
                    self.abort_batch();
                }
                ExecEvent::Failed { line_id, message } => {
                    let mut ids = vec![line_id];
                    ids.extend(self.state.running_extra_line_ids.iter().copied());
                    for id in &ids {
                        self.finalize_line(*id, LineStatus::Failed, None);
                    }
                    let batch_note = if self.state.is_batch_running() { " — batch stopped" } else { "" };
                    self.set_status(format!("Execution error: {message}{batch_note}"));
                    self.emit(AppEvent::ExecutionFailed { line_id, message });
                    self.state.end_run();
                    self.abort_batch();
                }
            }
        }
    }

    fn start_run(&mut self) {
        if self.state.is_running() {
            self.set_status("A command is already running — Ctrl+C to cancel it first".to_string());
            return;
        }

        let cursor_row = self.state.editor.cursor.row;
        let text_snapshot: Vec<String> = self.state.editor.buffer.lines().iter().map(|l| l.text.clone()).collect();

        if let Some((start, end)) = blocks::enclosing_block(&text_snapshot, cursor_row) {
            let block_lines = &self.state.editor.buffer.lines()[start..=end];
            let line_id = block_lines[0].id;
            let extra_ids: Vec<u64> = block_lines[1..].iter().map(|l| l.id).collect();
            let command = flatten_block(block_lines);
            self.begin_execution(line_id, extra_ids, command);
            return;
        }

        let line_id = match self.state.editor.current_line_id() {
            Some(id) => id,
            None => return,
        };
        let command = self.state.editor.current_line().to_string();

        if command.trim().is_empty() {
            self.set_status("Nothing to run on an empty line".to_string());
            return;
        }

        self.begin_execution(line_id, Vec::new(), command);
    }

    /// Ctrl+E: queue up every non-blank line *above* the cursor (not
    /// including it) and run them one after another, in order, through
    /// the persistent bash session — equivalent to pressing Ctrl+Enter
    /// on each of those lines by hand, top to bottom. A `for`/`while`/
    /// `until`/`if`/`case` block is queued as a single step (its lines
    /// run together as one unit), the same way running it directly does
    /// — otherwise a block's opener would run alone, never see its body
    /// or closer, and (for a `for`/`while`) hang or misbehave.
    fn start_run_all_before(&mut self) {
        if self.state.is_running() {
            self.set_status("A command is already running — Ctrl+C to cancel it first".to_string());
            return;
        }

        let cursor_row = self.state.editor.cursor.row;
        let text_snapshot: Vec<String> = self.state.editor.buffer.lines().iter().map(|l| l.text.clone()).collect();

        let mut queue: VecDeque<(u64, Vec<u64>, String)> = VecDeque::new();
        let mut row = 0usize;
        while row < cursor_row {
            if let Some((start, end)) = blocks::enclosing_block(&text_snapshot, row) {
                if start == row {
                    let block_lines = &self.state.editor.buffer.lines()[start..=end];
                    let line_id = block_lines[0].id;
                    let extra_ids: Vec<u64> = block_lines[1..].iter().map(|l| l.id).collect();
                    let command = flatten_block(block_lines);
                    queue.push_back((line_id, extra_ids, command));
                    // A block that starts before the cursor but extends
                    // past it is included in full — a partial block
                    // can't run correctly, so there's no useful "smaller"
                    // thing to do here.
                    row = end + 1;
                    continue;
                }
            }
            let line = &self.state.editor.buffer.lines()[row];
            if !line.text.trim().is_empty() {
                queue.push_back((line.id, Vec::new(), line.text.clone()));
            }
            row += 1;
        }

        if queue.is_empty() {
            self.set_status("Nothing above the cursor to run".to_string());
            return;
        }

        self.state.batch_total = queue.len();
        self.state.batch_position = 0;
        self.state.batch_remaining = queue;
        self.advance_batch();
    }

    /// Pop the next queued step (if any) and start it. Called both to
    /// kick off a fresh batch and to continue one after each step
    /// finishes normally.
    fn advance_batch(&mut self) {
        if let Some((line_id, extra_ids, command)) = self.state.batch_remaining.pop_front() {
            self.state.batch_position += 1;
            self.begin_execution(line_id, extra_ids, command);
        } else if self.state.is_batch_running() {
            let total = self.state.batch_total;
            self.state.batch_total = 0;
            self.state.batch_position = 0;
            self.set_status(format!("Batch finished: ran {total} line(s)"));
        }
    }

    /// Drop whatever's left in the queue without running it — used when
    /// a batch is interrupted by cancellation or an infrastructure
    /// failure, as opposed to a command simply exiting non-zero (which
    /// does not stop the batch — see `poll_events`).
    fn abort_batch(&mut self) {
        if self.state.is_batch_running() {
            self.state.batch_remaining.clear();
            self.state.batch_total = 0;
            self.state.batch_position = 0;
        }
    }

    /// Shared plumbing for starting a single run — either one line or a
    /// whole compound block treated as a unit — used by a lone
    /// `Ctrl+Enter`, each step of a `Ctrl+E` batch, and a block run.
    /// `extra_ids` is every line besides `line_id` that's part of this
    /// same run (empty for a normal single-line run).
    fn begin_execution(&mut self, line_id: u64, extra_ids: Vec<u64>, command: String) {
        self.state.history_open = false;
        for id in std::iter::once(line_id).chain(extra_ids.iter().copied()) {
            if let Some(line) = self.find_line_mut(id) {
                line.status = LineStatus::Running;
            }
        }
        let block_size = extra_ids.len() + 1;
        self.state.begin_run(line_id, command.clone());
        self.state.running_extra_line_ids = extra_ids;

        let first_line = command.lines().next().unwrap_or("");
        let status = match (self.state.is_batch_running(), block_size > 1) {
            (true, true) => format!(
                "Running {}/{} (block, {block_size} lines): {first_line}",
                self.state.batch_position, self.state.batch_total
            ),
            (true, false) => format!("Running {}/{}: {command}", self.state.batch_position, self.state.batch_total),
            (false, true) => format!("Running block ({block_size} lines): {first_line}"),
            (false, false) => format!("Running: {command}"),
        };
        self.set_status(status);
        self.emit(AppEvent::ExecutionStarted { line_id, command: command.clone() });

        self.executor.run_line(line_id, command);
    }

    fn cancel_run(&mut self) {
        if !self.state.is_running() {
            self.set_status("Nothing is running".to_string());
            return;
        }
        self.set_status("Sending Ctrl+C…".to_string());
        self.executor.cancel();
    }

    fn finalize_line(&mut self, line_id: u64, status: LineStatus, output: Option<&Output>) {
        if let Some(line) = self.find_line_mut(line_id) {
            line.status = status;
            if let Some(output) = output {
                line.exit_code = Some(output.exit_code);
                line.runtime_ms = Some(output.runtime_ms);
                let combined = if output.stderr.is_empty() {
                    output.stdout.clone()
                } else {
                    format!("{}{}", output.stdout, output.stderr)
                };
                line.last_output = Some(combined);
            }
        }
    }

    fn find_line_mut(&mut self, line_id: u64) -> Option<&mut Line> {
        self.state
            .editor
            .buffer
            .lines()
            .iter()
            .position(|l| l.id == line_id)
            .and_then(|idx| self.state.editor.buffer.line_mut(idx))
    }

    fn save(&mut self) {
        let path = match &self.state.opened_file {
            Some(p) => p.clone(),
            None => {
                self.set_status("No file name set — pass a path on startup to save".to_string());
                return;
            }
        };
        let text = self.state.editor.buffer.to_text();
        match FileManager::save(&path, &text) {
            Ok(()) => {
                self.state.editor.mark_saved();
                self.set_status(format!("Saved {}", path.display()));
                self.emit(AppEvent::FileSaved { path: path.display().to_string() });
            }
            Err(e) => self.set_status(format!("Save failed: {e}")),
        }
    }

    /// Set the status bar message *and* emit the corresponding
    /// `AppEvent::StatusMessage` — the single path every status update
    /// goes through, so the event bus genuinely reflects everything the
    /// user sees rather than being a parallel, partially-used mechanism.
    fn set_status(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.state.set_status(msg.clone());
        self.emit(AppEvent::StatusMessage(msg));
    }

    fn emit(&mut self, event: AppEvent) {
        if self.events.len() >= EVENT_LOG_CAP {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn shutdown(&mut self) {
        self.executor.shutdown();
    }
}
