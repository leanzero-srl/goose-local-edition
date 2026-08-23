//! Physical admission and provider-lifecycle binding for semantic observations.
//!
//! This adapter can produce an observation receipt. It cannot deliver the observed action or mutate
//! scheduler state.

use crate::broker::{
    AdmissionReceipt, BrokerError, LocalCompletionKind, ProviderTerminalKind, SourceRevisionKind,
    TaskVersion, WorkOpportunity, WorkRole,
};
use crate::control_plane::{AdmittedWork, PhysicalAdmissionControl, ProviderLifecycle};
use crate::event::EventSink;
use crate::semantic_observation::{
    SealedSemanticObservationSnapshot, SemanticObservationHandle, SemanticObservationPlane,
    SemanticObservationReceipt, SemanticObservationRejection, SemanticObservationRequest,
    SemanticObservationReviewer, SemanticObservationSubmission, SemanticProtocolFailureKind,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{oneshot, Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticObservationAdmissionPolicy {
    pub task_rank: u64,
    pub eligible_logical_device_ids: Vec<String>,
    pub preferred_model_id: Option<String>,
    pub excluded_logical_device_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AdmittedSemanticObservationRequest {
    pub observation: SemanticObservationRequest,
    pub admission: AdmissionReceipt,
    pub provider_request_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmittedSemanticReviewError {
    TerminalFailure(String),
    LocalFailureAfterTerminal {
        detail: String,
        provider_terminal: ProviderTerminalKind,
    },
    ProviderLifecycleUnresolved(String),
}

impl AdmittedSemanticReviewError {
    pub fn terminal_failure(detail: impl Into<String>) -> Self {
        Self::TerminalFailure(detail.into())
    }

    pub fn unresolved(detail: impl Into<String>) -> Self {
        Self::ProviderLifecycleUnresolved(detail.into())
    }

    pub fn local_failure_after_terminal(
        detail: impl Into<String>,
        provider_terminal: ProviderTerminalKind,
    ) -> Self {
        Self::LocalFailureAfterTerminal {
            detail: detail.into(),
            provider_terminal,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::TerminalFailure(detail) | Self::ProviderLifecycleUnresolved(detail) => detail,
            Self::LocalFailureAfterTerminal { detail, .. } => detail,
        }
    }
}

#[async_trait]
pub trait AdmittedSemanticObservationReviewer: Send + Sync {
    /// Exact logical lanes for which this reviewer has a provider binding. `None` preserves the
    /// generic adapter contract and leaves rejection to `verify_admission`; production route-bound
    /// adapters return `Some` so the scheduler never queues work for a provider it cannot serve.
    fn eligible_logical_device_ids(&self) -> Option<Vec<String>> {
        None
    }

    /// Validate that this exact admission can be served by a verified provider without starting a
    /// request. Production adapters use this to fail closed on route/model drift.
    fn verify_admission(
        &self,
        _request: &AdmittedSemanticObservationRequest,
    ) -> std::result::Result<(), String> {
        Ok(())
    }

    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> std::result::Result<String, AdmittedSemanticReviewError>;
}

#[derive(Clone, Debug)]
pub struct AdmittedSemanticObservationReceipt {
    pub admission: AdmissionReceipt,
    pub observation: SemanticObservationReceipt,
    pub local_completion: LocalCompletionKind,
}

impl AdmittedSemanticObservationReceipt {
    pub fn has_intervention_authority(&self) -> bool {
        false
    }
}

pub struct AdmittedSemanticObservationHandle {
    snapshot_hash: String,
    admission: AdmissionReceipt,
    completion: oneshot::Receiver<
        std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError>,
    >,
}

impl AdmittedSemanticObservationHandle {
    pub fn snapshot_hash(&self) -> &str {
        &self.snapshot_hash
    }

    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub async fn wait(
        self,
    ) -> std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError>
    {
        self.completion.await.map_err(|_| {
            SemanticObservationAdmissionError::ObserverCompletionClosed {
                snapshot_hash: self.snapshot_hash,
            }
        })?
    }
}

#[derive(Clone, Debug)]
pub struct RejectedSemanticObservationAdmission {
    pub admission: Option<AdmissionReceipt>,
    pub rejection: SemanticObservationRejection,
}

pub enum SemanticObservationAdmissionSubmission {
    Started(AdmittedSemanticObservationHandle),
    Rejected(RejectedSemanticObservationAdmission),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticObservationAdmissionStage {
    PublishSource,
    RevalidateSource,
    PhysicalAdmission,
    ProviderNotStarted,
    LocalCompletion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticObservationAdmissionError {
    EventSinkMismatch,
    ObservationPlaneAlreadyBound,
    Broker {
        stage: SemanticObservationAdmissionStage,
        error: BrokerError,
    },
    ObserverCompletionClosed {
        snapshot_hash: String,
    },
    ProviderLifecycleUnresolved {
        admission_id: String,
        reason: String,
    },
}

impl fmt::Display for SemanticObservationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventSinkMismatch => write!(
                formatter,
                "semantic observer and physical broker must share one event sink"
            ),
            Self::ObservationPlaneAlreadyBound => write!(
                formatter,
                "physical admission control already has a semantic observation plane"
            ),
            Self::Broker { stage, error } => {
                write!(formatter, "semantic observation {stage:?} failed: {error}")
            }
            Self::ObserverCompletionClosed { snapshot_hash } => write!(
                formatter,
                "semantic observer completion closed for snapshot `{snapshot_hash}`"
            ),
            Self::ProviderLifecycleUnresolved {
                admission_id,
                reason,
            } => write!(
                formatter,
                "semantic observation admission `{admission_id}` has unresolved provider lifecycle: {reason}"
            ),
        }
    }
}

impl std::error::Error for SemanticObservationAdmissionError {}

#[derive(Clone)]
pub struct BrokeredSemanticObservationPlane {
    control: PhysicalAdmissionControl,
    events: Arc<dyn EventSink>,
    observations: SemanticObservationPlane,
    task_lanes: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

impl BrokeredSemanticObservationPlane {
    pub fn new(
        control: PhysicalAdmissionControl,
        events: Arc<dyn EventSink>,
    ) -> std::result::Result<Self, SemanticObservationAdmissionError> {
        if !control.uses_sink(&events) {
            return Err(SemanticObservationAdmissionError::EventSinkMismatch);
        }
        if !control.claim_semantic_observation_plane() {
            return Err(SemanticObservationAdmissionError::ObservationPlaneAlreadyBound);
        }
        Ok(Self {
            control,
            events: events.clone(),
            observations: SemanticObservationPlane::new(events),
            task_lanes: Arc::new(AsyncMutex::new(HashMap::new())),
        })
    }

    pub async fn submit(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    ) -> std::result::Result<
        SemanticObservationAdmissionSubmission,
        SemanticObservationAdmissionError,
    > {
        self.submit_with_mode(snapshot, policy, reviewer, AdmissionMode::Queue)
            .await?
            .ok_or_else(
                || SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: "not-admitted".to_string(),
                    reason: "queued semantic observation unexpectedly produced no admission"
                        .to_string(),
                },
            )
    }

    /// Submit only when verified physical capacity is idle now. `None` means the opportunity was
    /// atomically withdrawn before admission; no provider-not-started receipt is fabricated because
    /// no provider lifecycle ever existed.
    pub async fn submit_if_idle(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    ) -> std::result::Result<
        Option<SemanticObservationAdmissionSubmission>,
        SemanticObservationAdmissionError,
    > {
        self.submit_with_mode(snapshot, policy, reviewer, AdmissionMode::ImmediateIdle)
            .await
    }

    async fn submit_with_mode(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
        mode: AdmissionMode,
    ) -> std::result::Result<
        Option<SemanticObservationAdmissionSubmission>,
        SemanticObservationAdmissionError,
    > {
        let source = semantic_observation_task_version(&snapshot);
        self.control
            .set_source_revision(source.clone())
            .await
            .map_err(|error| SemanticObservationAdmissionError::Broker {
                stage: SemanticObservationAdmissionStage::PublishSource,
                error,
            })?;
        if let Err(rejection) = self.observations.register_current(&snapshot) {
            return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                RejectedSemanticObservationAdmission {
                    admission: None,
                    rejection,
                },
            )));
        }
        self.events.emit(
            &crate::event::SwarmEvent::SemanticObservationSourcePublished {
                source: source.clone(),
            },
        );

        let task_lane = self.task_lane(snapshot.task_id()).await;
        self.control
            .set_source_revision(source.clone())
            .await
            .map_err(|error| SemanticObservationAdmissionError::Broker {
                stage: SemanticObservationAdmissionStage::RevalidateSource,
                error,
            })?;
        if let Err(rejection) = self.observations.register_current(&snapshot) {
            return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                RejectedSemanticObservationAdmission {
                    admission: None,
                    rejection,
                },
            )));
        }

        let work_id = semantic_observation_work_id(&snapshot);
        let opportunity = WorkOpportunity {
            work_id: work_id.clone(),
            role: WorkRole::SemanticJudgeObservation,
            priority: WorkRole::SemanticJudgeObservation.priority(),
            task_rank: policy.task_rank,
            source,
            eligible_logical_device_ids: policy.eligible_logical_device_ids,
            preferred_model_id: policy.preferred_model_id,
            excluded_logical_device_id: policy.excluded_logical_device_id,
        };
        let admitted = match mode {
            AdmissionMode::Queue => {
                Some(self.control.admit(opportunity).await.map_err(|error| {
                    SemanticObservationAdmissionError::Broker {
                        stage: SemanticObservationAdmissionStage::PhysicalAdmission,
                        error,
                    }
                })?)
            }
            AdmissionMode::ImmediateIdle => self
                .control
                .try_admit_idle(opportunity)
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::PhysicalAdmission,
                    error,
                })?,
        };
        let Some(admitted) = admitted else {
            self.events
                .emit(&crate::event::SwarmEvent::SemanticObservationDeferred {
                    task_id: snapshot.task_id().to_string(),
                    attempt: snapshot.attempt(),
                    source_revision: snapshot.source_revision(),
                    snapshot_hash: snapshot.snapshot_hash().to_string(),
                    reason: "no_verified_idle_provider_route".to_string(),
                });
            return Ok(None);
        };
        let admission = admitted.receipt().clone();
        let proof = Arc::new(Mutex::new(ProviderLifecycleProof::Pending));
        let lifecycle_reviewer = Arc::new(LifecycleBoundSemanticReviewer {
            inner: reviewer,
            admission: admission.clone(),
            lifecycle: admitted.lifecycle(),
            provider_request_id: format!("{work_id}:provider:0"),
            proof: proof.clone(),
        });

        let observation = match self.observations.submit(snapshot, lifecycle_reviewer) {
            SemanticObservationSubmission::Started(handle) => handle,
            SemanticObservationSubmission::Rejected(rejection) => {
                let admission_id = admission.admission_id.clone();
                let cleanup = tokio::spawn(close_rejected_admission(
                    admitted,
                    format!("semantic observation rejected before provider call: {rejection:?}"),
                ));
                cleanup.await.map_err(|error| {
                    SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                        admission_id,
                        reason: format!("pre-call rejection cleanup task failed: {error}"),
                    }
                })??;
                return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                    RejectedSemanticObservationAdmission {
                        admission: Some(admission),
                        rejection,
                    },
                )));
            }
        };

        let (sender, completion) = oneshot::channel();
        let snapshot_hash = observation.snapshot_hash().to_string();
        let admission_for_handle = admission.clone();
        tokio::spawn(async move {
            let result = finalize_admitted_observation(admitted, observation, proof).await;
            let _ = sender.send(result);
            drop(task_lane);
        });

        Ok(Some(SemanticObservationAdmissionSubmission::Started(
            AdmittedSemanticObservationHandle {
                snapshot_hash,
                admission: admission_for_handle,
                completion,
            },
        )))
    }

    async fn task_lane(&self, task_id: &str) -> OwnedMutexGuard<()> {
        let lane = {
            let mut lanes = self.task_lanes.lock().await;
            lanes
                .entry(task_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        lane.lock_owned().await
    }
}

#[derive(Clone, Copy)]
enum AdmissionMode {
    Queue,
    ImmediateIdle,
}

async fn close_rejected_admission(
    admitted: AdmittedWork,
    reason: String,
) -> std::result::Result<(), SemanticObservationAdmissionError> {
    admitted
        .lifecycle()
        .provider_not_started(reason)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::ProviderNotStarted,
            error,
        })?;
    admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })
}

