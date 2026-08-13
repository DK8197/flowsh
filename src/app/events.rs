//! Simple application event bus. Every module that produces something the
//! rest of the app cares about (execution progress, a file save, a
//! status change) emits an [`AppEvent`] rather than reaching into other
//! modules' internals directly. `App` is the sole consumer: it turns
//! executor events into `AppEvent`s, applies them to `AppState`, and
//! keeps a bounded log of them for observability.

use crate::executor::CancelReason;
use crate::models::Output;

#[derive(Debug, Clone)]
pub enum AppEvent {
    ExecutionStarted { line_id: u64, command: String },
    ExecutionOutputChunk { line_id: u64, chunk: String },
    ExecutionFinished { line_id: u64, output: Output },
    ExecutionCancelled { line_id: u64, reason: CancelReason },
    ExecutionFailed { line_id: u64, message: String },
    FileSaved { path: String },
    FileLoaded { path: String },
    StatusMessage(String),
}
