//! Lifecycle-capable execution seam for [`crate::broker::PhysicalBroker`].
//!
//! A task admission is a durable correlation envelope. Physical capacity is held only by the
//! initial reserved provider turn or by one exact live provider request. Every later provider turn
//! re-enters the same task-derived priority queue; local tool work therefore never occupies a
//! decoder slot.

use crate::broker::{
    AdmissionReceipt, BrokerError, BrokerGrant, CapacityUpdateReceipt, HostCapacityEvidence,
    LocalCompletionKind, PhysicalBroker, PhysicalFleetSnapshot, PhysicalHostOccupancy,
    ProviderNotStartedReceipt, ProviderRequestDisposition, ProviderRequestKey,
    ProviderRequestReceipt, ProviderTerminalKind, ProviderTerminalReceipt,
    ReleasedAdmissionReceipt, StaleWorkReceipt, TaskVersion, VerifiedPhysicalLane, WorkOpportunity,
};
use crate::dispatch::{
    DispatchError, DispatchRequest, ProviderDispatchClass, TaskDispatcher, TaskRunOutput,
};
use crate::event::{EventSink, SwarmEvent};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, Notify};

type AdmissionResult = Result<AdmissionReceipt, BrokerError>;
type ProviderRequestResult = Result<ProviderRequestReceipt, BrokerError>;

struct ControlState {
    broker: PhysicalBroker,
    admission_waiters: HashMap<String, oneshot::Sender<AdmissionResult>>,
    provider_waiters: HashMap<(String, ProviderRequestKey), oneshot::Sender<ProviderRequestResult>>,
    released: HashMap<String, ReleasedAdmissionReceipt>,
}

impl ControlState {
    fn clean_cancelled_waiters(&mut self, sink: &dyn EventSink) {
        let cancelled_work: Vec<String> = self
            .admission_waiters
            .iter()
            .filter(|(_, waiter)| waiter.is_closed())
            .map(|(work_id, _)| work_id.clone())
            .collect();
        for work_id in cancelled_work {
            self.admission_waiters.remove(&work_id);
            if let Some(receipt) = self.broker.withdraw_pending_work(&work_id) {
                sink.emit(&SwarmEvent::BrokerWorkWithdrawn { receipt });
            }
        }

        let cancelled_provider: Vec<(String, ProviderRequestKey)> = self
            .provider_waiters
            .iter()
            .filter(|(_, waiter)| waiter.is_closed())
            .map(|(key, _)| key.clone())
            .collect();
        for (admission_id, key) in cancelled_provider {
            self.provider_waiters
                .remove(&(admission_id.clone(), key.clone()));
            if let Ok(receipt) = self
                .broker
                .withdraw_pending_provider_request(&admission_id, &key)
            {
                if let Some(admission) = self.broker.active_receipt(&admission_id).cloned() {
                    sink.emit(&SwarmEvent::BrokerProviderRequestWithdrawn {
                        admission,
                        receipt,
                        reason: "provider permit waiter was cancelled before grant".to_string(),
                    });
                }
            }
        }
    }

    fn pump(&mut self, sink: &dyn EventSink) {
        loop {
            self.clean_cancelled_waiters(sink);
            let Some(grant) = self.broker.grant_next() else {
                break;
            };
            match grant {
                BrokerGrant::Admission(receipt) => {
                    let work_id = receipt.work_id.clone();
                    let delivered = self
                        .admission_waiters
                        .remove(&work_id)
                        .is_some_and(|waiter| waiter.send(Ok(receipt.clone())).is_ok());
                    if delivered {
                        sink.emit(&SwarmEvent::BrokerAdmissionGranted { receipt });
                    } else if let Ok(receipt) = self.broker.revoke_undelivered_admission(
                        &receipt.admission_id,
                        "admission receiver disappeared before consuming the grant",
                    ) {
                        sink.emit(&SwarmEvent::BrokerAdmissionGrantRevoked { receipt });
                    }
                }
                BrokerGrant::ProviderRequest { admission, receipt } => {
                    let waiter_key = (receipt.admission_id.clone(), receipt.key.clone());
                    let delivered = self
                        .provider_waiters
                        .remove(&waiter_key)
                        .is_some_and(|waiter| waiter.send(Ok(receipt.clone())).is_ok());
                    if delivered {
                        sink.emit(&SwarmEvent::BrokerProviderRequestPermitted {
                            admission,
                            receipt,
                        });
                    } else if let Ok(revoked) = self
                        .broker
                        .revoke_undelivered_provider_request(&receipt.admission_id, &receipt.key)
                    {
                        sink.emit(&SwarmEvent::BrokerProviderRequestGrantRevoked {
                            admission,
                            receipt: revoked,
                            reason: "provider waiter disappeared before consuming the permit"
                                .to_string(),
                        });
                    }
                }
            }
        }
    }

