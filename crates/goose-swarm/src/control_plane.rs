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
use crate::provider_lease::{
    ProviderLeaseBoundaryStatus, ProviderLeaseError, ProviderLeaseHttpBoundary,
    RunScopedProviderLeaseAuthority,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, Weak};
use tokio::sync::{oneshot, Mutex, Notify};

type AdmissionResult = Result<AdmissionReceipt, BrokerError>;
type ProviderRequestResult = Result<ProviderRequestReceipt, BrokerError>;

/// Lookup identity for the provider request currently owned by one admitted task attempt.
///
/// This key is routing data, not authority. The registry returns authority only when the key
/// selects an engine-published live request.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProviderStartKey {
    admission_id: String,
    task_id: String,
    attempt: u32,
}

impl ProviderStartKey {
    pub fn from_admission(admission: &AdmissionReceipt) -> Self {
        Self {
            admission_id: admission.admission_id.clone(),
            task_id: admission.source.task_id.clone(),
            attempt: admission.source.attempt,
        }
    }

    pub fn admission_id(&self) -> &str {
        &self.admission_id
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderStartLookupError {
    Missing {
        admission_id: String,
    },
    TaskMismatch {
        admission_id: String,
        expected_task_id: String,
        actual_task_id: String,
    },
    StaleAttempt {
        admission_id: String,
        task_id: String,
        expected_attempt: u32,
        actual_attempt: u32,
    },
    NotLive {
        admission_id: String,
    },
    Concurrent {
        admission_id: String,
    },
}

impl std::fmt::Display for ProviderStartLookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { admission_id } => {
                write!(formatter, "provider start `{admission_id}` is not published")
            }
            Self::TaskMismatch {
                admission_id,
                expected_task_id,
                actual_task_id,
            } => write!(
                formatter,
                "provider start `{admission_id}` belongs to task `{actual_task_id}`, not `{expected_task_id}`"
            ),
            Self::StaleAttempt {
                admission_id,
                task_id,
                expected_attempt,
                actual_attempt,
            } => write!(
                formatter,
                "provider start `{admission_id}` belongs to task `{task_id}` attempt {actual_attempt}, not attempt {expected_attempt}"
            ),
            Self::NotLive { admission_id } => {
                write!(formatter, "provider start `{admission_id}` is no longer live")
            }
            Self::Concurrent { admission_id } => write!(
                formatter,
                "provider start `{admission_id}` already has a different live request"
            ),
        }
    }
}

impl std::error::Error for ProviderStartLookupError {}

/// Dispatcher-owned cooperative delivery for one exact provider request.
///
/// Enqueue must be non-blocking. A successful enqueue reserves cancellation for this request; the
/// dispatcher queues the steer into its exact Agent/session and only then wakes `cancelled`.
#[async_trait]
pub trait ProviderNudgeDelivery: Send + Sync {
    fn try_enqueue(&self, guidance: String) -> Result<(), String>;
    fn natural_terminal_allowed(&self) -> bool;
    async fn cancelled(&self);
}

struct ProviderStartRegistryEntry {
    key: ProviderStartKey,
    request: Weak<ProviderRequestAuthority>,
}

/// Engine-owned channel from a lifecycle-wrapped provider call to its physical scheduler.
///
/// Entries retain only a weak reference to opaque request authority. They cannot keep a dropped or
/// terminal request alive, and no receipt is serialized or copied into the registry.
#[derive(Clone, Default)]
pub struct ProviderStartRegistry {
    entries: Arc<StdMutex<HashMap<String, ProviderStartRegistryEntry>>>,
}

impl ProviderStartRegistry {
    fn publish(
        &self,
        key: ProviderStartKey,
        request: &Arc<ProviderRequestAuthority>,
    ) -> Result<(), ProviderStartLookupError> {
        request.ensure_started_live()?;
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = entries.get(&key.admission_id) {
            if let Some(existing_request) = existing.request.upgrade() {
                if existing_request.is_started_live() && !Arc::ptr_eq(&existing_request, request) {
                    return Err(ProviderStartLookupError::Concurrent {
                        admission_id: key.admission_id,
                    });
                }
            }
        }
        entries.insert(
            key.admission_id.clone(),
            ProviderStartRegistryEntry {
                key,
                request: Arc::downgrade(request),
            },
        );
        Ok(())
    }

    pub fn query(
        &self,
        key: &ProviderStartKey,
    ) -> Result<ProviderStartSession, ProviderStartLookupError> {
        let (published_key, request) = {
            let entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = entries.get(&key.admission_id).ok_or_else(|| {
                ProviderStartLookupError::Missing {
                    admission_id: key.admission_id.clone(),
                }
            })?;
            if entry.key.task_id != key.task_id {
                return Err(ProviderStartLookupError::TaskMismatch {
                    admission_id: key.admission_id.clone(),
                    expected_task_id: key.task_id.clone(),
                    actual_task_id: entry.key.task_id.clone(),
                });
            }
            if entry.key.attempt != key.attempt {
                return Err(ProviderStartLookupError::StaleAttempt {
                    admission_id: key.admission_id.clone(),
                    task_id: key.task_id.clone(),
                    expected_attempt: key.attempt,
                    actual_attempt: entry.key.attempt,
                });
            }
            (entry.key.clone(), entry.request.clone())
        };
        let request = request
            .upgrade()
            .ok_or_else(|| ProviderStartLookupError::NotLive {
                admission_id: key.admission_id.clone(),
            })?;
        request.ensure_started_live()?;
        Ok(ProviderStartSession {
            key: published_key,
            request,
        })
    }
}

/// Opaque, non-cloneable access to the exact request published by [`StartedProviderRequest`].
pub struct ProviderStartSession {
    key: ProviderStartKey,
    request: Arc<ProviderRequestAuthority>,
}

