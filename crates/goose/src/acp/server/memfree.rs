//! ACP surface for "Free up memory": who holds memory on this Mac (goose-sidecar `memfree`
//! census), what freeing it would buy against the one fit rule, and the reclaim itself.

use super::*;

pub(super) async fn core_memory_holders(
    _req: MlxEngineMemoryHoldersRequest,
    _relayed: bool,
) -> Result<MlxEngineMemoryHoldersResponse, agent_client_protocol::Error> {
    Ok(MlxEngineMemoryHoldersResponse::default())
}

pub(super) async fn core_memory_free(
    _req: MlxEngineMemoryFreeRequest,
    _relayed: bool,
) -> Result<MlxEngineMemoryFreeResponse, agent_client_protocol::Error> {
    Ok(MlxEngineMemoryFreeResponse::default())
}
