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
    QuarantinedAdmissionReceipt, ReleasedAdmissionReceipt, StaleWorkReceipt, TaskVersion,
    VerifiedPhysicalLane, WorkOpportunity,
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
use serde::Serialize;
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
    RuntimeBinding {
        admission_id: String,
        reason: String,
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
            Self::RuntimeBinding {
                admission_id,
                reason,
            } => write!(
                formatter,
                "provider start `{admission_id}` runtime binding failed: {reason}"
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
    fn bind_request(&self, request: &ProviderRequestReceipt) -> Result<(), String>;

    /// Atomically compare the semantic capture's provider-stream evidence with the live stream and
    /// reserve one nudge while the same progress lock remains held. Implementations that cannot
    /// prove this correspondence fail closed.
    fn reserve_at_capture(
        &self,
        _capture: ProviderNudgeSafetySnapshot,
        _reserve: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        Err("provider nudge delivery has no stream-safety authority".to_string())
    }

    fn try_enqueue(&self, guidance: String) -> Result<(), String>;
    fn natural_terminal_allowed(&self) -> bool;
    fn cancellation_terminal_confirmation_required(&self) -> bool;
    async fn cancelled(&self);

    /// Accept the non-forgeable proof that the exact request reserved by this delivery reached
    /// the broker, journal, and provider-boundary `Cancelled` terminal.
    fn confirm_cancelled_terminal(&self, terminal: CompletedProviderRequest) -> Result<(), String>;

    /// Wait for the accepted cancellation terminal, not merely for a queued steer.
    async fn confirmed_cancelled_terminal(&self) -> Result<ProviderTerminalReceipt, String>;
}

/// Provider-stream fields sealed into the semantic capture that authorizes a nudge.
///
/// The full stream revision is retained as provenance. Nudge safety specifically requires the
/// structured-output fields to remain unchanged through reservation; ordinary reasoning progress
/// may continue while the independent judge runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderNudgeSafetySnapshot {
    pub provider_stream_revision: u64,
    pub provider_stream_chunks: u64,
    pub provider_stream_bytes: u64,
    pub provider_structured_output_chunks: u64,
    pub provider_structured_output_bytes: u64,
    pub provider_last_progress_elapsed_ms: u64,
    pub provider_structured_output_active: bool,
}

/// Serializes a semantic nudge's final safety check with its delivery reservation.
///
/// Implementations hold the same authority lock used by the changing safety signal while they
/// invoke `reserve`. This makes progress-before-reservation observable and progress-after-
/// reservation ordered after cancellation without exposing provider payloads here.
pub trait ProviderNudgeSafetyGate: Send + Sync {
    fn reserve(&self, reserve: &mut dyn FnMut() -> Result<(), String>) -> Result<(), String>;
}

struct ProviderStartRegistryEntry {
    key: ProviderStartKey,
    provider_request: ProviderRequestKey,
    request: Weak<ProviderRequestAuthority>,
}

/// Engine-owned channel from a lifecycle-wrapped provider call to its physical scheduler.
///
/// Entries retain only a weak reference to opaque request authority plus the non-authoritative
/// request key needed to retire one terminal observation binding. They cannot keep a dropped or
/// terminal request alive, and no receipt is serialized into the registry.
#[derive(Clone, Default)]
pub struct ProviderStartRegistry {
    entries: Arc<StdMutex<HashMap<String, ProviderStartRegistryEntry>>>,
    changed: Arc<Notify>,
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
                provider_request: request.receipt.key.clone(),
                request: Arc::downgrade(request),
            },
        );
        *request
            .provider_start_changed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(self.changed.clone());
        self.changed.notify_one();
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

    /// Return the exact request key most recently published for this task/admission binding even
    /// after that request terminalizes. The key carries no live-use authority; it only lets the
    /// scheduler suppress repeated capture attempts until a different request is published.
    pub(crate) fn current_request_key(
        &self,
        key: &ProviderStartKey,
    ) -> Result<Option<ProviderRequestKey>, ProviderStartLookupError> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = entries.get(&key.admission_id) else {
            return Ok(None);
        };
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
        Ok(Some(entry.provider_request.clone()))
    }

    pub(crate) async fn changed(&self) {
        self.changed.notified().await;
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
                state.broker.unresolved_admissions_for_drain(),
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
            let drained = {
                let state = self.inner.state.lock().await;
                state.broker.pending_len() == 0 && state.broker.active_len_for_drain() == 0
            };
            if drained {
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

    async fn quarantine_unproven_admission(
        &self,
        admission_id: &str,
        reason: String,
    ) -> Result<QuarantinedAdmissionReceipt, BrokerError> {
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
                        reason: "provider lifecycle closed before quarantine adjudication"
                            .to_string(),
                    });
            }
        }

        let admission = state.broker.active_receipt(admission_id).cloned();
        let local = state
            .broker
            .record_local_completion(admission_id, LocalCompletionKind::Error)?;
        self.inner.sink.emit(&SwarmEvent::BrokerWorkLocalCompleted {
            admission: admission.expect("quarantined completion belongs to active admission"),
            receipt: local,
        });
        let outcome = state
            .broker
            .quarantine_unresolved_admission(admission_id, reason)?;
        self.inner
            .sink
            .emit(&SwarmEvent::BrokerAdmissionQuarantined {
                receipt: Box::new(outcome.receipt.clone()),
            });
        for receipt in outcome.withdrawn_work {
            if let Some(waiter) = state.admission_waiters.remove(&receipt.work_id) {
                let _ = waiter.send(Err(BrokerError::InvalidOpportunity {
                    work_id: receipt.work_id.clone(),
                    reason: "every eligible physical host is quarantined by an unresolved provider request"
                        .to_string(),
                }));
            }
            self.inner
                .sink
                .emit(&SwarmEvent::BrokerWorkWithdrawn { receipt });
        }
        for receipt in outcome.withdrawn_provider_requests {
            let admission = state.broker.active_receipt(&receipt.admission_id).cloned();
            if let Some(waiter) = state
                .provider_waiters
                .remove(&(receipt.admission_id.clone(), receipt.key.clone()))
            {
                let _ = waiter.send(Err(BrokerError::InvalidProviderRequest {
                    admission_id: receipt.admission_id.clone(),
                    reason: "physical host is quarantined by an unresolved provider request"
                        .to_string(),
                }));
            }
            if let Some(admission) = admission {
                self.inner
                    .sink
                    .emit(&SwarmEvent::BrokerProviderRequestWithdrawn {
                        admission,
                        receipt,
                        reason: "physical host quarantined after an unproven provider outcome"
                            .to_string(),
                    });
            }
        }
        self.pump_if_journal_healthy(&mut state)?;
        drop(state);
        self.inner.changed.notify_waiters();
        Ok(outcome.receipt)
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

    /// Adjudicate a locally failed call whose exposed provider request has no terminal proof.
    ///
    /// The admission and its physical permit remain unresolved and the whole physical host is
    /// excluded from later admission. Phase drain may continue on other hosts, but no release or
    /// provider terminal receipt is fabricated for the quarantined request.
    pub async fn quarantine_unproven(
        self,
        reason: String,
    ) -> Result<QuarantinedAdmissionReceipt, BrokerError> {
        self.lifecycle
            .control
            .quarantine_unproven_admission(&self.receipt.admission_id, reason)
            .await
    }

    pub async fn complete_local(&self, kind: LocalCompletionKind) -> Result<(), BrokerError> {
        self.lifecycle
            .control
            .close_provider_starts(&self.receipt.admission_id)
            .await?;
        self.complete_local_after_close(kind).await.map(|_| ())
    }

    pub async fn complete_local_with_completion(
        self,
        kind: LocalCompletionKind,
    ) -> Result<CompletedAdmission, BrokerError> {
        self.lifecycle
            .control
            .close_provider_starts(&self.receipt.admission_id)
            .await?;
        self.complete_local_after_close(kind)
            .await
            .map(|released| CompletedAdmission {
                control: self.lifecycle.control.clone(),
                released,
            })
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
pub struct CompletedAdmission {
    control: PhysicalAdmissionControl,
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
    pub fn released(&self) -> &ReleasedAdmissionReceipt {
        &self.released
    }
}

/// Payload-free proof that one exact live provider request accepted a semantic redirect.
///
/// The receipt is minted only after the dispatcher-owned delivery channel returns the accepted
/// `Cancelled` terminal for the exact captured request. A queued steer alone is not delivery proof.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderNudgeDeliveryReceipt {
    pub delivery_id: String,
    pub observation_snapshot_hash: String,
    pub source_admission_id: String,
    pub source_provider_request: ProviderRequestKey,
    pub source_cancel_terminal: ProviderTerminalReceipt,
    pub source_physical_host_id: String,
    pub source_model_instance_id: String,
    pub judge_admission_id: String,
    pub judge_provider_request: ProviderRequestKey,
}

