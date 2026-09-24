# KV-cache compression on Qwen3.8-27B-Atlassian-Q8 — 2026-09-24

Engine: Rapid-MLX fork `lz/kv-quant-hybrid` (999d43ea6, tagged v0.14.3-lz.3), the owner's production flags
(prefix cache, max 8 concurrent, qwen3_coder_xml, deepseek_r1, text lane; MTP K=3 on the memory/decode runs),
one fresh engine per configuration on :8093 with `RAPID_MLX_PREFIX_CACHE_AUTOLOAD=0`. M-series Mac, 128 GB.
Harness: `../../kv_quant.py`, joins: `../../kv_quant_compare.py`. Per-prompt detail: `summary-same-prompt.md`
(the clean comparison) and `summary-cross-nonce.md` (memory, prefill, and the cross-nonce noise floor).

Before the fork change the pinned engine (v0.14.3-lz.2) refused the flag on this model:
`KVCacheQuantizationUnsupportedError: --kv-cache-dtype int8 cannot be honored … the loaded model is incompatible: ArraysCache`.

## The table

| | bf16 (off) | int8 | int4 |
|---|---|---|---|
| KV bytes per token (16 of 64 layers carry KV; packed layout arithmetic, matches the engine's admission price) | 65,536 | 34,816 (0.53×) | 18,432 (0.28×) |
| same memory holds | 1× context | 1.9× | 3.6× |
| Metal peak, 8k prompt (GB) | 36.73 | 36.44 | 36.29 |
| Metal peak, 32k prompt (GB) | 46.20 | 41.34 (−4.9) | 40.47 (−5.7) |
| Metal peak, 128k prompt (GB) | 67.25 | 57.74 (−9.5) | 54.14 (−13.1) |
| prefix-cache entries retained after the 128k request (same budget) | 1 (8.7 GB) | 3 (11.7 GB) | 8 (10.6 GB) |
| prefill tok/s 8k / 32k / 128k (cold, MTP on) | 166 / 165 / 122 | 186 / 172 / 114 | 190 / 179 / 127 |
| decode tok/s at 32,722 tokens, prefix cached, 300-token answers, median of 3 (MTP on) | 17.3 | 12.0 (69%) | 15.4 (89%) |
| SAME prompts, greedy: tokens before first divergence from bf16 (13 prompts) | 100% (bf16 rerun: 13/13 identical) | 62.4% | 22.1% |
| SAME prompts: answers identical to bf16 | 13/13 | 8/13 | 4/13 |
| bf16's own top-1 − top-2 margin where they diverge (nats, median / max) | — | 0.0 / 0.25 | 0.125 / 0.625 |
| fact buried 31k tokens deep in a 40k prompt | found | found | found |
| two facts buried 35k and 110k deep in a 128k prompt | both found | both found | both found |
| goose agent conversation (79 tools, 42k–49k prompts): tool chosen on 4 turns | reference | same tool 4/4, args differ on turn 4 (`sed` vs `rg`) | same tool 4/4, args differ on turn 4 |

## Where quality breaks

- **bf16 is deterministic on identical prompts** (fresh engine, no MTP): 13/13 answers byte-identical. So every
  int8/int4 divergence on the same prompts is the cache's doing, not run-to-run noise.
- **int8 flips only exact ties.** All five int8 divergences happen where bf16's own top-1 and top-2 logprobs are
  equal at bf16 resolution (median margin 0.0 nats, max 0.25); int8 then picks bf16's runner-up and continues a
  different, equally-formed answer. It never lost the buried facts and never changed which tool the agent called.
- **int4 perturbs real choices.** Divergences at margins up to 0.625 nats, 9 of 13 answers different, the
  earliest at token 1. Facts still found; tool names unchanged.
- **A different first-line nonce alone** (bf16 vs bf16, cross-nonce) agrees only 45% to first divergence, 5/13
  identical — this 27B sits on many near-ties, so any perturbation (a cache, a changed timestamp line) moves greedy
  text. One such flip went wrong: on the cross-nonce int8 run the arithmetic prompt opened on a tie token ("5"
  vs "1", margin 0.000) and ended at "54 jobs" (right: 30); on the same-prompt runs bf16 and int8 both said "360".
- **Decode cost**: int8 decodes 31% slower at 32k context (dequantize-on-read of the full-attention KV), int4 11%
  slower. Prefill is unchanged within noise.

## Caveats (read before quoting)

- Other agents shared this Mac during the runs (their node/cargo processes reached 100% CPU at times); the
  memory-phase decode figures over ~20-token answers are noise and are not reported — the decode row above comes
  from the dedicated 300-token phase.
- Logprobs are only requested on the quality runs, which run WITHOUT MTP: on an MTP-mounted engine a
  `logprobs:true` request aborts the whole engine process (`There is no Stream(gpu, 1) in current thread` in
  `_extract_token_logprob`) — a pre-existing engine defect, not caused by this change.
- Flash (qwen4_exp, 98 GB) was not measured: it does not fit single-node beside anything, the engine refuses a
  quantized cache on its layout (Qwen4ExpStateCache + CacheList/QSA), and the distributed runners have no
  quantized KV at all.
