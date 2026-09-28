//! The server's door for session loops (DESIGN-SESSION-LOOPS §5.1–§5.3, slice L2b): what one
//! connection and one `on_prompt` hand the process's loop runner (L2a), and nothing more.
//!
//! - A connection whose client hears goose's own notifications is a TICK DOOR: the runner's offers
//!   (`loops/tickDue`) and record changes (`loops/changed`) go to that window. It opens at
//!   `initialize` and closes when the connection's serving future ends — however it ends: measured
//!   (Q9, `tests/acp_connection_close_test.rs`), a closed websocket drops that future mid-await, so
//!   the close rides a guard, never code after the await.
//! - A prompt is a loop's TICK only when its `_meta.goose.loopTick` is this chat's open offer
//!   (`Runner::accept_offer`); every other prompt is the user's, whatever its meta says.
//! - A user's prompt holds a named user turn (`turn_priority::user_turn_in`) and tells the runner
//!   when it ends; a tick holds none — a tick that took one would yield to itself.
//! - A tick's turn is ended exactly once. An exit that never says how (its future dropped with the
//!   connection, a panic) still ends it, by name.

use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::{Client, ConnectionTo};
use goose_sdk_types::custom_notifications::{LoopsChangedNotification, LoopsTickDueNotification};
use tracing::warn;

use crate::session_loops::rules::TickEnd;
use crate::session_loops::runner::{DoorGuard, ReviewSlot, Runner, TickDoor, TickRun};
use crate::turn_priority::{self, UserTurn};

/// What a tick whose turn was dropped unfinished reads as (the connection's future dropped, Q9 (b)).
pub(super) const CONNECTION_ENDED: &str =
    "the window's connection to goose ended while the tick ran (a reload, a closed window, or a connection error)";

/// One connection's tick door.
struct ConnectionDoor {
    cx: ConnectionTo<Client>,
}

impl TickDoor for ConnectionDoor {
    fn tick_due(&self, due: &LoopsTickDueNotification) {
        if let Err(error) = self.cx.send_notification(due.clone()) {
            warn!(session_id = %due.session_id, n = due.n, ?error, "loop: the tick offer did not reach this window; it is sent again when a window connects or answers loops/ready");
        }
    }

    fn changed(&self, changed: &LoopsChangedNotification) {
        if let Err(error) = self.cx.send_notification(changed.clone()) {
            warn!(session_id = %changed.session_id, ?error, "loop: a loop change did not reach this window");
        }
    }
}

/// The connection's door slot: empty until `initialize` opens it, emptied when the connection ends.
#[derive(Default)]
pub(super) struct DoorSlot(StdMutex<Option<DoorGuard>>);

impl DoorSlot {
    /// Open this connection's door, once. A client that does not hear goose's own notifications
    /// never gets one: an offer sent there would reach no one, and its door would keep the loops
    /// "attached" after the last window that can run them closed.
    pub(super) fn open(&self, runner: &Runner, cx: &ConnectionTo<Client>, hears_goose: bool) {
        if !hears_goose {
            return;
        }
        let mut slot = self.0.lock().expect("loop door slot poisoned");
        if slot.is_none() {
            *slot = Some(runner.register_door(Arc::new(ConnectionDoor { cx: cx.clone() })));
        }
    }

    pub(super) fn close(&self) {
        let guard = self.0.lock().expect("loop door slot poisoned").take();
        drop(guard);
    }
}

/// A user's prompt (§5.2 steps 1 and 8): a named user turn for the prompt's life — the runner
/// holds a due tick behind it and a running tick yields to it (v1a) — and, when the prompt ends,
/// however it ends, the turn released and THEN the runner told.
pub(super) struct UserPrompt {
    turn: Option<UserTurn<'static>>,
    runner: Option<Runner>,
    session_id: String,
}

impl UserPrompt {
    pub(super) fn begin(runner: Option<Runner>, session_id: &str) -> Self {
        Self {
            turn: Some(turn_priority::user_turn_in(session_id)),
            runner,
            session_id: session_id.to_string(),
        }
    }
}

impl Drop for UserPrompt {
    fn drop(&mut self) {
        drop(self.turn.take());
        if let Some(runner) = &self.runner {
            runner.user_turn_ended(&self.session_id);
        }
    }
}

