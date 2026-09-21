//! goosed's mount of the CogniRunner task routes. The implementation is
//! `goose::api::cognirunner` — the same handlers `goose serve` mounts — over this process's agent
//! manager.

use crate::state::AppState;
use axum::Router;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    goose::api::cognirunner::routes(goose::api::AgentManagerSource::Ready(
        state.agent_manager.clone(),
    ))
}
