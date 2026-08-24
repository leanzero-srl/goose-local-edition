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
hash receipt, while the runtime binaries have separate frozen file hashes. Every scorer subprocess
PID, process-name hash, and kernel birth-time hash are captured once after `Popen` and fsynced to a
private journal without argv. The birth identity remains stable across executable-name transitions
and cannot authorize a reused PID. The worker continuously seals the authenticated full descendant
tree, including children that create new sessions. Capture or journal failures synchronously reap
the just-created process and its separate process group before the scorer can fail. If a positively
live descendant's birth identity cannot be queried, cleanup is unproven and the attempt fails. A retry is
forbidden until every authenticated descendant is gone and port `18970` is provably free. It
retries only abandoned disposable attempts. `stop` writes a closure-owned stop marker; it never
signals the live v17 harness, Goose, or monitor. `results` prints only the final non-secret receipt.

At terminal, the controller requires one authenticated natural Goose exit, the monitor's durable
`run_finished` completion, identical harness auto-verdict/aggregate artifacts, and their exact
16-lowercase-hex `fixture_seed`. Because the pre-existing harness is owned by `launchd`, its Unix
exit status cannot be reaped by this later supervisor; the receipt records that limitation and
derives harness success only from the authenticated zero-exit Goose verdict plus both complete
harness artifacts.

Before authoritative scoring, the controller proves the live processes are gone, uses `lsof` as
a closed-tree positive control, hashes the entire raw run twice, and never writes it again. It
scores a private disposable clone on serially locked port `18970`, with the exact seed and frozen
SB7 hashes. Both the raw auto-verdict and authoritative result must carry empty
`probe_unavailable` and `harness_missing` registries; missing registries, check-level unavailable
flags, or equivalent product-probe failure evidence reject the chain even when the frozen scorer
exits zero. `sched_unreached` remains valid scored app evidence. Publication uses the dedicated
create-only website publisher. That publisher can only
create/adopt the exact new ID `brun-fleet-qwen38-brainwaves-sb70`; it positively reads and hashes
both protected IDs before and after, and has no replace path. Persisted screenshot state is bound
to the current sealed file and decoded-pixel SHA-256 before an uploaded asset may be reused.
Completion requires HTTP-2xx HTML board/run responses with positive cache-bypass proof (`Age: 0`
plus an explicit miss/bypass, or response `no-store`) and rejects cache-serving status at any
observed layer, including an exact inner Next.js `HIT`/`STALE` behind an outer CDN `MISS`; malformed
or compound inner cache statuses are rejected. It also requires exact Sanity payload and telemetry,
ItemList/Dataset JSON-LD, fully decoded PNG dimensions and pixel buffers (including legitimate
uniform impaired-output evidence),
rendered screenshot references, and no `-rc` label on either public surface.

The score worker keeps its private empty `HOME` and `TMPDIR`; it does not inherit the operator's
home or Playwright cache. Instead it creates a private `PLAYWRIGHT_BROWSERS_PATH` view containing a
single symlink to the pinned headless-shell revision. A private hashed Node wrapper injects that
view and a `NODE_PATH` containing only the pinned Playwright module root into browser probes, without
leaking either variable into the entrant processes. The smoke launch resolves Playwright from the
frozen product probe's own location and requires its real `playwright/package.json` to equal the
configured module root before invoking the scorer. It repeats that exact resolution check after
scoring, then re-hashes the
module/browser/executable and wrapper after scoring and rejects the attempt if the runtime is
absent, resolves elsewhere, or changes. Those hashes are carried into the worker, provenance, and
final receipts.