impl std::fmt::Debug for ProviderStartSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStartSession")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl ProviderStartSession {
    pub fn key(&self) -> &ProviderStartKey {
        &self.key
    }

    pub fn ensure_live(&self) -> Result<(), ProviderStartLookupError> {
        self.request.ensure_started_live()
    }

    pub(crate) fn take_exposed_witness(
        &self,
    ) -> Result<ExposedProviderRequestWitness, ProviderLifecycleOperationError> {
        self.request.take_exposed_witness()
    }
}

struct ControlState {
    broker: PhysicalBroker,
    admission_waiters: HashMap<String, oneshot::Sender<AdmissionResult>>,
    provider_waiters: HashMap<(String, ProviderRequestKey), oneshot::Sender<ProviderRequestResult>>,
    released: HashMap<String, ReleasedAdmissionReceipt>,
}

pub trait ProviderLifecycleJournal: Send + Sync {
    fn provider_request_started(&self, receipt: &ProviderRequestReceipt) -> Result<(), String>;
    fn provider_terminal(&self, receipt: &ProviderTerminalReceipt) -> Result<(), String>;
}

struct NullProviderLifecycleJournal;

impl ProviderLifecycleJournal for NullProviderLifecycleJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        Ok(())
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        Ok(())
    }
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

    fn reject_waiters_after_journal_failure(&mut self, reason: &str, sink: &dyn EventSink) {
        let admission_waiters = std::mem::take(&mut self.admission_waiters);
        for (work_id, waiter) in admission_waiters {
            if let Some(receipt) = self.broker.withdraw_pending_work(&work_id) {
                sink.emit(&SwarmEvent::BrokerWorkWithdrawn { receipt });
            }
            let _ = waiter.send(Err(BrokerError::ProviderLifecycleJournal(
                reason.to_string(),
            )));
        }

        let provider_waiters = std::mem::take(&mut self.provider_waiters);
        for ((admission_id, key), waiter) in provider_waiters {
            let admission = self.broker.active_receipt(&admission_id).cloned();
            if let (Some(admission), Ok(receipt)) = (
                admission,
                self.broker
                    .withdraw_pending_provider_request(&admission_id, &key),
            ) {
                sink.emit(&SwarmEvent::BrokerProviderRequestWithdrawn {
                    admission,
                    receipt,
                    reason:
                        "provider lifecycle journal failed before the queued request was admitted"
                            .to_string(),
                });
            }
            let _ = waiter.send(Err(BrokerError::ProviderLifecycleJournal(
                reason.to_string(),
            )));
        }
    }
}