pub fn semantic_observation_task_version(
    snapshot: &SealedSemanticObservationSnapshot,
) -> TaskVersion {
    TaskVersion {
        task_id: snapshot.task_id().to_string(),
        attempt: snapshot.attempt(),
        revision: snapshot.source_revision(),
        kind: SourceRevisionKind::Trace {
            trace_sequence: snapshot.payload().trace.sequence,
            snapshot_hash: snapshot.snapshot_hash().to_string(),
        },
    }
}

fn semantic_observation_work_id(snapshot: &SealedSemanticObservationSnapshot) -> String {
    format!("semantic-observation:{}", snapshot.snapshot_hash())
}

#[derive(Clone, Debug)]
enum ProviderLifecycleProof {
    Pending,
    ProviderNotStarted,
    TerminalObserved(ProviderTerminalKind),
    Unresolved(String),
}

struct LifecycleBoundSemanticReviewer {
    inner: Arc<dyn AdmittedSemanticObservationReviewer>,
    admission: AdmissionReceipt,
    lifecycle: ProviderLifecycle,
    provider_request_id: String,
    proof: Arc<Mutex<ProviderLifecycleProof>>,
}

#[async_trait]
impl SemanticObservationReviewer for LifecycleBoundSemanticReviewer {
    async fn review(
        &self,
        observation: SemanticObservationRequest,
    ) -> std::result::Result<String, String> {
        let admitted_request = AdmittedSemanticObservationRequest {
            observation,
            admission: self.admission.clone(),
            provider_request_id: self.provider_request_id.clone(),
        };
        if let Err(detail) = self.inner.verify_admission(&admitted_request) {
            let detail = format!("semantic provider preflight rejected admission: {detail}");
            match self.lifecycle.provider_not_started(detail.clone()).await {
                Ok(()) => self.set_proof(ProviderLifecycleProof::ProviderNotStarted),
                Err(close_error) => self.set_proof(ProviderLifecycleProof::Unresolved(format!(
                    "{detail}; provider-not-started was rejected: {close_error}"
                ))),
            }
            return Err(detail);
        }
        let key = match self
            .lifecycle
            .provider_request_started(self.provider_request_id.clone())
            .await
        {
            Ok(key) => key,
            Err(error) => {
                let detail = format!("provider start rejected before semantic review: {error}");
                match self.lifecycle.provider_not_started(detail.clone()).await {
                    Ok(()) => self.set_proof(ProviderLifecycleProof::ProviderNotStarted),
                    Err(close_error) => self.set_proof(ProviderLifecycleProof::Unresolved(
                        format!("{detail}; provider-not-started was rejected: {close_error}"),
                    )),
                }
                return Err(detail);
            }
        };

        let reviewer = self.inner.clone();
        let reviewed =
            match tokio::spawn(async move { reviewer.review(admitted_request).await }).await {
                Ok(reviewed) => reviewed,
                Err(error) => {
                    let detail = format!("semantic provider task ended without a reply: {error}");
                    self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                    return Err(detail);
                }
            };
        let terminal_kind = match &reviewed {
            Ok(_) => ProviderTerminalKind::Finished,
            Err(AdmittedSemanticReviewError::TerminalFailure(_)) => ProviderTerminalKind::Failed,
            Err(AdmittedSemanticReviewError::LocalFailureAfterTerminal {
                provider_terminal,
                ..
            }) => *provider_terminal,
            Err(AdmittedSemanticReviewError::ProviderLifecycleUnresolved(detail)) => {
                self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                return Err(detail.clone());
            }
        };
        match self.lifecycle.provider_terminal(key, terminal_kind).await {
            Ok(()) => self.set_proof(ProviderLifecycleProof::TerminalObserved(terminal_kind)),
            Err(error) => {
                let detail = format!("semantic provider terminal was rejected: {error}");
                self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                return Err(detail);
            }
        }
        reviewed.map_err(|error| error.detail().to_string())
    }
}

