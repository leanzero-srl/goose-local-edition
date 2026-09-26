//! Q-132: a user's turn goes before goose's own end-of-turn checks.
//!
//! The end-of-turn reviewer (the memory assessment and the "goose check:" answer check) runs
//! detached after a turn ends, on the same model the user chats with. On 3.0.49 (2026-09-26
//! 14:28–14:33, the Flash pipeline split) the answer check held the engine for 278+ s, and an
//! engine that batches statically (the pipeline split: "requests arriving mid-batch wait for the
//! next one") makes the user's next turn wait for the check's whole batch. goose's router did not
//! queue that turn — the node had free slots — the ENGINE did, so the yield has to happen here:
//! a user turn that starts drops the check's in-flight call (the stream closes, the engine cancels
//! the row and ends its batch), and the check is asked again, from the start, once no user turn
//! runs. Ordering only: no clock and no count decides when the check runs or stops.

use std::future::Future;
use std::sync::LazyLock;

use tokio::sync::watch;

#[derive(Debug, Clone, Copy, Default)]
struct Turns {
    running: usize,
    /// Bumped by every turn that starts, so a check can tell "a turn began while I ran" from
    /// "the same turns are still running".
    started: u64,
}

pub struct TurnPriority {
    turns: watch::Sender<Turns>,
}

/// A user turn in progress; the checks wait while any exists.
pub struct UserTurn<'a> {
    priority: &'a TurnPriority,
}

impl Drop for UserTurn<'_> {
    fn drop(&mut self) {
        self.priority.turns.send_modify(|t| t.running -= 1);
    }
}

impl TurnPriority {
    pub fn new() -> Self {
        Self {
            turns: watch::channel(Turns::default()).0,
        }
    }

    pub fn user_turn(&self) -> UserTurn<'_> {
        self.turns.send_modify(|t| {
            t.running += 1;
            t.started += 1;
        });
        UserTurn { priority: self }
    }

    /// Runs `call` when no user turn runs; a user turn that starts while it runs drops it, and it
    /// is called again once that turn (and any other) has ended.
    pub async fn after_user_turns<T, Fut>(&self, what: &str, mut call: impl FnMut() -> Fut) -> T
    where
        Fut: Future<Output = T>,
    {
        let mut turns = self.turns.subscribe();
        loop {
            let started = turns
                .wait_for(|t| t.running == 0)
                .await
                .map(|t| t.started)
                .expect("the sender lives as long as self");
            tokio::select! {
                out = call() => return out,
                _ = turns.wait_for(|t| t.started != started) => {
                    tracing::info!(what, "yielded to a user turn; asked again once no turn runs");
                }
            }
        }
    }
}

impl Default for TurnPriority {
    fn default() -> Self {
        Self::new()
    }
}

static PRIORITY: LazyLock<TurnPriority> = LazyLock::new(TurnPriority::new);

/// Held by the ACP prompt handler for the life of a user's turn.
pub fn user_turn() -> UserTurn<'static> {
    PRIORITY.user_turn()
}

pub async fn after_user_turns<T, Fut>(what: &str, call: impl FnMut() -> Fut) -> T
where
    Fut: Future<Output = T>,
{
    PRIORITY.after_user_turns(what, call).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    /// The pipeline split's shape: ONE batch at a time. The check holds it for the life of its
    /// call (it never finishes on its own, like a 5,295-token reasoning run); the user's turn must
    /// get it without waiting for the check to end, and the check runs again after the turn.
    #[tokio::test]
    async fn a_user_turn_is_not_queued_behind_the_check() {
        let priority = Arc::new(TurnPriority::new());
        let engine = Arc::new(Semaphore::new(1));
        let asked = Arc::new(AtomicUsize::new(0));
        let finish = Arc::new(tokio::sync::Notify::new());

        let check = {
            let (priority, engine, asked, finish) = (
                priority.clone(),
                engine.clone(),
                asked.clone(),
                finish.clone(),
            );
            tokio::spawn(async move {
                priority
                    .after_user_turns("answer check", || {
                        let (engine, asked, finish) =
                            (engine.clone(), asked.clone(), finish.clone());
                        async move {
                            let _batch = engine.acquire_owned().await.unwrap();
                            let n = asked.fetch_add(1, Ordering::SeqCst) + 1;
                            finish.notified().await;
                            n
                        }
                    })
                    .await
            })
        };
        while asked.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(engine.available_permits(), 0, "the check holds the engine");

        let turn = priority.user_turn();
        let users_batch = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            engine.clone().acquire_owned(),
        )
        .await
        .expect("the user's turn waited behind the check")
        .unwrap();
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "the check is not asked again while the user's turn runs"
        );

        drop(users_batch);
        drop(turn);
        while asked.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        finish.notify_one();
        assert_eq!(
            check.await.unwrap(),
            2,
            "the check ran to its end after the turn"
        );
    }

    #[tokio::test]
    async fn a_check_waits_for_a_turn_already_running_and_runs_once() {
        let priority = TurnPriority::new();
        let asked = AtomicUsize::new(0);
        let turn = priority.user_turn();
        let check = priority.after_user_turns("assessment", || async {
            asked.fetch_add(1, Ordering::SeqCst);
        });
        tokio::pin!(check);
        for _ in 0..50 {
            tokio::select! {
                _ = &mut check => panic!("the check ran during the user's turn"),
                _ = tokio::task::yield_now() => {}
            }
        }
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        drop(turn);
        check.await;
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }
}