#[derive(Debug, Default)]
struct ProviderRequestExposureState {
    witness_issued: bool,
    live_use_closed: bool,
}

struct ProviderRequestAuthority {
    receipt: Arc<ProviderRequestReceipt>,
    exposure: StdMutex<ProviderRequestExposureState>,
    closed: Notify,
    boundary: StdMutex<Option<ProviderLeaseHttpBoundary>>,
    nudge_delivery: StdMutex<Option<Arc<dyn ProviderNudgeDelivery>>>,
    provider_start_changed: StdMutex<Option<Arc<Notify>>>,
}

impl ProviderRequestAuthority {
    fn new(receipt: ProviderRequestReceipt) -> Arc<Self> {
        Arc::new(Self {
            receipt: Arc::new(receipt),
            exposure: StdMutex::new(ProviderRequestExposureState::default()),
            closed: Notify::new(),
            boundary: StdMutex::new(None),
            nudge_delivery: StdMutex::new(None),
            provider_start_changed: StdMutex::new(None),
        })
    }

    fn resume(previous: &Self) -> Arc<Self> {
        let nudge_delivery = previous
            .nudge_delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Arc::new(Self {
            receipt: previous.receipt.clone(),
            exposure: StdMutex::new(ProviderRequestExposureState::default()),
            closed: Notify::new(),
            boundary: StdMutex::new(None),
            nudge_delivery: StdMutex::new(nudge_delivery),
            provider_start_changed: StdMutex::new(None),
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
        let changed = {
            let mut state = self
                .exposure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let changed = !state.live_use_closed;
            state.live_use_closed = true;
            changed
        };
        if changed {
            self.closed.notify_waiters();
            if let Some(registry_changed) = self
                .provider_start_changed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
            {
                registry_changed.notify_one();
            }
        }
    }

    fn bind_scheduler_runtime(
        &self,
        boundary: Option<ProviderLeaseHttpBoundary>,
        nudge_delivery: Option<Arc<dyn ProviderNudgeDelivery>>,
    ) -> Result<(), ProviderStartLookupError> {
        self.ensure_started_live()?;
        if let Some(delivery) = &nudge_delivery {
            delivery.bind_request(&self.receipt).map_err(|reason| {
                ProviderStartLookupError::RuntimeBinding {
                    admission_id: self.receipt.admission_id.clone(),
                    reason,
                }
            })?;
        }
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

/// Exact, non-rebindable request capability captured when semantic evidence was observed.
///
/// Holding this value does not keep a terminal request live. Delivery rechecks the shared request
/// authority, so a later Agent turn cannot be nudged with evidence captured from an earlier turn.
pub struct CapturedProviderRequest {
    session: LiveProviderRequestSession,
    observation_snapshot_hash: String,
}

impl std::fmt::Debug for CapturedProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedProviderRequest")
            .field("request", &self.session.request.receipt.key)
            .field("observation_snapshot_hash", &self.observation_snapshot_hash)
            .finish_non_exhaustive()
    }
}

impl CapturedProviderRequest {
    pub fn request(&self) -> &ProviderRequestReceipt {
        &self.session.request.receipt
    }

    pub fn observation_snapshot_hash(&self) -> &str {
        &self.observation_snapshot_hash
    }

    pub fn is_live(&self) -> bool {
        self.session.request.is_started_live()
    }

    /// Reserve an action against this exact captured request while it is still live.
    ///
    /// The request exposure lock prevents a terminal transition (and therefore a later request)
    /// from crossing the reservation. The supplied safety gate may hold an independent progress
    /// lock while it invokes `reserve`, so progress observed before the reservation cannot race the
    /// action either.
    pub fn reserve_while_live(
        &self,
        safety: &dyn ProviderNudgeSafetyGate,
        reserve: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), ProviderLifecycleOperationError> {
        let state = self
            .session
            .request
            .exposure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.live_use_closed {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "captured provider request is no longer live".to_string(),
            ));
        }
        safety
            .reserve(reserve)
            .map_err(ProviderLifecycleOperationError::Unresolved)
    }

    pub fn closed(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let request = self.session.request.clone();
        async move {
            loop {
                let notified = request.closed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !request.is_started_live() {
                    return;
                }
                notified.await;
            }
        }
    }
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
pub struct CompletedProviderRequest {
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
    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub fn request(&self) -> &ProviderRequestReceipt {
        &self.request
    }

