//! Host lifecycle composed by LinkManager. Listener access opens only after provider
//! enrollment and a current owner lease. Authority loss never cancels owner work.

use async_trait::async_trait;
use axum::{
    extract::{Request, State},
    response::{IntoResponse, Response},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{task::JoinHandle, time::Instant};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    controller::{self, ControllerBackend, ControllerContext, ControllerError},
    controller_auth::{AuthorizationError, ControllerAuthority, ReceiverBinding, WorkerAuthority},
    controller_tls::{start_controller_tls, ControllerTlsIdentity},
    controller_wire::*,
    manager::{LinkError, Mesh},
    mesh::{BackendState, MeshError},
    owner_identity::{
        invalid, load_or_create_private, read_private, OwnerIdentity, OwnerIdentityError,
    },
    state::PeerRegistry,
    worker_client::WorkerClient,
};

#[derive(Clone, Debug)]
pub struct HostWatermark {
    pub store_generation: Uuid,
    pub catalog_revision: u64,
}
pub type HostResourceSampler =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, crate::load::NodeLoad> + Send + Sync>;

#[async_trait]
pub trait ControllerHostFactory: Send + Sync {
    /// Lives beside the actual session store, not an account-selected scratch database.
    fn directory(&self) -> PathBuf;
    fn listen_address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], DEFAULT_CONTROLLER_PORT))
    }
    async fn watermark(&self) -> Result<HostWatermark, ControllerError>;
    async fn backend(
        &self,
        host: HostDescriptor,
        account_id: Uuid,
        resources: HostResourceSampler,
    ) -> Result<Arc<dyn ControllerBackend>, ControllerError>;
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountBoundary {
    version: u32,
    account_id: Uuid,
    owner_node_id: Uuid,
    email: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSecret {
    version: u32,
    email: String,
    secret: String,
}

pub(crate) fn private_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, OwnerIdentityError> {
    read_private(path)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).map_err(|source| OwnerIdentityError::Json {
                path: path.to_path_buf(),
                source,
            })
        })
        .transpose()
}

