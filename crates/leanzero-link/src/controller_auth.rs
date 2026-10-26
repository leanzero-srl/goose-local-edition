//! Opaque device authorization, receiver fencing and bounded stream freshness.

use crate::controller_wire::{AuthorizationLease, ControllerScope, DeviceRole};
use crate::worker_client::{WorkerClient, WorkerError};
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

/// Security policy, independent of model/run duration. Never extend a worker lease.
pub const MAX_AUTHORIZATION_LEASE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthorizationError {
    #[error("device credential is missing or invalid")]
    InvalidCredential,
    #[error("device authorization is revoked or denied")]
    Denied,
    #[error("device credential has expired")]
    Expired,
    #[error("authorization authority unavailable: {0}")]
    Unavailable(String),
    #[error("authorization does not match the receiving owner, runtime or scope")]
    BindingMismatch,
}

impl AuthorizationError {
    pub fn close_code(&self) -> u16 {
        match self {
            Self::InvalidCredential | Self::Expired => 4401,
            Self::Denied | Self::BindingMismatch => 4403,
            Self::Unavailable(_) => 4413,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverBinding {
    pub owner_node_id: Uuid,
    pub runtime_id: Uuid,
    pub store_generation: Uuid,
    pub host_lease_id: Uuid,
    pub host_revision: u64,
}

#[async_trait]
pub trait ControllerAuthority: Send + Sync {
    async fn introspect(
        &self,
        token: &str,
        receiver: &ReceiverBinding,
        scope: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError>;
}

pub struct WorkerAuthority {
    worker: WorkerClient,
    host_credential: String,
}

impl WorkerAuthority {
    pub fn new(worker: WorkerClient, host_credential: String) -> Result<Self, AuthorizationError> {
        if host_credential.is_empty() {
            return Err(AuthorizationError::InvalidCredential);
        }
        Ok(Self {
            worker,
            host_credential,
        })
    }
}

#[async_trait]
impl ControllerAuthority for WorkerAuthority {
    async fn introspect(
        &self,
        token: &str,
        receiver: &ReceiverBinding,
        scope: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        let body = serde_json::json!({
            "token": token, "owner_node_id": receiver.owner_node_id,
            "required_scope": scope, "runtime_id": receiver.runtime_id,
            "host_lease_id": receiver.host_lease_id,
        });
        // Read `active` first: an incomplete active response must fail to decode, never deny
        // quietly or invent account/device identity. RFC 7662 section 2.2.
        #[derive(Deserialize)]
        struct Active {
            active: bool,
        }
        let value: serde_json::Value = self
            .worker
            .controller_request(
                reqwest::Method::POST,
                "/v1/controller/introspect",
                &self.host_credential,
                None,
                Some(&body),
            )
            .await
            .map_err(|error| match error {
                WorkerError::Unexpected {
                    status: 401 | 403, ..
                } => AuthorizationError::Denied,
                other => AuthorizationError::Unavailable(other.to_string()),
            })?;
        let active: Active = serde_json::from_value(value.clone())
            .map_err(|error| AuthorizationError::Unavailable(error.to_string()))?;
        if !active.active {
            return Ok(None);
        }
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| AuthorizationError::Unavailable(error.to_string()))
    }
}

/// Fields cannot be constructed by a request extractor or caller JSON.
#[derive(Debug, Clone)]
pub struct AuthenticatedDevice {
    lease: AuthorizationLease,
    deadline: Instant,
    refresh_at: Instant,
    token_deadline: Instant,
}

impl AuthenticatedDevice {
    pub fn account_id(&self) -> Uuid {
        self.lease.account_id
    }
    pub fn device_id(&self) -> Uuid {
        self.lease.device_id
    }
    pub fn device_revision(&self) -> u64 {
        self.lease.device_revision
    }
    pub fn owner_node_id(&self) -> Uuid {
        self.lease.owner_node_id
    }
    pub fn scopes(&self) -> &[ControllerScope] {
        &self.lease.scopes
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn refresh_at(&self) -> Instant {
        self.refresh_at
    }
    pub fn check_fresh(&self) -> Result<(), AuthorizationError> {
        if Instant::now() >= self.token_deadline {
            return Err(AuthorizationError::Expired);
        }
        if Instant::now() >= self.deadline {
            return Err(AuthorizationError::Unavailable(
                "authorization lease expired".into(),
            ));
        }
        Ok(())
    }
    fn same_principal(&self, other: &Self) -> bool {
        self.account_id() == other.account_id()
            && self.device_id() == other.device_id()
            && self.device_revision() == other.device_revision()
    }
}

pub async fn authorize(
    authority: &dyn ControllerAuthority,
    token: &str,
    receiver: &ReceiverBinding,
    scope: ControllerScope,
) -> Result<AuthenticatedDevice, AuthorizationError> {
    if token.is_empty() {
        return Err(AuthorizationError::InvalidCredential);
    }
    let started = Instant::now();
    let wall_started = Utc::now();
    let lease = tokio::time::timeout(
        MAX_AUTHORIZATION_LEASE,
        authority.introspect(token, receiver, scope),
    )
    .await
    .map_err(|_| AuthorizationError::Unavailable("introspection deadline exceeded".into()))??
    .ok_or(AuthorizationError::Denied)?;
    if !lease.active
        || lease.role != DeviceRole::Controller
        || lease.owner_node_id != receiver.owner_node_id
        || lease.store_generation != receiver.store_generation
        || lease.host_lease_id != receiver.host_lease_id
        || lease.host_revision != receiver.host_revision
        || !lease.scopes.contains(&scope)
    {
        return Err(AuthorizationError::BindingMismatch);
    }
    let token_remaining = (lease.token_expires_at - wall_started)
        .to_std()
        .map_err(|_| AuthorizationError::Expired)?;
    let duration = Duration::from_secs(u64::from(lease.lease_seconds))
        .min(MAX_AUTHORIZATION_LEASE)
        .min(token_remaining);
    let device = AuthenticatedDevice {
        deadline: started + duration,
        refresh_at: started + duration / 2,
        token_deadline: started + token_remaining,
        lease,
    };
    device.check_fresh()?;
    Ok(device)
}

/// Retained with a durable queued command by the owner adapter. Call `dispatch` while
/// holding the owner/run control lock; it rechecks authorization before invoking work.
#[derive(Clone)]
pub struct DispatchAuthorization {
    authority: Arc<dyn ControllerAuthority>,
    token: String,
    receiver: ReceiverBinding,
    scope: ControllerScope,
    accepted: AuthenticatedDevice,
}

impl DispatchAuthorization {
    pub fn new(
        authority: Arc<dyn ControllerAuthority>,
        token: String,
        receiver: ReceiverBinding,
        scope: ControllerScope,
        accepted: AuthenticatedDevice,
    ) -> Self {
        Self {
            authority,
            token,
            receiver,
            scope,
            accepted,
        }
    }
    pub async fn recheck(&self) -> Result<AuthenticatedDevice, AuthorizationError> {
        let current = authorize(
            self.authority.as_ref(),
            &self.token,
            &self.receiver,
            self.scope,
        )
        .await?;
        if !self.accepted.same_principal(&current) {
            return Err(AuthorizationError::Denied);
        }
        Ok(current)
    }
    pub async fn dispatch<T, F, Fut>(&self, action: F) -> Result<T, AuthorizationError>
    where
        F: FnOnce(AuthenticatedDevice) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let device = self.recheck().await?;
        device.check_fresh()?;
        Ok(action(device).await)
    }
}
