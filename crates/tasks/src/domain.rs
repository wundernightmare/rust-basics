//! The tasks service's core model — no HTTP, Valkey or Kafka knowledge here.

use serde::{Deserialize, Serialize};

/// A single to-do item. Persisted as JSON in Valkey under `task:{id}`, with the
/// id also held in the `tasks` set so the list endpoint can enumerate them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub done: bool,
    /// Creation time as Unix epoch seconds (chrono-free to keep deps minimal).
    pub created_at: u64,
}

/// The event published to Kafka when a task is created. It is the contract
/// `crates/consumer` decodes; keep it backwards-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreatedEvent {
    pub id: String,
    pub title: String,
    pub created_at: u64,
}

impl From<&Task> for TaskCreatedEvent {
    fn from(t: &Task) -> Self {
        Self {
            id: t.id.clone(),
            title: t.title.clone(),
            created_at: t.created_at,
        }
    }
}
