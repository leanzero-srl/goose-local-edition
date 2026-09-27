//! Q-185: goose's model calls FOR a session that are not the answer being written — the
//! end-of-turn fact check, the memory review, the title, tool labels, compaction — each tagged
//! with its kind where the call is made.
//!
//! E2E #3i (3.0.60, 2026-09-27 17:23:25Z): the reply was done and the fact checker read its
//! 1,094-token prompt on the split while the Engine card said "Serving · Chat · <session>", Run it
//! warned it "cuts the answer being written", and the sidebar showed the session idle. Nothing
//! told the surfaces what the call was: the lease carried only the session id. Now the call's kind
//! rides a task-local the swarm router reads at its lease (so the MLX serving list names it), and
//! the call is listed here for its whole life (so `session_activity/get` names it for any
//! provider). One tag, set once, read everywhere.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};

pub use goose_sdk_types::custom_requests::BackgroundWorkKind;

tokio::task_local! {
    static KIND: BackgroundWorkKind;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundCall {
    pub session_id: String,
    pub kind: BackgroundWorkKind,
    pub started_at: DateTime<Utc>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static IN_FLIGHT: LazyLock<Mutex<BTreeMap<u64, BackgroundCall>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

struct Listed(u64);

impl Drop for Listed {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// Runs `call` as `kind` work for `session_id`: the session context carries the id (the router's
/// lease names the session), the kind rides the task (the lease names the work), and the call is
/// listed until it ends or is dropped.
pub async fn run<F>(kind: BackgroundWorkKind, session_id: &str, call: F) -> F::Output
where
    F: Future,
{
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        BackgroundCall {
            session_id: session_id.to_string(),
            kind,
            started_at: Utc::now(),
        },
    );
    let _listed = Listed(id);
    KIND.scope(
        kind,
        crate::session_context::with_session_id(Some(session_id.to_string()), call),
    )
    .await
}

/// The kind of background work the current task runs; `None` = the session's own turn (or no
/// session at all).
pub fn current_kind() -> Option<BackgroundWorkKind> {
    KIND.try_with(|kind| *kind).ok()
}

/// Every background call in flight, oldest first.
pub fn snapshot() -> Vec<BackgroundCall> {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed_for(session: &str) -> Vec<BackgroundWorkKind> {
        snapshot()
            .into_iter()
            .filter(|c| c.session_id == session)
            .map(|c| c.kind)
            .collect()
    }

    #[tokio::test]
    async fn a_call_is_listed_and_tagged_exactly_while_it_runs() {
        let session = "20260927_q185_listed";
        assert!(listed_for(session).is_empty());
        let seen = run(BackgroundWorkKind::FactCheck, session, async {
            (
                current_kind(),
                crate::session_context::current_session_id(),
                listed_for(session),
            )
        })
        .await;
        assert_eq!(seen.0, Some(BackgroundWorkKind::FactCheck));
        assert_eq!(seen.1.as_deref(), Some(session));
        assert_eq!(seen.2, vec![BackgroundWorkKind::FactCheck]);
        assert!(listed_for(session).is_empty());
        assert_eq!(current_kind(), None);
    }

    #[tokio::test]
    async fn a_dropped_call_leaves_the_list() {
        let session = "20260927_q185_dropped";
        let (entered, wait) = tokio::sync::oneshot::channel::<()>();
        let call = tokio::spawn(run(BackgroundWorkKind::Title, session, async move {
            entered.send(()).unwrap();
            std::future::pending::<()>().await
        }));
        wait.await.unwrap();
        assert_eq!(listed_for(session), vec![BackgroundWorkKind::Title]);
        call.abort();
        let _ = call.await;
        assert!(listed_for(session).is_empty());
    }
}