pub(crate) fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), OwnerIdentityError> {
    read_private(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| OwnerIdentityError::Io {
            path: parent.into(),
            source,
        })?;
    }
    let bytes = serde_json::to_vec(value).map_err(|source| OwnerIdentityError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    use std::io::Write;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|source| OwnerIdentityError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn remember_login_secret(
    identity_path: &Path,
    email: &str,
    secret: &str,
) -> Result<(), LinkError> {
    let path = secret_path(identity_path);
    if !valid_secret(secret) {
        return Err(LinkError::NoNodeSecret);
    }
    if let Some(previous) = private_json::<HostSecret>(&path)? {
        if previous.version != 1 || previous.email != email || previous.secret != secret {
            return Err(invalid(
                &path,
                "host secret changed; explicit logout and sign-in required",
            )
            .into());
        }
    }
    save_json(
        &path,
        &HostSecret {
            version: 1,
            email: email.into(),
            secret: secret.into(),
        },
    )?;
    Ok(())
}

fn valid_secret(secret: &str) -> bool {
    secret.len() == 64
        && secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn secret_path(identity_path: &Path) -> PathBuf {
    identity_path.with_file_name("host-secret.json")
}

pub(crate) fn login_secret(identity_path: &Path, email: &str) -> Result<String, LinkError> {
    let path = secret_path(identity_path);
    let secret = private_json::<HostSecret>(&path)?.ok_or(LinkError::NoNodeSecret)?;
    if secret.version != 1 || secret.email != email || !valid_secret(&secret.secret) {
        return Err(invalid(
            &path,
            "host sign-in secret is corrupt or belongs to another account",
        )
        .into());
    }
    Ok(secret.secret)
}

pub(crate) fn clear_login_secret(identity_path: &Path) -> Result<(), LinkError> {
    remove_private(&secret_path(identity_path))?;
    Ok(())
}

pub(crate) async fn logout_host(
    factory: &dyn ControllerHostFactory,
    worker: &WorkerClient,
) -> Result<(), LinkError> {
    let directory = factory.directory();
    let path = directory.join("host-credential.json");
    if let Some(credentials) = private_json::<HostCredentials>(&path)? {
        let runtime =
            private_json::<Uuid>(&directory.join("runtime.json"))?.unwrap_or_else(Uuid::new_v4);
        worker
            .release_controller_host(&credentials.host_credential, runtime)
            .await?;
        remove_private(&path)?;
    }
    Ok(())
}
fn remove_private(path: &Path) -> Result<(), OwnerIdentityError> {
    if read_private(path)?.is_some() {
        std::fs::remove_file(path).map_err(|source| OwnerIdentityError::Io {
            path: path.into(),
            source,
        })?;
    }
    Ok(())
}

#[derive(Clone)]
struct RouteLease {
    router: Router,
    receiver: ReceiverBinding,
    deadline: Instant,
}
type Gate = Arc<RwLock<Option<RouteLease>>>;

async fn route_current(State(gate): State<Gate>, request: Request) -> Response {
    if request.headers().contains_key(axum::http::header::ORIGIN) {
        let mut response = ControllerError::new(
            axum::http::StatusCode::FORBIDDEN,
            "origin_denied",
            "native controller requests cannot carry Origin",
            false,
        )
        .into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        return response;
    }
    let router = gate
        .read()
        .expect("controller gate poisoned")
        .as_ref()
        .filter(|lease| Instant::now() < lease.deadline)
        .map(|lease| lease.router.clone());
    let mut response = match router {
        Some(router) => match router.oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        },
        None => ControllerError::unavailable("host has no current authority lease").into_response(),
    };
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

struct LiveAuthority {
    worker: WorkerAuthority,
    gate: Gate,
    backend: Arc<dyn ControllerBackend>,
}
impl LiveAuthority {
    fn check(&self, receiver: &ReceiverBinding) -> Result<Duration, AuthorizationError> {
        let gate = self.gate.read().expect("controller gate poisoned");
        let lease = gate
            .as_ref()
            .filter(|lease| lease.receiver == *receiver)
            .ok_or(AuthorizationError::BindingMismatch)?;
        lease
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| AuthorizationError::Unavailable("host runtime lease expired".into()))
    }
}
#[async_trait]
impl ControllerAuthority for LiveAuthority {
    async fn introspect(
        &self,
        token: &str,
        receiver: &ReceiverBinding,
        scope: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        self.check(receiver)?;
        if !self.backend.allowed_scopes().contains(&scope) {
            return Err(AuthorizationError::Denied);
        }
        let mut lease = self.worker.introspect(token, receiver, scope).await?;
        let remaining = self.check(receiver)?;
        if !self.backend.allowed_scopes().contains(&scope) {
            return Err(AuthorizationError::Denied);
        }
        if let Some(lease) = lease.as_mut() {
            lease.lease_seconds = lease
                .lease_seconds
                .min(remaining.as_secs().try_into().unwrap_or(u32::MAX));
        }
        Ok(lease)
    }
}

pub(crate) struct PreparedHost {
    factory: Arc<dyn ControllerHostFactory>,
    worker: WorkerClient,
    credentials: HostCredentials,
    owner: OwnerIdentity,
    account: HostBootstrap,
    runtime_id: Uuid,
    directory: PathBuf,
    gate: Gate,
    tls: crate::controller_tls::ControllerTlsHandle,
    identity: ControllerTlsIdentity,
    name: String,
}

impl PreparedHost {
    pub(crate) async fn prepare(
        factory: Arc<dyn ControllerHostFactory>,
        worker: WorkerClient,
        account_token: &str,
        email: &str,
        name: &str,
    ) -> Result<Self, LinkError> {
        let directory = factory.directory();
        let owner = OwnerIdentity::load_or_create(directory.join("owner.json"))?;
        let boundary_path = directory.join("account.json");
        if let Some(boundary) = private_json::<AccountBoundary>(&boundary_path)? {
            if boundary.version != 1
                || boundary.email != email
                || boundary.owner_node_id != owner.owner_node_id
            {
                return Err(invalid(&boundary_path, "this session store belongs to another Link account; sign-in cannot reassign it").into());
            }
        }
        let identity =
            ControllerTlsIdentity::load_or_create(directory.join("tls.json"), owner.owner_node_id)?;
        let gate: Gate = Arc::new(RwLock::new(None));
        let router = Router::new()
            .fallback(route_current)
            .with_state(gate.clone());
        // Userspace tailscaled forwards inbound mesh TCP to the SAME loopback port.
        // https://github.com/tailscale/tailscale/blob/main/wgengine/netstack/netstack.go
        let address = factory.listen_address();
        if !address.ip().is_loopback() {
            return Err(LinkError::ControllerHost(
                "controller host must bind loopback for the owned userspace mesh".into(),
            ));
        }
        let tls = start_controller_tls(address, router, identity.clone()).await?;
        let credential_path = directory.join("host-credential.json");
        let previous = private_json::<HostCredentials>(&credential_path)?;
        let runtime_id = Uuid::new_v4();
        let credentials = worker
            .register_controller_host(
                account_token,
                previous.as_ref().map(|c| c.host_credential.as_str()),
                &HostRegistration {
                    runtime_id,
                    owner_node_id: owner.owner_node_id,
                    registration_id: owner.registration_id,
                    host_name: name.into(),
                    protocol_version: CONTROLLER_PROTOCOL_VERSION,
                    tls_spki_sha256: identity.spki_sha256(),
                    controller_port: tls.local_addr.port(),
                },
            )
            .await?;
        save_json(&credential_path, &credentials)?;
        let account = worker
            .bootstrap_controller_host(&credentials.host_credential)
            .await?;
        if account.owner_node_id != owner.owner_node_id || account.account_id.is_nil() {
            return Err(LinkError::ControllerHost(
                "worker bootstrap belongs to another owner or has no account".into(),
            ));
        }
        let boundary = AccountBoundary {
            version: 1,
            account_id: account.account_id,
            owner_node_id: owner.owner_node_id,
            email: email.into(),
        };
        let bytes = load_or_create_private(&boundary_path, || {
            serde_json::to_vec(&boundary).map_err(|source| OwnerIdentityError::Json {
                path: boundary_path.clone(),
                source,
            })
        })?;
        let saved: AccountBoundary =
            serde_json::from_slice(&bytes).map_err(|source| OwnerIdentityError::Json {
                path: boundary_path.clone(),
                source,
            })?;
        if saved.version != 1
            || saved.account_id != boundary.account_id
            || saved.owner_node_id != boundary.owner_node_id
            || saved.email != boundary.email
        {
            return Err(invalid(
                &boundary_path,
                "authenticated account does not own this session store",
            )
            .into());
        }
        Ok(Self {
            factory,
            worker,
            credentials,
            owner,
            account,
            runtime_id,
            directory,
            gate,
            tls,
            identity,
            name: name.into(),
        })
    }

    pub(crate) async fn login_secret(
        &self,
        identity_path: &Path,
        account_token: &str,
        email: &str,
    ) -> Result<String, LinkError> {
        match login_secret(identity_path, email) {
            Ok(secret) => return Ok(secret),
            Err(LinkError::NoNodeSecret) => {}
            Err(error) => return Err(error),
        }
        let recovered = self
            .worker
            .recover_host_node_secret(account_token, &self.credentials.host_credential)
            .await?;
        if recovered.account_id != self.account.account_id
            || recovered.owner_node_id != self.owner.owner_node_id
            || recovered.email != email
        {
            return Err(LinkError::ControllerHost(
                "recovered secret belongs to another account or owner".into(),
            ));
        }
        if !valid_secret(&recovered.node_secret) {
            return Err(LinkError::NoNodeSecret);
        }
        Ok(recovered.node_secret)
    }

    pub(crate) async fn mesh_context(&self) -> Result<HostMeshContext, LinkError> {
        let context = self
            .worker
            .controller_host_mesh_context(&self.credentials.host_credential)
            .await?;
        let url = reqwest::Url::parse(&context.login_server)
            .map_err(|_| LinkError::ControllerHost("host mesh control server is invalid".into()))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(LinkError::ControllerHost(
                "host mesh control server must be HTTPS without credentials".into(),
            ));
        }
        Ok(context)
    }

    pub(crate) async fn enroll(
        &self,
        mesh: &dyn Mesh,
        context: HostMeshContext,
        hostname: &str,
    ) -> Result<(), LinkError> {
        if let Some(enrollment) = context.enrollment {
            let status = mesh.status().await?;
            if enrollment.status == HostEnrollmentStatus::Confirmed {
                mesh.resume().await?;
                let status = mesh.status().await?;
                if status.self_node_key != enrollment.mesh_node_key || !status.online {
                    return Err(LinkError::ControllerHost("confirmed host mesh identity is not running; explicit mesh recovery required".into()));
                }
                return Ok(());
            }
            // A prior join can consume its one-use key yet lose the ACK. Check exact
            // provider key binding BEFORE issuing another reauthentication command.
            if let Some(key) = status.self_node_key.as_deref() {
                match self
                    .worker
                    .confirm_controller_host(
                        &self.credentials.host_credential,
                        enrollment.enrollment_id,
                        key,
                    )
                    .await
                {
                    Ok(confirmed) => {
                        if confirmed.owner_node_id != self.owner.owner_node_id
                            || confirmed.mesh_node_key != key
                        {
                            return Err(LinkError::ControllerHost(
                                "provider confirmation belongs to another host".into(),
                            ));
                        }
                        return Ok(());
                    }
                    Err(error) if error.has_controller_code("mesh_binding_unverified") => {}
                    Err(error) => return Err(error.into()),
                }
            }
            return self
                .consume_enrollment(
                    mesh,
                    enrollment.enrollment_id,
                    hostname,
                    &context.login_server,
                )
                .await;
        }
        let path = self.directory.join("pending-enrollment.json");
        // Request identity is durable before the external mint. Rotate only after a
        // positive authority response, never because an authority lookup timed out.
        let enrollment_id = match private_json::<Uuid>(&path)? {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4();
                save_json(&path, &id)?;
                id
            }
        };
        match self
            .consume_enrollment(mesh, enrollment_id, hostname, &context.login_server)
            .await
        {
            Err(LinkError::Worker(error)) if error.has_controller_code("enrollment_expired") => {
                let new_id = Uuid::new_v4();
                save_json(&path, &new_id)?;
                self.consume_enrollment(mesh, new_id, hostname, &context.login_server)
                    .await
            }
            result => result,
        }
    }

    async fn consume_enrollment(
        &self,
        mesh: &dyn Mesh,
        enrollment_id: Uuid,
        hostname: &str,
        login_server: &str,
    ) -> Result<(), LinkError> {
        let key = self
            .worker
            .join_controller_host(&self.credentials.host_credential, enrollment_id)
            .await?;
        if key.enrollment_id != enrollment_id
            || key.auth_key.is_empty()
            || key.login_server != login_server
        {
            return Err(LinkError::ControllerHost(
                "worker enrollment receipt is inconsistent".into(),
            ));
        }
        // A one-use key can bind at the provider before the CLI reports failure.
        // Reconcile only after the authority confirms this exact issued enrollment.
        let cli_failure = match mesh.reauthenticate(&key.auth_key, hostname).await {
            Ok(()) => None,
            Err(error @ MeshError::JoinFailed { .. }) => Some(error),
            Err(error) => return Err(error.into()),
        };
        let needs_resume = cli_failure.is_some();
        let status = mesh.status().await?;
        let cli_failure = match cli_failure {
            Some(error)
                if status.backend_state != BackendState::Running
                    || status.self_node_key.is_none() =>
            {
                return Err(error.into());
            }
            failure => failure,
        };
        let node_key = status.self_node_key.as_deref().ok_or_else(|| {
            LinkError::ControllerHost("joined host has no provider node key".into())
        })?;
        let confirmed = self
            .worker
            .confirm_controller_host(&self.credentials.host_credential, enrollment_id, node_key)
            .await
            .map_err(|source| match cli_failure {
                Some(join) => LinkError::HostEnrollmentReconciliation {
                    join: Box::new(join),
                    source: Box::new(source),
                },
                None => source.into(),
            })?;
        if confirmed.owner_node_id != self.owner.owner_node_id
            || confirmed.mesh_node_key != node_key
        {
            return Err(LinkError::ControllerHost(
                "provider confirmation belongs to another host".into(),
            ));
        }
        if needs_resume {
            mesh.resume().await?;
            let resumed = mesh.status().await?;
            if resumed.backend_state != BackendState::Running
                || resumed.self_node_key.as_deref() != Some(node_key)
            {
                return Err(LinkError::ControllerHost(
                    "provider-confirmed host identity changed or stopped during resume".into(),
                ));
            }
        }
        remove_private(&self.directory.join("pending-enrollment.json"))?;
        Ok(())
    }

    pub(crate) async fn activate(
        self,
        mesh: Arc<dyn Mesh>,
        registry: PeerRegistry,
        mesh_ip: String,
        peer_port: u16,
        resources: HostResourceSampler,
    ) -> Result<ControllerHostHandle, LinkError> {
        registry.peer_authority_unavailable();
        let host = HostDescriptor {
            owner_node_id: self.owner.owner_node_id,
            name: self.name.clone(),
            device_role: DeviceRole::Host,
            protocol_version: CONTROLLER_PROTOCOL_VERSION,
            availability: HostAvailability::Online,
            last_seen_at: Some(Utc::now()),
            endpoints: vec![MeshEndpoint {
                url: format!(
                    "https://{}:{}",
                    if mesh_ip.contains(':') {
                        format!("[{mesh_ip}]")
                    } else {
                        mesh_ip
                    },
                    self.tls.local_addr.port()
                ),
                tls_spki_sha256: self.identity.spki_sha256(),
            }],
            capabilities: Vec::new(),
        };
        let backend = self
            .factory
            .backend(host, self.account.account_id, resources)
            .await?;
        let authority: Arc<dyn ControllerAuthority> = Arc::new(LiveAuthority {
            worker: WorkerAuthority::new(
                self.worker.clone(),
                self.credentials.host_credential.clone(),
            )?,
            gate: self.gate.clone(),
            backend: backend.clone(),
        });
        save_json(&self.directory.join("runtime.json"), &self.runtime_id)?;
        self.renew(
            mesh.as_ref(),
            &registry,
            peer_port,
            backend.clone(),
            authority.clone(),
        )
        .await?;
        let gate = self.gate.clone();
        let error = Arc::new(RwLock::new(None));
        let task_error = error.clone();
        let task_registry = registry.clone();
        let worker = self.worker.clone();
        let credential = self.credentials.host_credential.clone();
        let directory = self.directory.clone();
        let runtime_id = self.runtime_id;
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                // Security lease cadence, never a limit on owner/model work.
                tokio::select! { _ = &mut stopped => break, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
                let result = if self.tls.is_finished() {
                    Err(LinkError::ControllerHost(
                        "controller TLS listener stopped".into(),
                    ))
                } else {
                    tokio::select! {
                        _ = &mut stopped => break,
                        result = self.renew(
                        mesh.as_ref(),
                        &task_registry,
                        peer_port,
                        backend.clone(),
                        authority.clone(),
                        ) => result,
                    }
                };
                match result {
                    Ok(()) => *task_error.write().expect("host error poisoned") = None,
                    Err(error) => {
                        *self.gate.write().expect("controller gate poisoned") = None;
                        task_registry.peer_authority_unavailable();
                        *task_error.write().expect("host error poisoned") = Some(error.to_string());
                    }
                }
            }
            self.tls
        });
        Ok(ControllerHostHandle {
            stop: Some(stop),
            task: Some(task),
            gate,
            registry,
            error,
            worker,
            credential,
            directory,
            runtime_id,
        })
    }

    async fn renew(
        &self,
        mesh: &dyn Mesh,
        registry: &PeerRegistry,
        peer_port: u16,
        backend: Arc<dyn ControllerBackend>,
        authority: Arc<dyn ControllerAuthority>,
    ) -> Result<(), LinkError> {
        let status = mesh.status().await?;
        let key = status
            .self_node_key
            .ok_or_else(|| LinkError::ControllerHost("host mesh identity is missing".into()))?;
        let watermark = self.factory.watermark().await?;
        let started = Instant::now();
        let wall_started = Utc::now();
        let lease = self
            .worker
            .lease_controller_host(
                &self.credentials.host_credential,
                &HostLeaseRequest {
                    owner_node_id: self.owner.owner_node_id,
                    runtime_id: self.runtime_id,
                    store_generation: watermark.store_generation,
                    catalog_revision: watermark.catalog_revision,
                    mesh_node_key: key,
                    capabilities: backend.allowed_scopes(),
                },
            )
            .await?;
        let remaining = (lease.lease_expires_at - wall_started)
            .to_std()
            .map_err(|_| LinkError::ControllerHost("host lease already expired".into()))?;
        let deadline = started + remaining;
        if deadline <= Instant::now() {
            return Err(LinkError::ControllerHost(
                "host lease expired in transit".into(),
            ));
        }
        let receiver = ReceiverBinding {
            owner_node_id: self.owner.owner_node_id,
            runtime_id: self.runtime_id,
            store_generation: watermark.store_generation,
            host_lease_id: lease.lease_id,
            host_revision: lease.host_revision,
        };
        let admission = self
            .worker
            .controller_peer_admission(&self.credentials.host_credential)
            .await?;
        let router = controller::router(ControllerContext {
            receiver: receiver.clone(),
            backend,
            authority,
        })?;
        registry.set_peer_admission(admission);
        // A role may become authoritative without any change to the mesh peer tuple.
        registry.set_mesh_peers(&status.peers, peer_port);
        *self.gate.write().expect("controller gate poisoned") = Some(RouteLease {
            router,
            receiver,
            deadline,
        });
        Ok(())
    }
}