    fn reject_stale_waiters(&mut self, stale: &[StaleWorkReceipt], sink: &dyn EventSink) {
        for receipt in stale {
            sink.emit(&SwarmEvent::BrokerWorkStale {
                receipt: receipt.clone(),
            });
            if let Some(waiter) = self.admission_waiters.remove(&receipt.work_id) {
                let _ = waiter.send(Err(BrokerError::StaleOpportunity {
                    work_id: receipt.work_id.clone(),
                    queued: Box::new(receipt.queued_source.clone()),
                    current: receipt.current_source.clone().map(Box::new),
                }));
            }
        }
    }
}

struct ControlInner {
    state: Mutex<ControlState>,
    sink: Arc<dyn EventSink>,
    changed: Notify,
    semantic_observation_plane_claimed: AtomicBool,
}

#[derive(Clone)]
pub struct PhysicalAdmissionControl {
    inner: Arc<ControlInner>,
}

impl PhysicalAdmissionControl {
    pub fn new(
        correlation_scope: impl Into<String>,
        snapshot: PhysicalFleetSnapshot,
        sink: Arc<dyn EventSink>,
    ) -> Result<Self, BrokerError> {
        let broker = PhysicalBroker::new(correlation_scope, snapshot)?;
        Ok(Self {
            inner: Arc::new(ControlInner {
                state: Mutex::new(ControlState {
                    broker,
                    admission_waiters: HashMap::new(),
                    provider_waiters: HashMap::new(),
                    released: HashMap::new(),
                }),
                sink,
                changed: Notify::new(),
                semantic_observation_plane_claimed: AtomicBool::new(false),
            }),
        })
    }

    pub async fn verified_lane(&self, logical_device_id: &str) -> Option<VerifiedPhysicalLane> {
        self.inner
            .state
            .lock()
            .await
            .broker
            .snapshot()
            .lanes
            .into_iter()
            .find(|lane| lane.logical_device_id == logical_device_id)
    }

    pub async fn verified_lanes(&self) -> Vec<VerifiedPhysicalLane> {
        let mut lanes = self.inner.state.lock().await.broker.snapshot().lanes;
        lanes.sort_by(|left, right| left.logical_device_id.cmp(&right.logical_device_id));
        lanes
    }

    pub(crate) fn uses_sink(&self, sink: &Arc<dyn EventSink>) -> bool {
        Arc::ptr_eq(&self.inner.sink, sink)
    }

