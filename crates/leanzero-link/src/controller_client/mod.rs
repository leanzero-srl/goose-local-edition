//! Process-owned desktop controller credentials, transport and metadata cache.
mod fleet;
#[cfg(test)]
mod tests;
mod transport;

use crate::{
    controller_host::{private_json, save_json},
    controller_wire::*,
    owner_identity::load_or_create_private,
    worker_client::{WorkerClient, WorkerError},
};
use chrono::{DateTime, Utc};
pub use fleet::{FleetCatalog, FleetHost, PendingCommand};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

pub type ClientResult<T> = Result<T, ClientError>;
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct ClientError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub status: u16,
}
impl ClientError {
    fn invalid(message: &str) -> Self {
        Self {
            code: "invalid_request".into(),
            message: message.into(),
            retryable: false,
            status: 400,
        }
    }
    fn unavailable(message: &str) -> Self {
        Self {
            code: "controller_unavailable".into(),
            message: message.into(),
            retryable: true,
            status: 503,
        }
    }
    fn signed_out() -> Self {
        Self {
            code: "signed_out".into(),
            message: "Sign in to Link on this Mac to use the controller".into(),
            retryable: false,
            status: 401,
        }
    }
}
impl From<WorkerError> for ClientError {
    fn from(error: WorkerError) -> Self {
        match error {
            WorkerError::Unexpected { status, error, .. } => {
                let value = serde_json::from_str::<serde_json::Value>(&error).ok();
                let code = value
                    .as_ref()
                    .and_then(|v| v.pointer("/error/code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("authority_refused");
                Self {
                    code: code.into(),
                    message: format!("Link authority refused controller access ({status})"),
                    retryable: status >= 500 || status == 429,
                    status,
                }
            }
            WorkerError::ControllerUpdateRequired { .. } => Self {
                code: "update_required".into(),
                message: "The Link authority needs a controller update".into(),
                retryable: false,
                status: 503,
            },
            _ => Self::unavailable("The Link authority could not be reached securely"),
        }
    }
}
impl From<crate::owner_identity::OwnerIdentityError> for ClientError {
    fn from(_: crate::owner_identity::OwnerIdentityError) -> Self {
        Self::unavailable("The private desktop controller store is unreadable or unsafe")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedIdentity {
    version: u32,
    authority: String,
    email: String,
    enrollment_id: Uuid,
    device_name: String,
    disabled: bool,
    credentials: Option<DeviceCredentials>,
    refresh_request_id: Option<Uuid>,
    logout_request_id: Option<Uuid>,
    challenge: Option<Challenge>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub challenge_id: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Serialize)]
pub struct ControllerStatus {
    pub state: &'static str,
    pub device: Option<DeviceRecord>,
    pub error: Option<String>,
}

pub struct DesktopController {
    worker: WorkerClient,
    path: PathBuf,
    outbox_path: PathBuf,
    identity: Mutex<SavedIdentity>,
    auth_flow: Mutex<()>,
    cache: RwLock<FleetCatalog>,
    cursors: Mutex<HashMap<Uuid, String>>,
    sync_lock: Mutex<()>,
    outbox: Mutex<Vec<PendingCommand>>,
    last_error: RwLock<Option<String>>,
    verified_at: RwLock<Option<tokio::time::Instant>>,
}

impl DesktopController {
    pub async fn matches_account(&self, email: &str, authority: &str) -> bool {
        let saved = self.identity.lock().await;
        saved.email == email && saved.authority == authority
    }

    pub async fn block_enrollment(&self, error: &ClientError) -> ClientResult<()> {
        if error.code == "enrollment_consumed" {
            let mut saved = self.identity.lock().await;
            saved.disabled = true;
            save_json(&self.path, &*saved)?;
        }
        self.record_error(error).await;
        Ok(())
    }
    pub fn load(
        directory: PathBuf,
        authority: String,
        email: String,
        device_name: String,
    ) -> ClientResult<Self> {
        let worker = WorkerClient::new(&authority)?;
        let path = directory.join("identity.json");
        let bytes = load_or_create_private(&path, || {
            serde_json::to_vec(&SavedIdentity {
                version: 1,
                authority: authority.clone(),
                email: email.clone(),
                device_name,
                enrollment_id: Uuid::new_v4(),
                disabled: false,
                credentials: None,
                refresh_request_id: None,
                logout_request_id: None,
                challenge: None,
            })
            .map_err(|source| crate::owner_identity::OwnerIdentityError::Json {
                path: path.clone(),
                source,
            })
        })?;
        let identity: SavedIdentity = serde_json::from_slice(&bytes)
            .map_err(|_| ClientError::invalid("Controller identity store is corrupt"))?;
        if identity.version != 1 || identity.authority != authority || identity.email != email {
            return Err(ClientError::invalid(
                "Controller identity belongs to another account or authority; sign out explicitly",
            ));
        }
        if let Some(credentials) = &identity.credentials {
            validate_credentials(credentials, None)?;
        }
        let outbox_path = directory.join("outbox.json");
        let outbox = private_json::<Vec<PendingCommand>>(&outbox_path)?.unwrap_or_default();
        let mut cache =
            private_json::<FleetCatalog>(&directory.join("catalog.json"))?.unwrap_or_default();
        for host in &mut cache.hosts {
            host.stale = true;
            host.error = Some("Awaiting verified Link refresh".into());
        }
        Ok(Self {
            worker,
            path,
            outbox_path,
            identity: Mutex::new(identity),
            auth_flow: Mutex::new(()),
            cache: RwLock::new(cache),
            cursors: Mutex::new(HashMap::new()),
            sync_lock: Mutex::new(()),
            outbox: Mutex::new(outbox),
            last_error: RwLock::new(None),
            verified_at: RwLock::new(None),
        })
    }

    pub async fn enrollment(&self) -> Option<(Uuid, String)> {
        let saved = self.identity.lock().await;
        if saved.disabled || saved.credentials.is_some() || saved.challenge.is_some() {
            None
        } else {
            Some((saved.enrollment_id, saved.device_name.clone()))
        }
    }

    pub async fn enroll_from_host(
        &self,
        manager: &crate::manager::LinkManager,
    ) -> ClientResult<()> {
        let _flow = self.auth_flow.lock().await;
        if let Some((id, name)) = self.enrollment().await {
            match manager.enroll_desktop_controller(id, &name).await {
                Ok(credentials) => self.accept_enrollment(credentials).await?,
                Err(error) => {
                    let error = match error {
                        crate::manager::LinkError::Worker(error) => ClientError::from(error),
                        _ => ClientError::unavailable(
                            "The signed-in Link host is not ready for controller enrollment",
                        ),
                    };
                    self.block_enrollment(&error).await?;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub async fn accept_enrollment(&self, credentials: DeviceCredentials) -> ClientResult<()> {
        validate_credentials(&credentials, None)?;
        let mut saved = self.identity.lock().await;
        if saved.disabled {
            return Err(ClientError::signed_out());
        }
        if saved
            .credentials
            .as_ref()
            .is_some_and(|c| c.device.device_id != credentials.device.device_id)
        {
            return Err(ClientError::invalid(
                "The desktop device identity changed during enrollment",
            ));
        }
        saved.credentials = Some(credentials);
        saved.challenge = None;
        save_json(&self.path, &*saved)?;
        Ok(())
    }

    pub async fn record_error(&self, error: &ClientError) {
        *self.last_error.write().await = Some(error.message.clone());
    }

    pub async fn status(&self) -> ControllerStatus {
        let saved = self.identity.lock().await;
        let error = self.last_error.read().await.clone();
        let usable = saved
            .credentials
            .as_ref()
            .is_some_and(|c| c.expires_at > Utc::now() && c.device.revoked_at.is_none())
            && self
                .verified_at
                .read()
                .await
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(30));
        ControllerStatus {
            state: if saved.disabled || saved.credentials.is_none() {
                "signed_out"
            } else if usable && error.is_none() {
                "ready"
            } else {
                "unavailable"
            },
            device: saved.credentials.as_ref().map(|c| c.device.clone()),
            error,
        }
    }

    async fn credentials(&self) -> ClientResult<DeviceCredentials> {
        let mut saved = self.identity.lock().await;
        if saved.disabled {
            return Err(ClientError::signed_out());
        }
        let previous = saved
            .credentials
            .clone()
            .ok_or_else(ClientError::signed_out)?;
        if previous.expires_at > Utc::now() + chrono::Duration::seconds(60)
            && saved.refresh_request_id.is_none()
        {
            return Ok(previous);
        }
        let request_id = *saved.refresh_request_id.get_or_insert_with(Uuid::new_v4);
        // Persist the exact rotation request before sending: replay after a lost ACK
        // recovers its receipt instead of triggering the authority's replay revocation.
        save_json(&self.path, &*saved)?;
        let result: Result<DeviceCredentials, WorkerError> = self.worker.controller_request(reqwest::Method::POST, "/v1/controller/auth/refresh", "", None, Some(&serde_json::json!({"device_id":previous.device.device_id,"refresh_token":previous.refresh_token,"request_id":request_id}))).await;
        let credentials = match result {
            Ok(value) => value,
            Err(error) => {
                let error = ClientError::from(error);
                if error.status == 401 || error.status == 403 {
                    saved.disabled = true;
                    save_json(&self.path, &*saved)?;
                }
                return Err(error);
            }
        };
        validate_credentials(&credentials, Some(previous.device.device_id))?;
        saved.credentials = Some(credentials.clone());
        saved.refresh_request_id = None;
        save_json(&self.path, &*saved)?;
        Ok(credentials)
    }

    pub async fn request_code(&self) -> ClientResult<Challenge> {
        let _flow = self.auth_flow.lock().await;
        let mut saved = self.identity.lock().await;
        if saved.credentials.is_some() {
            return Err(ClientError::invalid(
                "Sign out before enrolling another controller",
            ));
        }
        let challenge = self
            .worker
            .controller_request(
                reqwest::Method::POST,
                "/v1/controller/auth/request-code",
                "",
                None,
                Some(&serde_json::json!({"email":saved.email,"enrollment_id":saved.enrollment_id})),
            )
            .await?;
        saved.challenge = Some(challenge);
        saved.disabled = false;
        save_json(&self.path, &*saved)?;
        Ok(saved.challenge.clone().expect("assigned challenge"))
    }

    pub async fn verify(&self, code: &str) -> ClientResult<()> {
        let _flow = self.auth_flow.lock().await;
        let saved = self.identity.lock().await.clone();
        let challenge = saved
            .challenge
            .ok_or_else(|| ClientError::invalid("Request a controller code first"))?;
        let credentials = self.worker.controller_request(reqwest::Method::POST, "/v1/controller/auth/verify", "", None, Some(&serde_json::json!({"challenge_id":challenge.challenge_id,"enrollment_id":saved.enrollment_id,"code":code,"device_name":saved.device_name,"platform":"desktop"}))).await?;
        self.accept_enrollment(credentials).await
    }

    pub async fn logout(&self) -> ClientResult<()> {
        let _flow = self.auth_flow.lock().await;
        let _sync = self.sync_lock.lock().await;
        let already_disabled = {
            let saved = self.identity.lock().await;
            saved.disabled || saved.credentials.is_none()
        };
        if already_disabled {
            return self.finish_logout().await;
        }
        let credentials = match self.credentials().await {
            Ok(credentials) => credentials,
            Err(error) if error.status == 401 || error.status == 403 => {
                return self.finish_logout().await
            }
            Err(error) => return Err(error),
        };
        let mut saved = self.identity.lock().await;
        let request_id = *saved.logout_request_id.get_or_insert_with(Uuid::new_v4);
        save_json(&self.path, &*saved)?;
        let result: Result<serde_json::Value, WorkerError> = self
            .worker
            .controller_request(
                reqwest::Method::POST,
                "/v1/controller/auth/logout",
                &credentials.access_token,
                None,
                Some(&serde_json::json!({"request_id":request_id})),
            )
            .await;
        let result = match result {
            Ok(result) => result,
            Err(WorkerError::Unexpected {
                status: 401 | 403, ..
            }) => {
                drop(saved);
                return self.finish_logout().await;
            }
            Err(error) => return Err(error.into()),
        };
        if result.get("revoked").and_then(|v| v.as_bool()) != Some(true) {
            return Err(ClientError::invalid(
                "Authority did not confirm controller revocation",
            ));
        }
        drop(saved);
        self.finish_logout().await
    }

    async fn finish_logout(&self) -> ClientResult<()> {
        let mut saved = self.identity.lock().await;
        saved.disabled = true;
        saved.credentials = None;
        saved.refresh_request_id = None;
        saved.logout_request_id = None;
        saved.challenge = None;
        // Explicit future sign-in can enroll afresh; background sync cannot.
        saved.enrollment_id = Uuid::new_v4();
        save_json(&self.path, &*saved)?;
        self.outbox.lock().await.clear();
        save_json(&self.outbox_path, &Vec::<PendingCommand>::new())?;
        *self.cache.write().await = FleetCatalog::default();
        save_json(
            &self.path.with_file_name("catalog.json"),
            &FleetCatalog::default(),
        )?;
        self.cursors.lock().await.clear();
        *self.last_error.write().await = None;
        *self.verified_at.write().await = None;
        Ok(())
    }
}

fn validate_credentials(
    credentials: &DeviceCredentials,
    expected: Option<Uuid>,
) -> ClientResult<()> {
    if credentials.token_type != "Bearer"
        || credentials.access_token.is_empty()
        || credentials.refresh_token.is_empty()
        || credentials.device.role != DeviceRole::Controller
        || credentials.device.platform != "desktop"
        || credentials.device.revoked_at.is_some()
        || expected.is_some_and(|id| credentials.device.device_id != id)
    {
        return Err(ClientError::invalid(
            "Authority returned mismatched desktop credentials",
        ));
    }
    Ok(())
}
