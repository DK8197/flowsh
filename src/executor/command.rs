//! Small supporting types for the execution lifecycle.

use std::fmt;

/// Why a running command was interrupted before it finished naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The user pressed Ctrl+C.
    UserRequested,
    /// The command ran longer than [`crate::executor::executor::MAX_RUNTIME`]
    /// and was auto-interrupted as a safety net.
    Timeout,
}

impl fmt::Display for CancelReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CancelReason::UserRequested => write!(f, "cancelled"),
            CancelReason::Timeout => write!(f, "timed out"),
        }
    }
}