    pub(crate) fn claim_semantic_observation_plane(&self) -> bool {
        self.inner
            .semantic_observation_plane_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub async fn snapshot(&self) -> PhysicalFleetSnapshot {
        self.inner.state.lock().await.broker.snapshot()
    }

    pub async fn set_source_revision(&self, source: TaskVersion) -> Result<(), BrokerError> {
        let mut state = self.inner.state.lock().await;
        let stale = match state.broker.set_source_revision(source) {
            Ok(stale) => stale,
            Err(error) => {
                self.emit_rejection(None, None, None, "source_revision", &error);
                return Err(error);
            }
        };
        state.reject_stale_waiters(&stale, self.inner.sink.as_ref());
        state.pump(self.inner.sink.as_ref());
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    pub async fn remove_source_revision(&self, source: &TaskVersion) -> Result<(), BrokerError> {
        let mut state = self.inner.state.lock().await;
        let stale = match state.broker.remove_source_revision(source) {
            Ok(stale) => stale,
            Err(error) => {
                self.emit_rejection(None, None, None, "source_revision_removal", &error);
                return Err(error);
            }
        };
        state.reject_stale_waiters(&stale, self.inner.sink.as_ref());
        state.pump(self.inner.sink.as_ref());
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    pub async fn update_host_capacity(
        &self,
        host_id: &str,
        expected_fleet_snapshot_id: &str,
        evidence: HostCapacityEvidence,
    ) -> Result<CapacityUpdateReceipt, BrokerError> {
        let mut state = self.inner.state.lock().await;
        let receipt =
            match state
                .broker
                .update_host_capacity(host_id, expected_fleet_snapshot_id, evidence)
            {
                Ok(receipt) => receipt,
                Err(error) => {
                    self.emit_rejection(None, None, None, "capacity_update", &error);
                    return Err(error);
                }
            };
        self.inner.sink.emit(&SwarmEvent::BrokerCapacityUpdated {
            receipt: receipt.clone(),
        });
        state.pump(self.inner.sink.as_ref());
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(receipt)
    }

    pub async fn admit(&self, opportunity: WorkOpportunity) -> Result<AdmittedWork, BrokerError> {
        self.queue_admission(opportunity).await?.wait().await
    }

    /// Atomically admit work only when the common broker can grant it in the current pump.
    /// Auxiliary work uses this to consume genuinely spare verified capacity without sitting in
    /// front of a later implementation request. A miss leaves no queued work and creates no
    /// admission or provider lifecycle receipt.
    pub async fn try_admit_idle(
        &self,
        opportunity: WorkOpportunity,
    ) -> Result<Option<AdmittedWork>, BrokerError> {
        let work_id = opportunity.work_id.clone();
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.inner.state.lock().await;
            match state.broker.enqueue(opportunity) {
                Ok(receipt) => self
                    .inner
                    .sink
                    .emit(&SwarmEvent::BrokerWorkQueued { receipt }),
                Err(error) => {
                    self.emit_rejection(None, Some(work_id), None, "queue", &error);
                    return Err(error);
                }
            }
            state.admission_waiters.insert(work_id.clone(), sender);
            state.pump(self.inner.sink.as_ref());
            if state.admission_waiters.remove(&work_id).is_some() {
                let receipt = state
                    .broker
                    .withdraw_pending_work(&work_id)
                    .expect("an ungranted immediate admission remains queued");
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerWorkWithdrawn { receipt });
                drop(state);
                self.inner.changed.notify_waiters();
                return Ok(None);
            }
        }
        self.inner.changed.notify_waiters();
        let receipt = receiver
            .await
            .map_err(|_| BrokerError::AdmissionWaiterClosed(work_id))??;
        Ok(Some(AdmittedWork {
            lifecycle: ProviderLifecycle {
                control: self.clone(),
                admission: receipt.clone(),
                next_ordinal: Arc::new(AtomicU32::new(0)),
            },
            receipt,
        }))
    }

    pub(crate) async fn queue_admission(
        &self,
        opportunity: WorkOpportunity,
    ) -> Result<PendingAdmission, BrokerError> {
        let work_id = opportunity.work_id.clone();
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.inner.state.lock().await;
            match state.broker.enqueue(opportunity) {
                Ok(receipt) => self
                    .inner
                    .sink
                    .emit(&SwarmEvent::BrokerWorkQueued { receipt }),
                Err(error) => {
                    self.emit_rejection(None, Some(work_id), None, "queue", &error);
                    return Err(error);
                }
            }
            state.admission_waiters.insert(work_id.clone(), sender);
            state.pump(self.inner.sink.as_ref());
        }
        self.inner.changed.notify_waiters();
        Ok(PendingAdmission {
            control: self.clone(),
            receiver,
            guard: PendingAdmissionGuard::new(self.clone(), work_id.clone()),
            work_id,
        })
    }

    pub async fn occupancy(&self) -> (usize, usize) {
        let state = self.inner.state.lock().await;
        (state.broker.pending_len(), state.broker.active_len())
    }

    pub async fn physical_occupancy(&self) -> Vec<PhysicalHostOccupancy> {
        self.inner.state.lock().await.broker.physical_occupancy()
    }

