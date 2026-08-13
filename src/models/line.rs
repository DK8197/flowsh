//! Core `Line` data model: a single line of the buffer along with the
//! metadata describing the outcome of its most recent execution.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineStatus {
    /// Never executed.
    Idle,
    /// Currently running in the PTY.
    Running,
    /// Finished with exit code 0.
    Success,
    /// Finished with a non-zero exit code.
    Failed,
    /// Interrupted via Ctrl+C or an auto-timeout before it finished.
    Cancelled,
}

impl Default for LineStatus {
    fn default() -> Self {
        LineStatus::Idle
    }
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    pub id: u64,
    pub text: String,
    pub status: LineStatus,
    pub last_output: Option<String>,
    pub exit_code: Option<i32>,
    pub runtime_ms: Option<u128>,
}

impl Line {
    pub fn new(id: u64, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            status: LineStatus::Idle,
            last_output: None,
            exit_code: None,
            runtime_ms: None,
        }
    }
}