    pub fn terminal(&self) -> &ProviderTerminalReceipt {
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

    pub(crate) fn try_enqueue_nudge_at_capture(
        &self,
        guidance: String,
        capture: ProviderNudgeSafetySnapshot,
        on_pinned_enqueue: impl FnOnce(),
    ) -> Result<Arc<dyn ProviderNudgeDelivery>, String> {
        let _pin = self.try_pin().map_err(|error| error.to_string())?;
        let delivery = self
            .request
            .nudge_delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| "provider request has no dispatcher-owned nudge delivery".to_string())?;
        let mut guidance = Some(guidance);
        let mut on_pinned_enqueue = Some(on_pinned_enqueue);
        delivery.reserve_at_capture(capture, &mut || {
            let guidance = guidance.take().ok_or_else(|| {
                "provider nudge reservation was invoked more than once".to_string()
            })?;
            delivery.try_enqueue(guidance)?;
            on_pinned_enqueue
                .take()
                .expect("provider nudge reservation invokes its hook once")();
            Ok(())
        })?;
        Ok(delivery)
    }

    pub(crate) async fn confirm_reserved_nudge_terminal(
        &self,
        delivery: Arc<dyn ProviderNudgeDelivery>,
    ) -> Result<ProviderTerminalReceipt, String> {
        delivery.cancelled().await;
        let terminal = delivery.confirmed_cancelled_terminal().await?;
        let expected = &self.request.receipt;
        if terminal.kind != ProviderTerminalKind::Cancelled
            || terminal.admission_id != expected.admission_id
            || terminal.key != expected.key
            || terminal.physical_host_id != expected.physical_host_id
            || terminal.model_instance_id != expected.model_instance_id
        {
            return Err(
                "nudge delivery returned a cancellation terminal for a different provider request"
                    .to_string(),
            );
        }
        Ok(terminal)
    }

