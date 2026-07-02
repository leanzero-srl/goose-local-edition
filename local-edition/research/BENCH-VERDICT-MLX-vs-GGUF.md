# qwopus3.6-27b: MLX vs GGUF — final verdict (medium, 15 runs each, paired A/B)

Identical frozen 3-spec suite (crud/compute/txn ×5), same unchanged binary, same flags
(SMOKE+SPLIT+PREREVIEW+CONTRACTS), same 3-node fleet. GGUF = qwopus3.6-27b-coder-mtp
(jackrong original). MLX = qwopus3.6-27b-coder (mlx-community translation). Graded by RUNNING.

## Speed (wall-clock per run, seconds)
| spec | GGUF median | MLX median | winner | GGUF stdev | MLX stdev |
| --- | --- | --- | --- | --- | --- |
| overall | 1945.8 | 1923.9 | ~TIE (1.01×, MLX a hair faster) | 750.9 | 580.4 |
| compute | 1860.5 | 2192.6 | GGUF +15% (0.85×) | 464 | 599 |
| txn | 1945.8 | 1722.9 | MLX +13% (1.13×) | 658 | 527 |
| crud | 2239.2 | 2342.9 | ~tie (0.96×) | 831 | 442 |
- p90: MLX 3049.7 vs GGUF 3600.7. CAPS (hit the 60-min limit): MLX 0, GGUF 2 (both crud, post-build tail-churn — apps were correct).

## Quality
| | GGUF | MLX |
| --- | --- | --- |
| raw checks-pass | 80% (12/15) | 87% (13/15) |
| judged-by-running "app actually works" | ~87% (13/15) | ~87% (13/15) |
| swarm task success | 96% | 100% |
| per-spec checks | compute 100 / txn 60 / crud 80 | compute 80 / txn 80 / crud 100 |
- GGUF's lower RAW rate is inflated-downward by artifacts: 2 capped-but-correct crud runs + 1 false-partial. On judged-by-running they TIE (~13/15 both).
- Only genuine defects: both variants tripped on the multi-command txn spec (broken exec-GET / missing COUNT) — a WEAK-MODEL limit, present on BOTH, not a runtime difference.

## VERDICT
- **Remarkably close — no clear winner.** The hypothesis "MLX is faster with a quality hit" is NOT supported: MLX has NO quality hit (equal, slightly better raw) and is NOT slower (marginally faster median + MORE CONSISTENT: 0 caps, lower stdev/p90).
- GGUF is faster on compute-heavy work and has a higher peak, but SPIKIER (2 crud runs capped).
- MLX is the steadier runtime; GGUF the faster-on-its-day one. Practically: MLX is a safe default (no downgrade from the translation); GGUF suits compute-bound work.
- CAVEAT (important): GGUF's 2 caps are caused by a SWARM post-build tail-churn bug, not the model. Fixing it (see BENCH-FIXES-BACKLOG.md, MED-HIGH) would lift GGUF's median/p90/consistency and could tilt the comparison toward GGUF. This A/B is partly penalizing GGUF for a swarm defect that happened to hit its crud runs. A re-benchmark after the fix is warranted.

## Data
- runs/BENCHMARK.md, runs/benchmark-runs.csv (30 rows), runs/benchmark-summary.csv.
