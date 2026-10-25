//! The ORIGIN node's job queue: an untargeted job no Mac has room for waits here, first in
//! first out, and the manager offers the head to the Macs again every
//! [`crate::jobs::JobsConfig::queue_tick`] from fresh peer load.
//!
//! It belongs to the [`crate::manager::LinkManager`], not to the control service: the service
//! is rebuilt on every Link reconnect, and a queue inside it would lose every waiting job with
//! it (the 3.0.128 D1 lesson for running jobs). The control service only READS it, so this
//! Mac's card (`/nodes` self load) and peers' `GET /v1/swarm/jobs[/{id}]` show the waiting jobs.

use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use tokio::task::JoinHandle;

use crate::jobs::{
    JobRequest, JobView, NodeRefusal, PlacedOn, PlacementStrategy, QueueInfo, Refusal, RefusalCode,
};
use crate::load::{JobBrief, JobState};

/// Ended queue entries (cancelled while waiting, or placed) kept for the list and for a dialog
/// that still follows them.
const ENDED_KEEP: usize = 100;

struct Waiting {
    job_id: String,
    origin_node: String,
    req: JobRequest,
    strategy: PlacementStrategy,
    queued_at: DateTime<Utc>,
    refusals: Vec<NodeRefusal>,
}

impl Waiting {
    fn brief(&self, position: u32) -> JobBrief {
        JobBrief {
            job_id: self.job_id.clone(),
            origin_node: self.origin_node.clone(),
            kind: self.req.kind,
            started_at: self.queued_at,
            state: JobState::Queued,
            heavy: self.req.heavy,
            queue: Some(QueueInfo {
                position,
                strategy: self.strategy,
                refusals: self.refusals.clone(),
            }),
        }
    }

    fn view(&self, position: u32) -> JobView {
        JobView {
            brief: self.brief(position),
            ended_at: None,
            exit_code: None,
            reason: None,
            session_id: None,
            log_bytes: 0,
            placed_on: None,
        }
    }
}

/// What a queue id names now.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueLookup {
    /// Still waiting.
    Waiting(JobView),
    /// Placed: the job runs on `PlacedOn` (the view is the stub kept at placement).
    Placed(PlacedOn, JobView),
    /// Cancelled while it waited.
    Cancelled(JobView),
}

#[derive(Default)]
struct State {
    waiting: VecDeque<Waiting>,
    /// Newest last.
    ended: VecDeque<(JobView, Option<PlacedOn>)>,
}

/// One per manager. See the module docs.
#[derive(Default)]
pub struct JobQueue {
    state: Mutex<State>,
    ticker: Mutex<Option<JoinHandle<()>>>,
    drain: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobQueue")
            .field("waiting", &self.len())
            .finish_non_exhaustive()
    }
}

impl Drop for JobQueue {
    fn drop(&mut self) {
        if let Some(t) = self.ticker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            t.abort();
        }
    }
}

fn new_queue_id() -> String {
    format!(
        "q{}-{:06x}",
        Utc::now().format("%Y%m%d%H%M%S"),
        rand::random::<u32>() & 0xff_ffff
    )
}

impl JobQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn len(&self) -> usize {
        self.st().waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Put a job at the back of the line. Returns its queue id and position (1 = next).
    pub fn enqueue(
        &self,
        origin_node: &str,
        req: JobRequest,
        strategy: PlacementStrategy,
        refusals: Vec<NodeRefusal>,
    ) -> (String, u32) {
        let job_id = new_queue_id();
        let mut st = self.st();
        st.waiting.push_back(Waiting {
            job_id: job_id.clone(),
            origin_node: origin_node.to_string(),
            req,
            strategy,
            queued_at: Utc::now(),
            refusals,
        });
        (job_id, st.waiting.len() as u32)
    }

    /// The waiting jobs in line order: `(queue id, request, strategy)`.
    pub fn snapshot(&self) -> Vec<(String, JobRequest, PlacementStrategy)> {
        self.st()
            .waiting
            .iter()
            .map(|w| (w.job_id.clone(), w.req.clone(), w.strategy))
            .collect()
    }

