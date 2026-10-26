use super::*;
use crate::controller::{authenticate, bearer, check_generation, ControllerContext};
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};

pub(crate) fn router() -> Router<ControllerContext> {
    Router::new()
        .route("/v1/controller/management/models", get(models))
        .route("/v1/controller/management/downloads", get(download))
        .route("/v1/controller/management/providers", get(providers))
        .route("/v1/controller/management/commands", post(command))
        .route("/v1/controller/management/commands/{id}", get(receipt))
}
fn backend(
    ctx: &ControllerContext,
) -> Result<std::sync::Arc<dyn ManagementBackend>, ControllerError> {
    ctx.backend.management().ok_or_else(|| {
        ControllerError::new(
            StatusCode::NOT_IMPLEMENTED,
            "management_unavailable",
            "host management is not installed",
            false,
        )
    })
}
async fn models(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::ModelsRead).await?;
    let result = backend(&ctx)?.models(&device).await?;
    device.check_fresh()?;
    Ok(Json(result))
}
#[derive(Deserialize)]
struct DownloadQuery {
    repo_id: String,
}
async fn download(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Query(query): Query<DownloadQuery>,
) -> Result<Json<serde_json::Value>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::ModelsRead).await?;
    let result = backend(&ctx)?.download(&device, &query.repo_id).await?;
    device.check_fresh()?;
    Ok(Json(result))
}
async fn providers(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
) -> Result<Json<ManagedProviders>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::ProvidersRead).await?;
    let result = backend(&ctx)?.providers(&device).await?;
    device.check_fresh()?;
    Ok(Json(result))
}
async fn command(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    body: Result<Json<ManagementCommand>, JsonRejection>,
) -> Result<(StatusCode, Json<ManagementReceipt>), ControllerError> {
    bearer(&headers)?;
    let Json(command) = body.map_err(|_| {
        ControllerError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid management command",
            false,
        )
    })?;
    let scope = command.action.required_scope();
    let (token, device) = authenticate(&ctx, &headers, scope).await?;
    if command.owner_node_id != ctx.receiver.owner_node_id {
        return Err(ControllerError::new(
            StatusCode::FORBIDDEN,
            "wrong_owner",
            "command target differs from the receiving host",
            false,
        ));
    }
    check_generation(&ctx, command.expected_store_generation)?;
    let guard = DispatchAuthorization::new(
        ctx.authority.clone(),
        token,
        ctx.receiver.clone(),
        scope,
        device.clone(),
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(backend(&ctx)?.accept(&device, command, guard).await?),
    ))
}
async fn receipt(
    State(ctx): State<ControllerContext>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ManagementReceipt>, ControllerError> {
    let (_, device) = authenticate(&ctx, &headers, ControllerScope::CatalogRead).await?;
    let result = backend(&ctx)?.receipt(&device, id).await?;
    device.check_fresh()?;
    Ok(Json(result))
}
