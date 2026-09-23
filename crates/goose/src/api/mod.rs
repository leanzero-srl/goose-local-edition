//! The HTTP API surfaces both goose servers mount — the OpenAI-compatible shim and the
//! CogniRunner task routes — written ONCE over an [`AgentManagerSource`] so `goose serve` (the
//! engine the desktop runs) and goosed serve the same handlers.

pub mod cognirunner;
pub mod mlx_serving;
pub mod openai_compat;

use crate::acp::server_factory::AcpServer;
use crate::execution::manager::AgentManager;
use std::sync::Arc;

/// Where a route finds the agent manager it runs sessions on. goosed owns one up front; `goose
/// serve` lets its [`AcpServer`] build one on first use, so a router can be constructed
/// synchronously at boot.
#[derive(Clone)]
pub enum AgentManagerSource {
    Ready(Arc<AgentManager>),
    FromAcpServer(Arc<AcpServer>),
}

impl AgentManagerSource {
    pub async fn manager(&self) -> anyhow::Result<Arc<AgentManager>> {
        match self {
            Self::Ready(manager) => Ok(Arc::clone(manager)),
            Self::FromAcpServer(server) => server.agent_manager().await,
        }
    }
}
