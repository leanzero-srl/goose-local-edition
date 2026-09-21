//! The in-memory task table: bounded (64 tasks), finished tasks evicted after an hour. Only
//! reconciliation data lives here — status, sequence, timestamps, the session id — never an
//! event's content.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const TASK_CAP: usize = 64;
pub const FINISHED_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        self != Self::Running
    }
}

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: String,
    pub session_id: String,
    pub thread_id: String,
    pub status: TaskStatus,
    pub seq: u64,
    pub created_at: DateTime<Utc>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub finished_at: Option<Instant>,
    pub cancel: CancellationToken,
}

impl TaskRecord {
    pub fn new(id: String, session_id: String, thread_id: String) -> Self {
        Self {
            id,
            session_id,
            thread_id,
            status: TaskStatus::Running,
            seq: 0,
            created_at: Utc::now(),
            last_event_at: None,
            finished_at: None,
            cancel: CancellationToken::new(),
        }
    }
}

#[derive(Debug)]
pub struct RegistryFull;

pub struct TaskRegistry {
    tasks: Mutex<HashMap<String, TaskRecord>>,
    cap: usize,
    finished_ttl: Duration,
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new(TASK_CAP, FINISHED_TTL)
    }
}

impl TaskRegistry {
    pub fn new(cap: usize, finished_ttl: Duration) -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            cap,
            finished_ttl,
        }
    }

    fn evict_expired(tasks: &mut HashMap<String, TaskRecord>, ttl: Duration, now: Instant) {
        tasks.retain(|_, task| {
            task.finished_at
                .is_none_or(|finished| now.duration_since(finished) < ttl)
        });
    }

    pub fn insert(&self, record: TaskRecord) -> Result<(), RegistryFull> {
        self.insert_at(record, Instant::now())
    }

    pub fn insert_at(&self, record: TaskRecord, now: Instant) -> Result<(), RegistryFull> {
        let mut tasks = self.tasks.lock().unwrap();
        Self::evict_expired(&mut tasks, self.finished_ttl, now);
        if tasks.len() >= self.cap {
            return Err(RegistryFull);
        }
        tasks.insert(record.id.clone(), record);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<TaskRecord> {
        self.tasks.lock().unwrap().get(id).cloned()
    }

    pub fn len(&self) -> usize {
        self.tasks.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The next sequence number for `id`, stamped as the event's `seq` and recorded as the last
    /// event time. `None` when the task is unknown.
    pub fn next_seq(&self, id: &str, at: DateTime<Utc>) -> Option<u64> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks.get_mut(id)?;
        task.seq += 1;
        task.last_event_at = Some(at);
        Some(task.seq)
    }

    pub fn set_status(&self, id: &str, status: TaskStatus) {
        self.set_status_at(id, status, Instant::now());
    }

    pub fn set_status_at(&self, id: &str, status: TaskStatus, now: Instant) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(id) {
            task.status = status;
            if status.is_terminal() && task.finished_at.is_none() {
                task.finished_at = Some(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> TaskRecord {
        TaskRecord::new(id.into(), format!("s-{id}"), "thread".into())
    }

    #[test]
    fn the_table_is_bounded_and_finished_tasks_expire() {
        let registry = TaskRegistry::new(2, Duration::from_secs(60));
        let t0 = Instant::now();
        registry.insert_at(record("a"), t0).unwrap();
        registry.insert_at(record("b"), t0).unwrap();
        assert!(registry.insert_at(record("c"), t0).is_err());

        registry.set_status_at("a", TaskStatus::Done, t0);
        assert!(registry
            .insert_at(record("c"), t0 + Duration::from_secs(59))
            .is_err());
        registry
            .insert_at(record("c"), t0 + Duration::from_secs(60))
            .unwrap();
        assert!(registry.get("a").is_none());
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn seq_is_monotonic_and_stamps_the_last_event_time() {
        let registry = TaskRegistry::default();
        registry.insert(record("a")).unwrap();
        let at = Utc::now();
        assert_eq!(registry.next_seq("a", at), Some(1));
        assert_eq!(registry.next_seq("a", at), Some(2));
        assert_eq!(registry.next_seq("missing", at), None);
        let task = registry.get("a").unwrap();
        assert_eq!(task.seq, 2);
        assert_eq!(task.last_event_at, Some(at));
        assert_eq!(task.status, TaskStatus::Running);
    }

    #[test]
    fn a_terminal_status_is_stamped_once() {
        let registry = TaskRegistry::default();
        registry.insert(record("a")).unwrap();
        let t0 = Instant::now();
        registry.set_status_at("a", TaskStatus::Cancelled, t0);
        registry.set_status_at("a", TaskStatus::Done, t0 + Duration::from_secs(5));
        let task = registry.get("a").unwrap();
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.finished_at, Some(t0));
    }
}
