//! Coalesces a task's events into pushes: at most ONE push per `min_interval` per task, streamed
//! text deltas of one message glued into one `text` event, tool events riding along in the same
//! push. Pure — it takes `now` as an argument so the rate rule is testable without a clock.

use super::events::{MappedText, TaskEvent};
use std::time::{Duration, Instant};

pub struct Batcher {
    pending: Vec<TaskEvent>,
    last_text_id: Option<String>,
    last_push: Option<Instant>,
    min_interval: Duration,
}

impl Batcher {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            pending: Vec::new(),
            last_text_id: None,
            last_push: None,
            min_interval,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn push(&mut self, event: TaskEvent) {
        self.last_text_id = None;
        self.pending.push(event);
    }

    /// The rule `openai_compat::TextAccumulator` uses: consecutive deltas that share a message
    /// id (or carry none) extend the pending `text`; a new id starts a new paragraph in it.
    pub fn push_text(&mut self, text: MappedText) {
        if let Some(TaskEvent::Text { text: pending }) = self.pending.last_mut() {
            let same_message =
                self.last_text_id.is_none() || text.id.is_none() || self.last_text_id == text.id;
            if !same_message {
                pending.push_str("\n\n");
            }
            pending.push_str(&text.text);
        } else {
            self.pending.push(TaskEvent::Text { text: text.text });
        }
        if text.id.is_some() {
            self.last_text_id = text.id;
        }
    }

    /// When the pending events may next go out: `None` while nothing is pending, otherwise the
    /// later of `now` and one interval after the previous push.
    pub fn ready_at(&self, now: Instant) -> Option<Instant> {
        if self.pending.is_empty() {
            return None;
        }
        Some(match self.last_push {
            Some(last) => (last + self.min_interval).max(now),
            None => now,
        })
    }

    /// Everything pending, if the rate rule allows a push at `now`; otherwise nothing.
    pub fn take(&mut self, now: Instant) -> Vec<TaskEvent> {
        match self.ready_at(now) {
            Some(ready) if ready <= now => {
                self.last_push = Some(now);
                self.last_text_id = None;
                std::mem::take(&mut self.pending)
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::cognirunner::events::ToolPhase;

    fn tool(reference: &str) -> TaskEvent {
        TaskEvent::Tool {
            name: "t".into(),
            phase: ToolPhase::Started,
            ok: None,
            summary: String::new(),
            reference: reference.into(),
        }
    }

    fn text(id: Option<&str>, s: &str) -> MappedText {
        MappedText {
            id: id.map(str::to_string),
            text: s.to_string(),
        }
    }

    #[test]
    fn deltas_of_one_message_coalesce_into_one_text_event() {
        let mut b = Batcher::new(Duration::from_secs(1));
        b.push_text(text(Some("m1"), "A"));
        b.push_text(text(Some("m1"), " clear"));
        b.push_text(text(Some("m1"), " sky."));
        b.push_text(text(Some("m2"), "Second."));
        let now = Instant::now();
        assert_eq!(
            b.take(now),
            vec![TaskEvent::Text {
                text: "A clear sky.\n\nSecond.".into()
            }]
        );
    }

    #[test]
    fn a_tool_event_between_texts_starts_a_new_text_event() {
        let mut b = Batcher::new(Duration::from_secs(1));
        b.push_text(text(Some("m1"), "before"));
        b.push(tool("c1"));
        b.push_text(text(Some("m1"), "after"));
        let batch = b.take(Instant::now());
        assert_eq!(batch.len(), 3);
        assert_eq!(
            batch[0],
            TaskEvent::Text {
                text: "before".into()
            }
        );
        assert_eq!(batch[1], tool("c1"));
        assert_eq!(
            batch[2],
            TaskEvent::Text {
                text: "after".into()
            }
        );
    }

    #[test]
    fn at_most_one_push_per_interval() {
        let mut b = Batcher::new(Duration::from_secs(1));
        let t0 = Instant::now();
        b.push(tool("c1"));
        assert_eq!(b.ready_at(t0), Some(t0));
        assert_eq!(b.take(t0).len(), 1);

        b.push(tool("c2"));
        b.push(tool("c3"));
        assert_eq!(b.ready_at(t0), Some(t0 + Duration::from_secs(1)));
        assert!(b.take(t0 + Duration::from_millis(999)).is_empty());
        let second = b.take(t0 + Duration::from_secs(1));
        assert_eq!(second, vec![tool("c2"), tool("c3")]);
        assert!(b.is_empty());
        assert_eq!(b.ready_at(t0 + Duration::from_secs(1)), None);
    }

    #[test]
    fn ready_at_never_lies_in_the_past() {
        let mut b = Batcher::new(Duration::from_secs(1));
        let t0 = Instant::now();
        b.push(tool("c1"));
        b.take(t0);
        b.push(tool("c2"));
        let late = t0 + Duration::from_secs(5);
        assert_eq!(b.ready_at(late), Some(late));
    }
}