struct ControlInner {
    state: Mutex<ControlState>,
    sink: Arc<dyn EventSink>,
    journal: Arc<dyn ProviderLifecycleJournal>,
    journal_failure: StdMutex<Option<String>>,
    changed: Notify,
    semantic_observation_plane_claimed: AtomicBool,
    provider_leases: Option<RunScopedProviderLeaseAuthority>,
    provider_starts: ProviderStartRegistry,
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
        Self::new_with_journal(
            correlation_scope,
            snapshot,
            sink,
            Arc::new(NullProviderLifecycleJournal),
        )
    }

    pub fn new_with_journal(
        correlation_scope: impl Into<String>,
        snapshot: PhysicalFleetSnapshot,
        sink: Arc<dyn EventSink>,
        journal: Arc<dyn ProviderLifecycleJournal>,
    ) -> Result<Self, BrokerError> {
        Self::new_with_journal_and_provider_leases(correlation_scope, snapshot, sink, journal, None)
    }

    pub fn new_with_journal_and_provider_leases(
        correlation_scope: impl Into<String>,
        snapshot: PhysicalFleetSnapshot,
        sink: Arc<dyn EventSink>,
        journal: Arc<dyn ProviderLifecycleJournal>,
        provider_leases: Option<RunScopedProviderLeaseAuthority>,
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
                journal,
                journal_failure: StdMutex::new(None),
                changed: Notify::new(),
                semantic_observation_plane_claimed: AtomicBool::new(false),
                provider_leases,
                provider_starts: ProviderStartRegistry::default(),
            }),
        })
    }

    pub fn provider_start_registry(&self) -> ProviderStartRegistry {
        self.inner.provider_starts.clone()
    }

    fn journal_failure(&self) -> Option<String> {
        self.inner
            .journal_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn ensure_journal_healthy(&self) -> Result<(), BrokerError> {
        match self.journal_failure() {
            Some(reason) => Err(BrokerError::ProviderLifecycleJournal(reason)),
            None => Ok(()),
        }
    }

    fn reject_after_journal_failure(
        &self,
        state: &mut ControlState,
        failed_start: Option<&ProviderRequestReceipt>,
        reason: &str,
    ) {
        if let Some(receipt) = failed_start {
            let admission = state.broker.active_receipt(&receipt.admission_id).cloned();
            if let (Some(admission), Ok(revoked)) = (
                admission,
                state
                    .broker
                    .revoke_undelivered_provider_request(&receipt.admission_id, &receipt.key),
            ) {
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerProviderRequestGrantRevoked {
                        admission,
                        receipt: revoked,
                        reason:
                            "provider lifecycle start could not be durably journaled before HTTP"
                                .to_string(),
                    });
            }
        }
        state.reject_waiters_after_journal_failure(reason, self.inner.sink.as_ref());
    }

    fn pump_if_journal_healthy(&self, state: &mut ControlState) -> Result<(), BrokerError> {
        if let Some(reason) = self.journal_failure() {
            self.reject_after_journal_failure(state, None, &reason);
            return Err(BrokerError::ProviderLifecycleJournal(reason));
        }
        state.pump(self.inner.sink.as_ref());
        Ok(())
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
        self.ensure_journal_healthy()?;
        let mut state = self.inner.state.lock().await;
        let stale = match state.broker.set_source_revision(source) {
            Ok(stale) => stale,
            Err(error) => {
                self.emit_rejection(None, None, None, "source_revision", &error);
                return Err(error);
            }
        };
        state.reject_stale_waiters(&stale, self.inner.sink.as_ref());
        self.pump_if_journal_healthy(&mut state)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(())
    }

    pub async fn remove_source_revision(&self, source: &TaskVersion) -> Result<(), BrokerError> {
        self.ensure_journal_healthy()?;
        let mut state = self.inner.state.lock().await;
        let stale = match state.broker.remove_source_revision(source) {
            Ok(stale) => stale,
            Err(error) => {
                self.emit_rejection(None, None, None, "source_revision_removal", &error);
                return Err(error);
            }
        };
        state.reject_stale_waiters(&stale, self.inner.sink.as_ref());
        self.pump_if_journal_healthy(&mut state)?;
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
        self.ensure_journal_healthy()?;
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
        self.pump_if_journal_healthy(&mut state)?;
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
        self.ensure_journal_healthy()?;
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
            self.pump_if_journal_healthy(&mut state)?;
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
                outstanding: Arc::new(StdMutex::new(None)),
            },
            receipt,
        }))
    }

    pub(crate) async fn queue_admission(
        &self,
        opportunity: WorkOpportunity,
    ) -> Result<PendingAdmission, BrokerError> {
        self.ensure_journal_healthy()?;
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
            self.pump_if_journal_healthy(&mut state)?;
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

    pub async fn wait_until_drained(&self) -> Result<(), BrokerError> {
        self.ensure_journal_healthy()?;
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
                return Ok(());
            }
            changed.await;
            self.ensure_journal_healthy()?;
        }
    }

    async fn request_provider_turn(
        &self,
        receipt: ProviderRequestReceipt,
    ) -> Result<ProviderRequestReceipt, BrokerError> {
        self.ensure_journal_healthy()?;
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
                    if let Err(error) = self.journal_provider_start(&receipt) {
                        let reason = self.journal_failure().unwrap_or_else(|| error.to_string());
                        self.reject_after_journal_failure(&mut state, Some(&receipt), &reason);
                        drop(state);
                        self.inner.changed.notify_waiters();
                        return Err(error);
                    }
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
                    self.pump_if_journal_healthy(&mut state)?;
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
        if let Err(error) = self.journal_provider_start(&receipt) {
            let mut state = self.inner.state.lock().await;
            let reason = self.journal_failure().unwrap_or_else(|| error.to_string());
            self.reject_after_journal_failure(&mut state, Some(&receipt), &reason);
            drop(state);
            self.inner.changed.notify_waiters();
            return Err(error);
        }
        Ok(receipt)
    }

    async fn close_provider_starts(&self, admission_id: &str) -> Result<(), BrokerError> {
        self.ensure_journal_healthy()?;
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
        self.ensure_journal_healthy()?;
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
        self.ensure_journal_healthy()?;
        let admission_id = receipt.admission_id.clone();
        let mut state = self.inner.state.lock().await;
        let admission = state.broker.active_receipt(&admission_id).cloned();
        if let Err(error) = state.broker.validate_provider_terminal(&receipt) {
            self.emit_rejection(
                admission,
                None,
                Some(receipt.key.clone()),
                "provider_terminal",
                &error,
            );
            return Err(error);
        }
        if let Err(error) = self.journal_provider_terminal(&receipt) {
            let reason = self.journal_failure().unwrap_or_else(|| error.to_string());
            self.reject_after_journal_failure(&mut state, None, &reason);
            drop(state);
            self.inner.changed.notify_waiters();
            return Err(error);
        }
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
        self.ensure_journal_healthy()?;
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
        self.ensure_journal_healthy()?;
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
            self.ensure_journal_healthy()?;
        }
    }

    fn release_and_pump(
        &self,
        state: &mut ControlState,
        admission_id: &str,
    ) -> Result<(), BrokerError> {
        self.ensure_journal_healthy()?;
        if let Some(receipt) = state.broker.release_if_terminal(admission_id)? {
            self.inner.sink.emit(&SwarmEvent::BrokerAdmissionReleased {
                receipt: receipt.clone(),
            });
            state.released.insert(admission_id.to_string(), receipt);
        }
        // A provider terminal releases a physical turn permit even while the task envelope remains
        // active for local tool work. Re-run admission on every lifecycle transition, not only when
        // the whole task envelope is released.
        self.pump_if_journal_healthy(state)?;
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
        if self.journal_failure().is_none() {
            state.pump(self.inner.sink.as_ref());
        }
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
        if self.journal_failure().is_none() {
            state.pump(self.inner.sink.as_ref());
        }
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

    fn journal_provider_start(&self, receipt: &ProviderRequestReceipt) -> Result<(), BrokerError> {
        let reason = {
            let mut failure = self
                .inner
                .journal_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(reason) = failure.as_ref() {
                return Err(BrokerError::ProviderLifecycleJournal(reason.clone()));
            }
            match self.inner.journal.provider_request_started(receipt) {
                Ok(()) => return Ok(()),
                Err(reason) => {
                    *failure = Some(reason.clone());
                    reason
                }
            }
        };
        self.inner.sink.write_value(serde_json::json!({
            "event": "physical_provider_journal_failed",
            "transition": "provider_request_started",
            "admission_id": receipt.admission_id,
            "provider_request_id": receipt.key.provider_request_id,
            "reason": reason,
        }));
        self.inner.changed.notify_waiters();
        Err(BrokerError::ProviderLifecycleJournal(reason))
    }

    fn journal_provider_terminal(
        &self,
        receipt: &ProviderTerminalReceipt,
    ) -> Result<(), BrokerError> {
        let reason = {
            let mut failure = self
                .inner
                .journal_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(reason) = failure.as_ref() {
                return Err(BrokerError::ProviderLifecycleJournal(reason.clone()));
            }
            match self.inner.journal.provider_terminal(receipt) {
                Ok(()) => return Ok(()),
                Err(reason) => {
                    *failure = Some(reason.clone());
                    reason
                }
            }
        };
        self.inner.sink.write_value(serde_json::json!({
            "event": "physical_provider_journal_failed",
            "transition": "provider_terminal",
            "admission_id": receipt.admission_id,
            "provider_request_id": receipt.key.provider_request_id,
            "reason": reason,
        }));
        self.inner.changed.notify_waiters();
        Err(BrokerError::ProviderLifecycleJournal(reason))
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
                outstanding: Arc::new(StdMutex::new(None)),
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

    pub(crate) async fn complete_local_with_completion(
        self,
        kind: LocalCompletionKind,
    ) -> Result<CompletedAdmission, BrokerError> {
        self.lifecycle
            .control
            .close_provider_starts(&self.receipt.admission_id)
            .await?;
        self.complete_local_after_close(kind)
            .await
            .map(|released| CompletedAdmission { released })
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

/// Opaque proof of the broker's exact released admission state.
pub(crate) struct CompletedAdmission {
    released: ReleasedAdmissionReceipt,
}

impl std::fmt::Debug for CompletedAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompletedAdmission")
            .field("released", &self.released)
            .finish_non_exhaustive()
    }
}

impl CompletedAdmission {
    pub(crate) fn released(&self) -> &ReleasedAdmissionReceipt {
        &self.released
    }
}

#[derive(Debug, Default)]
struct ProviderRequestExposureState {
    witness_issued: bool,
    live_use_closed: bool,
}

struct ProviderRequestAuthority {
    receipt: Arc<ProviderRequestReceipt>,
    exposure: StdMutex<ProviderRequestExposureState>,
    boundary: StdMutex<Option<ProviderLeaseHttpBoundary>>,
    nudge_delivery: StdMutex<Option<Arc<dyn ProviderNudgeDelivery>>>,
}

impl ProviderRequestAuthority {
    fn new(receipt: ProviderRequestReceipt) -> Arc<Self> {
        Arc::new(Self {
            receipt: Arc::new(receipt),
            exposure: StdMutex::new(ProviderRequestExposureState::default()),
            boundary: StdMutex::new(None),
            nudge_delivery: StdMutex::new(None),
        })
    }

    fn resume(previous: &Self) -> Arc<Self> {
        Arc::new(Self {
            receipt: previous.receipt.clone(),
            exposure: StdMutex::new(ProviderRequestExposureState::default()),
            boundary: StdMutex::new(None),
            nudge_delivery: StdMutex::new(None),
        })
    }

    fn is_started_live(&self) -> bool {
        !self
            .exposure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .live_use_closed
    }

    fn ensure_started_live(&self) -> Result<(), ProviderStartLookupError> {
        if self.is_started_live() {
            Ok(())
        } else {
            Err(ProviderStartLookupError::NotLive {
                admission_id: self.receipt.admission_id.clone(),
            })
        }
    }

    fn close_live_use(&self) {
        self.exposure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .live_use_closed = true;
    }

    fn bind_scheduler_runtime(
        &self,
        boundary: Option<ProviderLeaseHttpBoundary>,
        nudge_delivery: Option<Arc<dyn ProviderNudgeDelivery>>,
    ) -> Result<(), ProviderStartLookupError> {
        self.ensure_started_live()?;
        *self
            .boundary
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = boundary;
        *self
            .nudge_delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = nudge_delivery;
        Ok(())
    }

    fn take_exposed_witness(
        self: &Arc<Self>,
    ) -> Result<ExposedProviderRequestWitness, ProviderLifecycleOperationError> {
        let status = self
            .boundary
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .ok_or_else(|| {
                ProviderLifecycleOperationError::Unresolved(
                    "provider request has no verified physical lease boundary".to_string(),
                )
            })?
            .status()?;
        if status != ProviderLeaseBoundaryStatus::Exposed {
            return Err(ProviderLifecycleOperationError::Unresolved(format!(
                "provider request is not exposed at witness mint: {status:?}"
            )));
        }
        let mut state = self
            .exposure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.live_use_closed || state.witness_issued {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "provider request exposure witness is closed or already issued".to_string(),
            ));
        }
        state.witness_issued = true;
        Ok(ExposedProviderRequestWitness {
            request: self.clone(),
        })
    }
}

