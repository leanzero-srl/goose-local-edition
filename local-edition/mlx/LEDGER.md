# MLX engine campaign — LEDGER

Append-only, newest first, READ BACK at the start of every session. Every entry: what happened,
what it changed, what it means next time. Machine twin: `experiments.jsonl` (one row per real
experiment/event; a failed or inconclusive row carries a `void_reason` string, never a bare boolean).

`experiments.jsonl` row schema:
`{"ts", "experiment", "engine", "config": {...}, "result": {...}, "verdict": "pass|fail|inconclusive", "void_reason": "<only when not pass>", "commit"}`

---

## 2026-08-30/31 night — BAKE-OFF VERDICT: Rapid-MLX (evidence in experiments.jsonl, 8 scored runs)

**Protocol:** identical instrument (bench/swarm_bench.py) per engine, serial and hermetic, one
engine at a time on ports 8090/8091, memory-gated, fleet snapshot ALLOW before/after every run.
Model: freshly downloaded mlx-community/Qwen3.5-9B-MLX-4bit served from ~/.goose/models (both
engines took the arbitrary models_dir directly — no fork patch needed). rapid-mlx 0.13.1 (brew
core), omlx 0.6.4 (their tap).

**Decided it:** sustained-load stability at N=8, the swarm's exact profile. Successive N=8 runs,
same night, same host: rapid TTFT mean 8.0 → 7.6 → 7.3 s (improving); omlx 7.7 → 13.1 → 11.7 s
with p95 hitting 20 s and aggregate falling 49 → 30/33 tps — degradation with session age that did
NOT recover (probe cold-TTFT stayed 2.5 s vs 1.15 s fresh). The third omlx run was run specifically
to test whether run 2's decay was transient; it reproduced.

**Also for rapid:** working hybrid-aware prefix cache (hit TTFT −26% with fidelity held; omlx's
cache showed zero hit benefit on the DeltaNet hybrid — safe but inert); RSS 4.4 vs 6.2–6.5 GB;
per-model auto-config (hermes parser + qwen3 reasoning for dense qwen3.5, with documented dense-vs-
MoE hybrid distinctions); `--watchdog-ppid` (parent-death watchdog made for sidecar supervision);
presence/frequency penalty already upstream; goose precedent (author-tested via Ollama provider).

**Ties:** tool fidelity 1.0 with ZERO errors and ZERO malformed calls for BOTH engines across all
runs — the June hybrid footgun is dead in both current versions (rapid hit-fidelity 1.0, omlx
hit-fidelity 1.0). The 9B is emphatically NOT tool-inept; Mihai's fallback-model permission unused.

**Fairness notes (both directions):** omlx defaults thinking ON (every tool call pays a reasoning
preamble; 2x wall) — corrected via `chat_template_kwargs: {enable_thinking: false}` and RE-scored
before the verdict (its N=1 then beat rapid: TTFT 1.22 s); the degradation verdict rests on
no-think runs only. rapid ran with `--enable-prefix-cache` as omlx ran with its SSD cache dir —
headline features on for both. Instrument defects found and fixed mid-bake (decode_tps
divide-by-near-zero → req_tps; RSS sampler silent-zero → loud failure); the affected first-row
metrics are voided in-ledger, never edited.

**Next:** fork raullenchai/Rapid-MLX under leanzero; oMLX card filled in TRUMP-CARDS.md.

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
