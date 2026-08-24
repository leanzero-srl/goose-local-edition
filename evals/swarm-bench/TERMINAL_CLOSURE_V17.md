# v17 terminal closure

`terminal_closure.py` is the restart-safe terminal chain for the already-running immutable
Brainwaves v17 build. It never signals or writes into the live run. Its state, scorer clones,
logs, receipts, and frozen control snapshot live under
`/Users/mihaiperdum/goose-builds/local-sb7-engine-v17-terminal-closure` with a `077` umask,
`0700` directories, and `0600` receipts/logs.

The frozen config is `terminal-closure-v17.json`. A read-only preflight authenticates the live
process receipts and every frozen input without creating closure state. It also hashes the exact
Playwright 1.57.0 module tree and Chromium headless-shell revision 1200, mounts only that pinned
revision into an empty-`HOME` temporary view, and proves a real browser page can launch and close:

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

The score worker keeps its private empty `HOME` and `TMPDIR`; it does not inherit the operator's
home or Playwright cache. Instead it creates a private `PLAYWRIGHT_BROWSERS_PATH` view containing a
single symlink to the pinned headless-shell revision. A private hashed Node wrapper injects that
view and a `NODE_PATH` containing only the pinned Playwright module root into browser probes, without
leaking either variable into the entrant processes. The smoke launch resolves Playwright from the
frozen product probe's own location before invoking the scorer, then the worker re-hashes the
module/browser/executable and wrapper after scoring and rejects the attempt if the runtime is
absent, resolves elsewhere, or changes. Those hashes are carried into the worker, provenance, and
final receipts.