/// One-shot proof that the exact engine-owned request crossed its verified provider boundary.
///
/// Receipt bytes alone are not authority. This witness is non-cloneable, is minted only while
/// borrowing the live [`StartedProviderRequest`], and shares terminal/drop state with that request.
pub(crate) struct ExposedProviderRequestWitness {
    request: Arc<ProviderRequestAuthority>,
}

pub(crate) struct LiveProviderRequestSession {
    request: Arc<ProviderRequestAuthority>,
}

impl std::fmt::Debug for LiveProviderRequestSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveProviderRequestSession")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ExposedProviderRequestWitness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExposedProviderRequestWitness")
            .finish_non_exhaustive()
    }
}

pub(crate) struct LiveProviderRequestPin<'a> {
    _state: StdMutexGuard<'a, ProviderRequestExposureState>,
}

/// Opaque proof that one exact engine-owned provider request reached an accepted terminal.
///
/// The request/terminal pair cannot be reconstructed from public receipts: this value is
/// non-cloneable and is minted only after the lease, journal, and physical broker all accept the
/// terminal transition.
pub(crate) struct CompletedProviderRequest {
    admission: AdmissionReceipt,
    request: ProviderRequestReceipt,
    terminal: ProviderTerminalReceipt,
}

impl std::fmt::Debug for CompletedProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompletedProviderRequest")
            .field("admission", &self.admission)
            .field("request", &self.request)
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

