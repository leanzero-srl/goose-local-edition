# MLX engine campaign — LEDGER

Append-only, newest first, READ BACK at the start of every session. Every entry: what happened,
what it changed, what it means next time. Machine twin: `experiments.jsonl` (one row per real
experiment/event; a failed or inconclusive row carries a `void_reason` string, never a bare boolean).

`experiments.jsonl` row schema:
`{"ts", "experiment", "engine", "config": {...}, "result": {...}, "verdict": "pass|fail|inconclusive", "void_reason": "<only when not pass>", "commit"}`

---

## 2026-08-30 — Campaign opened; finalists chosen from verified research; hybrid footgun inherited

**Did:** Verified Mihai's engine research against the live web (all three engines real; citations
garbled; framing correct). Dropped MTPLX (MTP-quant lock-in vs our model reuse) and mlx-serve (Zig,
new-model lag) → TRUMP-CARDS.md. Finalists Rapid-MLX vs oMLX, to be decided by a swarm-shaped
bake-off (1/4/8 concurrent tool-calling streams, qwen3.5-9b-4bit downloaded fresh, memory-gated).
Two Explore agents mapped the repo (provider layer, swarm coupling surface, subprocess idioms); one
adversarial Plan agent validated the integration design and killed two wrong turns (per-device
endpoint assumption — doesn't exist; cloud-device path — strips local extras).

**Learned, and it changed the design:** `local-edition/docs/EXPERIMENTS.md` (2026-06-25 spike)
records that raw mlx-lm/omlx prefix-cache HITs silently broke tool-calling on Gated-DeltaNet hybrids
(omlx #825, mlx-lm #980) and that this was THE reason LM Studio won last time. Consequence: the
bake-off scores hybrid-prefix-cache-vs-tool-calling as a first-class dimension — it directly attacks
oMLX's headline strength and must be re-verified on current engine versions, not assumed fixed.

**Gates stood up the same day:** `gates.py` (memory-mount, port-safety, fleet-untouched) with
BLOCK+ALLOW self-tests in `gates_selftest.py`, on the acting path before any mount.
