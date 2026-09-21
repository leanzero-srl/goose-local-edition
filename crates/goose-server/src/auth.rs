use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
pub use goose::acp::transport::auth::check_acp_token;
use goose::acp::transport::auth::{accepts_bearer, bearer_token, token_matches};

pub async fn check_token(
    State(state): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.uri().path() == "/status"
        || request.uri().path() == "/mcp-app-proxy"
        || request.uri().path() == "/mcp-app-guest"
    {
        return Ok(next.run(request).await);
    }
    let secret_key = request
        .headers()
        .get("X-Secret-Key")
        .and_then(|value| value.to_str().ok());

    // The API routes (OpenAI-compatible, CogniRunner task mode) are driven by clients that
    // send `Authorization: Bearer <key>`. It is the SAME secret, compared the same constant-time
    // way, and accepted on those paths only (`goose::acp::transport::auth::accepts_bearer`, the
    // one predicate both servers consult) — every other route still requires X-Secret-Key.
    let bearer = if accepts_bearer(request.uri().path()) {
        bearer_token(request.headers())
    } else {
        None
    };

    if token_matches(secret_key, &state) || token_matches(bearer, &state) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