    pub async fn wait_until_drained(&self) {
        let (pending_work_ids, unresolved) = {
            let state = self.inner.state.lock().await;
            (
                state.broker.pending_work_ids(),
                state.broker.unresolved_admissions(),
            )
        };
        if !pending_work_ids.is_empty() || !unresolved.is_empty() {
            self.inner.sink.emit(&SwarmEvent::BrokerDrainPending {
                pending_work_ids,
                unresolved,
            });
        }
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.occupancy().await == (0, 0) {
                return;
            }
            changed.await;
        }
    }

    async fn request_provider_turn(
        &self,
        receipt: ProviderRequestReceipt,
    ) -> Result<ProviderRequestReceipt, BrokerError> {
        let admission_id = receipt.admission_id.clone();
        let key = receipt.key.clone();
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.inner.state.lock().await;
            let admission = state.broker.active_receipt(&admission_id).cloned();
            match state.broker.request_provider_turn(receipt.clone()) {
                Ok(ProviderRequestDisposition::Granted(receipt)) => {
                    let admission =
                        admission.expect("provider request belongs to active admission");
                    self.inner
                        .sink
                        .emit(&SwarmEvent::BrokerProviderRequestPermitted {
                            admission,
                            receipt: receipt.clone(),
                        });
                    return Ok(receipt);
                }
                Ok(ProviderRequestDisposition::Queued(queue_receipt)) => {
                    self.inner
                        .sink
                        .emit(&SwarmEvent::BrokerProviderRequestQueued {
                            admission: admission
                                .expect("queued provider request belongs to active admission"),
                            receipt: queue_receipt,
                        });
                    state
                        .provider_waiters
                        .insert((admission_id.clone(), key.clone()), sender);
                    state.pump(self.inner.sink.as_ref());
                }
                Err(error) => {
                    self.emit_rejection(admission, None, Some(key), "provider_request", &error);
                    return Err(error);
                }
            }
        }
        self.inner.changed.notify_waiters();
        let mut guard = PendingProviderGuard::new(self.clone(), admission_id.clone(), key);
        let receipt = receiver.await.map_err(|_| {
            BrokerError::AdmissionWaiterClosed(format!("{admission_id}:provider"))
        })??;
        guard.disarm();
        Ok(receipt)
    }

    async fn close_provider_starts(&self, admission_id: &str) -> Result<(), BrokerError> {
        let mut state = self.inner.state.lock().await;
        let closed = state.broker.close_provider_starts(admission_id)?;
        if let Some(admission) = closed.admission {
            self.inner
                .sink
                .emit(&SwarmEvent::BrokerProviderStartsClosed {
                    admission: admission.clone(),
                });
            if let Some(receipt) = closed.provider_not_started {
                self.inner.sink.emit(&SwarmEvent::BrokerProviderNotStarted {
                    admission: admission.clone(),
                    receipt,
                });
            }
            if let Some(receipt) = closed.pending_provider_request {
                if let Some(waiter) = state
                    .provider_waiters
                    .remove(&(admission_id.to_string(), receipt.key.clone()))
                {
                    let _ = waiter.send(Err(BrokerError::ProviderStartsClosed(
                        admission_id.to_string(),
                    )));
                }
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerProviderRequestWithdrawn {
                        admission,
                        receipt,
                        reason: "provider lifecycle closed before the queued request was admitted"
                            .to_string(),
                    });
            }
        }
        self.release_and_pump(&mut state, admission_id)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    async fn record_provider_not_started(
        &self,
        receipt: ProviderNotStartedReceipt,
    ) -> Result<(), BrokerError> {
        let admission_id = receipt.admission_id.clone();
        let mut state = self.inner.state.lock().await;
        let admission = state.broker.active_receipt(&admission_id).cloned();
        match state.broker.record_provider_not_started(receipt.clone()) {
            Ok(()) => {
                let admission =
                    admission.expect("provider-not-started belongs to active admission");
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerProviderStartsClosed {
                        admission: admission.clone(),
                    });
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerProviderNotStarted { admission, receipt });
            }
            Err(error) => {
                self.emit_rejection(admission, None, None, "provider_not_started", &error);
                return Err(error);
            }
        }
        self.release_and_pump(&mut state, &admission_id)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    async fn observe_provider_terminal(
        &self,
        receipt: ProviderTerminalReceipt,
    ) -> Result<(), BrokerError> {
        let admission_id = receipt.admission_id.clone();
        let mut state = self.inner.state.lock().await;
        let admission = state.broker.active_receipt(&admission_id).cloned();
        match state.broker.observe_provider_terminal(receipt.clone()) {
            Ok(()) => self
                .inner
                .sink
                .emit(&SwarmEvent::BrokerProviderTerminalObserved {
                    admission: admission.expect("provider terminal belongs to active admission"),
                    receipt,
                }),
            Err(error) => {
                self.emit_rejection(
                    admission,
                    None,
                    Some(receipt.key),
                    "provider_terminal",
                    &error,
                );
                return Err(error);
            }
        }
        self.release_and_pump(&mut state, &admission_id)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    async fn record_local_completion(
        &self,
        admission_id: &str,
        kind: LocalCompletionKind,
    ) -> Result<(), BrokerError> {
        let mut state = self.inner.state.lock().await;
        let admission = state.broker.active_receipt(admission_id).cloned();
        match state.broker.record_local_completion(admission_id, kind) {
            Ok(receipt) => self.inner.sink.emit(&SwarmEvent::BrokerWorkLocalCompleted {
                admission: admission.expect("local completion belongs to active admission"),
                receipt,
            }),
            Err(error) => {
                self.emit_rejection(admission, None, None, "local_completion", &error);
                return Err(error);
            }
        }
        self.release_and_pump(&mut state, admission_id)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    async fn wait_for_release(
        &self,
        admission_id: &str,
    ) -> Result<ReleasedAdmissionReceipt, BrokerError> {
        {
            let state = self.inner.state.lock().await;
            if let Some(receipt) = state.released.get(admission_id).cloned() {
                return Ok(receipt);
            }
            let unresolved: Vec<_> = state
                .broker
                .unresolved_admissions()
                .into_iter()
                .filter(|receipt| receipt.admission.admission_id == admission_id)
                .collect();
            self.inner.sink.emit(&SwarmEvent::BrokerDrainPending {
                pending_work_ids: state.broker.pending_work_ids(),
                unresolved,
            });
        }
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(receipt) = self
                .inner
                .state
                .lock()
                .await
                .released
                .get(admission_id)
                .cloned()
            {
                return Ok(receipt);
            }
            changed.await;
        }
    }

    fn release_and_pump(
        &self,
        state: &mut ControlState,
        admission_id: &str,
    ) -> Result<(), BrokerError> {
        if let Some(receipt) = state.broker.release_if_terminal(admission_id)? {
            self.inner.sink.emit(&SwarmEvent::BrokerAdmissionReleased {
                receipt: receipt.clone(),
            });
            state.released.insert(admission_id.to_string(), receipt);
        }
        // A provider terminal releases a physical turn permit even while the task envelope remains
        // active for local tool work. Re-run admission on every lifecycle transition, not only when
        // the whole task envelope is released.
        state.pump(self.inner.sink.as_ref());
        Ok(())
    }

    async fn cancel_admission_waiter(&self, work_id: &str) {
        let mut state = self.inner.state.lock().await;
        state.admission_waiters.remove(work_id);
        if let Some(receipt) = state.broker.withdraw_pending_work(work_id) {
            self.inner
                .sink
                .emit(&SwarmEvent::BrokerWorkWithdrawn { receipt });
        } else if let Some(admission) = state.broker.active_receipt_for_work(work_id).cloned() {
            if let Ok(receipt) = state.broker.revoke_undelivered_admission(
                &admission.admission_id,
                "admission future was cancelled before consuming the grant",
            ) {
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerAdmissionGrantRevoked { receipt });
            }
        }
        state.pump(self.inner.sink.as_ref());
        drop(state);
        self.inner.changed.notify_waiters();
    }

    async fn cancel_provider_waiter(&self, admission_id: &str, key: &ProviderRequestKey) {
        let mut state = self.inner.state.lock().await;
        state
            .provider_waiters
            .remove(&(admission_id.to_string(), key.clone()));
        let admission = state.broker.active_receipt(admission_id).cloned();
        let withdrawn = state
            .broker
            .withdraw_pending_provider_request(admission_id, key)
            .or_else(|_| {
                state
                    .broker
                    .revoke_undelivered_provider_request(admission_id, key)
            });
        if let (Some(admission), Ok(receipt)) = (admission, withdrawn) {
            self.inner
                .sink
                .emit(&SwarmEvent::BrokerProviderRequestWithdrawn {
                    admission,
                    receipt,
                    reason: "provider request future was cancelled before consuming its permit"
                        .to_string(),
                });
        }
        state.pump(self.inner.sink.as_ref());
        drop(state);
        self.inner.changed.notify_waiters();
    }

    fn emit_rejection(
        &self,
        admission: Option<AdmissionReceipt>,
        work_id: Option<String>,
        provider_request: Option<ProviderRequestKey>,
        receipt_kind: &str,
        error: &BrokerError,
    ) {
        self.inner.sink.emit(&SwarmEvent::BrokerReceiptRejected {
            admission,
            work_id,
            provider_request,
            receipt_kind: receipt_kind.to_string(),
            reason: error.to_string(),
        });
    }

    fn emit_provider_free_dispatch(&self, req: &DispatchRequest) {
        self.inner
            .sink
            .emit(&SwarmEvent::ProviderFreeDispatchStarted {
                task_id: req.task_id.clone(),
                attempt: req.attempt,
                class: ProviderDispatchClass::DeterministicProviderFree,
            });
    }
}

