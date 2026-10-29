//! Isolated device router. Mount only on the pinned TLS listener, never the node-token router.

use crate::controller_auth::{
    authorize, AuthenticatedDevice, AuthorizationError, ControllerAuthority, DispatchAuthorization,
    ReceiverBinding,
};
use crate::controller_wire::*;
use async_trait::async_trait;
use axum::{
    extract::{
        rejection::JsonRejection,
        ws::{CloseFrame, Message, WebSocket},
        Path, Query, Request, State, WebSocketUpgrade,
    },
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::{stream::BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{sleep_until, timeout_at};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct ControllerErrorBody {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{body:?}")]
pub struct ControllerError {
    pub status: StatusCode,
    pub body: ControllerErrorBody,
}

impl ControllerError {
    pub fn new(
        status: StatusCode,
        code: &str,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            body: ControllerErrorBody {
                code: code.into(),
                message: message.into(),
                retryable,
            },
        }
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "owner_unavailable",
            message,
            true,
        )
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message, false)
    }
}

impl From<AuthorizationError> for ControllerError {
    fn from(error: AuthorizationError) -> Self {
        let (status, code, retryable) = match &error {
            AuthorizationError::InvalidCredential | AuthorizationError::Expired => {
                (StatusCode::UNAUTHORIZED, "invalid_credential", false)
            }
            AuthorizationError::Denied => (StatusCode::FORBIDDEN, "authorization_denied", false),
            AuthorizationError::BindingMismatch => (StatusCode::FORBIDDEN, "wrong_owner", false),
            AuthorizationError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "authority_unavailable",
                true,
            ),
        };
        Self::new(status, code, error.to_string(), retryable)
    }
}

impl IntoResponse for ControllerError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({"error":self.body}))).into_response()
    }
}

