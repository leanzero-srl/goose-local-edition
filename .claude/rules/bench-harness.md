---
paths:
  - "evals/swarm-bench/**"
---

# The sb-7 benchmark harness

## Launch through the BENCHMARK VIEW, never a chat

```bash
~/goose-builds/loop-state/stop_local_run.sh 9897    # MUST exit 0 — it gates on `lms ps`
open -n /Applications/Goose.app --args --remote-debugging-port=9897
node ~/goose-builds/loop-state/bench_dispatch.mjs 9897 sb-7 3
```

`launch.sh` alone types the raw spec into the desktop, so the prompt keeps its literal `{BASE_URL}` /
`{DOCS_URL}` / `{API_KEY}` and there is no vendor to sync from. That produced a full day of void runs.

**Verify it is a real benchmark**: `pgrep -fl run_build.py` must carry `--sb7`;
`curl 127.0.0.1:8850/v3/docs` must return 200; and there must be **no** "# Build `app`" chat in the
sidebar — if there is, it is a chat, not a benchmark. Do NOT start a vendor yourself; `run_build` owns
the port.

## Stopping

`pkill -9 -f 'Goose.app/Contents/MacOS/Goose'` is WRONG — it matches the Electron binary only, so
`Resources/bin/goose swarm run` survives, reparents to launchd, and keeps the whole fleet GENERATING for
a run whose window is gone (measured: ~25 minutes across three nodes). `stop_local_run.sh` kills all
three command lines and refuses to exit 0 while any node is still generating.

Also: an anchored pattern like `MacOS/Goose$` does not match a process launched with
`--remote-debugging-port` appended. A two-hour-old app survived that way and served CDP all morning while
every UI verdict was about old code.

## Scoring — five wrong-number mechanisms, three of them now gates in the scorer

```bash
# the run's OWN seed lives in the harness vendor trace header, not in the run dir:
head -c 300 "$HOME/Library/Application Support/Goose/benchmark/runs/build/trace-<run>.jsonl"   # "fixture_seed"
python3 ~/goose-builds/loop-state/tick.py | grep orphans          # must read 0 before any scoring
GOOSE_SWARM_RENDER_NODE=$HOME/.nvm/versions/node/v22.22.0/bin/node \
python3 evals/swarm-bench/bench/score_sb7.py --tree <fresh rsync clone, .swarm and logs excluded> \
    --port 8850 --seed <fixture_seed> --json-out verdict.json
python3 ~/goose-builds/loop-state/compare_vs_cloud.py verdict.json   # FINAL = the verdict's own score
```

Hermetically, serially, at the advertised port (8850 for desktop-launched runs), with the run's own seed,
under the node that has playwright. **Never a `run_build` auto-score.** Report `inner`, `crit_mult` and
the unsuppressed criticals, not just the number — a better app with more unsuppressed criticals scores
LOWER (a run with 2.6× the inner scored 0.017 against 0.0273).

The scorer now REFUSES rather than printing a wrong number: exit 2 without `--seed`/`--fresh-seed` (r0 was
scored twice on a drawn seed); exit 3 when `product_probe_v3.mjs --preflight` fails under the configured
node (hermit's node has no playwright — 30 of 99 checks came back PROBE-UNAVAILABLE and 0.0832 printed
as if comparable); exit 2 on a held vendor port (`_port_holder` names the pid); and `_error_obj()` so a
string error body `{"error": "Not found"}` cannot crash a check (it did, at four sites). It reaps its
own app children on exit, SIGINT and SIGTERM. `run_build.py` runs the same port and preflight refusals
before its vendor binds.

Run dirs live under `~/Library/Application Support/Goose/benchmark/runs/build/` — not `~/goose-builds/`
(`mdfind -name <run>` finds them). The heartbeat of a harness run sits at the TREE ROOT next to
`run.jsonl`, not in `.swarm/`.

## Replaying the gate without a run

`./target/release/goose swarm gate <tree> --spec evals/swarm-bench/spec-build-sb7.md` runs
`run_spec_contract` and BOOTS the archived app — the proof for gate changes and for the exit-hang class
(old binary leaked 2 servers, new leaks 0). `swarm verify <tree>` checks imports and owned files only and
cannot show either. Bound a replay with `python3 -c 'subprocess.run(..., timeout=300)'`; macOS has no
`timeout`.

## Leaked app servers

Every boot probe and scorer used to leave the wrapper's `ledgerd`/`notifierd` grandchildren alive (41
found on 2026-08-29, 25 from one run), each holding a port the next probe then refused to conclude on.
`tick.py` prints `orphans: N leaked app servers` (PPID 1 and a `-m app` command or a cwd in a run tree);
`launch.sh` kills them and refuses to launch over survivors; the engine spawns process groups since
`44b2ad6cd`. Zero is the only acceptable number, and never kill a LIVE scorer's children (their PPID is
the scorer).

## First tick after a launch

`~/goose-builds/loop-state/first_tick_r1.sh <build-sha>` — eleven engine-truth checks: `run_build.py`
with `--sb7`, vendor 200 on 8850, `levers_resolved.build_sha` and `.levers.benchmark`, prompt length,
heartbeat at the tree root, orphans 0, the engine process is `Resources/bin/goose` (the Benchmark view runs
the BUNDLE's engine, never `target/release`), and the installed binary carries the sha (`strip =
"symbols"` erases every function name, so the build sha is the only string probe).

## The binary on PATH is not the one you built

`which goose` is `~/.local/bin/goose`, **version 1.38.0 from June**, with no `swarm` subcommand at all.
Use `./target/release/goose` by path.
