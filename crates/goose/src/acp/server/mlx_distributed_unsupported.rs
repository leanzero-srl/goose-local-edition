//! The DISTRIBUTED MLX engine's ACP surface on a platform without it. `goose_sidecar::distributed`
//! compiles only on Unix — it drives ranks over `/bin/sh`, `ssh` and POSIX signals, and its
//! engine is MLX, which is macOS-only — so every distributed request here is refused with a named
//! reason rather than answered with an empty status that would read as "configured, stopped".

use super::*;

const UNSUPPORTED: &str = "distributedUnsupported: distributed MLX inference requires macOS";

fn unsupported<T>() -> Result<T, agent_client_protocol::Error> {
    Err(agent_client_protocol::Error::invalid_request().data(UNSUPPORTED))
}

/// No goosed on this platform can supervise a distributed engine, so none can own the machine.
pub(super) async fn refuse_single_mount_while_distributed(
) -> Result<(), agent_client_protocol::Error> {
    Ok(())
}

pub(super) async fn shutdown_distributed_engine() -> String {
    format!("nothing supervised ({UNSUPPORTED})")
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_distributed_status(
        &self,
        _req: MlxEngineDistributedStatusRequest,
    ) -> Result<MlxEngineDistributedStatusResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_preflight(
        &self,
        _req: MlxEngineDistributedPreflightRequest,
    ) -> Result<MlxEngineDistributedPreflightResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_start(
        &self,
        _req: MlxEngineDistributedStartRequest,
    ) -> Result<MlxEngineDistributedStartResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_stop(
        &self,
        _req: MlxEngineDistributedStopRequest,
    ) -> Result<MlxEngineDistributedStopResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_make_room(
        &self,
        _req: MlxEngineDistributedMakeRoomRequest,
    ) -> Result<MlxEngineDistributedMakeRoomResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_peer_candidates(
        &self,
        _req: MlxEngineDistributedPeerCandidatesRequest,
    ) -> Result<MlxEngineDistributedPeerCandidatesResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_discover(
        &self,
        _req: MlxEngineDistributedDiscoverRequest,
    ) -> Result<MlxEngineDistributedDiscoverResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_provision(
        &self,
        _req: MlxEngineDistributedProvisionRequest,
    ) -> Result<MlxEngineDistributedProvisionResponse, agent_client_protocol::Error> {
        unsupported()
    }

    pub(super) async fn on_mlx_engine_distributed_config_update(
        &self,
        _req: MlxEngineDistributedConfigUpdateRequest,
    ) -> Result<MlxEngineDistributedConfigResponse, agent_client_protocol::Error> {
        unsupported()
    }
}
