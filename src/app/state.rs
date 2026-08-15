//! `AppState` — the heart of the application. Every other module reads
//! from and writes to this struct (through `App`'s controlled API); it is
//! the single source of truth for what's currently on screen.

use crate::editor::Editor;
use crate::models::Output;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Output,
}

pub struct AppState {
    pub editor: Editor,
    pub opened_file: Option<PathBuf>,
    /// Most recent output for a given line id, keyed by line id so the
    /// editor gutter can show per-line status even after reordering.
    pub outputs: HashMap<u64, Output>,
    /// Full chronological run history, newest last.
    pub output_history: Vec<Output>,
    pub status_message: Option<String>,
    pub focus: Focus,
    pub output_scroll: u16,
    pub should_quit: bool,

    // --- Async execution tracking -----------------------------------
    /// The id of the line currently running, if any. `Some` for the
    /// entire lifetime of a run (from dispatch until Finished/Cancelled/
    /// Failed is processed), independent of how the executor thread is
    /// actually progressing.
    pub running_line_id: Option<u64>,
    pub running_command: Option<String>,
    pub running_started_at: Option<Instant>,
    /// Other line ids that are part of the *same* run as
    /// `running_line_id` — non-empty only for a multi-line compound
    /// block (`for`/`while`/`until`/`if`/`case`) run as one unit, where
    /// `running_line_id` is the block's opening line. Empty for a normal
    /// single-line run. All of these get marked Running together and
    /// finalized with the same result together — see
    /// `App::begin_execution` / `App::poll_events`.
    pub running_extra_line_ids: Vec<u64>,
    /// Output streamed so far for the currently-running line. Cleared
    /// once the run finishes (the finalized `Output` takes over display
    /// duties via `output_history`).
    pub live_output: String,

    // --- Editor viewport (scrolling) ---------------------------------
    /// Index of the first buffer line currently visible in the editor
    /// pane. Adjusted by the renderer each frame so the cursor always
    /// stays on screen — see `ui::renderer::draw_editor`.
    pub editor_viewport_top: usize,

    // --- Execution history browsing -----------------------------------
    /// Whether the history browser (Ctrl+R / F6) is currently showing in
    /// the output pane, replacing the live/last-result view.
    pub history_open: bool,
    /// Index into `output_history`, counted as *distance from the most
    /// recent entry* (0 = most recent, 1 = second most recent, ...) —
    /// not a raw `Vec` index. This way the selection stays meaningful as
    /// new runs get appended: "the entry I was looking at" doesn't shift
    /// out from under you just because something new ran elsewhere.
    pub history_selected: usize,

    // --- Batch execution (Ctrl+E: run everything above the cursor) ----
    /// Lines still queued to run after the current one finishes. Empty
    /// outside of a batch run. Cleared (aborting the rest of the batch)
    /// on cancellation or an infrastructure failure — only a normal
    /// `Finished` (regardless of that command's own exit code) advances
    /// to the next queued line. Each entry is (representative line id,
    /// any other line ids in the same step, joined command text) — a
    /// step is more than one line when it's a compound block being run
    /// as a unit, same shape as a normal run's tracking fields.
    pub batch_remaining: std::collections::VecDeque<(u64, Vec<u64>, String)>,
    /// 1-based position of the line currently running within the batch,
    /// and the batch's total line count. `batch_total == 0` means no
    /// batch is in progress (a lone `Ctrl+Enter` run doesn't set these).
    pub batch_position: usize,
    pub batch_total: usize,

    // --- Command-history recall (Alt+Up / Alt+Down) --------------------
    /// Position within `output_history` while recalling, counted the
    /// same way as `history_selected` (0 = most recent). `None` means
    /// not currently recalling.
    pub history_recall_index: Option<usize>,
    /// What the current line contained before recall started, restored
    /// if you recall past the most recent entry back to "nothing
    /// selected" — same behavior as pressing Down past the newest
    /// history entry at a real shell prompt.
    pub history_recall_saved_line: Option<String>,
}

impl AppState {
    pub fn new(editor: Editor, opened_file: Option<PathBuf>) -> Self {
        Self {
            editor,
            opened_file,
            outputs: HashMap::new(),
            output_history: Vec::new(),
            status_message: None,
            focus: Focus::Editor,
            output_scroll: 0,
            should_quit: false,
            running_line_id: None,
            running_command: None,
            running_started_at: None,
            running_extra_line_ids: Vec::new(),
            live_output: String::new(),
            editor_viewport_top: 0,
            history_open: false,
            history_selected: 0,
            batch_remaining: std::collections::VecDeque::new(),
            batch_position: 0,
            batch_total: 0,
            history_recall_index: None,
            history_recall_saved_line: None,
        }
    }

    pub fn is_batch_running(&self) -> bool {
        self.batch_total > 0
    }

    /// The currently-selected history entry, if any (most recent first).
    pub fn selected_history_entry(&self) -> Option<&Output> {
        if self.output_history.is_empty() {
            return None;
        }
        let idx = self.output_history.len() - 1 - self.history_selected.min(self.output_history.len() - 1);
        self.output_history.get(idx)
    }

    pub fn is_running(&self) -> bool {
        self.running_line_id.is_some()
    }

    pub fn record_output(&mut self, line_id: u64, output: Output) {
        self.outputs.insert(line_id, output.clone());
        self.output_history.push(output);
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn begin_run(&mut self, line_id: u64, command: String) {
        self.running_line_id = Some(line_id);
        self.running_command = Some(command);
        self.running_started_at = Some(Instant::now());
        self.live_output.clear();
    }

    pub fn end_run(&mut self) {
        self.running_line_id = None;
        self.running_command = None;
        self.running_started_at = None;
        self.running_extra_line_ids.clear();
        self.live_output.clear();
    }
}
