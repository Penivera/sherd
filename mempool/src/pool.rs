use std::collections::HashMap;
use std::sync::RwLock;

use wire::{NodeId, TaskId};
use crate::error::MempoolError;
use crate::task::{Task, TaskState};

/// Thread-safe in-memory task pool providing atomic state transitions,
/// duplicate suppression, and deterministic task ordering.
pub struct Mempool {
    tasks: RwLock<HashMap<TaskId, Task>>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
        }
    }

    /// Submit a task into the local mempool.
    /// Rejects duplicate submissions if the task is already present and non-pending.
    pub fn submit(&self, task: Task) -> Result<TaskId, MempoolError> {
        let mut lock = self.tasks.write().unwrap();
        let id = task.id;

        if let Some(existing) = lock.get_mut(&id) {
            if existing.state == TaskState::Pending && task.state == TaskState::Available {
                *existing = task;
                return Ok(id);
            }
            return Err(MempoolError::DuplicateTask(id));
        }

        lock.insert(id, task);
        Ok(id)
    }

    /// Retrieve a clone of a task by its TaskId.
    pub fn get(&self, task_id: &TaskId) -> Option<Task> {
        let lock = self.tasks.read().unwrap();
        lock.get(task_id).cloned()
    }

    /// Check if a TaskId is known in the mempool.
    pub fn contains(&self, task_id: &TaskId) -> bool {
        let lock = self.tasks.read().unwrap();
        lock.contains_key(task_id)
    }

    /// Total count of tasks tracked in the mempool across all states.
    pub fn len(&self) -> usize {
        let lock = self.tasks.read().unwrap();
        lock.len()
    }

    pub fn is_empty(&self) -> bool {
        let lock = self.tasks.read().unwrap();
        lock.is_empty()
    }

    /// Retrieve all tasks currently in the `Available` state, ordered deterministically by:
    /// 1. Priority (highest first)
    /// 2. Creation time (oldest first)
    /// 3. TaskId (lexicographically ascending)
    pub fn list_available(&self) -> Vec<Task> {
        let lock = self.tasks.read().unwrap();
        let mut available: Vec<Task> = lock
            .values()
            .filter(|t| t.state == TaskState::Available)
            .cloned()
            .collect();

        available.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });

        available
    }

    /// Atomically transition a task from `Available` to `Claimed`.
    pub fn claim(&self, task_id: &TaskId, claimant: NodeId, at: u64) -> Result<(), MempoolError> {
        let mut lock = self.tasks.write().unwrap();
        let task = lock.get_mut(task_id).ok_or(MempoolError::TaskNotFound(*task_id))?;

        match &task.state {
            TaskState::Available => {
                task.state = TaskState::Claimed {
                    by: claimant,
                    claimed_at: at,
                };
                Ok(())
            }
            TaskState::Claimed { by, .. } => Err(MempoolError::AlreadyClaimed {
                task_id: *task_id,
                claimed_by: *by,
            }),
            other => Err(MempoolError::InvalidStateTransition {
                task_id: *task_id,
                current: other.name().into(),
                attempted: "Claim",
            }),
        }
    }

    /// Complete a task with its execution result data.
    pub fn complete(&self, task_id: &TaskId, result: Vec<u8>, at: u64) -> Result<(), MempoolError> {
        let mut lock = self.tasks.write().unwrap();
        let task = lock.get_mut(task_id).ok_or(MempoolError::TaskNotFound(*task_id))?;

        match &task.state {
            TaskState::Claimed { .. } => {
                task.state = TaskState::Completed {
                    completed_at: at,
                    result,
                };
                Ok(())
            }
            other => Err(MempoolError::InvalidStateTransition {
                task_id: *task_id,
                current: other.name().into(),
                attempted: "Complete",
            }),
        }
    }

    /// Mark a task as failed with an explanatory reason.
    pub fn fail(&self, task_id: &TaskId, reason: String, at: u64) -> Result<(), MempoolError> {
        let mut lock = self.tasks.write().unwrap();
        let task = lock.get_mut(task_id).ok_or(MempoolError::TaskNotFound(*task_id))?;

        match &task.state {
            TaskState::Available | TaskState::Claimed { .. } => {
                task.state = TaskState::Failed {
                    failed_at: at,
                    reason,
                };
                Ok(())
            }
            other => Err(MempoolError::InvalidStateTransition {
                task_id: *task_id,
                current: other.name().into(),
                attempted: "Fail",
            }),
        }
    }

    /// Attach payload data to a previously `Pending` task, moving it to `Available`.
    pub fn set_payload(&self, task_id: &TaskId, payload: Vec<u8>) -> Result<(), MempoolError> {
        let mut lock = self.tasks.write().unwrap();
        let task = lock.get_mut(task_id).ok_or(MempoolError::TaskNotFound(*task_id))?;

        let expected_hash = TaskId::compute(&task.author, task.priority, task.created_at, &payload);
        if expected_hash != *task_id {
            return Err(MempoolError::InvalidTaskHash {
                expected: *task_id,
                actual: expected_hash,
            });
        }

        task.payload = payload;
        task.state = TaskState::Available;
        Ok(())
    }
}