    async fn enqueue_nudge_and_wait(
        &self,
        guidance: String,
        safety: &dyn ProviderNudgeSafetyGate,
    ) -> Result<ProviderTerminalReceipt, String> {
        let delivery = {
            let state = self
                .request
                .exposure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.live_use_closed {
                return Err("provider request is no longer live".to_string());
            }
            self.request
                .nudge_delivery
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .ok_or_else(|| {
                    "provider request has no dispatcher-owned nudge delivery".to_string()
                })?
        };
        let mut guidance = Some(guidance);
        safety.reserve(&mut || {
            let guidance = guidance.take().ok_or_else(|| {
                "provider nudge reservation was invoked more than once".to_string()
            })?;
            delivery.try_enqueue(guidance)
        })?;
        self.confirm_reserved_nudge_terminal(delivery).await
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
    UnprovenProviderRequest(ProviderRequestReceipt),
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
            Self::UnprovenProviderRequest(receipt) => write!(
                formatter,
                "outstanding provider request `{}` has no proven cancelled terminal",
                receipt.key.provider_request_id
            ),
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

    /// Mark a request whose owning provider future is about to be dropped for exact cancelled
    /// terminal reconciliation. The actual terminal remains broker/journal/lease mediated.
    pub fn arm_cancelled_reconciliation_on_drop(&mut self) {
        if self.pending_terminal.is_none() {
            self.pending_terminal = Some(ProviderTerminalKind::Cancelled);
        }
    }

    pub async fn provider_terminal(
        self,
        kind: ProviderTerminalKind,
    ) -> Result<(), ProviderLifecycleTransitionError> {
        self.provider_terminal_with_completion(kind).await.map(drop)
    }

    pub async fn provider_terminal_with_completion(
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

enum ProviderDropReconcileAction {
    None,
    Claim(RecoverableProviderRequest),
    Wait,
    Unproven(ProviderRequestReceipt),
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

    /// Capture the exact provider turn that was live when semantic evidence was observed.
    pub fn capture_live_provider_request(
        &self,
        observation_snapshot_hash: String,
    ) -> Result<CapturedProviderRequest, ProviderLifecycleOperationError> {
        if observation_snapshot_hash.is_empty() {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "semantic observation snapshot hash is empty".to_string(),
            ));
        }
        let source_key = ProviderStartKey::from_admission(&self.admission);
        let started = self
            .control
            .inner
            .provider_starts
            .query(&source_key)
            .map_err(|error| ProviderLifecycleOperationError::Unresolved(error.to_string()))?;
        let session = LiveProviderRequestSession {
            request: started.request,
        };
        Ok(CapturedProviderRequest {
            session,
            observation_snapshot_hash,
        })
    }

    pub fn current_live_provider_request_receipt(
        &self,
    ) -> Result<ProviderRequestReceipt, ProviderLifecycleOperationError> {
        let source_key = ProviderStartKey::from_admission(&self.admission);
        self.control
            .inner
            .provider_starts
            .query(&source_key)
            .map(|started| started.request.receipt.as_ref().clone())
            .map_err(|error| ProviderLifecycleOperationError::Unresolved(error.to_string()))
    }

    /// Reconcile a provider future that was explicitly dropped by its owner.
    ///
    /// A missing pending terminal is never upgraded to cancellation here: unproven transport
    /// failure remains unresolved. Retryable journal/lease contention is retried without an
    /// attempt or elapsed-time cap because the exact request authority is retained.
    pub async fn reconcile_cancelled_after_drop(
        &self,
    ) -> Result<Option<CompletedProviderRequest>, ProviderLifecycleStartError> {
        loop {
            let action = {
                let mut outstanding = self
                    .outstanding
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match outstanding.take() {
                    None => ProviderDropReconcileAction::None,
                    Some(OutstandingProviderRequest::Recoverable(request))
                        if request.pending_terminal == Some(ProviderTerminalKind::Cancelled) =>
                    {
                        *outstanding = Some(OutstandingProviderRequest::Claimed);
                        ProviderDropReconcileAction::Claim(request)
                    }
                    Some(OutstandingProviderRequest::Recoverable(request)) => {
                        let receipt = request.request.receipt.as_ref().clone();
                        *outstanding = Some(OutstandingProviderRequest::Recoverable(request));
                        ProviderDropReconcileAction::Unproven(receipt)
                    }
                    Some(OutstandingProviderRequest::Starting) => {
                        *outstanding = Some(OutstandingProviderRequest::Starting);
                        ProviderDropReconcileAction::Wait
                    }
                    Some(OutstandingProviderRequest::Claimed) => {
                        *outstanding = Some(OutstandingProviderRequest::Claimed);
                        ProviderDropReconcileAction::Wait
                    }
                    Some(OutstandingProviderRequest::Failed(reason)) => {
                        *outstanding = Some(OutstandingProviderRequest::Failed(reason.clone()));
                        ProviderDropReconcileAction::Failed(reason)
                    }
                }
            };
            let request = match action {
                ProviderDropReconcileAction::None => return Ok(None),
                ProviderDropReconcileAction::Claim(request) => request,
                ProviderDropReconcileAction::Wait => {
                    tokio::task::yield_now().await;
                    continue;
                }
                ProviderDropReconcileAction::Unproven(receipt) => {
                    return Err(ProviderLifecycleStartError::UnprovenProviderRequest(
                        receipt,
                    ));
                }
                ProviderDropReconcileAction::Failed(reason) => {
                    return Err(ProviderLifecycleStartError::Operation(
                        ProviderLifecycleOperationError::Unresolved(reason),
                    ));
                }
            };
            let mut guard = ClaimedProviderRequestGuard {
                lifecycle: self.clone(),
                request: Some(request),
                armed: true,
            };
            let recoverable = guard
                .request
                .as_mut()
                .expect("claimed provider request is present");
            if recoverable.boundary.is_none() {
                if let Some(authority) = &self.control.inner.provider_leases {
                    recoverable.boundary = Some(
                        authority
                            .reserve_provider_request(&self.admission, &recoverable.request.receipt)
                            .await?,
                    );
                }
            }
            let started = guard.into_started();
            let delivery = started
                .request_authority()
                .nudge_delivery
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let confirmation_required = delivery
                .as_ref()
                .is_some_and(|delivery| !delivery.natural_terminal_allowed());
            match started
                .provider_terminal_with_completion(ProviderTerminalKind::Cancelled)
                .await
            {
                Ok(completed) => {
                    if confirmation_required {
                        let _ = delivery
                            .expect("confirmation-required delivery exists")
                            .confirm_cancelled_terminal(completed);
                        return Ok(None);
                    }
                    return Ok(Some(completed));
                }
                Err(ProviderLifecycleTransitionError::Retryable { request, .. }) => {
                    drop(request);
                    tokio::task::yield_now().await;
                }
                Err(error) => {
                    return Err(ProviderLifecycleStartError::TerminalReconciliation(error));
                }
            }
        }
    }

    /// Deliver one semantic redirect to this exact live provider request after an independently
    /// admitted judge has completed successfully on a distinct physical host.
    pub async fn deliver_nudge_after_judge(
        &self,
        captured: CapturedProviderRequest,
        judge: CompletedAdmission,
        guidance: String,
        safety: &dyn ProviderNudgeSafetyGate,
    ) -> Result<ProviderNudgeDeliveryReceipt, ProviderLifecycleOperationError> {
        if !Arc::ptr_eq(&self.control.inner, &judge.control.inner) {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "source and judge receipts do not belong to the same physical broker".to_string(),
            ));
        }
        let released = judge.released();
        let judge_admission = &released.admission;
        if judge_admission.role != crate::broker::WorkRole::SemanticJudgeObservation
            || released.local_completion != LocalCompletionKind::Success
            || released.provider_not_started
            || released.provider_terminals.len() != 1
            || released.provider_terminals[0].kind != ProviderTerminalKind::Finished
        {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "semantic judge has no successful released provider-terminal receipt".to_string(),
            ));
        }
        match &judge_admission.source.kind {
            crate::broker::SourceRevisionKind::Trace { snapshot_hash, .. }
                if snapshot_hash == captured.observation_snapshot_hash() => {}
            crate::broker::SourceRevisionKind::Trace { .. } => {
                return Err(ProviderLifecycleOperationError::Unresolved(
                    "semantic judge trace does not match the captured observation snapshot"
                        .to_string(),
                ));
            }
            _ => {
                return Err(ProviderLifecycleOperationError::Unresolved(
                    "semantic judge admission is not bound to a trace snapshot".to_string(),
                ));
            }
        }
        if judge_admission.physical_host_id == self.admission.physical_host_id {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "semantic judge did not run on a distinct physical host".to_string(),
            ));
        }
        let source_request = captured.session.request.receipt.clone();
        if source_request.admission_id != self.admission.admission_id
            || source_request.physical_host_id != self.admission.physical_host_id
            || source_request.model_instance_id != self.admission.model_instance_id
        {
            return Err(ProviderLifecycleOperationError::Unresolved(
                "captured provider request does not belong to the source admission".to_string(),
            ));
        }
        let source_cancel_terminal = captured
            .session
            .enqueue_nudge_and_wait(guidance, safety)
            .await
            .map_err(ProviderLifecycleOperationError::Unresolved)?;
        Ok(ProviderNudgeDeliveryReceipt {
            delivery_id: format!("provider-nudge-delivery:{:032x}", rand::random::<u128>()),
            observation_snapshot_hash: captured.observation_snapshot_hash,
            source_admission_id: self.admission.admission_id.clone(),
            source_provider_request: source_request.key.clone(),
            source_cancel_terminal,
            source_physical_host_id: self.admission.physical_host_id.clone(),
            source_model_instance_id: self.admission.model_instance_id.clone(),
            judge_admission_id: judge_admission.admission_id.clone(),
            judge_provider_request: released.provider_terminals[0].key.clone(),
        })
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
        if let Err(reconciliation_error) =
            admitted.lifecycle().reconcile_cancelled_after_drop().await
        {
            let reason = match &result {
                Ok(_) => format!(
                    "provider dispatcher returned success without terminal proof: {reconciliation_error}"
                ),
                Err(error) => format!(
                    "provider dispatcher failed ({error}) without terminal proof: {reconciliation_error}"
                ),
            };
            admitted
                .quarantine_unproven(reason)
                .await
                .map_err(|quarantine_error| {
                    DispatchError::Terminal(format!(
                        "physical lifecycle could not quarantine an unproven provider request after {reconciliation_error}: {quarantine_error}"
                    ))
                })?;
            return match result {
                Ok(_) => Err(DispatchError::Transient(format!(
                    "physical provider request has no proven terminal: {reconciliation_error}"
                ))),
                Err(error) => Err(error),
            };
        }
        self.control
            .close_provider_starts(&admitted.receipt().admission_id)
            .await
            .map_err(|error| {
                DispatchError::Terminal(format!(
                    "physical lifecycle could not close provider starts: {error}"
                ))
            })?;
        let completion = match &result {
            Ok(output) if !output.salvaged => LocalCompletionKind::Success,
            Ok(_) | Err(_) => LocalCompletionKind::Error,
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
    use crate::broker::{
        AuthorityScope, SourceRevisionKind, VerifiedPhysicalIdentity, WorkOpportunity, WorkRole,
    };
    use crate::event::{EventSink, NullSink, SwarmEvent};
    use std::sync::atomic::{AtomicBool, AtomicUsize};

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

    const TRANSPORT: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct DeferredNudgeDelivery {
        bound: StdMutex<Option<ProviderRequestReceipt>>,
        reserved: AtomicBool,
        released: AtomicBool,
        queued: Notify,
        release: Notify,
        confirmation: StdMutex<Option<Result<ProviderTerminalReceipt, String>>>,
        confirmed: Notify,
    }

    #[async_trait]
    impl ProviderNudgeDelivery for DeferredNudgeDelivery {
        fn bind_request(&self, request: &ProviderRequestReceipt) -> Result<(), String> {
            let mut bound = self.bound.lock().unwrap();
            match bound.as_ref() {
                Some(existing) if existing != request => {
                    Err("delivery already bound to another request".to_string())
                }
                Some(_) => Ok(()),
                None => {
                    *bound = Some(request.clone());
                    Ok(())
                }
            }
        }

        fn try_enqueue(&self, _guidance: String) -> Result<(), String> {
            if self.reserved.swap(true, Ordering::SeqCst) {
                return Err("delivery already reserved".to_string());
            }
            self.queued.notify_waiters();
            Ok(())
        }

        fn natural_terminal_allowed(&self) -> bool {
            !self.reserved.load(Ordering::SeqCst)
        }

        fn cancellation_terminal_confirmation_required(&self) -> bool {
            self.reserved.load(Ordering::SeqCst)
        }

        async fn cancelled(&self) {
            while !self.released.load(Ordering::SeqCst) {
                self.release.notified().await;
            }
        }

        fn confirm_cancelled_terminal(
            &self,
            completed: CompletedProviderRequest,
        ) -> Result<(), String> {
            let bound = self.bound.lock().unwrap();
            let result = match bound.as_ref() {
                Some(request)
                    if completed.request() == request
                        && completed.terminal().kind == ProviderTerminalKind::Cancelled
                        && completed.terminal().admission_id == request.admission_id
                        && completed.terminal().key == request.key =>
                {
                    Ok(completed.terminal().clone())
                }
                _ => Err("cancel terminal does not match bound request".to_string()),
            };
            drop(bound);
            *self.confirmation.lock().unwrap() = Some(result.clone());
            self.confirmed.notify_waiters();
            result.map(drop)
        }

        async fn confirmed_cancelled_terminal(&self) -> Result<ProviderTerminalReceipt, String> {
            loop {
                let notified = self.confirmed.notified();
                if let Some(result) = self.confirmation.lock().unwrap().clone() {
                    return result;
                }
                notified.await;
            }
        }
    }

    struct AllowNudge;

    impl ProviderNudgeSafetyGate for AllowNudge {
        fn reserve(&self, reserve: &mut dyn FnMut() -> Result<(), String>) -> Result<(), String> {
            reserve()
        }
    }

    struct RejectReservation;

    impl ProviderNudgeSafetyGate for RejectReservation {
        fn reserve(&self, _reserve: &mut dyn FnMut() -> Result<(), String>) -> Result<(), String> {
            Err("structured output advanced before retirement reservation".to_string())
        }
    }

    #[derive(Default)]
    struct LifecycleEventCounts {
        cancelled_terminals: AtomicUsize,
        finished_terminals: AtomicUsize,
    }

    impl EventSink for LifecycleEventCounts {
        fn emit(&self, event: &SwarmEvent) {
            if let SwarmEvent::BrokerProviderTerminalObserved { receipt, .. } = event {
                match receipt.kind {
                    ProviderTerminalKind::Cancelled => {
                        self.cancelled_terminals.fetch_add(1, Ordering::SeqCst);
                    }
                    ProviderTerminalKind::Finished => {
                        self.finished_terminals.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }
    }

    fn nudge_control(same_host: bool) -> PhysicalAdmissionControl {
        nudge_control_with_sink(same_host, Arc::new(NullSink))
    }

    fn nudge_control_with_sink(
        same_host: bool,
        sink: Arc<dyn EventSink>,
    ) -> PhysicalAdmissionControl {
        let host_capacity = if same_host {
            HostCapacityEvidence::MeasuredProfile {
                profile_hash: "same-host-capacity".to_string(),
                profile_key: "same-host-profile".to_string(),
                max_concurrent: 2,
            }
        } else {
            HostCapacityEvidence::ProbeSingleStream {
                probe_epoch: "single-stream".to_string(),
            }
        };
        let source = VerifiedPhysicalIdentity {
            host_id: "source-host".to_string(),
            model_instance_id: "source-instance".to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            advertised_instance_capacity: 1,
            capacity_evidence: host_capacity.clone(),
            route_evidence_id: "source-route".to_string(),
        }
        .into_lane("source-device".to_string(), "source-model".to_string(), 1);
        let judge = VerifiedPhysicalIdentity {
            host_id: if same_host {
                "source-host".to_string()
            } else {
                "judge-host".to_string()
            },
            model_instance_id: "judge-instance".to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            advertised_instance_capacity: 1,
            capacity_evidence: host_capacity,
            route_evidence_id: "judge-route".to_string(),
        }
        .into_lane("judge-device".to_string(), "judge-model".to_string(), 1);
        let snapshot = PhysicalFleetSnapshot::new("nudge-fleet", vec![source, judge]).unwrap();
        PhysicalAdmissionControl::new("nudge-test", snapshot, sink).unwrap()
    }

    async fn admit_role(
        control: &PhysicalAdmissionControl,
        task_id: &str,
        role: WorkRole,
        device: &str,
    ) -> AdmittedWork {
        let source = TaskVersion {
            authority_scope: AuthorityScope::new("nudge-test", "pre-scheduler"),
            phase_epoch: 0,
            task_id: task_id.to_string(),
            attempt: 0,
            revision: 1,
            kind: if role == WorkRole::SemanticJudgeObservation {
                SourceRevisionKind::Trace {
                    trace_sequence: 1,
                    snapshot_hash: format!("snapshot-{task_id}"),
                }
            } else {
                SourceRevisionKind::TaskAttempt
            },
        };
        control.set_source_revision(source.clone()).await.unwrap();
        control
            .admit(WorkOpportunity {
                work_id: format!("work-{task_id}"),
                role,
                priority: role.priority(),
                task_rank: 1,
                source,
                eligible_logical_device_ids: vec![device.to_string()],
                preferred_model_id: None,
                excluded_logical_device_id: None,
            })
            .await
            .unwrap()
    }

    struct DispatchAuthority {
        eligible_logical_device_ids: Vec<String>,
    }

    #[async_trait]
    impl PhysicalDispatchAuthority for DispatchAuthority {
        async fn opportunity(
            &self,
            req: &DispatchRequest,
        ) -> Result<WorkOpportunity, DispatchError> {
            Ok(WorkOpportunity {
                work_id: format!("work-{}-{}", req.task_id, req.attempt),
                role: WorkRole::Build,
                priority: WorkRole::Build.priority(),
                task_rank: 0,
                source: TaskVersion {
                    authority_scope: AuthorityScope::new("dispatcher-test", "build"),
                    phase_epoch: 0,
                    task_id: req.task_id.clone(),
                    attempt: req.attempt,
                    revision: u64::from(req.attempt) + 1,
                    kind: SourceRevisionKind::TaskAttempt,
                },
                eligible_logical_device_ids: self.eligible_logical_device_ids.clone(),
                preferred_model_id: None,
                excluded_logical_device_id: None,
            })
        }

        async fn route_admitted(
            &self,
            req: &mut DispatchRequest,
            admission: &AdmissionReceipt,
        ) -> Result<(), DispatchError> {
            req.device_id = admission.logical_device_id.clone();
            req.model_id = admission.model_id.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct DropFirstProviderBody {
        calls: AtomicUsize,
        physical_hosts: StdMutex<Vec<String>>,
    }

    #[async_trait]
    impl ProviderLifecycleDispatcher for DropFirstProviderBody {
        async fn run_admitted(
            &self,
            _req: DispatchRequest,
            admission: AdmissionReceipt,
            lifecycle: ProviderLifecycle,
        ) -> Result<TaskRunOutput, DispatchError> {
            self.physical_hosts
                .lock()
                .unwrap()
                .push(admission.physical_host_id);
            let request = lifecycle
                .start_provider_request()
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                drop(request);
                return Err(DispatchError::Transient(
                    "error decoding response body after streamed chunks".to_string(),
                ));
            }
            request
                .provider_terminal(ProviderTerminalKind::Finished)
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            Ok("completed on a distinct physical host".to_string().into())
        }
    }

    #[derive(Default)]
    struct CancelFirstProviderBody {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ProviderLifecycleDispatcher for CancelFirstProviderBody {
        async fn run_admitted(
            &self,
            _req: DispatchRequest,
            _admission: AdmissionReceipt,
            lifecycle: ProviderLifecycle,
        ) -> Result<TaskRunOutput, DispatchError> {
            let mut request = lifecycle
                .start_provider_request()
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                request.arm_cancelled_reconciliation_on_drop();
                drop(request);
                return Err(DispatchError::Transient(
                    "provider stream crossed the progress watchdog".to_string(),
                ));
            }
            request
                .provider_terminal(ProviderTerminalKind::Finished)
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            Ok("completed after watchdog retry".to_string().into())
        }
    }

    struct CancelEveryProviderBody;

    #[async_trait]
    impl ProviderLifecycleDispatcher for CancelEveryProviderBody {
        async fn run_admitted(
            &self,
            _req: DispatchRequest,
            _admission: AdmissionReceipt,
            lifecycle: ProviderLifecycle,
        ) -> Result<TaskRunOutput, DispatchError> {
            let mut request = lifecycle
                .start_provider_request()
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            request.arm_cancelled_reconciliation_on_drop();
            drop(request);
            Err(DispatchError::Transient(
                "provider stream crossed the progress watchdog".to_string(),
            ))
        }
    }

    fn dispatch_request(attempt: u32) -> DispatchRequest {
        DispatchRequest {
            task_id: "truncated-body-build".to_string(),
            description: "reproduce a truncated LM Studio stream".to_string(),
            device_id: String::new(),
            model_id: String::new(),
            context_slice: String::new(),
            dependency_files: Vec::new(),
            attempt,
            owned_files: vec!["src/lib.rs".to_string()],
            all_files: vec!["src/lib.rs".to_string()],
            prior_hint: None,
            subsplit: Vec::new(),
            speculative: false,
            user_decisions: String::new(),
            doc_facts: String::new(),
            neighborhood: vec!["truncated-body-build".to_string()],
            replan_authority: None,
            activity_publisher: None,
        }
    }

    fn body_drop_dispatcher(
        same_physical_host: bool,
    ) -> (
        PhysicalAdmissionControl,
        Arc<DropFirstProviderBody>,
        BrokeredTaskDispatcher,
    ) {
        let control = nudge_control(same_physical_host);
        let inner = Arc::new(DropFirstProviderBody::default());
        let authority = Arc::new(DispatchAuthority {
            eligible_logical_device_ids: vec![
                "source-device".to_string(),
                "judge-device".to_string(),
            ],
        });
        let dispatcher = BrokeredTaskDispatcher::new(control.clone(), inner.clone(), authority);
        (control, inner, dispatcher)
    }

    fn dispatch_authority() -> Arc<DispatchAuthority> {
        Arc::new(DispatchAuthority {
            eligible_logical_device_ids: vec![
                "source-device".to_string(),
                "judge-device".to_string(),
            ],
        })
    }

    #[tokio::test]
    async fn brokered_watchdog_drop_reconciles_once_drains_and_retries() {
        let events = Arc::new(LifecycleEventCounts::default());
        let control = nudge_control_with_sink(false, events.clone());
        let inner = Arc::new(CancelFirstProviderBody::default());
        let dispatcher =
            BrokeredTaskDispatcher::new(control.clone(), inner.clone(), dispatch_authority());

        let first = dispatch_request(0);
        dispatcher.prepare(&first).await;
        let first_error =
            tokio::time::timeout(std::time::Duration::from_millis(100), dispatcher.run(first))
                .await
                .expect("watchdog cancellation did not reconcile and drain")
                .unwrap_err();
        assert!(matches!(
            first_error,
            DispatchError::Transient(ref detail)
                if detail == "provider stream crossed the progress watchdog"
        ));
        assert_eq!(events.cancelled_terminals.load(Ordering::SeqCst), 1);
        assert_eq!(events.finished_terminals.load(Ordering::SeqCst), 0);
        assert_eq!(control.occupancy().await, (0, 0));
        control.wait_until_drained().await.unwrap();

        let retry = dispatch_request(1);
        dispatcher.prepare(&retry).await;
        tokio::time::timeout(std::time::Duration::from_millis(100), dispatcher.run(retry))
            .await
            .expect("watchdog retry did not finish")
            .unwrap();
        assert_eq!(inner.calls.load(Ordering::SeqCst), 2);
        assert_eq!(events.cancelled_terminals.load(Ordering::SeqCst), 1);
        assert_eq!(events.finished_terminals.load(Ordering::SeqCst), 1);
        assert_eq!(control.occupancy().await, (0, 0));
        control.wait_until_drained().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_brokered_watchdog_drops_both_reconcile_and_drain() {
        let events = Arc::new(LifecycleEventCounts::default());
        let control = nudge_control_with_sink(false, events.clone());
        let dispatcher = BrokeredTaskDispatcher::new(
            control.clone(),
            Arc::new(CancelEveryProviderBody),
            dispatch_authority(),
        );
        let first = dispatch_request(0);
        let mut second = dispatch_request(0);
        second.task_id = "concurrent-watchdog-build".to_string();
        second.owned_files = vec!["src/second.rs".to_string()];
        second.all_files = second.owned_files.clone();
        dispatcher.prepare(&first).await;
        dispatcher.prepare(&second).await;

        let (first_result, second_result) =
            tokio::time::timeout(std::time::Duration::from_millis(100), async {
                tokio::join!(dispatcher.run(first), dispatcher.run(second))
            })
            .await
            .expect("concurrent watchdog cancellations did not reconcile and drain");
        assert!(matches!(first_result, Err(DispatchError::Transient(_))));
        assert!(matches!(second_result, Err(DispatchError::Transient(_))));
        assert_eq!(events.cancelled_terminals.load(Ordering::SeqCst), 2);
        assert_eq!(events.finished_terminals.load(Ordering::SeqCst), 0);
        assert_eq!(control.occupancy().await, (0, 0));
        control.wait_until_drained().await.unwrap();
    }

    #[tokio::test]
    async fn unproven_body_drop_quarantines_host_then_retries_distinct_host() {
        let (control, inner, dispatcher) = body_drop_dispatcher(false);
        let first = dispatch_request(0);
        dispatcher.prepare(&first).await;
        let error = dispatcher.run(first).await.unwrap_err();
        assert!(matches!(
            error,
            DispatchError::Transient(ref detail)
                if detail == "error decoding response body after streamed chunks"
        ));
        assert_eq!(control.occupancy().await, (0, 1));
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            control.wait_until_drained(),
        )
        .await
        .expect("quarantined transport drop blocked drain")
        .unwrap();

        let retry = dispatch_request(1);
        dispatcher.prepare(&retry).await;
        dispatcher.run(retry).await.unwrap();
        let physical_hosts = inner.physical_hosts.lock().unwrap().clone();
        assert_eq!(physical_hosts.len(), 2);
        assert_ne!(physical_hosts[0], physical_hosts[1]);
        assert_eq!(control.occupancy().await, (0, 1));
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            control.wait_until_drained(),
        )
        .await
        .expect("distinct-host retry did not drain")
        .unwrap();
    }

    #[tokio::test]
    async fn unproven_body_drop_rejects_retry_when_only_one_physical_host_exists() {
        let (control, _inner, dispatcher) = body_drop_dispatcher(true);
        let first = dispatch_request(0);
        dispatcher.prepare(&first).await;
        assert!(matches!(
            dispatcher.run(first).await.unwrap_err(),
            DispatchError::Transient(_)
        ));
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            control.wait_until_drained(),
        )
        .await
        .expect("quarantined one-host admission blocked drain")
        .unwrap();

        let retry = dispatch_request(1);
        dispatcher.prepare(&retry).await;
        let rejected = dispatcher.run(retry).await.unwrap_err();
        assert!(matches!(
            rejected,
            DispatchError::Terminal(ref detail) if detail.contains("every eligible physical host is quarantined")
        ));
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

    #[tokio::test]
    async fn captured_request_reservation_rejects_progress_change_and_request_rollover() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "retirement-source",
            WorkRole::ResearchEvidence,
            "source-device",
        )
        .await;
        let lifecycle = source.lifecycle();
        let first = lifecycle.start_provider_request().await.unwrap();
        first.publish_for_scheduler().unwrap();
        let captured = lifecycle
            .capture_live_provider_request("recurrence-snapshot".to_string())
            .unwrap();

        let reserved = Arc::new(AtomicBool::new(false));
        let mark_reserved = reserved.clone();
        let mut reject_action = || {
            mark_reserved.store(true, Ordering::SeqCst);
            Ok(())
        };
        assert!(captured
            .reserve_while_live(&RejectReservation, &mut reject_action)
            .is_err());
        assert!(!reserved.load(Ordering::SeqCst));

        let mark_reserved = reserved.clone();
        let mut accept_action = || {
            mark_reserved.store(true, Ordering::SeqCst);
            Ok(())
        };
        captured
            .reserve_while_live(&AllowNudge, &mut accept_action)
            .unwrap();
        assert!(reserved.load(Ordering::SeqCst));

        first
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        let second = lifecycle.start_provider_request().await.unwrap();
        second.publish_for_scheduler().unwrap();
        let stale_reserved = Arc::new(AtomicBool::new(false));
        let mark_stale_reserved = stale_reserved.clone();
        let mut stale_action = || {
            mark_stale_reserved.store(true, Ordering::SeqCst);
            Ok(())
        };
        assert!(captured
            .reserve_while_live(&AllowNudge, &mut stale_action)
            .is_err());
        assert!(!stale_reserved.load(Ordering::SeqCst));

        second
            .provider_terminal(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn completed_distinct_host_judge_mints_receipt_only_after_delivery_confirmation() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "source-task",
            WorkRole::ResearchEvidence,
            "source-device",
        )
        .await;
        let judge = admit_role(
            &control,
            "judge-task",
            WorkRole::SemanticJudgeObservation,
            "judge-device",
        )
        .await;
        let source_lifecycle = source.lifecycle();
        let source_request = source_lifecycle.start_provider_request().await.unwrap();
        let delivery = Arc::new(DeferredNudgeDelivery::default());
        source_request
            .publish_for_scheduler_with_nudge_delivery(delivery.clone())
            .unwrap();
        let captured = source_lifecycle
            .capture_live_provider_request("snapshot-judge-task".to_string())
            .unwrap();
        let judge_request = judge.lifecycle().start_provider_request().await.unwrap();
        judge_request
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        let judge_admission_id = judge.receipt().admission_id.clone();
        let completed_judge = judge
            .complete_local_with_completion(LocalCompletionKind::Success)
            .await
            .unwrap();

        let deliver = tokio::spawn({
            let source_lifecycle = source_lifecycle.clone();
            async move {
                source_lifecycle
                    .deliver_nudge_after_judge(
                        captured,
                        completed_judge,
                        "redirect".to_string(),
                        &AllowNudge,
                    )
                    .await
            }
        });
        while !delivery.reserved.load(Ordering::SeqCst) {
            delivery.queued.notified().await;
        }
        assert!(
            !deliver.is_finished(),
            "receipt preceded delivery confirmation"
        );
        delivery.released.store(true, Ordering::SeqCst);
        delivery.release.notify_waiters();
        let source_terminal = source_request
            .provider_terminal_with_completion(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        delivery
            .confirm_cancelled_terminal(source_terminal)
            .unwrap();
        let receipt = deliver.await.unwrap().unwrap();
        assert_eq!(receipt.source_admission_id, source.receipt().admission_id);
        assert_eq!(receipt.judge_admission_id, judge_admission_id);
        assert_eq!(
            receipt.source_cancel_terminal.kind,
            ProviderTerminalKind::Cancelled
        );
        assert_eq!(receipt.observation_snapshot_hash, "snapshot-judge-task");

        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn judge_completion_from_another_snapshot_cannot_authorize_delivery() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "snapshot-source",
            WorkRole::ResearchEvidence,
            "source-device",
        )
        .await;
        let judge = admit_role(
            &control,
            "snapshot-judge",
            WorkRole::SemanticJudgeObservation,
            "judge-device",
        )
        .await;
        let source_lifecycle = source.lifecycle();
        let source_request = source_lifecycle.start_provider_request().await.unwrap();
        let delivery = Arc::new(DeferredNudgeDelivery::default());
        source_request
            .publish_for_scheduler_with_nudge_delivery(delivery.clone())
            .unwrap();
        let captured = source_lifecycle
            .capture_live_provider_request("different-snapshot".to_string())
            .unwrap();
        judge
            .lifecycle()
            .start_provider_request()
            .await
            .unwrap()
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        let completed_judge = judge
            .complete_local_with_completion(LocalCompletionKind::Success)
            .await
            .unwrap();

        let error = source_lifecycle
            .deliver_nudge_after_judge(
                captured,
                completed_judge,
                "stale judge".to_string(),
                &AllowNudge,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("does not match"));
        assert!(!delivery.reserved.load(Ordering::SeqCst));

        source_request
            .provider_terminal(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_confirmation_rejects_wrong_request_without_waiting_forever() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "confirmation-source",
            WorkRole::ResearchEvidence,
            "source-device",
        )
        .await;
        let other = admit_role(
            &control,
            "confirmation-other",
            WorkRole::SemanticJudgeObservation,
            "judge-device",
        )
        .await;
        let source_request = source.lifecycle().start_provider_request().await.unwrap();
        let delivery = Arc::new(DeferredNudgeDelivery::default());
        source_request
            .publish_for_scheduler_with_nudge_delivery(delivery.clone())
            .unwrap();
        delivery.try_enqueue("redirect".to_string()).unwrap();
        let wrong = other
            .lifecycle()
            .start_provider_request()
            .await
            .unwrap()
            .provider_terminal_with_completion(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        assert!(delivery.confirm_cancelled_terminal(wrong).is_err());
        let confirmation = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            delivery.confirmed_cancelled_terminal(),
        )
        .await
        .expect("mismatched proof must finalize the confirmation channel");
        assert!(confirmation.is_err());

        source_request
            .provider_terminal(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        other
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn stale_capture_cannot_nudge_a_later_provider_turn() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "two-turn-source",
            WorkRole::PlanningAuthority,
            "source-device",
        )
        .await;
        let judge = admit_role(
            &control,
            "two-turn-judge",
            WorkRole::SemanticJudgeObservation,
            "judge-device",
        )
        .await;
        let lifecycle = source.lifecycle();
        let first_delivery = Arc::new(DeferredNudgeDelivery::default());
        let first = lifecycle.start_provider_request().await.unwrap();
        first
            .publish_for_scheduler_with_nudge_delivery(first_delivery)
            .unwrap();
        let captured = lifecycle
            .capture_live_provider_request("snapshot-two-turn-judge".to_string())
            .unwrap();
        first
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();

        let second_delivery = Arc::new(DeferredNudgeDelivery::default());
        let second = lifecycle.start_provider_request().await.unwrap();
        second
            .publish_for_scheduler_with_nudge_delivery(second_delivery.clone())
            .unwrap();
        let judge_request = judge.lifecycle().start_provider_request().await.unwrap();
        judge_request
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        let completed_judge = judge
            .complete_local_with_completion(LocalCompletionKind::Success)
            .await
            .unwrap();
        let error = lifecycle
            .deliver_nudge_after_judge(
                captured,
                completed_judge,
                "stale redirect".to_string(),
                &AllowNudge,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no longer live"));
        assert!(!second_delivery.reserved.load(Ordering::SeqCst));

        second
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        source
            .complete_local(LocalCompletionKind::Success)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn same_host_judge_cannot_authorize_a_nudge() {
        let control = nudge_control(true);
        let source = admit_role(
            &control,
            "same-source",
            WorkRole::PlanningAuthority,
            "source-device",
        )
        .await;
        let judge = admit_role(
            &control,
            "same-judge",
            WorkRole::SemanticJudgeObservation,
            "judge-device",
        )
        .await;
        let source_lifecycle = source.lifecycle();
        let source_request = source_lifecycle.start_provider_request().await.unwrap();
        source_request
            .publish_for_scheduler_with_nudge_delivery(Arc::new(DeferredNudgeDelivery::default()))
            .unwrap();
        let captured = source_lifecycle
            .capture_live_provider_request("snapshot-same-judge".to_string())
            .unwrap();
        let judge_request = judge.lifecycle().start_provider_request().await.unwrap();
        judge_request
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .unwrap();
        let completed_judge = judge
            .complete_local_with_completion(LocalCompletionKind::Success)
            .await
            .unwrap();
        let error = source_lifecycle
            .deliver_nudge_after_judge(
                captured,
                completed_judge,
                "redirect".to_string(),
                &AllowNudge,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("distinct physical host"));
        source_request
            .provider_terminal(ProviderTerminalKind::Cancelled)
            .await
            .unwrap();
        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn immediate_idle_judge_miss_leaves_no_supervision_work_queued() {
        let control = nudge_control(false);
        let source = admit_role(
            &control,
            "busy-source",
            WorkRole::ResearchEvidence,
            "source-device",
        )
        .await;
        let blocker = admit_role(
            &control,
            "busy-judge-host",
            WorkRole::PlanningAuthority,
            "judge-device",
        )
        .await;
        let judge_source = TaskVersion {
            authority_scope: AuthorityScope::new("nudge-test", "pre-scheduler"),
            phase_epoch: 0,
            task_id: "idle-only-judge".to_string(),
            attempt: 0,
            revision: 1,
            kind: SourceRevisionKind::Trace {
                trace_sequence: 1,
                snapshot_hash: "idle-only-snapshot".to_string(),
            },
        };
        control
            .set_source_revision(judge_source.clone())
            .await
            .unwrap();
        let admitted = control
            .try_admit_idle(WorkOpportunity {
                work_id: "idle-only-judge-work".to_string(),
                role: WorkRole::SemanticJudgeObservation,
                priority: WorkRole::SemanticJudgeObservation.priority(),
                task_rank: 1,
                source: judge_source,
                eligible_logical_device_ids: vec!["judge-device".to_string()],
                preferred_model_id: None,
                excluded_logical_device_id: None,
            })
            .await
            .unwrap();
        assert!(admitted.is_none());
        assert_eq!(control.occupancy().await, (0, 2));
        source
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        blocker
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }
}