    /// Record why each Mac said no at the latest try.
    pub fn set_refusals(&self, job_id: &str, refusals: Vec<NodeRefusal>) {
        if let Some(w) = self.st().waiting.iter_mut().find(|w| w.job_id == job_id) {
            w.refusals = refusals;
        }
    }

    /// The job went to `on`. `false` = it is no longer waiting (cancelled while the submit was
    /// in flight): the caller must cancel what it just placed.
    pub fn mark_placed(&self, job_id: &str, on: PlacedOn) -> bool {
        let mut st = self.st();
        let Some(i) = st.waiting.iter().position(|w| w.job_id == job_id) else {
            return false;
        };
        let w = st.waiting.remove(i).expect("index from position");
        let mut view = w.view(0);
        view.brief.queue = None;
        view.brief.state = JobState::Running;
        view.reason = Some(format!("placed on {} as {}", on.node_id, on.job_id));
        view.placed_on = Some(on.clone());
        push_ended(&mut st, view, Some(on));
        true
    }

    /// Briefs of the waiting jobs, for this node's load report (its card's JOBS list).
    pub fn briefs(&self) -> Vec<JobBrief> {
        self.st()
            .waiting
            .iter()
            .enumerate()
            .map(|(i, w)| w.brief(i as u32 + 1))
            .collect()
    }

    /// Waiting jobs in line order, then the ended ones newest first.
    pub fn list(&self) -> Vec<JobView> {
        let st = self.st();
        let mut out: Vec<JobView> = st
            .waiting
            .iter()
            .enumerate()
            .map(|(i, w)| w.view(i as u32 + 1))
            .collect();
        out.extend(st.ended.iter().rev().map(|(v, _)| v.clone()));
        out
    }

    pub fn lookup(&self, job_id: &str) -> Option<QueueLookup> {
        let st = self.st();
        if let Some(i) = st.waiting.iter().position(|w| w.job_id == job_id) {
            return Some(QueueLookup::Waiting(st.waiting[i].view(i as u32 + 1)));
        }
        st.ended
            .iter()
            .find(|(v, _)| v.brief.job_id == job_id)
            .map(|(v, on)| match on {
                Some(on) => QueueLookup::Placed(on.clone(), v.clone()),
                None => QueueLookup::Cancelled(v.clone()),
            })
    }

    /// Cancel a WAITING job: `None` = not a waiting job of this queue (placed, ended or
    /// unknown); `Some(view)` = it left the line, state `cancelled`.
    pub fn cancel_waiting(&self, job_id: &str) -> Option<JobView> {
        let mut st = self.st();
        let i = st.waiting.iter().position(|w| w.job_id == job_id)?;
        let w = st.waiting.remove(i).expect("index from position");
        let mut view = w.view(0);
        view.brief.queue = None;
        view.brief.state = JobState::Cancelled;
        view.ended_at = Some(Utc::now());
        view.reason = Some("cancelled while it waited in the queue".into());
        push_ended(&mut st, view.clone(), None);
        Some(view)
    }

    /// A cancel of a queue id the queue cannot act on itself: placed (the caller follows it)
    /// or already cancelled.
    pub fn cancel_refusal(job_id: &str) -> Refusal {
        Refusal::new(
            RefusalCode::AlreadyFinished,
            format!("job {job_id} already left the queue"),
        )
    }

    /// Held while one drain runs, so the ticker and a manual drain never place the same job.
    pub async fn drain_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.drain.lock().await
    }

    /// Start the re-place loop unless it runs. `spawn` is called under the ticker lock.
    pub fn ensure_ticker(&self, spawn: impl FnOnce() -> JoinHandle<()>) {
        let mut t = self.ticker.lock().unwrap_or_else(|p| p.into_inner());
        if t.as_ref().is_none_or(|h| h.is_finished()) {
            *t = Some(spawn());
        }
    }

    /// The loop's exit check: `true` (and the loop forgotten) only when nothing waits, decided
    /// under the ticker lock so an enqueue racing it starts a new loop.
    pub fn retire_ticker_if_empty(&self) -> bool {
        let mut t = self.ticker.lock().unwrap_or_else(|p| p.into_inner());
        if self.is_empty() {
            *t = None;
            true
        } else {
            false
        }
    }
}

