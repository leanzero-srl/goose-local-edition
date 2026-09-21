use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

pub fn token_matches(candidate: Option<&str>, expected: &str) -> bool {
    candidate
        .map(|key| bool::from(key.as_bytes().ct_eq(expected.as_bytes())))
        .unwrap_or(false)
}

/// The routes driven by server-to-server API clients (OpenAI SDKs, CogniRunner) — the only
/// paths on which `Authorization: Bearer <secret>` is accepted in place of `X-Secret-Key`.
/// Both `goose serve` and goosed consult this ONE predicate.
pub fn accepts_bearer(path: &str) -> bool {
    path == "/v1/models" || path == "/v1/chat/completions" || path.starts_with("/cognirunner/")
}

pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then(|| token.trim())
                .filter(|token| !token.is_empty())
        })
}

pub async fn check_acp_token(
    State(state): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header_token = request
        .headers()
        .get("X-Secret-Key")
        .and_then(|value| value.to_str().ok());

    let query_token = request.uri().query().and_then(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.into_owned())
    });

    if token_matches(header_token, &state) || token_matches(query_token.as_deref(), &state) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Auth for the API routes (`/v1/*`, `/cognirunner/*`) under `goose serve`: the SAME secret as
/// the ACP endpoint, as `X-Secret-Key` or — on the paths [`accepts_bearer`] names — as a Bearer
/// token. No query-string token: an API client never puts its secret in a URL.
pub async fn check_api_token(
    State(state): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header_token = request
        .headers()
        .get("X-Secret-Key")
        .and_then(|value| value.to_str().ok());
    let bearer = if accepts_bearer(request.uri().path()) {
        bearer_token(request.headers())
    } else {
        None
    };
    if token_matches(header_token, &state) || token_matches(bearer, &state) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_is_accepted_only_on_the_api_paths() {
        assert!(accepts_bearer("/v1/models"));
        assert!(accepts_bearer("/v1/chat/completions"));
        assert!(accepts_bearer("/cognirunner/tasks"));
        assert!(accepts_bearer("/cognirunner/tasks/abc/messages"));
        assert!(!accepts_bearer("/v1/swarm/stream"));
        assert!(!accepts_bearer("/acp"));
        assert!(!accepts_bearer("/config"));
    }

    #[test]
    fn bearer_token_parses_the_scheme_case_insensitively_and_refuses_empty() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer  xyz "),
        );
        assert_eq!(bearer_token(&headers), Some("xyz"));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer_token(&headers), None);
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(bearer_token(&headers), None);
    }
}
