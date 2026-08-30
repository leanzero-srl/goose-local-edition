# QUEUED FIXES — triage ON ARRIVAL: IMPLEMENT / DROP / SCHEDULED

Rule (implement-don't-backlog): a finding is triaged the moment it lands, batched by file, and the
ratio (implemented : dropped : scheduled) is what gets reported — never a raw count of open items.
SCHEDULED requires a named condition, not a vague "later".

| # | Item | Verdict | Condition / where |
|---|---|---|---|
| 1 | Rapid-MLX presence_penalty pass-through to SamplingParams (upstream issue #355) | SCHEDULED | Only if Rapid-MLX wins the bake-off → first fork patch, verified by repetitive-prompt A/B |
| 2 | Hybrid prefix-cache tool-calling re-verification on CURRENT engine versions (omlx #825 / mlx-lm #980 class) | IMPLEMENT | Bake-off dimension D4 — in the bench harness, not a separate task |
| 3 | Engine `/admin/status` endpoint (residency, context length, size) for swarm parity | SCHEDULED | After bake-off verdict → fork patch on the winner |
| 4 | Load-with-TTL on the winner (oMLX has natively; Rapid-MLX needs it) | SCHEDULED | After bake-off verdict, only if Rapid-MLX wins |
| 5 | Serving from an arbitrary `models_dir` (incl. `publisher/model` two-level layout) | IMPLEMENT | Verified per engine during bake-off setup; fork patch if either can't |
