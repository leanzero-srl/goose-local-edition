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
