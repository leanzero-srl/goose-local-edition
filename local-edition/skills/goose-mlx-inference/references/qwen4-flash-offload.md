# Qwen3.8-Flash-Next memory offload (dated facts)

## 2026-10-26 — n-gram (PLE) table on disk SHIPPED; routed-expert offload STOPPED

Owner decision: PLE offload at ~72 GB is good enough. Expert offload is stopped: its code is in
the tag, default OFF, unfinished, and goose never sends `--qwen4-expert-*`. Do not resume it
without the owner's word.

Engine: Rapid-MLX tag `v0.14.3-lz.14-ple.1` (fork branch lz/qwen4-expert-offload off lz.14;
module rapid_mlx/models/qwen4_offload.py; flag `serve --qwen4-ple-offload` → env
RAPID_MLX_QWEN4_PLE_OFFLOAD=1, OFFLOAD_DIR = the model dir). What it does: the 128 shards of
`model.language_model.layers.1.ple.ple_embedding.ngram_embedding.shard_{0..127}`
(weight U32[2500012,20] + bf16 scales/biases [2500012,5], 32,000,153,600 B) are dropped at
sanitize and read with np.memmap from the checkpoint; ~16 rows/token gathered on 8 threads,
dequantized on GPU. Bit-exact vs the resident QuantizedEmbedding (unit test).

Measured, workmule M5 Max 128 GB, MTP k=2, --disable-prefix-cache (fork doc
docs/engineering/performance/2026-10-26-qwen4-ple-expert-offload.md):
| config | MLX active idle | decode tok/s code/prose/json/zh/long/copy | 7.8k prefill |
| resident | 104.25 GB (peak 108.1 at 8k) | 65.7/52.3/75.7/54.0/51.7/114 | 7.18 s |
| PLE on disk | 72.25 GB | 64.3/51.2/63.2/55.9/50.7/119 | 6.95 s |
Cold first gather of a long prompt 2.0 s (page cache), warm 0.23 s. MTP-off greedy shas identical
resident vs offload; MTP-on is not run-to-run deterministic on this model (pre-existing).

goose (branch ple-offload): ModelProfile.ple_offload Option<bool> (`crate::ple_offload`);
lever "Keep n-gram table on disk" offered only when model.safetensors.index.json names a
`ple_embedding.ngram_embedding` tensor. Auto = ON for qwen4_exp when weights-on-disk + KV at the
smallest useful context exceed the fit budget at 70% of RAM available
(TYPICAL_AVAILABLE_RATIO; static RAM + Metal ceiling only, so the argv never moves with live
memory). 128 GB M4 Max → on; 256 GB → off. Mount gate + planner charge size − 29.8 GiB and append
"fits with the n-gram table on disk (~X held, 29.8 GB read from the SSD as needed)".

Traps:
- Bench with --disable-prefix-cache: the persistent prefix cache makes a fresh server
  "prefill" 8k in 0.25 s.
- Never bench from an NFS/TB-mounted model dir: the gather reads the checkpoint per token.
  Copy to the Mac's internal disk.
- goose serves Flash-Next WITHOUT MTP: its MTP head is inside the shards (no mtp.safetensors),
  and the engine accepts num_speculative_tokens 1-2 for it while goose sends 3. Prompt lookup
  only runs inside MTP, so on goose it is inert for Flash-Next until in-shard MTP detection +
  a per-model depth exist (not done: has_mtp_sidecar feeds the UI, the prompt-lookup lane and
  the distributed paths).

Tag check (2026-10-26): fork suites at the tag vs lz.14, same venv and commands
(`pytest tests --ignore=tests/integrations --timeout 180`, 3 file shards per side): 27035 vs
27027 tests, failures+errors 93 vs 93, identical set (pre-existing: real-model gemma4 tokenizer,
git-dependent train-gates, etc.); +8 offload tests. `uvx --from "rapid-mlx[mtp] @ git+…@v0.14.3-lz.14-ple.1"
rapid-mlx serve --help` lists the flag. A full serial run takes >60 min: node-run's default
--timeout 3600 killed the first attempt at 90% (looked like a hang).

MacBook final test: NOT RUN (2026-10-26 22:4x). The laptop had 87-91 GiB free on its internal
disk; the model is 97.5 GiB, and the PLE gather must read a LOCAL copy (not the TB share). Needs
>= ~110 GiB free (model + 10 GiB headroom) freed by the owner; nothing of his was deleted.

goose build trap: a cargo run WITHOUT `--config 'profile.dev.package."*".strip=false'` (e.g. the
flag placed after `--`) writes a stripped libsqlx_macros that macOS 27 refuses ("mis-aligned
LINKEDIT"); later correct runs reuse it. Fix: `cargo clean -p sqlx-macros -p sqlx-macros-core`,
then rerun with the flag BEFORE `--`.
