# local-swarm-agent (working name — pending final branding)

A CLI coding agent for **local AI on Apple Silicon**, built as a thin additive fork of
[Goose](https://github.com/aaif-goose/goose) with **LM Studio** as the inference engine.

Three pillars:
1. **Always-plan + multi-device swarm** — a planner decomposes work into independent subtasks
   and dispatches them, in parallel, across multiple local machines via **LM Studio LM Link**
   (one OpenAI endpoint that routes by model-id to whichever device holds the model).
2. **Local-model support** — first-class targeting of **Qwen3.6-27B** (dense, the smart
   planner/verifier) and **Qwen3.6-35B-A3B** (MoE, the fast worker).
3. **Quality-first context optimization** — keep only meaningful context for hybrid MLX models,
   compacting only when it is safe (in progress).

## Status (2026-06-25)
- Spike GO/NO-GO **PASSED**: tool-calling is safe across cache hits on both Qwen3.6 models over LM Link.
- Goose CLI built from source and validated against the live fleet.
- **Multi-device swarm proven end-to-end** as pure recipe config (no core changes):
  planner on 27B@workhorse → parallel workers on 35B@mac.lan → integrate → test PASS.
- See `docs/EXPERIMENTS.md` for the full, reproducible log and `docs/ARCHITECTURE.md` for the design.

## Fleet (all LM Link-linked)
| Node | Chip / RAM | Role | Model held |
|---|---|---|---|
| MacBook `Mihai-Macbook-2` | M4 Max / 128GB | control (runs the CLI + LM Link front `:1234`) | (various) |
| `WorksMacStudio.lan` (workhorse) | M3 Ultra | planner / verifier | `qwen/qwen3.6-27b` |
| `Mac.lan` (192.168.8.222) | M3 Max / 64GB | fast worker | `qwen/qwen3.6-35b-a3b` |

## Run the swarm PoC
```bash
export PATH="$HOME/.local/bin:$PATH"   # or use the from-source build at ~/Projects/goose/target/release/goose
LMSTUDIO_HOST=http://localhost:1234 LMSTUDIO_API_KEY=lm-studio \
  goose run --recipe recipes/swarm.yaml -n swarm-poc --max-turns 50
```

## Layout
- `recipes/` — swarm/planner + worker sub-recipes (the swarm, as config).
- `skills/` — Agent Skills (LM Link setup, etc.). SKILL.md format, Claude-Code-compatible.
- `config/` — example Goose config for the lmstudio provider.
- `docs/` — EXPERIMENTS.md (chronological proof log), ARCHITECTURE.md (design + verified mechanism), DECISIONS.md.

> Companion to the upstream clone at `~/Projects/goose`. These artifacts are additive and fold
> into the fork later; kept separate for now to keep upstream pristine and our work isolated.
