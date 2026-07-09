# Self-built tools — spike result (2026-07-09)

Feature: AgentMode axis + a `create_tool` platform tool (committed 1ec0f7ee2). In Agent mode the
model may author its own reusable tool — a Python MCP server — which is registered live and
optionally persisted. This note records the end-to-end spike proving it works on the WEAK local 27b.

## Spike 1 — author → register → call, SAME TURN — PROVEN ✅

Command: `GOOSE_AGENT_MODE=agent GOOSE_MODE=auto GOOSE_MODEL=mihai-qwopus3.6-27b-coder-mlx (warm)
./target/release/goose run --text "...create a pong tool then call it..."`

What happened (verbatim from the run):
1. The model called `create_tool` with name=pong and `code` = a VALID Python MCP server:
   ```python
   from mcp.server.fastmcp import FastMCP
   mcp = FastMCP("pong")
   @mcp.tool()
   def pong() -> str:
       """Returns the exact string 'pong'."""
       return "pong"
   if __name__ == "__main__":
       mcp.run()
   ```
   → It used the `mcp.server.fastmcp` import (mandated by the system.md fix), NOT the standalone
     `fastmcp` package that would have failed to launch.
2. The InlinePython server launched via `uvx --with mcp python` (the tool call succeeded, so the
   subprocess started — the pre-warmed uvx cache made this instant).
3. The just-authored `pong` tool was refreshed into the live tool set and CALLED in the SAME turn
   (`▸ pong`) — this is exactly what the same-turn-refresh fix (agent.rs) enables; without it the
   tool would only have been callable on the next user turn.
4. It returned `pong`. The model reported: "The pong tool returned exactly: `pong`".

CONCLUSION: the whole machinery works — a weak local model can author a tool it doesn't have,
have it registered live, and use it within one reasoning turn. This is the flagship "goose builds
its own tool calls" capability, proven.

Prereqs that mattered: uvx present (0.11.24) + the `mcp` wheel cache pre-warmed (so the first
in-agent uvx spawn didn't cold-resolve and hang). The two adversarial-review fixes were both
load-bearing: (a) same-turn refresh, (b) the correct FastMCP import.

## Spike 2 — persistence (persist=true → a FRESH session sees the tool) — PROVEN ✅

1. `create_tool` with `persist=true` (tool `pingback` returning `pingback-ok`) wrote a real
   extension into `~/.config/goose/config.yaml`:
   ```yaml
   pingback:
     type: inline_python
     name: pingback
     description: Returns the exact string 'pingback-ok'. Takes no arguments.
     code: |
       from mcp.server.fastmcp import FastMCP
       mcp = FastMCP("pingback")
       @mcp.tool()
       def pingback() -> str:
           return "pingback-ok"
       ...
   ```
2. A brand-new `goose run` session (NOT Agent mode, and NOT creating anything) was asked to just
   call `pingback`. It auto-loaded the persisted extension, called the tool, and returned:
   "The pingback tool returned exactly: `pingback-ok`".

CONCLUSION: persistence works — a tool the model authored in one session survives to future
sessions and is callable by any session without Agent mode (only `create_tool` itself is Agent-mode
gated; a persisted tool is a normal extension thereafter).

## Verdict
Goal 3 "goose builds AND saves its own tool calls" is fully proven end-to-end on the weak local
qwopus-27b: author → register-live → call-same-turn → persist → reuse-in-a-fresh-session. Machinery
(AgentMode gating + create_tool + InlinePython/uvx + same-turn refresh + config persistence) all
verified. The spike artifact (`pingback`) was removed from config after the proof.

