//! `goose swarm serve` (Lever C) — expose the local worker fleet to an interactive goose session as an MCP
//! extension over stdio. This SCAFFOLD ships one READ-ONLY tool, `swarm_status`. The async offload tools
//! (`swarm_dispatch`/`swarm_collect`/`swarm_fanout`) are added in a later, separately-reviewed increment —
//! they carry the real risk (detached spawn, the 300s MCP tool-call cap, keeping child stdout off the MCP
//! stdio stream, process-group teardown), so the seam is proven first with a side-effect-free tool.

use anyhow::Result;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{
        CallToolResult, Content, ErrorData, Implementation, InitializeResult, ServerCapabilities,
        ServerInfo,
    },
    tool, tool_handler, tool_router, ServerHandler,
};

pub struct SwarmServer {
    tool_router: ToolRouter<Self>,
}

impl Default for SwarmServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl SwarmServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "swarm_status",
        description = "Show the local worker fleet available to `goose swarm`: the models currently loaded and \
                       generating across the swarm devices (via `lms ps`). Read-only — no side effects. Call \
                       this to see what capacity is available before offloading work to the fleet."
    )]
    pub async fn swarm_status(&self) -> Result<CallToolResult, ErrorData> {
        // `lms` is a blocking subprocess — run it off the async executor. Never touches the MCP stdio stream.
        let out =
            tokio::task::spawn_blocking(|| std::process::Command::new("lms").arg("ps").output())
                .await;
        let text = match out {
            Ok(Ok(o)) => {
                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.trim().is_empty() {
                    s.push_str(&err);
                }
                if s.trim().is_empty() {
                    "No models are currently loaded on the fleet (`lms ps` returned nothing). Load a model \
                     in LM Studio to give the swarm capacity."
                        .to_string()
                } else {
                    s
                }
            }
            _ => "Could not query the fleet — the `lms` (LM Studio) CLI is not available on PATH."
                .to_string(),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SwarmServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "goose-swarm-serve",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "goose swarm serve — offload research and scratch tasks to the local worker fleet from an \
                 interactive session. swarm_status shows the loaded models and current generation. (Async \
                 dispatch/collect are a forthcoming increment.)"
                    .to_string(),
            )
    }
}

/// Serve the swarm MCP server over stdio; blocks until the client disconnects.
pub async fn run() -> Result<()> {
    goose_mcp::mcp_server_runner::serve(SwarmServer::new()).await
}
