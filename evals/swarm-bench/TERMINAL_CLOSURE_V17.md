# v17 terminal closure

`terminal_closure.py` is the restart-safe terminal chain for the already-running immutable
Brainwaves v17 build. It never signals or writes into the live run. Its state, scorer clones,
logs, receipts, and frozen control snapshot live under
`/Users/mihaiperdum/goose-builds/local-sb7-engine-v17-terminal-closure` with a `077` umask,
`0700` directories, and `0600` receipts/logs.

The frozen config is `terminal-closure-v17.json`. A read-only preflight authenticates the live
process receipts and every frozen input without creating closure state:

```bash
python3 evals/swarm-bench/terminal_closure.py preflight \
  --config evals/swarm-bench/terminal-closure-v17.json
```

The controller is intentionally not auto-started by the repository. When the operator starts
it, the process detaches into its own session and snapshots the reviewed controller, publisher,
and config before it waits:

```bash
python3 evals/swarm-bench/terminal_closure.py start \
  --config evals/swarm-bench/terminal-closure-v17.json
python3 evals/swarm-bench/terminal_closure.py status \
  --config evals/swarm-bench/terminal-closure-v17.json
python3 evals/swarm-bench/terminal_closure.py watch \
  --config evals/swarm-bench/terminal-closure-v17.json
```

`resume` adopts a still-running detached scorer or publisher from its PID/start-time/process-name
hash receipt, while the runtime binaries have separate frozen file hashes. It retries only abandoned disposable attempts. `stop` writes a closure-owned stop
marker; it never signals the live v17 harness, Goose, or monitor. `results` prints only the final
non-secret receipt.

At terminal, the controller requires one authenticated natural Goose exit, the monitor's durable
`run_finished` completion, identical harness auto-verdict/aggregate artifacts, and their exact
16-lowercase-hex `fixture_seed`. Because the pre-existing harness is owned by `launchd`, its Unix
exit status cannot be reaped by this later supervisor; the receipt records that limitation and
derives harness success only from the authenticated zero-exit Goose verdict plus both complete
harness artifacts.

Before authoritative scoring, the controller proves the live processes are gone, uses `lsof` as
a closed-tree positive control, hashes the entire raw run twice, and never writes it again. It
scores a private disposable clone on serially locked port `18970`, with the exact seed and frozen
SB7 hashes. Publication uses the dedicated create-only website publisher. That publisher can only
create/adopt the exact new ID `brun-fleet-qwen38-brainwaves-sb70`; it positively reads and hashes
both protected IDs before and after, and has no replace path. Completion requires fresh no-cache
board/run fetches, exact Sanity payload and telemetry, ItemList/Dataset JSON-LD, direct PNG checks,
rendered screenshot references, and no `-rc` label on either public surface.
