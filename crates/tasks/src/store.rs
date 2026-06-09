//! Valkey-backed task store. With no relational database in this workspace,
//! Valkey is the source of truth: each task is JSON at `task:{id}` and every id
//! is kept in the `tasks` set so the list endpoint can enumerate them.

use valkey::Cache;

use crate::domain::Task;

const INDEX_KEY: &str = "tasks";

fn task_key(id: &str) -> String {
    format!("task:{id}")
}

/// The task repository. Cheap to [`Clone`] (the cache handle is shared).
#[derive(Clone)]
pub struct TaskStore {
    cache: Cache,
}

impl TaskStore {
    /// Wrap a Valkey cache as a task store.
    #[must_use]
    pub fn new(cache: Cache) -> Self {
        Self { cache }
    }

    /// Persist a task and add it to the index.
    ///
    /// # Errors
    /// Propagates Valkey failures.
    pub async fn create(&self, task: &Task) -> anyhow::Result<()> {
        self.cache.set_json(&task_key(&task.id), task, None).await?;
        self.cache.set_add(INDEX_KEY, &task.id).await?;
        Ok(())
    }

    /// Fetch a task by id, or `None` if it does not exist.
    ///
    /// # Errors
    /// Propagates Valkey failures.
    pub async fn get(&self, id: &str) -> anyhow::Result<Option<Task>> {
        self.cache.get_json(&task_key(id)).await
    }

    /// Return every task, newest first.
    ///
    /// # Errors
    /// Propagates Valkey failures.
    pub async fn list(&self) -> anyhow::Result<Vec<Task>> {
        let ids = self.cache.set_members(INDEX_KEY).await?;
        let mut tasks = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(task) = self.cache.get_json::<Task>(&task_key(&id)).await? {
                tasks.push(task);
            }
        }
        tasks.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        Ok(tasks)
    }

    /// Delete a task, returning whether it existed.
    ///
    /// # Errors
    /// Propagates Valkey failures.
    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let existed = self.get(id).await?.is_some();
        if existed {
            self.cache.del(&task_key(id)).await?;
            self.cache.set_remove(INDEX_KEY, id).await?;
        }
        Ok(existed)
    }
}
