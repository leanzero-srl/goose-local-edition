//! `GET /mlx-engine/serving` — the work this goose process has in flight (see
//! [`crate::providers::mlx_serving`]), each entry joined to its session's name and type so the
//! desktop can say "Chat · <name>" or "External client via /v1" without inventing either. Read by
//! the desktop's MAIN process (the menu-bar tray and the MLX state tile) under the server secret.

use super::AgentManagerSource;
use crate::providers::mlx_serving::{self, ServingEntry};
use crate::session::session_manager::SessionType;
use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServingRow {
    #[serde(flatten)]
    pub entry: ServingEntry,
    pub session_name: Option<String>,
    pub session_type: Option<SessionType>,
    /// Why the session could not be read (it was deleted mid-turn, the store failed) — the row
    /// stays, named by its id, rather than disappearing or borrowing a name.
    pub session_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServingResponse {
    pub serving: Vec<ServingRow>,
}

async fn serving(
    State(source): State<AgentManagerSource>,
) -> Result<Json<ServingResponse>, (StatusCode, String)> {
    let entries = mlx_serving::snapshot();
    if entries.is_empty() {
        return Ok(Json(ServingResponse {
            serving: Vec::new(),
        }));
    }
    let agents = source.manager().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("No agent manager: {e}"),
        )
    })?;
    let sessions = agents.session_manager();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let (session_name, session_type, session_error) = match &entry.session_id {
            None => (None, None, None),
            Some(id) => match sessions.get_session(id, false).await {
                Ok(session) => (Some(session.name), Some(session.session_type), None),
                Err(e) => (None, None, Some(e.to_string())),
            },
        };
        rows.push(ServingRow {
            entry,
            session_name,
            session_type,
            session_error,
        });
    }
    Ok(Json(ServingResponse { serving: rows }))
}

pub fn routes(source: AgentManagerSource) -> Router {
    Router::new()
        .route("/mlx-engine/serving", get(serving))
        .with_state(source)
}
