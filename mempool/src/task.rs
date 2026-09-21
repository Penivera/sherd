use wire::{NodeId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Available,
    Claimed { by: NodeId, claimed_at: u64 },
    Completed { completed_at: u64, result: Vec<u8> },
    Failed { failed_at: u64, reason: String },
}

impl TaskState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Available => "Available",
            Self::Claimed { .. } => "Claimed",
            Self::Completed { .. } => "Completed",
            Self::Failed { .. } => "Failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub author: NodeId,
    pub priority: u32,
    pub created_at: u64,
    pub payload: Vec<u8>,
    pub state: TaskState,
}

impl Task {
    /// Create a new available task with canonical content-addressed TaskId.
    pub fn new_available(
        author: NodeId,
        priority: u32,
        created_at: u64,
        payload: Vec<u8>,
    ) -> Self {
        let id = TaskId::compute(&author, priority, created_at, &payload);
        Self {
            id,
            author,
            priority,
            created_at,
            payload,
            state: TaskState::Available,
        }
    }

    /// Create a pending task representation (payload yet to be received).
    pub fn new_pending(id: TaskId, author: NodeId, priority: u32, created_at: u64) -> Self {
        Self {
            id,
            author,
            priority,
            created_at,
            payload: Vec::new(),
            state: TaskState::Pending,
        }
    }
}
