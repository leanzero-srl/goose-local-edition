# Placement planner — design (2026-09-24, for the owner's review; nothing built yet)

Owner: "how do we better expose this in the UI for the best possible UX? people won't know and won't have time to
verify every nook and cranny. can we create a deterministic way … based on resourcing and the model to choose the
best option?" — then: "first please design it and really research every piece".

## 1. What the research established (each rule below rests on one of these)

Measured on our two Macs (M3 Ultra 96 GB 819 GB/s, M4 Max 128 GB 546 GB/s; STEP1/STEP1b REPORTs, FLASH-* runs):

| Model | Placement | Prefill tok/s | Decode tok/s |
|---|---|---|---|
| Qwen3.8-27B Q8 | M3 Ultra alone | 335 | **22.2** |
| ″ | M4 Max alone (quiet) | 224 | 15.1 |
| ″ | Tensor split, JACCL | **406** | 14.6 |
| ″ | Tensor split, ring | 373 | 10.4 |
| Qwen3-32B 4-bit | M3 Ultra alone / M4 Max / tensor JACCL / ring | 269 / 179 / 342 / 281 | **31.9** / 21.4 / 15.7 / 9.6 |
| Flash (qwen4_exp, 98 GB) | Pipeline, JACCL / ring | 683 / 716 | 21–24 / 22–23 |
| ″ | Pipeline, batch 2 | — | ~47 aggregate |

Link: JACCL 21 µs / 9.3 GB/s, ring 115–164 µs / ~4.5 GB/s (ours + avlp12).

External (sources in the research notes of this session; the key ones):
- Decode is memory-bandwidth-bound, prefill compute-bound (Apple MLX/M5 paper). Per-chip fit of
  `t_token = c + bytes/BW_eff` from llama.cpp #4167: M3 Ultra c≈5.2 ms, BW_eff≈675 GB/s; M4 Max c≈4.3 ms, ≈493 GB/s.
- MoE reaches far less of the bandwidth than dense (Qwen3-235B-A22B ≈36%, Kimi K2.5 ≈24% vs dense ≈70–80%), so
  efficiency is per model CLASS, not one number. Long context: the KV read dominates (Qwen 32B 31 → 8.5 tok/s 1K → 128K).
- Tensor parallel = 2 all_sums per layer per token, even divisor required, paced by the SLOWER rank; pipeline = 1 hop
  per stage, uneven split OK, no single-stream gain, near-linear gain with concurrent requests (exo: 49 → 96 → 109).
- The naive tensor formula OVER-predicts: for our 27B it gives ~28 tok/s, we measured 14.6. Independent runs also failed
  to reproduce exo's TP speedup claims. → analytical numbers are only a prior; measurements must win.
- Kernel panics come from WIRED memory, which jetsam can't reclaim; unbounded KV growth and abnormal collective
  teardown are the documented triggers (mlx-lm #883, mlx #3186, avlp12). Apple publishes no headroom figure.
- UX complaints elsewhere: hidden/over-conservative guardrails (LM Studio), estimates that ignore KV growth,
  "slow on your device" surprises. Good pattern: one recommendation with a predicted range + a one-line reason, the
  alternatives with why each lost, "estimated" vs "measured", an override that is never hidden.

What goose already has (codebase pass): Metal ceiling + wired limit per node (preflight), available memory (fixed
measure), TB link speed + RDMA, models per node (discover / Link ModelsList), per-model KV bytes/token (kv_cache.rs),
tensor facts + per-layer bytes (plan.rs / fork planner), `mountCost` fit readout. MISSING: chip identity + bandwidth,
MoE active bytes, a place to store measured speeds, and — important — routing chat to an engine on ANOTHER Mac (the
single engine binds 127.0.0.1 only; Link carries model management but no chat proxy).

## 2. The rule (deterministic)

Inputs: the model's facts (bytes, architecture, dense/MoE active bytes per token, KV bytes per token, max context),
each Mac's facts (chip, bandwidth, GPU ceiling, available after compaction, link), the goal (default: fastest single
conversation), the context wanted (default: the largest that fits, capped at the model's max).

1. Enumerate every placement the architecture supports AND goose can run:
   single on each Mac · pipeline over the Macs · tensor over the Macs (JACCL only, even divisor, half must fit the
   smaller Mac).
2. Drop what doesn't fit: weights + KV(for the context) + margin ≤ budget, budget = min(GPU ceiling, available after
   compaction). A placement that fits only at a smaller context is kept with that context shown.
3. Predict each survivor: MEASURED number for (model, placement) if we have one; otherwise the calibrated formula
   (per-chip c + BW_eff fitted to our own runs, class factor dense/MoE, KV read at the context, + hop / all-sum costs
   from our measured link latency), labelled "estimated".
4. Pick by the goal:
   - Chat (default): highest decode tok/s; tie → fewer Macs. In practice: fits one Mac → the fastest Mac alone;
     doesn't fit → pipeline; tensor only where measured faster (today: never for decode on this pair).
   - Long documents: highest prefill tok/s — the one case tensor can win (27B: 406 vs 335).
   - Many parallel requests (swarm): total throughput — one copy per Mac when it fits each (2× the 27B), else
     pipeline with batching.
5. Nothing fits → say exactly what's short per Mac, offer Make room, name the top memory users, or "needs a third Mac".

## 3. The UX

- Model picker: a badge per model — "Fits this Mac" / "Fits the Studio" / "Needs both Macs" / "Too big (short 14 GB)".
- Choosing a model shows ONE card: "Best: Work's Mac Studio alone · ~22 tok/s · 262k context · measured" [Use this]
  and, folded, the alternatives with the reason each lost ("Both Macs, tensor · 14.6 tok/s · only faster reading long
  prompts"). A goal switch: Chat · Long documents · Many requests.
- [Use this] does everything: Make room → unload what's in the way → start (single, remote single via Link, or split)
  → route chat to it. The tile/tray then show predicted vs actual; every real run feeds the measurement store.
- Override stays one click away (Advanced: any placement, context, backend).

## 4. What has to be built (phases, each shippable)

1. Facts: chip identity → bandwidth (sysctl hw.model / machdep.cpu.brand_string + GPU core count; Apple spec table),
   MoE active bytes from config.json (num_experts, num_experts_per_tok, moe sizes), a capability table per
   model_type (what goose can run: rapid-mlx single; mlx_lm tensor for qwen3_5; fork pipeline for qwen4_exp).
2. Planner + prediction + measurement store (every run appends model, placement, context, prefill/decode) + the card
   and badges, applying the placements that exist today (local single, split).
3. Remote single: run the single engine on another Mac and route chat to it through an authenticated Link proxy
   (streaming passthrough). Unlocks "Work's Mac Studio alone" — the fastest option for the 27B.
4. Throughput mode: one copy per Mac, the swarm router spreading requests.
5. More runners (mlx_lm can tensor-split ~15 more architectures and pipeline ~5) as models need them.

## 5. Confidence and what must be measured

- High: fit/refusal logic, "fits one Mac → one Mac" for chat, pipeline for too-big models — all measured here.
- Lower: formula estimates for unmeasured model/placement pairs (MoE factors, tensor overhead on a MIXED pair — the
  naive formula was 2× off). Shown as ranges labelled "estimated" until a run measures them; a 30-second calibration
  run on first use of a model/placement replaces the estimate.
- Open: the all-sum latency on our mixed pair under load (MLX all_sum bench), M5 Ultra numbers when it arrives.
