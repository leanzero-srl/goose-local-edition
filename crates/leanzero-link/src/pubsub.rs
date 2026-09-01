//! Broadcast hub for `/v1/swarm/stream`: tokio broadcast + a bounded replay
//! `VecDeque` + an `AtomicU64` sequence — the exact shape (capacities, locking
//! discipline, replay/eviction semantics) of goose-server's
//! `session_event_bus.rs`, so both cursors behave identically for clients.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{broadcast, Mutex};

use crate::wire::LinkEvent;

pub const BROADCAST_CAPACITY: usize = 256;
pub const REPLAY_BUFFER_CAPACITY: usize = 512;

/// Error returned by [`PubSub::subscribe`].
#[derive(Debug, PartialEq, Eq)]
pub enum SubscribeError {
    /// The client's `?since=` cursor has been evicted from the replay buffer,
    /// so events have been irrecoverably lost.
    ClientTooFarBehind,
}

/// Where an event entered this node's hub: produced locally, or folded in from a
/// mesh peer's stream. Peer-to-peer subscriptions filter on this (`?scope=local`)
/// so relayed events are never re-relayed — no echo loops in the fabric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventOrigin {
    Local,
    Peer,
}

#[derive(Clone, Debug)]
pub struct StampedEvent {
    /// Monotonic per-node stream cursor, written as `StreamFrame.seq`.
    pub seq: u64,
    pub origin: EventOrigin,
    pub event: LinkEvent,
}

pub struct PubSub {
    tx: broadcast::Sender<StampedEvent>,
    buffer: Mutex<VecDeque<StampedEvent>>,
    next_seq: AtomicU64,
}

impl PubSub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx,
            buffer: Mutex::new(VecDeque::with_capacity(REPLAY_BUFFER_CAPACITY)),
            next_seq: AtomicU64::new(1),
        }
    }

    /// Publish an event to the hub. The sequence number is assigned under the
    /// buffer lock so concurrent publishers cannot reorder events.
    pub async fn publish(&self, origin: EventOrigin, event: LinkEvent) -> u64 {
        let stamped = {
            let mut buf = self.buffer.lock().await;
            let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
            let stamped = StampedEvent { seq, origin, event };
            buf.push_back(stamped.clone());
            while buf.len() > REPLAY_BUFFER_CAPACITY {
                buf.pop_front();
            }
            stamped
        };

        let _ = self.tx.send(stamped.clone());

        stamped.seq
    }

    /// Subscribe to live events, replaying buffered events with `seq > since`.
    /// Returns `(replay_events, replay_max_seq, live_receiver)`; the caller must
    /// skip live events with `seq <= replay_max_seq` to deduplicate at the
    /// replay/live handoff boundary.
    ///
    /// The live receiver is created before the buffer snapshot so no event can
    /// fall into the gap between the two steps. A `since` cursor older than the
    /// buffer's oldest entry means events were evicted: `ClientTooFarBehind`.
    /// A cursor newer than the buffer max (e.g. from before a restart) is
    /// clamped so it cannot suppress live events.
    pub async fn subscribe(
        &self,
        since: Option<u64>,
    ) -> Result<(Vec<StampedEvent>, u64, broadcast::Receiver<StampedEvent>), SubscribeError> {
        let rx = self.tx.subscribe();

        let (replay, replay_max_seq) = {
            let buf = self.buffer.lock().await;
            let buf_max = buf.back().map(|e| e.seq).unwrap_or(0);
            let buf_min = buf.front().map(|e| e.seq).unwrap_or(0);
            let since = since.unwrap_or(0);

            if since > 0 && buf_min > 0 && since < buf_min {
                return Err(SubscribeError::ClientTooFarBehind);
            }

            let events: Vec<_> = buf.iter().filter(|e| e.seq > since).cloned().collect();
            let max_seq = events.last().map(|e| e.seq).unwrap_or(since.min(buf_max));
            (events, max_seq)
        };

        Ok((replay, replay_max_seq, rx))
    }
}

impl Default for PubSub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::SessionDeltaKind;

    fn delta(n: u64) -> LinkEvent {
        LinkEvent::SessionDelta {
            session_id: "s1".to_string(),
            seq: n,
            kind: SessionDeltaKind::Message,
            payload: serde_json::json!({"n": n}),
        }
    }

    #[tokio::test]
    async fn publish_and_subscribe_replays_all() {
        let hub = PubSub::new();
        hub.publish(EventOrigin::Local, delta(1)).await;
        hub.publish(EventOrigin::Peer, delta(2)).await;

        let (replay, replay_max_seq, _rx) = hub.subscribe(None).await.unwrap();
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].seq, 1);
        assert_eq!(replay[1].seq, 2);
        assert_eq!(replay[1].origin, EventOrigin::Peer);
        assert_eq!(replay_max_seq, 2);
    }

    #[tokio::test]
    async fn since_cursor_skips_replayed_events() {
        let hub = PubSub::new();
        for n in 0..3 {
            hub.publish(EventOrigin::Local, delta(n)).await;
        }
        let (replay, replay_max_seq, _rx) = hub.subscribe(Some(2)).await.unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].seq, 3);
        assert_eq!(replay_max_seq, 3);
    }

    #[tokio::test]
    async fn stale_cursor_is_clamped_not_suppressing() {
        let hub = PubSub::new();
        for n in 0..3 {
            hub.publish(EventOrigin::Local, delta(n)).await;
        }
        let (replay, replay_max_seq, _rx) = hub.subscribe(Some(9999)).await.unwrap();
        assert_eq!(replay.len(), 0);
        assert_eq!(replay_max_seq, 3);
    }

    #[tokio::test]
    async fn evicted_cursor_is_client_too_far_behind() {
        let hub = PubSub::new();
        for n in 0..(REPLAY_BUFFER_CAPACITY as u64 + 10) {
            hub.publish(EventOrigin::Local, delta(n)).await;
        }
        assert_eq!(
            hub.subscribe(Some(1)).await.unwrap_err(),
            SubscribeError::ClientTooFarBehind
        );
        // The oldest surviving cursor still works.
        let buf_min = REPLAY_BUFFER_CAPACITY as u64 + 10 - REPLAY_BUFFER_CAPACITY as u64 + 1;
        assert!(hub.subscribe(Some(buf_min)).await.is_ok());
    }
}