impl CompletedProviderRequest {
    pub(crate) fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub(crate) fn request(&self) -> &ProviderRequestReceipt {
        &self.request
    }

    pub(crate) fn terminal(&self) -> &ProviderTerminalReceipt {
        &self.terminal
    }

    #[cfg(test)]
    pub(crate) fn forge_spliced_for_replay(request_from: &Self, terminal_from: &Self) -> Self {
        Self {
            admission: request_from.admission.clone(),
            request: request_from.request.clone(),
            terminal: terminal_from.terminal.clone(),
        }
    }
}

impl ExposedProviderRequestWitness {
    pub(crate) fn try_pin(
        &self,
    ) -> Result<LiveProviderRequestPin<'_>, ProviderLifecycleOperationError> {
        pin_live_provider_request(&self.request.exposure)
    }

    pub(crate) fn bind_provider_start_session(
        &self,
        started: &ProviderStartSession,
    ) -> Result<LiveProviderRequestSession, ProviderLifecycleOperationError> {
        if !Arc::ptr_eq(&self.request, &started.request) {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "provider exposure witness does not belong to the registry session".to_string(),
            ));
        }
        let pin = self.try_pin()?;
        drop(pin);
        Ok(LiveProviderRequestSession {
            request: self.request.clone(),
        })
    }
}

impl LiveProviderRequestSession {
    pub(crate) fn receipt(&self) -> &ProviderRequestReceipt {
        &self.request.receipt
    }

    pub(crate) fn try_pin(
        &self,
    ) -> Result<LiveProviderRequestPin<'_>, ProviderLifecycleOperationError> {
        pin_live_provider_request(&self.request.exposure)
    }

    pub(crate) fn try_enqueue_nudge(
        &self,
        guidance: String,
        on_pinned_enqueue: impl FnOnce(),
    ) -> Result<(), String> {
        let _pin = self.try_pin().map_err(|error| error.to_string())?;
        let delivery = self
            .request
            .nudge_delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| "provider request has no dispatcher-owned nudge delivery".to_string())?;
        delivery.try_enqueue(guidance)?;
        on_pinned_enqueue();
        Ok(())
    }
}

fn pin_live_provider_request(
    state: &StdMutex<ProviderRequestExposureState>,
) -> Result<LiveProviderRequestPin<'_>, ProviderLifecycleOperationError> {
    let state = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !state.witness_issued || state.live_use_closed {
        return Err(ProviderLifecycleOperationError::Unresolved(
            "provider request exposure witness is no longer live".to_string(),
        ));
    }
    Ok(LiveProviderRequestPin { _state: state })
}

struct RecoverableProviderRequest {
    request: Arc<ProviderRequestAuthority>,
    boundary: Option<ProviderLeaseHttpBoundary>,
    issued_to_provider: bool,
    pending_terminal: Option<ProviderTerminalKind>,
}

enum OutstandingProviderRequest {
    Starting,
    Claimed,
    Recoverable(RecoverableProviderRequest),
    Failed(String),
}

#[derive(Debug)]
pub enum ProviderLifecycleOperationError {
    Broker(BrokerError),
    Lease(ProviderLeaseError),
    Unresolved(String),
}

impl std::fmt::Display for ProviderLifecycleOperationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Broker(error) => write!(formatter, "{error}"),
            Self::Lease(error) => write!(formatter, "{error}"),
            Self::Unresolved(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for ProviderLifecycleOperationError {}

impl From<BrokerError> for ProviderLifecycleOperationError {
    fn from(error: BrokerError) -> Self {
        Self::Broker(error)
    }
}

impl From<ProviderLeaseError> for ProviderLifecycleOperationError {
    fn from(error: ProviderLeaseError) -> Self {
        Self::Lease(error)
    }
}

#[derive(Debug)]
pub enum ProviderLifecycleTransitionError {
    Retryable {
        error: ProviderLifecycleOperationError,
        request: Box<StartedProviderRequest>,
    },
    Fatal(ProviderLifecycleOperationError),
}

impl ProviderLifecycleTransitionError {
    pub fn error(&self) -> &ProviderLifecycleOperationError {
        match self {
            Self::Retryable { error, .. } | Self::Fatal(error) => error,
        }
    }

    pub fn into_retryable_request(self) -> Option<StartedProviderRequest> {
        match self {
            Self::Retryable { request, .. } => Some(*request),
            Self::Fatal(_) => None,
        }
    }
}

impl std::fmt::Display for ProviderLifecycleTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error().fmt(formatter)
    }
}

impl std::error::Error for ProviderLifecycleTransitionError {}

#[derive(Debug)]
pub enum ProviderLifecycleStartError {
    Operation(ProviderLifecycleOperationError),
    TerminalReconciliation(ProviderLifecycleTransitionError),
}