struct PendingAdmissionGuard {
    control: PhysicalAdmissionControl,
    work_id: String,
    armed: bool,
}

pub(crate) struct PendingAdmission {
    control: PhysicalAdmissionControl,
    receiver: oneshot::Receiver<AdmissionResult>,
    guard: PendingAdmissionGuard,
    work_id: String,
}

impl PendingAdmission {
    async fn wait(mut self) -> Result<AdmittedWork, BrokerError> {
        let receipt = self
            .receiver
            .await
            .map_err(|_| BrokerError::AdmissionWaiterClosed(self.work_id.clone()))??;
        self.guard.disarm();
        Ok(AdmittedWork {
            lifecycle: ProviderLifecycle {
                control: self.control,
                admission: receipt.clone(),
                next_ordinal: Arc::new(AtomicU32::new(0)),
            },
            receipt,
        })
    }
}

impl PendingAdmissionGuard {
    fn new(control: PhysicalAdmissionControl, work_id: String) -> Self {
        Self {
            control,
            work_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingAdmissionGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let control = self.control.clone();
                let work_id = self.work_id.clone();
                handle.spawn(async move {
                    control.cancel_admission_waiter(&work_id).await;
                });
            }
        }
    }
}

struct PendingProviderGuard {
    control: PhysicalAdmissionControl,
    admission_id: String,
    key: ProviderRequestKey,
    armed: bool,
}

