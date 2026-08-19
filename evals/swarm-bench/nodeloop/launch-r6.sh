#!/bin/zsh
# r6: SAME setup as r5 (controlled comparison for the fix batch) — spec v3, port 8990,
# only-rep 6, temp 0.2 via REGIME.env, product probe v2, author pitfalls ON.
set -e
cd /Users/mihaiperdum/Projects/goose/evals/swarm-bench
set -a; source nodeloop/REGIME.env; set +a
export GOOSE_SWARM_RENDER_PROBE=$PWD/bench/product_probe_v2.mjs
export BENCH_SPEC=$PWD/spec-build-v3.md
export GOOSE_SWARM_AUTHOR_PITFALLS=1
exec python3 bench/run_build.py --sb6 --entrant swarm-3node --only-rep 6 \
  --timeout 10800 --port 8990 --out runs/sb6-fleet