fn push_ended(st: &mut State, view: JobView, on: Option<PlacedOn>) {
    st.ended.push_back((view, on));
    while st.ended.len() > ENDED_KEEP {
        st.ended.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::JobKind;

    fn req(cmd: &str) -> JobRequest {
        JobRequest {
            kind: JobKind::Shell,
            command: cmd.into(),
            cwd: None,
            need_mem_gb: None,
            heavy: true,
            timeout_s: None,
            origin_node: None,
            allow_user_active: false,
            strategy: None,
        }
    }

    #[test]
    fn three_jobs_wait_in_order_and_move_up_when_the_head_leaves() {
        let q = JobQueue::new();
        let ids: Vec<String> = ["a", "b", "c"]
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (id, pos) = q.enqueue("me", req(c), PlacementStrategy::LocalFirst, vec![]);
                assert_eq!(pos, i as u32 + 1);
                id
            })
            .collect();
        let order: Vec<String> = q.snapshot().into_iter().map(|(id, ..)| id).collect();
        assert_eq!(order, ids);
        let pos: Vec<u32> = q
            .briefs()
            .iter()
            .map(|b| b.queue.as_ref().unwrap().position)
            .collect();
        assert_eq!(pos, vec![1, 2, 3]);
        assert!(q.mark_placed(
            &ids[0],
            PlacedOn {
                node_id: "studio".into(),
                job_id: "j1".into()
            }
        ));
        let b = q.briefs();
        assert_eq!(b[0].job_id, ids[1]);
        assert_eq!(b[0].queue.as_ref().unwrap().position, 1);
        assert!(matches!(q.lookup(&ids[0]), Some(QueueLookup::Placed(on, _)) if on.job_id == "j1"));
    }

    #[test]
    fn cancel_while_waiting_leaves_the_line_and_a_late_placement_is_refused() {
        let q = JobQueue::new();
        let (a, _) = q.enqueue("me", req("a"), PlacementStrategy::Selfish, vec![]);
        let (b, _) = q.enqueue("me", req("b"), PlacementStrategy::Selfish, vec![]);
        let v = q.cancel_waiting(&a).expect("waiting");
        assert_eq!(v.brief.state, JobState::Cancelled);
        assert!(v.ended_at.is_some());
        assert_eq!(q.len(), 1);
        assert_eq!(q.briefs()[0].queue.as_ref().unwrap().position, 1);
        assert!(q.cancel_waiting(&a).is_none(), "twice is not waiting");
        assert!(matches!(q.lookup(&a), Some(QueueLookup::Cancelled(_))));
        // A placement that lands after the cancel must be undone by the caller.
        assert!(!q.mark_placed(
            &a,
            PlacedOn {
                node_id: "x".into(),
                job_id: "j".into()
            }
        ));
        let list = q.list();
        assert_eq!(list[0].brief.job_id, b);
        assert_eq!(list[1].brief.job_id, a);
    }

    #[test]
    fn a_queued_brief_carries_position_strategy_and_refusals_on_the_wire() {
        let q = JobQueue::new();
        q.enqueue(
            "me",
            req("a"),
            PlacementStrategy::Selfish,
            vec![NodeRefusal {
                node_id: "studio".into(),
                code: RefusalCode::HeavySlotHeld,
                reason: "heavy slot held".into(),
            }],
        );
        let v = serde_json::to_value(&q.briefs()[0]).unwrap();
        assert_eq!(v["state"], "queued");
        assert_eq!(v["queue"]["position"], 1);
        assert_eq!(v["queue"]["strategy"], "selfish");
        assert_eq!(v["queue"]["refusals"][0]["code"], "heavy_slot_held");
        assert!(v.get("command").is_none());
    }
}