impl PendingProviderGuard {
    fn new(
        control: PhysicalAdmissionControl,
        admission_id: String,
        key: ProviderRequestKey,
    ) -> Self {
        Self {
            control,
            admission_id,
            key,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingProviderGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let control = self.control.clone();
                let admission_id = self.admission_id.clone();
                let key = self.key.clone();
                handle.spawn(async move {
                    control.cancel_provider_waiter(&admission_id, &key).await;
                });
            }
        }
    }
}

pub struct AdmittedWork {
    receipt: AdmissionReceipt,
    lifecycle: ProviderLifecycle,
}

impl AdmittedWork {
    pub fn receipt(&self) -> &AdmissionReceipt {
        &self.receipt
    }

    pub fn lifecycle(&self) -> ProviderLifecycle {
        self.lifecycle.clone()
    }

    pub async fn complete_local(&self, kind: LocalCompletionKind) -> Result<(), BrokerError> {
        self.lifecycle
            .control
            .close_provider_starts(&self.receipt.admission_id)
            .await?;
        self.complete_local_after_close(kind).await.map(|_| ())
    }

    async fn complete_local_after_close(
        &self,
        kind: LocalCompletionKind,
    ) -> Result<ReleasedAdmissionReceipt, BrokerError> {
        self.lifecycle
            .control
            .record_local_completion(&self.receipt.admission_id, kind)
            .await?;
        let released = self
            .lifecycle
            .control
            .wait_for_release(&self.receipt.admission_id)
            .await?;
        if kind == LocalCompletionKind::Success
            && released.local_completion != LocalCompletionKind::Success
        {
            return Err(BrokerError::OutcomeConflict {
                admission_id: self.receipt.admission_id.clone(),
                reason: "local success was contradicted by provider-not-started or a non-finished provider terminal"
                    .to_string(),
            });
        }
        Ok(released)
    }
}

