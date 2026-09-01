# QUEUED FIXES — triage ON ARRIVAL: IMPLEMENT / DROP / SCHEDULED

Rule (implement-don't-backlog): a finding is triaged the moment it lands, batched by file, and the
ratio (implemented : dropped : scheduled) is what gets reported — never a raw count of open items.
SCHEDULED requires a named condition, not a vague "later".

| # | Item | Verdict | Condition / where |
|---|---|---|---|
| 1 | Rapid-MLX presence_penalty pass-through to SamplingParams (upstream issue #355) | DROP (2026-08-30) | Already shipped upstream — 0.13.1 has `--default-presence-penalty`/`--default-frequency-penalty`; per-request bite still gets the A/B check in the UI phase |
| 2 | Hybrid prefix-cache tool-calling re-verification on CURRENT engine versions (omlx #825 / mlx-lm #980 class) | IMPLEMENTED (2026-08-30) | Bench `prefix_probe`; rapid-mlx 0.13.1: no footgun (hit fidelity 1.0, hit TTFT 0.63s vs cold 0.85s) |
| 3 | Engine `/admin/status` endpoint (residency, context length, size) for swarm parity | SCHEDULED | After bake-off verdict → fork patch on the winner. Note: rapid-mlx `/v1/models` already returns context_window, parsers, hybrid flags — the gap may be residency-state only |
| 4 | Load-with-TTL on the winner | DROP as fork patch (2026-08-30) | Both have it natively: oMLX per-model TTL; rapid-mlx `--resident-model-idle-ttl` |
| 5 | Serving from an arbitrary `models_dir` (incl. `publisher/model` two-level layout) | IMPLEMENTED for rapid-mlx (2026-08-30) | Served `~/.goose/models/...` local path directly; oMLX `--model-dir` verified next |

Ratio so far: 3 implemented/resolved : 2 dropped-with-reason : 1 scheduled-with-condition (#3).

| 6 | Download-progress rows vanish from view when switching tabs mid-download (state lives in ModelsSection; backend keeps downloading) | SCHEDULED | Next UI round — lift tracker state to the view shell |
| 7 | Mount picker can preselect an incomplete model; Mount not gated on completeness (backend refuses loudly today) | IMPLEMENT | Next UI touch — disable Mount + incomplete tag in the picker |
| 8 | Report-a-Bug / Request-a-Feature / Diagnostics still link the PARENT repo's issues | IMPLEMENT | Pass C — point at leanzero-srl or remove |
| 9 | Justfile release-fork accepts a version below 2.0.0 (would silently regress the own-version line) | IMPLEMENT | Pass C round — recipe refuses <2.0.0 |
| 10 | Tray tooltip + main-process dialogs still say plain Goose (main.ts has no edition awareness) | IMPLEMENT | Pass C — brand from one constant |
| 11 | Two projects registries now exist (desktop userData projects.json vs engine ~/.local/share/goose/projects.json map) | SCHEDULED | Unify when the CLI grows project awareness — desktop is source of truth for UI |
| 12 | Fleet panel dead: LM Studio :1234 HTTP now requires an API token (invalid_api_key on /api/v0/models); useFleet reads offline; discovery works via lms ps IPC only | IMPLEMENT | Next round — LM Studio token setting in the app (secret) + Authorization header in useFleet/RecipeChatWizard |
| 13 | Nodes-tab weight maps to SwarmDevice.weight (engine: CONCURRENCY); if Mihai means routing share it is speed_weight — one-field flip | SCHEDULED | Awaiting Mihai's word on which semantic he wants |
| 14 | Recipe flows + goose:// deeplinks + PairRouteWrapper still create project-less sessions at getInitialWorkingDir | SCHEDULED | Awaiting Mihai's word — scope them to a project picker, or accept as power paths |
| 15 | Backend mints the literal 'New Chat' (acp/server/new_session.rs:42, DB default, goose-server agent.rs:182) | IMPLEMENT | Backend follow-up — mint 'New Session' at the source; renderer normalization already covers display |
| 16 | Auto-naming end-to-end (model turn → header updates) not live-verified — compute paths were off-limits mid-benchmark | SCHEDULED | First session after the fleet benchmark ends (or on the mounted 9B) |
| 17 | Swarm engine supports exactly 4 cloud families as nodes (CLOUD_DEFS: bedrock/zai/google/deepseek) — configuring any OTHER cloud provider cannot make it node-addable | SCHEDULED | Mihai's call: extend the engine's cloud-node support to arbitrary configured providers (swarm.rs CLOUD_DEFS + roster validation + dispatcher registry mapping) |
| 18 | Cross-node task delegation (node A → node B's goose over the mesh) — the acting path the idle GUARD needs; guard is dead enforcement without it | DECISION (Mihai) | Build the delegation subsystem now, or ship P1-P3 + guard-ready and defer |
| 19 | Full per-message session mirroring (P4 SessionDelta) needs a goose-server process-wide MessageEvent tap injected into SwarmStateSource | DECISION (Mihai) | This pass or follow-up — structural mirror (index+status) already works |
