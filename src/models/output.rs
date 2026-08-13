//! `Output` data model: the record of a single command execution.

#[derive(Debug, Clone)]
pub struct Output {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub runtime_ms: u128,
}

impl Output {
    pub fn new(command: impl Into<String>, stdout: String, stderr: String, exit_code: i32, runtime_ms: u128) -> Self {
        Self {
            command: command.into(),
            stdout,
            stderr,
            exit_code,
            runtime_ms,
        }
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}