#[derive(Clone)]
pub struct ProviderLifecycle {
    control: PhysicalAdmissionControl,
    admission: AdmissionReceipt,
    next_ordinal: Arc<AtomicU32>,
}

impl ProviderLifecycle {
    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub async fn provider_request_started(
        &self,
        provider_request_id: impl Into<String>,
    ) -> Result<ProviderRequestKey, BrokerError> {
        let key = ProviderRequestKey {
            ordinal: self.next_ordinal.fetch_add(1, Ordering::SeqCst),
            provider_request_id: provider_request_id.into(),
        };
        self.control
            .request_provider_turn(ProviderRequestReceipt {
                admission_id: self.admission.admission_id.clone(),
                key: key.clone(),
                physical_host_id: self.admission.physical_host_id.clone(),
                model_instance_id: self.admission.model_instance_id.clone(),
            })
            .await?;
        Ok(key)
    }

    pub async fn provider_terminal(
        &self,
        key: ProviderRequestKey,
        kind: ProviderTerminalKind,
    ) -> Result<(), BrokerError> {
        self.control
            .observe_provider_terminal(ProviderTerminalReceipt {
                admission_id: self.admission.admission_id.clone(),
                key,
                physical_host_id: self.admission.physical_host_id.clone(),
                model_instance_id: self.admission.model_instance_id.clone(),
                kind,
            })
            .await
    }

    pub async fn provider_not_started(&self, reason: impl Into<String>) -> Result<(), BrokerError> {
        self.control
            .record_provider_not_started(ProviderNotStartedReceipt {
                admission_id: self.admission.admission_id.clone(),
                physical_host_id: self.admission.physical_host_id.clone(),
                model_instance_id: self.admission.model_instance_id.clone(),
                reason: reason.into(),
            })
            .await
    }
}

#[async_trait]
pub trait ProviderLifecycleDispatcher: Send + Sync {
    /// Defaulting to provider-required makes an absent or incomplete classifier fail closed through
    /// physical admission. Implementations may certify only work that cannot invoke a model.
    fn provider_dispatch_class(&self, _req: &DispatchRequest) -> ProviderDispatchClass {
        ProviderDispatchClass::ProviderRequired
    }

    /// Runs locally certified deterministic work without a physical admission or provider receipt.
    /// The default rejects a certification that has no provider-free implementation.
    async fn run_provider_free(
        &self,
        req: DispatchRequest,
    ) -> Result<TaskRunOutput, DispatchError> {
        Err(DispatchError::Terminal(format!(
            "task `{}` was certified provider-free, but the dispatcher has no deterministic implementation",
            req.task_id
        )))
    }

    async fn run_admitted(
        &self,
        req: DispatchRequest,
        admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError>;
}

enum PreparedDispatch {
    ProviderRequired(Result<PendingAdmission, String>),
    DeterministicProviderFree,
}

#[async_trait]
pub(crate) trait PhysicalDispatchAuthority: Send + Sync {
    async fn opportunity(&self, req: &DispatchRequest) -> Result<WorkOpportunity, DispatchError>;