pub(crate) struct ControllerHostHandle {
    task: Option<JoinHandle<crate::controller_tls::ControllerTlsHandle>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    gate: Gate,
    registry: PeerRegistry,
    error: Arc<RwLock<Option<String>>>,
    worker: WorkerClient,
    credential: String,
    directory: PathBuf,
    runtime_id: Uuid,
}
impl ControllerHostHandle {
    pub(crate) fn error(&self) -> Option<String> {
        self.error.read().expect("host error poisoned").clone()
    }
    pub(crate) async fn shutdown(mut self, logout: bool) -> Result<(), LinkError> {
        let mut errors = Vec::new();
        *self.gate.write().expect("controller gate poisoned") = None;
        self.registry.peer_authority_unavailable();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            match task.await {
                Ok(tls) => {
                    if let Err(error) = tls.shutdown().await {
                        errors.push(error.to_string());
                    }
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        let withdrawal = if logout {
            self.worker
                .release_controller_host(&self.credential, self.runtime_id)
                .await
        } else {
            self.worker
                .withdraw_controller_host(&self.credential, self.runtime_id)
                .await
        };
        match withdrawal {
            Ok(()) if logout => {
                if let Err(error) = remove_private(&self.directory.join("host-credential.json")) {
                    errors.push(error.to_string());
                }
            }
            Ok(()) => {}
            Err(error) => errors.push(error.to_string()),
        }
        if !errors.is_empty() {
            return Err(LinkError::ControllerHost(errors.join("; ")));
        }
        Ok(())
    }
}
impl Drop for ControllerHostHandle {
    fn drop(&mut self) {
        *self.gate.write().expect("controller gate poisoned") = None;
        self.registry.peer_authority_unavailable();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(test)]
mod tests;
