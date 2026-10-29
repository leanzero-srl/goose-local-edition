# Prompt lookup (n-gram copy drafting): the per-model lever and what it measured

Dated record, 2026-10-26. Branch `ngram-decoding` (goose-wt-ngram). Engine read: Rapid-MLX v0.14.3-lz.14.

## What the lever is

Prompt lookup copies a continuation that already exists in the context (the file being edited, a quoted
document) as a long speculative draft, verified like any draft, so the output stays the model's own.
goose exposes it per model in Settings > MLX engine > the model's profile ("Prompt lookup (n-gram)"):
Auto / On / Off plus three numbers. Auto sends nothing and the engine keeps its family default.

It drives one of two engine lanes, decided per model at mount (`prompt_lookup::lane()` in
crates/goose-sidecar/src/prompt_lookup.rs; the UI mirrors it in `promptLookupLane()`):

| lane | when | how goose drives it | numbers |
|---|---|---|---|
| inside MTP | the model is served with its MTP head (auto or `speculative: mtp`, `mtp.safetensors` present) | env `RAPID_MLX_MTP_PROMPT_LOOKUP=1/0`, `_MIN_NGRAM` (>=2), `_MAX_NGRAM` (>=min), `_MAX_TOKENS` | shortest n-gram, longest n-gram, longest draft (31/63/95 fill whole 32-row verify tiles) |
| suffix decoding | served WITHOUT MTP and config.json has no recurrent-state layers | `--suffix-decoding --suffix-max-draft <max_draft> --suffix-max-suffix-len <max_ngram>` (On only) | longest n-gram, longest draft; greedy only, batch 1 |
| unavailable | served without MTP and the model is hybrid (GatedDeltaNet, Mamba, ...) or config.json unreadable | nothing sent; the UI locks On/Off with the reason, Auto stays reachable | none |

Engine facts behind it: the env is read per request (`_effective_prompt_lookup_policy`); the qwen3_5 family
policy is enabled by default (min 8, max 64, max tokens 31) and also under sampling. The suffix lane is
refused on hybrids by the engine itself (`[SuffixDecoding] disabled: model is hybrid`, no crash).
Distributed and remote-single engines do NOT get the env. goose's hybrid check is a heuristic list; the
engine's `supports_spec_decode` is the authority.

Wire: `MlxModelProfileDto.promptLookup {mode, minNgram, maxNgram, maxDraft}` (absent = auto);
`MlxLocalModelDto.hasMtpHead / hybrid / hybridError`. A change restarts the engine (argv AND env are
compared for mount reuse and `restart_required`).

## Measured 2026-10-26 (data: /Volumes/AI-workhorse/runs/2026-10-26-pld-qwen38, NOTES.md)

Qwen3.8-27B-Atlassian Q8 (qwen3_5 hybrid, mtp.safetensors), engine lz.14, greedy, N=3, decode tok/s median.

| prompt | MTP off | MTP, PLD off | MTP + PLD | PLD acceptance |
|---|---|---|---|---|
| code edit | 21.4 | 42.8 (MTP acc 85.7%) | **134.8** (3.15x over MTP) | 1926/1940 = 99.3% |
| doc quote / summarise | 22.1 | 33.3 | 32.4 | 64/123 = 52%, no gain |
| free chat | 22.0 | 37.3 | 37.1 | never proposes, no gain |

- PLD=1 explicit equals the default (the family default is ON), so Auto already gets the code-edit win on Qwen3.5/3.6/3.8.
- Code edit: all 12 outputs byte-identical across every config.
- MTP greedy is nondeterministic run to run: doc_quote/free_chat outputs differ even repeat to repeat INSIDE
  MTP with PLD off, and in a free_chat run where PLD drafted 0 tokens. So it is MTP's (adaptive K), not
  PLD's; the root cause is unproven.
- Suffix decoding on Mellum2.1-12B (lz/mellum-lz12): SLOWER (90.4 vs 99.5, 99.7 vs 109.4 tok/s), 6660
  `non_trimmable_cache` skips, 8/96 drafts accepted, and free_chat output diverged, so it is not lossless
  there. That is why Auto leaves the suffix lane off and the UI says to compare before turning it on.

## Open engine defects seen during the measurement (file for the engine owner)

1. Streaming drops the space before ` [` / ` &` tokens ("clamped to [0.90" arrives without its space),
   4x per code-edit answer, in EVERY config (MTP off too, so not PLD). Non-stream output was correct on the
   Mellum build.
2. Qwen3.8-Flash-Next (`model_type: qwen4_exp`) gets no MTP from goose, so no prompt lookup either: its
   MTP weights live inside the shards (75 `mtp.*` tensors in model.safetensors.index.json, no
   `mtp.safetensors`), and goose only serves MTP when `mtp.safetensors` exists. Even if it did, goose sends
   `num_speculative_tokens: 3` (MTP_SPECULATIVE_TOKENS) and the engine refuses that for qwen4_exp
   (`_resolve_mtp_depth_for_model`: "supports num_speculative_tokens in [1, 2]", exit 2; default K=1).
   Enabling MTP there needs both: detect in-shard MTP heads and send K 1 or 2 for qwen4_exp. Being hybrid,
   the model shows the lever as unavailable today.

## Verification status

Rust (sidecar + ACP mapping + round-trip test), the desktop gate (lint:check incl. typecheck and i18n,
test:run) are green on the branch. Not verified: a live mount through the packaged app (CDP), i.e. that a
saved profile reaches the engine env/argv end to end in a running build.