    async fn route_admitted(
        &self,
        req: &mut DispatchRequest,
        admission: &AdmissionReceipt,
    ) -> Result<(), DispatchError>;
}

pub(crate) struct BrokeredTaskDispatcher {
    control: PhysicalAdmissionControl,
    inner: Arc<dyn ProviderLifecycleDispatcher>,
    authority: Arc<dyn PhysicalDispatchAuthority>,
    prepared: Mutex<HashMap<(String, u32), PreparedDispatch>>,
}

impl BrokeredTaskDispatcher {
    pub(crate) fn new(
        control: PhysicalAdmissionControl,
        inner: Arc<dyn ProviderLifecycleDispatcher>,
        authority: Arc<dyn PhysicalDispatchAuthority>,
    ) -> Self {
        Self {
            control,
            inner,
            authority,
            prepared: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn prepare(&self, req: &DispatchRequest) {
        let key = (req.task_id.clone(), req.attempt);
        let prepared = match self.inner.provider_dispatch_class(req) {
            ProviderDispatchClass::DeterministicProviderFree => {
                PreparedDispatch::DeterministicProviderFree
            }
            ProviderDispatchClass::ProviderRequired => {
                PreparedDispatch::ProviderRequired(match self.authority.opportunity(req).await {
                    Ok(opportunity) => match self
                        .control
                        .set_source_revision(opportunity.source.clone())
                        .await
                    {
                        Ok(()) => self
                            .control
                            .queue_admission(opportunity)
                            .await
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error.to_string()),
                    },
                    Err(error) => Err(error.to_string()),
                })
            }
        };
        let replaced = self.prepared.lock().await.insert(key, prepared);
        debug_assert!(
            replaced.is_none(),
            "a physical task attempt was prepared twice"
        );
    }
}

#[async_trait]
impl TaskDispatcher for BrokeredTaskDispatcher {
    async fn run(&self, mut req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        if req.speculative {
            return Err(DispatchError::Terminal(
                "physical admission refuses speculative twins because they require an admitted-request kill"
                    .to_string(),
            ));
        }
        if req.replan_authority.is_some() {
            return Err(DispatchError::Terminal(
                "physical admission refuses runtime reviews at the build boundary; submit their exact contract authority as auxiliary evidence"
                    .to_string(),
            ));
        }
        let prepared = self
            .prepared
            .lock()
            .await
            .remove(&(req.task_id.clone(), req.attempt))
            .ok_or_else(|| {
                DispatchError::Terminal(format!(
                    "physical task `{}` attempt {} was not prepared by the scheduler",
                    req.task_id, req.attempt
                ))
            })?;
        let pending = match prepared {
            PreparedDispatch::DeterministicProviderFree => {
                self.control.emit_provider_free_dispatch(&req);
                return self.inner.run_provider_free(req).await;
            }
            PreparedDispatch::ProviderRequired(prepared) => prepared.map_err(|error| {
                DispatchError::Terminal(format!("physical admission rejected task: {error}"))
            })?,
        };
        let admitted = pending.wait().await.map_err(|error| {
            DispatchError::Terminal(format!("physical admission rejected task: {error}"))
        })?;
        if let Err(error) = self
            .authority
            .route_admitted(&mut req, admitted.receipt())
            .await
        {
            self.control
                .close_provider_starts(&admitted.receipt().admission_id)
                .await
                .map_err(|close_error| {
                    DispatchError::Terminal(format!(
                        "route admission failed ({error}); lifecycle close also failed: {close_error}"
                    ))
                })?;
            admitted
                .complete_local_after_close(LocalCompletionKind::Error)
                .await
                .map_err(|completion_error| {
                    DispatchError::Terminal(format!(
                        "route admission failed ({error}); lifecycle completion also failed: {completion_error}"
                    ))
                })?;
            return Err(error);
        }
        let result = self
            .inner
            .run_admitted(req, admitted.receipt().clone(), admitted.lifecycle())
            .await;
        self.control
            .close_provider_starts(&admitted.receipt().admission_id)
            .await
            .map_err(|error| {
                DispatchError::Terminal(format!(
                    "physical lifecycle could not close provider starts: {error}"
                ))
            })?;
        let completion = if result.is_ok() {
            LocalCompletionKind::Success
        } else {
            LocalCompletionKind::Error
        };
        admitted
            .complete_local_after_close(completion)
            .await
            .map_err(|error| {
                DispatchError::Terminal(format!(
                    "physical lifecycle rejected local completion: {error}"
                ))
            })?;
        result
    }
}