impl LifecycleBoundSemanticReviewer {
    fn set_proof(&self, proof: ProviderLifecycleProof) {
        *lock_proof(&self.proof) = proof;
    }
}

async fn finalize_admitted_observation(
    admitted: AdmittedWork,
    observation: SemanticObservationHandle,
    proof: Arc<Mutex<ProviderLifecycleProof>>,
) -> std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError> {
    let snapshot_hash = observation.snapshot_hash().to_string();
    let observation = match observation.wait().await {
        Ok(observation) => observation,
        Err(_) => {
            close_after_observer_loss(&admitted, &proof).await?;
            return Err(
                SemanticObservationAdmissionError::ObserverCompletionClosed { snapshot_hash },
            );
        }
    };
    let lifecycle_proof = lock_proof(&proof).clone();
    let local_completion = match lifecycle_proof {
        ProviderLifecycleProof::TerminalObserved(ProviderTerminalKind::Finished) => {
            if matches!(
                observation.decision.failure().map(|failure| &failure.kind),
                Some(SemanticProtocolFailureKind::ReviewerFailed)
            ) {
                LocalCompletionKind::Error
            } else {
                LocalCompletionKind::Success
            }
        }
        ProviderLifecycleProof::TerminalObserved(
            ProviderTerminalKind::Failed | ProviderTerminalKind::Cancelled,
        )
        | ProviderLifecycleProof::ProviderNotStarted => LocalCompletionKind::Error,
        ProviderLifecycleProof::Pending => {
            admitted
                .lifecycle()
                .provider_not_started("semantic observer returned without a provider start")
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::ProviderNotStarted,
                    error,
                })?;
            LocalCompletionKind::Error
        }
        ProviderLifecycleProof::Unresolved(reason) => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason,
                },
            )
        }
    };
    admitted
        .complete_local(local_completion)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })?;
    Ok(AdmittedSemanticObservationReceipt {
        admission: admitted.receipt().clone(),
        observation,
        local_completion,
    })
}

async fn close_after_observer_loss(
    admitted: &AdmittedWork,
    proof: &Arc<Mutex<ProviderLifecycleProof>>,
) -> std::result::Result<(), SemanticObservationAdmissionError> {
    let lifecycle_proof = { lock_proof(proof).clone() };
    match lifecycle_proof {
        ProviderLifecycleProof::Pending => {
            admitted
                .lifecycle()
                .provider_not_started("semantic observer ended before the provider call")
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::ProviderNotStarted,
                    error,
                })?;
        }
        ProviderLifecycleProof::Unresolved(reason) => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason,
                },
            )
        }
        ProviderLifecycleProof::ProviderNotStarted
        | ProviderLifecycleProof::TerminalObserved(_) => {}
    }
    admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })
}

fn lock_proof(
    proof: &Arc<Mutex<ProviderLifecycleProof>>,
) -> MutexGuard<'_, ProviderLifecycleProof> {
    proof
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