/// The owner adapter is responsible for durable cursor authentication, snapshot retention,
/// account/device-bound receipts, visibility filtering, and serializing actual run dispatch.
/// No default method fabricates successful empty data.
#[async_trait]
pub trait ControllerBackend: Send + Sync {
    fn host(&self) -> HostDescriptor;
    fn allowed_scopes(&self) -> Vec<ControllerScope>;
    fn management(&self) -> Option<Arc<dyn crate::controller_management::ManagementBackend>> {
        None
    }
    async fn catalog(
        &self,
        device: &AuthenticatedDevice,
        page_token: Option<&str>,
    ) -> Result<CatalogPage, ControllerError>;
    async fn changes(
        &self,
        device: &AuthenticatedDevice,
        cursor: &str,
    ) -> Result<CatalogChanges, ControllerError>;
    async fn catalog_stream(
        &self,
        device: &AuthenticatedDevice,
        cursor: &str,
    ) -> Result<BoxStream<'static, Result<CatalogChange, ControllerError>>, ControllerError>;
    async fn snapshot(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
    ) -> Result<WorkSnapshot, ControllerError>;
    async fn history(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
        page_token: Option<&str>,
    ) -> Result<HistoryPage, ControllerError>;
    async fn history_latest(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
        cursor: &str,
    ) -> Result<HistoryPage, ControllerError>;
    async fn work_stream(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        cursor: &str,
    ) -> Result<BoxStream<'static, Result<WorkFrame, ControllerError>>, ControllerError>;
    async fn work_changes(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
        cursor: &str,
    ) -> Result<WorkChanges, ControllerError>;
    /// Acknowledge only after durable acceptance. Keep `dispatch_auth` with any queue item
    /// and invoke its dispatch guard under the owner lock immediately before side effects.
    async fn accept_command(
        &self,
        device: &AuthenticatedDevice,
        command: WorkCommand,
        dispatch_auth: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError>;
    async fn receipt(
        &self,
        device: &AuthenticatedDevice,
        command_id: Uuid,
    ) -> Result<CommandReceipt, ControllerError>;
    async fn resources(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<ResourceSnapshot, ControllerError>;
}

#[derive(Clone)]
pub struct ControllerContext {
    pub receiver: ReceiverBinding,
    pub authority: Arc<dyn ControllerAuthority>,
    pub backend: Arc<dyn ControllerBackend>,
}

/// This router has no query-token compatibility and no legacy execute/inference/job routes.
/// The TLS owner starts it only after registration and a current runtime lease exist.
pub fn router(ctx: ControllerContext) -> Result<Router, ControllerError> {
    let host = ctx.backend.host();
    if host.owner_node_id != ctx.receiver.owner_node_id
        || host.device_role != DeviceRole::Host
        || host.protocol_version != CONTROLLER_PROTOCOL_VERSION
        || host.availability != HostAvailability::Online
    {
        return Err(ControllerError::unavailable(
            "registered protocol-1 owner is not online",
        ));
    }
    Ok(Router::new()
        .route("/v1/controller/capabilities", get(capabilities))
        .route("/v1/controller/catalog", get(catalog))
        .route("/v1/controller/catalog/changes", get(changes))
        .route("/v1/controller/catalog/stream", get(catalog_stream))
        .route("/v1/controller/work/{kind}/{work_id}", get(snapshot))
        .route("/v1/controller/work/{kind}/{work_id}/history", get(history))
        .route(
            "/v1/controller/work/{kind}/{work_id}/history/latest",
            get(history_latest),
        )
        .route(
            "/v1/controller/work/{kind}/{work_id}/changes",
            get(work_changes),
        )
        .route(
            "/v1/controller/work/{kind}/{work_id}/stream",
            get(work_stream),
        )
        .route(
            "/v1/controller/work/{kind}/{work_id}/commands",
            post(command),
        )
        .route("/v1/controller/commands/{command_id}", get(receipt))
        .route("/v1/controller/resources", get(resources))
        .merge(crate::controller_management::router())
        .layer(axum::middleware::from_fn(reject_origin))
        .with_state(ctx))
}

async fn reject_origin(request: Request, next: Next) -> Response {
    let mut response = if request.headers().contains_key(header::ORIGIN) {
        ControllerError::new(
            StatusCode::FORBIDDEN,
            "origin_forbidden",
            "browser origins are not allowed",
            false,
        )
        .into_response()
    } else {
        next.run(request).await
    };
    let status = response.status();
    if (status.is_client_error() || status.is_server_error())
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        let code = match status {
            StatusCode::NOT_FOUND => "route_not_found",
            StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
            _ => "invalid_request",
        };
        response = ControllerError::new(
            status,
            code,
            status.canonical_reason().unwrap_or("request refused"),
            false,
        )
        .into_response();
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

pub(crate) fn bearer(headers: &HeaderMap) -> Result<String, AuthorizationError> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(AuthorizationError::InvalidCredential);
    }
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty() && !v.chars().any(char::is_whitespace))
        .ok_or(AuthorizationError::InvalidCredential)?;
    Ok(value.to_string())
}

pub(crate) async fn authenticate(
    ctx: &ControllerContext,
    headers: &HeaderMap,
    scope: ControllerScope,
) -> Result<(String, AuthenticatedDevice), ControllerError> {
    let token = bearer(headers)?;
    if !ctx.backend.allowed_scopes().contains(&scope) {
        return Err(ControllerError::new(
            StatusCode::FORBIDDEN,
            "host_policy_denied",
            "owner disabled this controller operation",
            false,
        ));
    }
    let device = authorize(ctx.authority.as_ref(), &token, &ctx.receiver, scope).await?;
    Ok((token, device))
}

async fn capabilities(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
) -> Result<Json<ControllerCapabilities>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let allowed = ctx.backend.allowed_scopes();
    Ok(Json(ControllerCapabilities {
        protocol_version: CONTROLLER_PROTOCOL_VERSION,
        host: ctx.backend.host(),
        store_generation: ctx.receiver.store_generation,
        allowed_scopes: device
            .scopes()
            .iter()
            .filter(|s| allowed.contains(s))
            .copied()
            .collect(),
    }))
}