impl std::fmt::Display for ProviderLifecycleStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operation(error) => error.fmt(formatter),
            Self::TerminalReconciliation(error) => {
                write!(
                    formatter,
                    "prior provider terminal reconciliation failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for ProviderLifecycleStartError {}

impl From<BrokerError> for ProviderLifecycleStartError {
    fn from(error: BrokerError) -> Self {
        Self::Operation(error.into())
    }
}

impl From<ProviderLeaseError> for ProviderLifecycleStartError {
    fn from(error: ProviderLeaseError) -> Self {
        Self::Operation(error.into())
    }
}

pub struct StartedProviderRequest {
    lifecycle: ProviderLifecycle,
    request: Option<Arc<ProviderRequestAuthority>>,
    boundary: Option<ProviderLeaseHttpBoundary>,
    pending_terminal: Option<ProviderTerminalKind>,
    armed: bool,
}

impl std::fmt::Debug for StartedProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StartedProviderRequest")
            .field("boundary", &self.boundary)
            .field("pending_terminal", &self.pending_terminal)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl StartedProviderRequest {
    pub fn receipt(&self) -> &ProviderRequestReceipt {
        &self
            .request
            .as_ref()
            .expect("live started provider request retains its engine authority")
            .receipt
    }

    fn request_authority(&self) -> &Arc<ProviderRequestAuthority> {
        self.request
            .as_ref()
            .expect("live started provider request retains its engine authority")
    }

    #[cfg(test)]
    pub(crate) fn provider_start_session_for_test(
        &self,
    ) -> Result<ProviderStartSession, ProviderStartLookupError> {
        self.request_authority()
            .bind_scheduler_runtime(self.boundary.clone(), None)?;
        Ok(ProviderStartSession {
            key: ProviderStartKey::from_admission(&self.lifecycle.admission),
            request: self.request_authority().clone(),
        })
    }

    /// Publish this exact engine-owned request to the scheduler before entering provider HTTP.
    pub fn publish_for_scheduler(&self) -> Result<(), ProviderStartLookupError> {
        self.publish_for_scheduler_with_delivery(None)
    }

    pub fn publish_for_scheduler_with_nudge_delivery(
        &self,
        delivery: Arc<dyn ProviderNudgeDelivery>,
    ) -> Result<(), ProviderStartLookupError> {
        self.publish_for_scheduler_with_delivery(Some(delivery))
    }

    fn publish_for_scheduler_with_delivery(
        &self,
        delivery: Option<Arc<dyn ProviderNudgeDelivery>>,
    ) -> Result<(), ProviderStartLookupError> {
        let key = ProviderStartKey::from_admission(&self.lifecycle.admission);
        if self.receipt().admission_id != key.admission_id {
            return Err(ProviderStartLookupError::Missing {
                admission_id: key.admission_id,
            });
        }
        self.request_authority()
            .bind_scheduler_runtime(self.boundary.clone(), delivery)?;
        self.lifecycle
            .control
            .inner
            .provider_starts
            .publish(key, self.request_authority())
    }

    pub fn http_protocol(&self) -> Option<goose_provider_types::base::ProviderHttpProtocol> {
        self.boundary
            .as_ref()
            .map(ProviderLeaseHttpBoundary::protocol)
    }

    pub fn transport_identity(&self) -> Option<&str> {
        self.boundary
            .as_ref()
            .map(ProviderLeaseHttpBoundary::transport_identity)
    }

    pub async fn scope_http<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        match &self.boundary {
            Some(boundary) => boundary.scope_http(future).await,
            None => future.await,
        }
    }

    pub async fn provider_terminal(
        self,
        kind: ProviderTerminalKind,
    ) -> Result<(), ProviderLifecycleTransitionError> {
        self.provider_terminal_with_completion(kind).await.map(drop)
    }

    pub(crate) async fn provider_terminal_with_completion(
        mut self,
        kind: ProviderTerminalKind,
    ) -> Result<CompletedProviderRequest, ProviderLifecycleTransitionError> {
        self.close_live_use();
        if self.pending_terminal.is_some_and(|pending| pending != kind) {
            let error = ProviderLifecycleOperationError::Unresolved(
                "provider request has a different pending terminal kind".to_string(),
            );
            self.latch_failure(error.to_string());
            return Err(ProviderLifecycleTransitionError::Fatal(error));
        }
        self.pending_terminal = Some(kind);
        let terminal = self.terminal_receipt(kind);
        if let Some(boundary) = &self.boundary {
            let already_abandoned = matches!(
                boundary.status(),
                Ok(ProviderLeaseBoundaryStatus::Abandoned)
            );
            if !already_abandoned {
                if let Err(error) = boundary.provider_terminal(&terminal) {
                    if error == ProviderLeaseError::AuthorityContended {
                        return Err(ProviderLifecycleTransitionError::Retryable {
                            error: error.into(),
                            request: Box::new(self),
                        });
                    }
                    let error = ProviderLifecycleOperationError::Lease(error);
                    self.latch_failure(error.to_string());
                    return Err(ProviderLifecycleTransitionError::Fatal(error));
                }
            } else if kind != ProviderTerminalKind::Cancelled {
                let error = ProviderLifecycleOperationError::Unresolved(
                    "an abandoned reservation can reconcile only a cancelled terminal".to_string(),
                );
                self.latch_failure(error.to_string());
                return Err(ProviderLifecycleTransitionError::Fatal(error));
            }
        }
        if let Err(error) = self
            .lifecycle
            .control
            .observe_provider_terminal(terminal.clone())
            .await
        {
            return Err(ProviderLifecycleTransitionError::Retryable {
                error: error.into(),
                request: Box::new(self),
            });
        }
        let admission = self.lifecycle.admission.clone();
        let request = self.receipt().clone();
        self.complete();
        Ok(CompletedProviderRequest {
            admission,
            request,
            terminal,
        })
    }

    pub async fn abandon_before_exposure(
        mut self,
        reason: &str,
    ) -> Result<(), ProviderLifecycleTransitionError> {
        self.close_live_use();
        self.pending_terminal = Some(ProviderTerminalKind::Cancelled);
        if let Some(boundary) = &self.boundary {
            if let Err(error) = boundary.abandon_reserved(reason) {
                if error == ProviderLeaseError::AuthorityContended {
                    return Err(ProviderLifecycleTransitionError::Retryable {
                        error: error.into(),
                        request: Box::new(self),
                    });
                }
                let error = ProviderLifecycleOperationError::Lease(error);
                self.latch_failure(error.to_string());
                return Err(ProviderLifecycleTransitionError::Fatal(error));
            }
        }
        let terminal = self.terminal_receipt(ProviderTerminalKind::Cancelled);
        if let Err(error) = self
            .lifecycle
            .control
            .observe_provider_terminal(terminal)
            .await
        {
            return Err(ProviderLifecycleTransitionError::Retryable {
                error: error.into(),
                request: Box::new(self),
            });
        }
        self.complete();
        Ok(())
    }

    fn terminal_receipt(&self, kind: ProviderTerminalKind) -> ProviderTerminalReceipt {
        let receipt = self.receipt();
        ProviderTerminalReceipt {
            admission_id: receipt.admission_id.clone(),
            key: receipt.key.clone(),
            physical_host_id: receipt.physical_host_id.clone(),
            model_instance_id: receipt.model_instance_id.clone(),
            kind,
        }
    }

    fn complete(&mut self) {
        self.close_live_use();
        let mut outstanding = self
            .lifecycle
            .outstanding
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(
            outstanding.as_ref(),
            Some(OutstandingProviderRequest::Claimed)
        ) {
            *outstanding = None;
        }
        self.armed = false;
        self.request.take();
        self.boundary.take();
    }

    fn latch_failure(&mut self, reason: String) {
        self.close_live_use();
        let mut outstanding = self
            .lifecycle
            .outstanding
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *outstanding = Some(OutstandingProviderRequest::Failed(reason));
        self.armed = false;
        self.request.take();
        self.boundary.take();
    }

    fn close_live_use(&self) {
        if let Some(request) = &self.request {
            request.close_live_use();
        }
    }
}

impl Drop for StartedProviderRequest {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.close_live_use();
        let Some(request) = self.request.take() else {
            return;
        };
        let mut outstanding = self
            .lifecycle
            .outstanding
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(
            outstanding.as_ref(),
            Some(OutstandingProviderRequest::Claimed)
        ) {
            *outstanding = Some(OutstandingProviderRequest::Recoverable(
                RecoverableProviderRequest {
                    request: ProviderRequestAuthority::resume(&request),
                    boundary: self.boundary.take(),
                    issued_to_provider: true,
                    pending_terminal: self.pending_terminal,
                },
            ));
        }
    }
}