/// A tick's turn (§5.2 steps 4–7), ended exactly once.
pub(super) struct TickPrompt {
    run: Option<TickRun>,
}

impl TickPrompt {
    pub(super) fn new(run: TickRun) -> Self {
        Self { run: Some(run) }
    }

    /// `tick_ended`; the slot the tick's end-of-turn reviewers go in.
    pub(super) fn ended(mut self, end: TickEnd) -> ReviewSlot {
        self.run
            .take()
            .expect("a tick's turn is ended once")
            .ended(end)
    }

    /// Ended on an exit before the reply ran.
    pub(super) fn failed(self, error_class: &str, error: String) {
        drop(self.ended(TickEnd::Errored {
            error_class: error_class.to_string(),
            error,
        }));
    }
}

impl Drop for TickPrompt {
    fn drop(&mut self) {
        if let Some(run) = self.run.take() {
            let (error_class, error) = if std::thread::panicking() {
                ("panic", "the tick's turn panicked".to_string())
            } else {
                ("connection", CONNECTION_ENDED.to_string())
            };
            drop(run.ended(TickEnd::Errored {
                error_class: error_class.to_string(),
                error,
            }));
        }
    }
}

/// The holder kind the prompt's reply opens with (§5.5): a tick's never reads as a user's reply.
#[cfg(unix)]
pub(super) fn reply_kind(is_tick: bool) -> goose_sidecar::holders::ReplyKind {
    if is_tick {
        goose_sidecar::holders::ReplyKind::Tick
    } else {
        goose_sidecar::holders::ReplyKind::User
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use goose_sdk_types::custom_requests::{
        LoopCadence, LoopNextReason, LoopStatus, LoopStatusReason,
    };

    use super::*;
    use crate::session_loops::agent_sync::testing::{a_chat, a_loop, store, stored};
    use crate::session_loops::owner::{self, SystemProcesses};
    use crate::session_loops::runner::{CheckPath, RunnerDeps, SystemClock};
    use crate::turn_priority::TurnPriority;

    #[cfg(unix)]
    #[test]
    fn a_ticks_reply_is_a_tick_holder_and_every_other_reply_a_persons() {
        use goose_sidecar::holders::ReplyKind;
        assert_eq!(reply_kind(true), ReplyKind::Tick);
        assert_eq!(reply_kind(false), ReplyKind::User);
    }

    /// §5.2 step 8: a person's prompt names its chat while it runs, and when it ends — however it
    /// ends — the turn is released first and then the runner is told, so the loop whose question
    /// that turn answered goes on.
    #[tokio::test]
    async fn a_persons_prompt_names_its_chat_and_its_end_reaches_the_runner() {
        let (dir, sessions, chat) = a_chat().await;
        let mut asked = a_loop(LoopCadence::SelfPaced, 1, false);
        asked.status = LoopStatus::NeedsYou;
        asked.status_reason = Some(LoopStatusReason::AnswerRunning { n: 1 });
        store(&sessions, &chat, asked).await;
        let runner = Runner::new(RunnerDeps {
            sessions: sessions.clone(),
            clock: Arc::new(SystemClock),
            processes: Arc::new(SystemProcesses),
            me: owner::this_process(&SystemProcesses).unwrap(),
            turns: Box::leak(Box::new(TurnPriority::new())),
            logs_dir: dir.path().join("logs"),
            check_path: CheckPath::Inherited,
        });

        let prompt = UserPrompt::begin(Some(runner.clone()), &chat);
        assert!(
            turn_priority::global().running().sessions.contains(&chat),
            "the person's turn names its chat"
        );
        assert_eq!(
            stored(&sessions, &chat).await.unwrap().status,
            LoopStatus::NeedsYou,
            "nothing moves while the answer's turn runs"
        );
        drop(prompt);
        assert!(!turn_priority::global().running().sessions.contains(&chat));

        let mut waited = 0;
        loop {
            let rec = stored(&sessions, &chat).await.unwrap();
            if rec.status == LoopStatus::Waiting {
                assert_eq!(
                    rec.next_tick.map(|next| next.reason),
                    Some(LoopNextReason::AfterYourAnswer)
                );
                break;
            }
            waited += 1;
            assert!(waited < 500, "the runner never heard the turn end: {rec:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
