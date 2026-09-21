use mesh::NetworkError;
use wire::{NodeId, TaskId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MempoolError {
    #[error("Task not found: {0}")]
    TaskNotFound(TaskId),
    #[error("Duplicate task already exists: {0}")]
    DuplicateTask(TaskId),
    #[error("Invalid task state transition for {task_id}: current state {current:?}, attempted {attempted}")]
    InvalidStateTransition {
        task_id: TaskId,
        current: String,
        attempted: &'static str,
    },
    #[error("Task {task_id} is already claimed by {claimed_by}")]
    AlreadyClaimed {
        task_id: TaskId,
        claimed_by: NodeId,
    },
    #[error("Task claim rejected: {0}")]
    ClaimRejected(String),
    #[error("Task content hash mismatch: expected {expected}, actual {actual}")]
    InvalidTaskHash { expected: TaskId, actual: TaskId },
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Timed out waiting for response")]
    Timeout,
}
