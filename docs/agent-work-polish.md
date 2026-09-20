# Agent and MCP workspace polish

Use the existing Goose palette and controls. Agent setup fills the workspace, with readable form sections and an optional configuration preview. Results lead the agent view; expandable activity carries tool inputs, outputs and failures. Project chat uses the same disclosure treatment and must never imply success without a result.

MCP setup presents supported settings as named fields. LeanZero servers have one configuration surface, live capability discovery, and an explicit source collection workflow with provenance. Executable details stay available under a disclosure. Secrets stay out of evidence and error logs. Saved settings survive edits.

Acceptance: exercise setup validation, save/reopen, enable/disable, tool discovery, public-page collection and failure recovery. Run harmless demo agents and inspect their actual output. Existing private workflows require explicit adapters for their authentication, prepared queues and approval safeguards; do not migrate or execute them as a UI test.

## Verification (2026-09-20)

Desktop: 230 test files / 1,942 tests passed before the shared MCP capability disclosure; the subsequent full run passed 1,941 with one existing modal test timing out under concurrent local inference. That exact seven-test modal suite passed on isolated rerun. TypeScript, ESLint and all 15 locale checks pass. Final full rerun after stopping inference: all 230 files / 1,942 tests passed; the added document authentication fields also passed the focused MCP suite.

Real ACP acceptance uses `evals/agent-work/probe-mcp-setup.mjs`, isolated settings and real bundled servers. It discovered 11 web tools and 17 document tools, reloaded public settings while withholding the test secret, and collected the MDN JavaScript page with source/date provenance. Local-file URLs are rejected. Receipt: `/tmp/goose-polish-mcp-acceptance-final.log`.

Agent Work guard/configuration gates: 23 focused tests, 11 development gates, build and workspace Clippy passed. Broader CLI run: 916 passed, one ignored; two existing tests excluded after confirming failures (shell-profile stderr contamination and SIGTERM teardown expectation). These are not claimed as passing.

Live demo: `/Users/mihaiperdum/goose-agents/public-web-demo-text`, isolated configuration and local Rapid model. Parserless startup emitted XML as plain text. Explicit template-compatible parser plus the app's existing text-only mode produced real structured tools and a successful web-MCP page read. Full tick completion is still being verified. No client workflows were run.

Desktop visual acceptance remains blocked by the locked Mac. The development app is running; this polish has not been released or installed over the signed 3.0.0 application. Private workflow compatibility inspection found that their authentication, prepared queues and approval adapters must remain explicit; the generic agent UI does not magically import those workflows.
