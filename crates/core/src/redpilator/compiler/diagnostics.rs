//! Caller-facing errors, recovery hints and compile statistics.
use crate::redpilator::scratch::ScratchError;
use std::time::Duration;

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Unsupported(ScratchError),
    UnsupportedComponents { first: Vec<ScratchError>, total: usize },
    Cancelled,
}

impl std::fmt::Display for CompileError {
    /// Format a compilation rejection for the caller.
    ///
    /// Inputs: The error variant and destination formatter.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => f.write_str("reset the active compiler before recompiling"),
            Self::Unsupported(error) => {
                if let Some(position) = error.position {
                    write!(f, "Cannot compile at {position}: {}", error.reason)
                } else {
                    f.write_str(error.reason)
                }
            }
            Self::UnsupportedComponents { first, total } => {
                for (index, error) in first.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    if let Some(pos) = error.position {
                        write!(f, "{} at ({}, {}, {})", error.reason, pos.x, pos.y, pos.z)?;
                    } else {
                        f.write_str(error.reason)?;
                    }
                }
                if *total > first.len() {
                    write!(f, "; and {} other unsupported LLVM components", total - first.len())?;
                }
                Ok(())
            }
            Self::Cancelled => f.write_str("compilation cancelled"),
        }
    }
}

impl std::error::Error for CompileError {}

impl CompileError {
    /// Choose a useful next action after compilation fails.
    ///
    /// Inputs: The structured compilation error and its reason text.
    ///
    /// Match known backend, entry-state and capture-boundary failures to the corresponding recovery
    /// message.
    pub(crate) fn recovery_hint(&self) -> Option<&'static str> {
        use crate::messages;
        let reason = match self {
            Self::UnsupportedComponents { .. } => return Some(messages::REDPILATOR_RETRY_RUST),
            Self::Unsupported(error) => error.reason,
            _ => return None,
        };
        if reason.starts_with("LLVM does not support") {
            Some(messages::REDPILATOR_RETRY_RUST)
        } else if reason.starts_with("LLVM cannot compile during")
            || reason == "LLVM cannot import pending piston work; let it settle before compiling"
            || matches!(
                reason,
                "scratch entry is mid-tick"
                    | "scratch entry has queued piston work"
                    | "scratch entry has piston motion"
                    | "scratch entry has pending movement work"
            )
        {
            Some(messages::REDPILATOR_RETRY_SETTLED)
        } else if matches!(
            reason,
            "LLVM DLL ABI/version mismatch"
                | "LLVM module program identity mismatch"
                | "LLVM DLL is missing a required export"
                | "LLVM module is missing a required export"
                | "could not load LLVM DLL (set MCHPRS_LLVM_DLL)"
                | "could not load LLVM module"
        ) {
            Some(messages::REDPILATOR_REBUILD_LLVM)
        } else if reason.starts_with("LLVM requires an idle start") {
            Some(messages::REDPILATOR_RETRY_SETTLED)
        } else if reason.contains("outside compiled region") || reason.contains("outside captured boundary") {
            Some(messages::REDPILATOR_CAPTURE_WHOLE_CIRCUIT)
        } else if matches!(
            reason,
            "invalid native Program descriptor" | "LLVM rejected the sampled Program" | "LLVM state size rejected"
        ) {
            Some(messages::REDPILATOR_INTERNAL_FAILURE_HELP)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionError(
    /// underlying runtime failure, including its optional world position and reason.
    pub(super) ScratchError,
);

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(position) = self.0.position {
            write!(f, "Compiled execution failed at {position}: {}", self.0.reason)
        } else {
            write!(f, "Compiled execution failed: {}", self.0.reason)
        }
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Debug, Clone)]
pub struct CompileStatistics {
    /// number of compiled redstone components.
    pub components: usize,
    /// number of host scheduled ticks copied into the compiled runtime.
    pub pending_ticks: usize,
    /// elapsed compile time.
    pub duration: Duration,
    /// Whether a prepared program or an exported executor checkpoint was reused.
    pub cached: bool,
}

impl CompileStatistics {
    pub fn chat_summary_lines(&self) -> Vec<String> {
        vec![format!(
            "Redpilator {} {} components; {} pending ticks; {:?}",
            if self.cached { "reused" } else { "compiled" },
            self.components,
            self.pending_ticks,
            self.duration
        )]
    }
}