#[derive(Deserialize)]
struct PageQuery {
    page_token: Option<String>,
}
#[derive(Deserialize)]
struct CursorQuery {
    cursor: String,
}
#[derive(Deserialize)]
struct WorkQuery {
    store_generation: Uuid,
    page_token: Option<String>,
}

#[derive(Deserialize)]
struct WorkChangesQuery {
    store_generation: Uuid,
    cursor: String,
}

async fn work_changes(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    Query(q): Query<WorkChangesQuery>,
) -> Result<Json<WorkChanges>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::HistoryRead).await?;
    check_generation(&ctx, q.store_generation)?;
    let result = ctx
        .backend
        .work_changes(
            &device,
            &work_ref(&ctx, kind, id),
            q.store_generation,
            &q.cursor,
        )
        .await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn catalog(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<CatalogPage>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let result = ctx
        .backend
        .catalog(&device, q.page_token.as_deref())
        .await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn changes(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Query(q): Query<CursorQuery>,
) -> Result<Json<CatalogChanges>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let result = ctx.backend.changes(&device, &q.cursor).await?;
    device.check_fresh()?;
    Ok(Json(result))
}

fn work_ref(ctx: &ControllerContext, kind: WorkKind, work_id: Uuid) -> WorkRef {
    WorkRef {
        owner_node_id: ctx.receiver.owner_node_id,
        kind,
        work_id,
    }
}

pub(crate) fn check_generation(
    ctx: &ControllerContext,
    generation: Uuid,
) -> Result<(), ControllerError> {
    if generation != ctx.receiver.store_generation {
        return Err(ControllerError::new(
            StatusCode::CONFLICT,
            "store_generation_mismatch",
            "store generation changed; take a new snapshot",
            false,
        ));
    }
    Ok(())
}

async fn snapshot(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    Query(q): Query<WorkQuery>,
) -> Result<Json<WorkSnapshot>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::HistoryRead).await?;
    check_generation(&ctx, q.store_generation)?;
    let result = ctx
        .backend
        .snapshot(&device, &work_ref(&ctx, kind, id), q.store_generation)
        .await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn history(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    Query(q): Query<WorkQuery>,
) -> Result<Json<HistoryPage>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::HistoryRead).await?;
    check_generation(&ctx, q.store_generation)?;
    let result = ctx
        .backend
        .history(
            &device,
            &work_ref(&ctx, kind, id),
            q.store_generation,
            q.page_token.as_deref(),
        )
        .await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn history_latest(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    Query(q): Query<WorkChangesQuery>,
) -> Result<Json<HistoryPage>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::HistoryRead).await?;
    check_generation(&ctx, q.store_generation)?;
    let result = ctx
        .backend
        .history_latest(
            &device,
            &work_ref(&ctx, kind, id),
            q.store_generation,
            &q.cursor,
        )
        .await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn command(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    body: Result<Json<WorkCommand>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandReceipt>), ControllerError> {
    // Reject missing credentials before parsing a potentially forged command body.
    bearer(&headers)?;
    let Json(command) = body.map_err(|e| ControllerError::bad_request(e.body_text()))?;
    let scope = command.action.action().required_scope();
    let (token, device) = authenticate(&ctx, &headers, scope).await?;
    if command.work != work_ref(&ctx, kind, id) {
        return Err(ControllerError::new(
            StatusCode::FORBIDDEN,
            "wrong_owner",
            "command target differs from the receiving work",
            false,
        ));
    }
    check_generation(&ctx, command.expected_store_generation)?;
    if matches!(
        command.action.action(),
        WorkAction::Steer | WorkAction::Stop
    ) && command.expected_run_id.is_none()
    {
        return Err(ControllerError::bad_request(
            "this action requires an exact active run",
        ));
    }
    let guard = DispatchAuthorization::new(
        ctx.authority.clone(),
        token,
        ctx.receiver.clone(),
        scope,
        device.clone(),
    );
    let receipt = ctx.backend.accept_command(&device, command, guard).await?;
    Ok((StatusCode::ACCEPTED, Json(receipt)))
}

