use super::OwnerRunRegistry;
use crate::session::Session;
use agent_client_protocol::schema::v1::{
    SessionId, SessionInfoUpdate, SessionNotification, SessionUpdate,
};
use agent_client_protocol::{Client, ConnectionTo};
use goose_sdk_types::custom_notifications::GooseSessionNotification;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct OwnerObservers {
    sessions: HashMap<String, HashMap<Uuid, Observer>>,
    active: HashMap<String, RunReplay>,
}

struct Observer {
    cx: ConnectionTo<Client>,
    replay: Option<Vec<OwnerNotification>>,
    supports_custom: bool,
}

struct RunReplay {
    run_id: Uuid,
    baseline: Session,
    notifications: Vec<OwnerNotification>,
}

pub(crate) struct ObserverReplay {
    registry: Arc<OwnerRunRegistry>,
    observer_id: Uuid,
    pub session: Session,
    completed: bool,
}

impl ObserverReplay {
    pub fn complete(mut self) -> Result<(), agent_client_protocol::Error> {
        let mut state = self
            .registry
            .observers
            .write()
            .expect("owner observers poisoned");
        let observer = state
            .sessions
            .get_mut(&self.session.id)
            .and_then(|observers| observers.get_mut(&self.observer_id))
            .ok_or_else(|| {
                agent_client_protocol::Error::internal_error().data("replay observer disconnected")
            })?;
        let notifications = observer.replay.take().ok_or_else(|| {
            agent_client_protocol::Error::internal_error().data("observer is not replaying")
        })?;
        for notification in notifications {
            notification.send(observer)?;
        }
        self.completed = true;
        Ok(())
    }
}

impl Drop for ObserverReplay {
    fn drop(&mut self) {
        if !self.completed {
            self.registry
                .detach_session_observer(self.observer_id, &self.session.id);
        }
    }
}

impl OwnerRunRegistry {
    pub(crate) async fn begin_observer_replay(
        self: &Arc<Self>,
        observer_id: Uuid,
        session_id: &str,
        cx: ConnectionTo<Client>,
        supports_custom: bool,
    ) -> anyhow::Result<ObserverReplay> {
        let runs = self.runs.lock().await;
        let baseline = if runs.contains_key(session_id) {
            None
        } else {
            Some(self.session_manager.get_session(session_id, true).await?)
        };
        let mut state = self.observers.write().expect("owner observers poisoned");
        let (session, notifications) = match baseline {
            Some(session) => (session, Vec::new()),
            None => {
                let history = state
                    .active
                    .get(session_id)
                    .ok_or_else(|| anyhow::anyhow!("active owner run has no replay boundary"))?;
                (history.baseline.clone(), history.notifications.clone())
            }
        };
        let observers = state.sessions.entry(session_id.to_owned()).or_default();
        anyhow::ensure!(
            observers
                .get(&observer_id)
                .is_none_or(|observer| observer.replay.is_none()),
            "observer replay is already in progress"
        );
        observers.insert(
            observer_id,
            Observer {
                cx,
                replay: Some(notifications),
                supports_custom,
            },
        );
        Ok(ObserverReplay {
            registry: self.clone(),
            observer_id,
            session,
            completed: false,
        })
    }

    pub(super) fn begin_run_replay(&self, baseline: Session, run_id: Uuid) {
        let session_id = baseline.id.clone();
        self.observers
            .write()
            .expect("owner observers poisoned")
            .active
            .insert(
                session_id.clone(),
                RunReplay {
                    run_id,
                    baseline,
                    notifications: Vec::new(),
                },
            );
        self.notify_session(run_update(&session_id, Some(run_id)));
    }

    pub(super) fn finish_run_replay(&self, session_id: &str, run_id: &str) {
        let mut state = self.observers.write().expect("owner observers poisoned");
        if state
            .active
            .get(session_id)
            .is_some_and(|run| run.run_id.to_string() == run_id)
        {
            state.notify(
                OwnerNotification::Standard(Box::new(run_update(session_id, None))),
                None,
            );
            state.active.remove(session_id);
        }
    }