struct StartingProviderRequestGuard {
    outstanding: Arc<StdMutex<Option<OutstandingProviderRequest>>>,
    armed: bool,
}

impl Drop for StartingProviderRequestGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut outstanding = self
            .outstanding
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(
            outstanding.as_ref(),
            Some(OutstandingProviderRequest::Starting)
        ) {
            *outstanding = None;
        }
    }
}

struct ClaimedProviderRequestGuard {
    lifecycle: ProviderLifecycle,
    request: Option<RecoverableProviderRequest>,
    armed: bool,
}

impl ClaimedProviderRequestGuard {
    fn into_started(mut self) -> StartedProviderRequest {
        let request = self
            .request
            .take()
            .expect("claimed provider request is present");
        self.armed = false;
        StartedProviderRequest {
            lifecycle: self.lifecycle.clone(),
            request: Some(request.request),
            boundary: request.boundary,
            pending_terminal: request.pending_terminal,
            armed: true,
        }
    }
}

impl Drop for ClaimedProviderRequestGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(request) = self.request.take() else {
            return;
        };
        let mut outstanding = self
            .lifecycle
            .outstanding
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(
            outstanding.as_ref(),
            Some(OutstandingProviderRequest::Claimed)
        ) {
            *outstanding = Some(OutstandingProviderRequest::Recoverable(request));
        }
    }
}

enum ProviderStartAction {
    Start,
    Claim(RecoverableProviderRequest),
    Wait,
    Failed(String),
}

#[derive(Clone)]
pub struct ProviderLifecycle {
    control: PhysicalAdmissionControl,
    admission: AdmissionReceipt,
    next_ordinal: Arc<AtomicU32>,
    outstanding: Arc<StdMutex<Option<OutstandingProviderRequest>>>,
}

