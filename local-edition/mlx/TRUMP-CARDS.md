# TRUMP CARDS — the idea bank (house doctrine, Mihai 2026-08-30)

Every solution we evaluated but did not pick, recorded WITH the one idea it holds. When hunting
improvements: fan ONE agent per card (minimal brief: the repo, the idea, our exact pain point —
nothing else) to mine its code/issues/commits for transplantable techniques; reduce in code;
adversarially verify; survivors land in QUEUED-FIXES.md as IMPLEMENT items.

| Card | What it is | The ONE idea worth stealing | Revisit when | Link |
|---|---|---|---|---|
| MTPLX | MLX server specialized in native MTP speculative decoding | 1.6–2.24x decode from the model's OWN MTP heads — no draft model, exact rejection sampling preserves the output distribution | MTP-preserving quants of our production models exist AND decode speed is the bottleneck | github.com/youssofal/MTPLX |
| mlx-serve | ~7 MB single Zig binary, no Python, OpenAI+Anthropic+Ollama APIs | Single-binary packaging — ships inside a desktop app with zero runtime deps | We ship goose desktop to end users and the uvx/Python sidecar hurts install UX | github.com/ddalcu/mlx-serve |
| mlx-lm upstream | Apple's reference LM library both finalists sit on | It IS the update stream: model-family support, samplers (presence/frequency penalty live here), server fixes | Every fork-update cycle — merge upstream first, then our engine fork | github.com/ml-explore/mlx-lm |
| LM Studio mlx-engine | LM Studio's open MLX engine (MIT) | Hybrid-aware KV save/restore at 256-token boundaries — the technique that dodged the DeltaNet prefix-cache tool-calling bug; also Outlines-based structured generation | Our chosen engine shows the hybrid prefix-cache footgun, or we need constrained JSON output | github.com/lmstudio-ai/mlx-engine |
| oMLX (bake-off loser, 2026-08-31) | 21k★ MLX server; lost on sustained-N=8 decay (TTFT 7.7→13.1→11.7 s across a session, non-recovering) | Memory-guard tiers with a live process enforcer (ceiling = RAM−8GB, 1s interval) + per-model TTL/pinning/LRU; the admin dashboard's one-click agent integrations; the cluster worker shim (multi-host fronting — relevant to our fleet phase); SSD KV cold tier IF we ever serve pure-attention models | Rapid-MLX shows memory pressure or we reach the fleet-routing phase (its cluster shim is prior art) | github.com/jundot/omlx |

Cards are never deleted — a spent card gets a dated note on what was drawn from it.
