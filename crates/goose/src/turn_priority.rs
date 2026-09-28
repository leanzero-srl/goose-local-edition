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

#[derive(Debug, Clone, Default)]
struct Turns {
    running: usize,
    /// Bumped by every turn that starts, so a check can tell "a turn began while I ran" from
    /// "the same turns are still running".
    started: u64,
    /// The chats of the running turns that named theirs (`user_turn_in`), one entry per turn.
    sessions: Vec<String>,
    /// The chat of the turn that started last, when it named one.
    last_started: Option<String>,
}

pub struct TurnPriority {
    turns: watch::Sender<Turns>,
}

/// A user turn in progress; the checks wait while any exists.
pub struct UserTurn<'a> {
    priority: &'a TurnPriority,
    session: Option<String>,
}

impl Drop for UserTurn<'_> {
    fn drop(&mut self) {
        let session = self.session.take();
        self.priority.turns.send_modify(|t| {
            t.running -= 1;
            if let Some(at) = session
                .as_ref()
                .and_then(|session| t.sessions.iter().position(|s| s == session))
            {
                t.sessions.remove(at);
            }
        });
    }
}

/// The user turns running now: how many, the start count, and the chats of those that named
/// theirs (a turn opened with `user_turn` is counted but not named).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTurns {
    pub running: usize,
    pub started: u64,
    pub sessions: Vec<String>,
}

/// A user turn that started after a mark: its chat, when it named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedTurn {
    pub session: Option<String>,
}

impl TurnPriority {
    pub fn new() -> Self {
        Self {
            turns: watch::channel(Turns::default()).0,
        }
    }

    fn begin(&self, session: Option<String>) -> UserTurn<'_> {
        self.turns.send_modify(|t| {
            t.running += 1;
            t.started += 1;
            t.last_started = session.clone();
            if let Some(session) = &session {
                t.sessions.push(session.clone());
            }
        });
        UserTurn {
            priority: self,
            session,
        }
    }

    pub fn user_turn(&self) -> UserTurn<'_> {
        self.begin(None)
    }

    /// A user turn in the chat `session_id`, so the loop runner can say whose turn a due tick
    /// waits for, or a running tick yielded to.
    pub fn user_turn_in(&self, session_id: &str) -> UserTurn<'_> {
        self.begin(Some(session_id.to_string()))
    }

    pub fn running(&self) -> RunningTurns {
        let t = self.turns.borrow();
        RunningTurns {
            running: t.running,
            started: t.started,
            sessions: t.sessions.clone(),
        }
    }

    /// Returns once no user turn runs, with the start count at that moment: the mark
    /// `user_turn_started_since` compares against.
    pub async fn wait_no_user_turn(&self) -> u64 {
        self.turns
            .subscribe()
            .wait_for(|t| t.running == 0)
            .await
            .map(|t| t.started)
            .expect("the sender lives as long as self")
    }

    /// Returns once a user turn has started after the mark `started`.
    pub async fn user_turn_started_since(&self, started: u64) -> StartedTurn {
        self.turns
            .subscribe()
            .wait_for(|t| t.started != started)
            .await
            .map(|t| StartedTurn {
                session: t.last_started.clone(),
            })
            .expect("the sender lives as long as self")
    }

    /// Runs `call` when no user turn runs; a user turn that starts while it runs drops it, and it
    /// is called again once that turn (and any other) has ended.
    pub async fn after_user_turns<T, Fut>(&self, what: &str, mut call: impl FnMut() -> Fut) -> T
    where
        Fut: Future<Output = T>,
    {
        loop {
            let started = self.wait_no_user_turn().await;
            tokio::select! {
                out = call() => return out,
                _ = self.user_turn_started_since(started) => {
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

/// This process's turn priority, the one `on_prompt`, the reviewers and the loop runner share.
pub fn global() -> &'static TurnPriority {
    &PRIORITY
}

/// Held by the ACP prompt handler for the life of a user's turn.
pub fn user_turn() -> UserTurn<'static> {
    PRIORITY.user_turn()
}

/// Held by the ACP prompt handler for the life of a user's turn in `session_id`.
pub fn user_turn_in(session_id: &str) -> UserTurn<'static> {
    PRIORITY.user_turn_in(session_id)
}

pub async fn wait_no_user_turn() -> u64 {
    PRIORITY.wait_no_user_turn().await
}

pub async fn user_turn_started_since(started: u64) -> StartedTurn {
    PRIORITY.user_turn_started_since(started).await
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

    /// The two halves the loop runner uses (Q-228 L2a): "wait until no user turn runs", then "a
    /// user turn started since that mark" — naming the chat when the turn named it.
    #[tokio::test]
    async fn the_halves_name_the_chat_of_a_turn_that_names_it() {
        let priority = TurnPriority::new();
        let unnamed = priority.user_turn();
        let named = priority.user_turn_in("chat-b");
        assert_eq!(
            priority.running(),
            RunningTurns {
                running: 2,
                started: 2,
                sessions: vec!["chat-b".to_string()],
            }
        );
        let idle = priority.wait_no_user_turn();
        tokio::pin!(idle);
        for _ in 0..50 {
            tokio::select! {
                _ = &mut idle => panic!("no-turn returned while two turns run"),
                _ = tokio::task::yield_now() => {}
            }
        }
        drop(named);
        assert_eq!(priority.running().sessions, Vec::<String>::new());
        drop(unnamed);
        let mark = idle.await;
        assert_eq!(mark, 2);

        let since = priority.user_turn_started_since(mark);
        tokio::pin!(since);
        for _ in 0..50 {
            tokio::select! {
                _ = &mut since => panic!("started-since returned with no new turn"),
                _ = tokio::task::yield_now() => {}
            }
        }
        let _turn = priority.user_turn_in("chat-c");
        assert_eq!(
            since.await,
            StartedTurn {
                session: Some("chat-c".to_string())
            }
        );
        let _unnamed = priority.user_turn();
        assert_eq!(
            priority.user_turn_started_since(mark + 1).await,
            StartedTurn { session: None },
            "an unnamed turn is never reported as the named one before it"
        );
    }
}