impl ProviderLifecycle {
    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub fn shares_admission_control(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.control.inner, &other.control.inner)
    }

    pub async fn start_provider_request(
        &self,
    ) -> Result<StartedProviderRequest, ProviderLifecycleStartError> {
        loop {
            let action = {
                let mut outstanding = self
                    .outstanding
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match outstanding.take() {
                    None => {
                        *outstanding = Some(OutstandingProviderRequest::Starting);
                        ProviderStartAction::Start
                    }
                    Some(OutstandingProviderRequest::Recoverable(request)) => {
                        *outstanding = Some(OutstandingProviderRequest::Claimed);
                        ProviderStartAction::Claim(request)
                    }
                    Some(OutstandingProviderRequest::Starting) => {
                        *outstanding = Some(OutstandingProviderRequest::Starting);
                        ProviderStartAction::Wait
                    }
                    Some(OutstandingProviderRequest::Claimed) => {
                        *outstanding = Some(OutstandingProviderRequest::Claimed);
                        ProviderStartAction::Wait
                    }
                    Some(OutstandingProviderRequest::Failed(reason)) => {
                        *outstanding = Some(OutstandingProviderRequest::Failed(reason.clone()));
                        ProviderStartAction::Failed(reason)
                    }
                }
            };

            match action {
                ProviderStartAction::Wait => tokio::task::yield_now().await,
                ProviderStartAction::Failed(reason) => {
                    return Err(ProviderLifecycleStartError::Operation(
                        ProviderLifecycleOperationError::Unresolved(reason),
                    ));
                }
                ProviderStartAction::Start => {
                    let mut guard = StartingProviderRequestGuard {
                        outstanding: self.outstanding.clone(),
                        armed: true,
                    };
                    let ordinal = self.next_ordinal.fetch_add(1, Ordering::SeqCst);
                    let request = ProviderRequestReceipt {
                        admission_id: self.admission.admission_id.clone(),
                        key: ProviderRequestKey {
                            ordinal,
                            provider_request_id: format!(
                                "engine-provider-request:{:032x}",
                                rand::random::<u128>()
                            ),
                        },
                        physical_host_id: self.admission.physical_host_id.clone(),
                        model_instance_id: self.admission.model_instance_id.clone(),
                    };
                    let receipt = self.control.request_provider_turn(request).await?;
                    let mut outstanding = self
                        .outstanding
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if !matches!(
                        outstanding.as_ref(),
                        Some(OutstandingProviderRequest::Starting)
                    ) {
                        return Err(ProviderLifecycleStartError::Operation(
                            ProviderLifecycleOperationError::Unresolved(
                                "provider request start ownership changed before sealing"
                                    .to_string(),
                            ),
                        ));
                    }
                    *outstanding = Some(OutstandingProviderRequest::Recoverable(
                        RecoverableProviderRequest {
                            request: ProviderRequestAuthority::new(receipt),
                            boundary: None,
                            issued_to_provider: false,
                            pending_terminal: None,
                        },
                    ));
                    guard.armed = false;
                }
                ProviderStartAction::Claim(request) => {
                    let mut guard = ClaimedProviderRequestGuard {
                        lifecycle: self.clone(),
                        request: Some(request),
                        armed: true,
                    };
                    let recoverable = guard
                        .request
                        .as_mut()
                        .expect("claimed provider request is present");
                    if recoverable.issued_to_provider {
                        let may_resume = match &recoverable.boundary {
                            Some(boundary) if recoverable.pending_terminal.is_some() => matches!(
                                boundary.status(),
                                Ok(ProviderLeaseBoundaryStatus::Exposed)
                                    | Ok(ProviderLeaseBoundaryStatus::Terminal)
                                    | Ok(ProviderLeaseBoundaryStatus::Abandoned)
                            ),
                            Some(boundary) => matches!(
                                boundary.status(),
                                Ok(ProviderLeaseBoundaryStatus::Reserved)
                            ),
                            None => recoverable.pending_terminal.is_some(),
                        };
                        if !may_resume {
                            return Err(ProviderLifecycleStartError::Operation(
                                ProviderLifecycleOperationError::Unresolved(
                                    "prior provider request may already be externally visible and has no exact terminal proof"
                                        .to_string(),
                                ),
                            ));
                        }
                    }
                    if recoverable.boundary.is_none() {
                        if let Some(authority) = &self.control.inner.provider_leases {
                            recoverable.boundary = Some(
                                authority
                                    .reserve_provider_request(
                                        &self.admission,
                                        &recoverable.request.receipt,
                                    )
                                    .await?,
                            );
                        }
                    }
                    let pending_terminal = recoverable.pending_terminal;
                    let started = guard.into_started();
                    if let Some(kind) = pending_terminal {
                        match started.provider_terminal(kind).await {
                            Ok(()) => continue,
                            Err(error) => {
                                return Err(ProviderLifecycleStartError::TerminalReconciliation(
                                    error,
                                ));
                            }
                        }
                    }
                    return Ok(started);
                }
            }
        }
    }

    pub async fn provider_request_started(
        &self,
        provider_request_id: impl Into<String>,
    ) -> Result<ProviderRequestKey, BrokerError> {
        if self.control.inner.provider_leases.is_some() {
            return Err(BrokerError::InvalidProviderRequest {
                admission_id: self.admission.admission_id.clone(),
                reason: "caller-supplied provider request identities are disabled by the sealed lease authority"
                    .to_string(),
            });
        }
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
        if self.control.inner.provider_leases.is_some() {
            return Err(BrokerError::InvalidProviderRequest {
                admission_id: self.admission.admission_id.clone(),
                reason: "caller-constructed provider terminals are disabled by the sealed lease authority"
                    .to_string(),
            });
        }
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

#[cfg(test)]
mod provider_start_registry_tests {
    use super::*;

    fn request_authority(admission_id: &str) -> Arc<ProviderRequestAuthority> {
        ProviderRequestAuthority::new(ProviderRequestReceipt {
            admission_id: admission_id.to_string(),
            key: ProviderRequestKey {
                ordinal: 0,
                provider_request_id: "engine-provider-request:test-only".to_string(),
            },
            physical_host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
        })
    }

    fn key(task_id: &str, attempt: u32) -> ProviderStartKey {
        ProviderStartKey {
            admission_id: "admission-a".to_string(),
            task_id: task_id.to_string(),
            attempt,
        }
    }

    #[test]
    fn provider_start_registry_rejects_cross_task_and_stale_attempt_queries() {
        let registry = ProviderStartRegistry::default();
        let authority = request_authority("admission-a");
        registry.publish(key("task-a", 2), &authority).unwrap();

        assert!(matches!(
            registry.query(&key("task-b", 2)),
            Err(ProviderStartLookupError::TaskMismatch { .. })
        ));
        assert!(matches!(
            registry.query(&key("task-a", 1)),
            Err(ProviderStartLookupError::StaleAttempt { .. })
        ));
        registry.query(&key("task-a", 2)).unwrap();
    }

    #[test]
    fn provider_start_registry_rejects_terminal_and_dropped_requests() {
        let registry = ProviderStartRegistry::default();
        let authority = request_authority("admission-a");
        let provider_start = key("task-a", 0);
        registry
            .publish(provider_start.clone(), &authority)
            .unwrap();
        let session = registry.query(&provider_start).unwrap();

        authority.close_live_use();
        assert!(matches!(
            session.ensure_live(),
            Err(ProviderStartLookupError::NotLive { .. })
        ));
        assert!(matches!(
            registry.query(&provider_start),
            Err(ProviderStartLookupError::NotLive { .. })
        ));

        let dropped_registry = ProviderStartRegistry::default();
        let dropped_authority = request_authority("admission-a");
        dropped_registry
            .publish(provider_start.clone(), &dropped_authority)
            .unwrap();
        drop(dropped_authority);
        assert!(matches!(
            dropped_registry.query(&provider_start),
            Err(ProviderStartLookupError::NotLive { .. })
        ));
    }
}