async fn receipt(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<CommandReceipt>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let result = ctx.backend.receipt(&device, id).await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn resources(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
) -> Result<Json<ResourceSnapshot>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::ResourcesRead).await?;
    let result = ctx.backend.resources(&device).await?;
    device.check_fresh()?;
    Ok(Json(result))
}

async fn catalog_stream(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Query(q): Query<CursorQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ControllerError> {
    let (token, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let events = ctx.backend.catalog_stream(&device, &q.cursor).await?;
    device.check_fresh()?;
    Ok(ws.on_upgrade(move |socket| {
        stream_socket(
            socket,
            ctx,
            token,
            device,
            ControllerScope::CatalogRead,
            events,
        )
    }))
}

async fn work_stream(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path((kind, id)): Path<(WorkKind, Uuid)>,
    Query(q): Query<CursorQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ControllerError> {
    let (token, device) = authenticate(&ctx, &headers, ControllerScope::HistoryRead).await?;
    let events = ctx
        .backend
        .work_stream(&device, &work_ref(&ctx, kind, id), &q.cursor)
        .await?;
    device.check_fresh()?;
    Ok(ws.on_upgrade(move |socket| {
        stream_socket(
            socket,
            ctx,
            token,
            device,
            ControllerScope::HistoryRead,
            events,
        )
    }))
}

async fn stream_socket<T: Serialize + Send + 'static>(
    mut socket: WebSocket,
    ctx: ControllerContext,
    token: String,
    mut device: AuthenticatedDevice,
    scope: ControllerScope,
    mut events: BoxStream<'static, Result<T, ControllerError>>,
) {
    let close = loop {
        if let Err(error) = device.check_fresh() {
            break error.close_code();
        }
        tokio::select! {
            biased;
            _ = sleep_until(device.deadline()) => { break device.check_fresh().err().map(|e| e.close_code()).unwrap_or(4413); }
            _ = sleep_until(device.refresh_at()) => {
                // Block frame delivery during refresh. Even a stuck authority cannot keep an
                // old lease alive because the deadline surrounds the entire request.
                match timeout_at(device.deadline(), authorize(ctx.authority.as_ref(), &token, &ctx.receiver, scope)).await {
                    Ok(Ok(fresh)) if fresh.account_id() == device.account_id() && fresh.device_id() == device.device_id()
                        && fresh.device_revision() == device.device_revision() && ctx.backend.allowed_scopes().contains(&scope) => device = fresh,
                    Ok(Ok(_)) => break 4403,
                    Ok(Err(error)) => break error.close_code(),
                    Err(_) => break device.check_fresh().err().map(|e| e.close_code()).unwrap_or(4413),
                }
            }
            incoming = socket.recv() => {
                match incoming { Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return, _ => {} }
            }
            event = events.next() => {
                let event = match event { Some(Ok(event)) => event, Some(Err(_)) => break 4413, None => break 1000 };
                if !ctx.backend.allowed_scopes().contains(&scope) { break 4403; }
                if let Err(error) = device.check_fresh() { break error.close_code(); }
                let json = match serde_json::to_string(&event) { Ok(json) => json, Err(_) => break 4413 };
                // A backpressured connection may not drain frames beyond its lease either.
                match timeout_at(device.deadline(), socket.send(Message::Text(json.into()))).await {
                    Ok(Ok(())) => {}, Ok(Err(_)) => return, Err(_) => break 4413,
                }
            }
        }
    };
    let reason = match close {
        4401 => "credential_expired",
        4403 => "authorization_denied",
        4413 => "authorization_unavailable",
        _ => "stream_complete",
    };
    // Bound transport close so a slow reader cannot hold a task indefinitely.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        socket.send(Message::Close(Some(CloseFrame {
            code: close,
            reason: reason.into(),
        }))),
    )
    .await;
}
