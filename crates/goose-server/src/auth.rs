use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
pub use goose::acp::transport::auth::check_acp_token;
use goose::acp::transport::auth::token_matches;

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

    // The OpenAI-compatible routes are driven by OpenAI clients, which only know how to send
    // `Authorization: Bearer <key>`. It is the SAME secret, compared the same constant-time way,
    // and accepted on those two paths only — every other route still requires X-Secret-Key.
    let bearer = if is_openai_compat_path(request.uri().path()) {
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

pub fn is_openai_compat_path(path: &str) -> bool {
    path == "/v1/models" || path == "/v1/chat/completions"
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
        })
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::AUTHORIZATION, HeaderMap, HeaderValue};

    #[test]
    fn bearer_token_parses_the_scheme_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bearer abc"));
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer_token(&headers), None);
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn only_the_two_openai_paths_take_bearer() {
        assert!(is_openai_compat_path("/v1/models"));
        assert!(is_openai_compat_path("/v1/chat/completions"));
        assert!(!is_openai_compat_path("/v1/swarm/stream"));
        assert!(!is_openai_compat_path("/config"));
        assert!(!is_openai_compat_path("/reply"));
    }
}