    pub async fn attach_observer(
        self: &Arc<Self>,
        id: Uuid,
        session_id: &str,
        cx: ConnectionTo<Client>,
        supports_custom: bool,
    ) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .sessions
            .entry(session_id.to_owned())
            .or_default()
            .entry(id)
            .or_insert_with(|| Observer {
                cx: cx.clone(),
                replay: None,
                supports_custom,
            });
        let pending = {
            let handlers = self.input_handlers.lock().await;
            handlers
                .iter()
                .map(|(id, handler)| (*id, handler.clone()))
                .collect::<Vec<_>>()
        };
        for (input_id, handler) in pending {
            if let Ok(Some(json)) = async {
                Ok::<_, anyhow::Error>(sqlx::query_scalar::<_, String>("SELECT input_json FROM owner_inputs WHERE input_id=?1 AND session_id=?2 AND state='open' UNION ALL SELECT input_json FROM owner_preflights WHERE input_id=?1 AND session_id=?2 AND state='waiting_input'")
                    .bind(input_id.to_string()).bind(session_id).fetch_optional(self.pool().await?).await?)
            }.await {
                match serde_json::from_str(&json) {
                    Ok(input) => handler.observe(&cx, self.clone(), input),
                    Err(error) => tracing::error!(%error, "pending owner input is unreadable"),
                }
            }
        }
    }

    pub fn detach_observer(&self, id: Uuid) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .sessions
            .retain(|_, observers| {
                observers.remove(&id);
                !observers.is_empty()
            });
    }

    fn detach_session_observer(&self, id: Uuid, session_id: &str) {
        let mut state = self.observers.write().expect("owner observers poisoned");
        if let Some(observers) = state.sessions.get_mut(session_id) {
            observers.remove(&id);
            if observers.is_empty() {
                state.sessions.remove(session_id);
            }
        }
    }

    pub fn notify_session(&self, notification: SessionNotification) {
        self.notify_session_except(notification, None);
    }

    pub(crate) fn notify_session_except(
        &self,
        notification: SessionNotification,
        except_live: Option<Uuid>,
    ) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .notify(
                OwnerNotification::Standard(Box::new(notification)),
                except_live,
            );
    }

    pub(crate) fn notify_custom_session(&self, notification: GooseSessionNotification) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .notify(OwnerNotification::Custom(Box::new(notification)), None);
    }

    pub(super) fn input_observers(&self, session_id: &str) -> Vec<ConnectionTo<Client>> {
        self.observers
            .read()
            .expect("owner observers poisoned")
            .sessions
            .get(session_id)
            .map(|observers| {
                observers
                    .values()
                    .map(|observer| observer.cx.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl OwnerObservers {
    fn notify(&mut self, notification: OwnerNotification, except_live: Option<Uuid>) {
        let session_id = notification.session_id();
        if let Some(run) = self.active.get_mut(session_id) {
            run.notifications.push(notification.clone());
        }
        if let Some(observers) = self.sessions.get_mut(session_id) {
            observers.retain(|id, observer| match &mut observer.replay {
                Some(pending) => {
                    pending.push(notification.clone());
                    true
                }
                None if Some(*id) == except_live => true,
                None => notification.clone().send(observer).is_ok(),
            });
        }
    }
}

#[derive(Clone)]
enum OwnerNotification {
    Standard(Box<SessionNotification>),
    Custom(Box<GooseSessionNotification>),
}

impl OwnerNotification {
    fn session_id(&self) -> &str {
        match self {
            Self::Standard(notification) => notification.session_id.0.as_ref(),
            Self::Custom(notification) => &notification.session_id,
        }
    }
    fn send(self, observer: &Observer) -> Result<(), agent_client_protocol::Error> {
        match self {
            Self::Standard(notification) => observer.cx.send_notification(*notification),
            Self::Custom(notification) if observer.supports_custom => {
                observer.cx.send_notification(*notification)
            }
            Self::Custom(_) => Ok(()),
        }
    }
}

fn run_update(session_id: &str, run_id: Option<Uuid>) -> SessionNotification {
    let mut meta = serde_json::Map::new();
    meta.insert("goose".into(), serde_json::json!({"activeRunId": run_id}));
    SessionNotification::new(
        SessionId::new(session_id),
        SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().meta(meta)),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use crate::work_control::tests::Fixture;
    use agent_client_protocol::schema::v1::{ContentChunk, TextContent};
    use tokio::io::{AsyncBufReadExt, BufReader, DuplexStream, Lines};
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    struct ClientWire {
        cx: ConnectionTo<Client>,
        lines: Lines<BufReader<DuplexStream>>,
        _writer: DuplexStream,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for ClientWire {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn client_wire() -> ClientWire {
        let (read, write) = tokio::io::duplex(64 * 1024);
        let (server_read, client_write) = tokio::io::duplex(64 * 1024);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = agent_client_protocol::Agent
                .builder()
                .connect_with(
                    agent_client_protocol::ByteStreams::new(
                        write.compat_write(),
                        server_read.compat(),
                    ),
                    async move |cx| {
                        tx.send(cx)
                            .map_err(|_| agent_client_protocol::Error::internal_error())?;
                        std::future::pending::<Result<(), agent_client_protocol::Error>>().await
                    },
                )
                .await;
        });
        ClientWire {
            cx: rx.await.unwrap(),
            lines: BufReader::new(read).lines(),
            _writer: client_write,
            task,
        }
    }

    fn chunk(session: &str, text: &str) -> SessionNotification {
        SessionNotification::new(
            SessionId::new(session),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                agent_client_protocol::schema::v1::ContentBlock::Text(TextContent::new(text)),
            )),
        )
    }

    async fn words_through(wire: &mut ClientWire, last: &str) -> Vec<String> {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut words = Vec::new();
            loop {
                let line = wire.lines.next_line().await.unwrap().unwrap();
                let frame: serde_json::Value = serde_json::from_str(&line).unwrap();
                if let Some(text) = frame["params"]["update"]["content"]["text"].as_str() {
                    words.push(text.to_owned());
                    if text == last {
                        return words;
                    }
                }
            }
        })
        .await
        .expect("observer receives replay and live data")
    }

    #[tokio::test]
    async fn replay_keeps_stream_boundary_across_persisted_id_rewrites_and_next_run() {
        let f = Fixture::new().await;
        f.manager
            .add_message(
                &f.session,
                &Message::user()
                    .with_id("baseline")
                    .with_text("committed before run"),
            )
            .await
            .unwrap();
        let first = f.reserve_run().await;
        f.runs.notify_session(chunk(&f.session, "early thinking"));
        // Agent moves thinking onto a fresh tool message before storage. Replaying the
        // current DB here plus the live trail would render these words twice.
        f.manager
            .add_message(
                &f.session,
                &Message::assistant()
                    .with_id("rewritten-tool-id")
                    .with_text("early thinking"),
            )
            .await
            .unwrap();
        let mut wire = client_wire().await;
        let replay = f
            .runs
            .begin_observer_replay(Uuid::new_v4(), &f.session, wire.cx.clone(), true)
            .await
            .unwrap();
        let baseline = replay.session.conversation.as_ref().unwrap().messages();
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].as_concat_text(), "committed before run");
        f.runs.notify_session(chunk(&f.session, "during replay"));
        f.runs.finish(&f.session, &first.run_id.to_string()).await;
        let next = f.reserve_run().await;
        f.runs
            .notify_session(chunk(&f.session, "new run during replay"));
        replay.complete().unwrap();
        f.runs.notify_session(chunk(&f.session, "live after drain"));
        assert_eq!(
            words_through(&mut wire, "live after drain").await,
            [
                "early thinking",
                "during replay",
                "new run during replay",
                "live after drain"
            ]
        );
        f.runs.finish(&f.session, &next.run_id.to_string()).await;
    }

    #[tokio::test]
    async fn idle_replay_captures_a_run_that_starts_before_replay_finishes() {
        let f = Fixture::new().await;
        f.manager
            .add_message(
                &f.session,
                &Message::user()
                    .with_id("baseline")
                    .with_text("idle history"),
            )
            .await
            .unwrap();
        let mut wire = client_wire().await;
        let id = Uuid::new_v4();
        let replay = f
            .runs
            .begin_observer_replay(id, &f.session, wire.cx.clone(), true)
            .await
            .unwrap();
        let run = f.reserve_run().await;
        f.runs
            .notify_session(chunk(&f.session, "run started during replay"));
        f.runs
            .attach_observer(id, &f.session, wire.cx.clone(), true)
            .await;
        f.runs
            .notify_session(chunk(&f.session, "registered without losing buffer"));
        replay.complete().unwrap();
        f.runs.notify_session(chunk(&f.session, "live"));
        assert_eq!(
            words_through(&mut wire, "live").await,
            [
                "run started during replay",
                "registered without losing buffer",
                "live"
            ]
        );
        f.runs.finish(&f.session, &run.run_id.to_string()).await;
    }
}
