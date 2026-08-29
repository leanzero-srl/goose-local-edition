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

## Scoring

Hermetically, serially, at the advertised port, in a disposable clone with a fixed fixture seed. **Never
a `run_build` auto-score.** Report `inner`, `crit_mult` and the unsuppressed criticals, not just the
number — a better app with more unsuppressed criticals scores LOWER, and that has already happened (a run
with 2.6× the inner scored 0.017 against 0.0273).

## The binary on PATH is not the one you built

`which goose` is `~/.local/bin/goose`, **version 1.38.0 from June**, with no `swarm` subcommand at all.
Use `./target/release/goose` by path.
