# goose Local Edition

goose Local Edition is a fork of [goose](https://github.com/aaif-goose/goose) (originally `block/goose`, now part of the Agentic AI Foundation at the Linux Foundation) adapted to answer one question: can several machines running the same local model, orchestrated correctly, build working software of measurably higher quality than one machine running that model alone?

The fork adds three things to upstream goose:

1. **A swarm engine** (`crates/goose-swarm` and the `goose swarm` CLI command) that plans, dispatches, supervises, verifies and repairs a software build across a pool of LM Studio devices — one model, several machines.
2. **A benchmark product** — a Benchmark page in the desktop app that runs the swarm against a frozen specification, scores the resulting application with an execution-based scorer (sb-5.3, 60 checks across seven tiers; older eras stay published under the board's scorer selector), and publishes the result to the public board at [leanzero.net/agentic-benchmarks](https://leanzero.net/agentic-benchmarks).
3. **A measurement harness** (`evals/swarm-bench/`) — the scorer, its determinism and isolation controls, the run supervisor, and the findings ledger that recorded the entire campaign (873 numbered findings over roughly two and a half weeks).

Everything upstream goose does — the desktop app, the CLI, providers, MCP extensions — continues to work. The fork point is upstream commit `a0aed81f3` (2026-07-03); the `local-edition` branch carries approximately 2,257 commits on top of it.

The register of this document is deliberate: it describes what was built, what was measured, and what the numbers are — including the ones that are not flattering. Scores below are stated against the published cloud baselines, not against 1.0, because the scorer is built so that 1.0 stays out of reach.

---

## Contents

- [What the fork is](#what-the-fork-is)
- [Architecture](#architecture)
  - [Planning: drafts, agreement, and the ladder](#planning-drafts-agreement-and-the-ladder)
  - [Frozen contracts](#frozen-contracts)
  - [The dispatch DAG and the scheduler](#the-dispatch-dag-and-the-scheduler)
  - [Supervision: the judge and pre-review](#supervision-the-judge-and-pre-review)
  - [The check → block → repair chain](#the-check--block--repair-chain)
  - [The detectors](#the-detectors)
- [How quality is enforced](#how-quality-is-enforced)
- [The benchmark product](#the-benchmark-product)
- [The fleet](#the-fleet)
- [Current results](#current-results)
- [Install and usage](#install-and-usage)
- [Measurement discipline](#measurement-discipline)
- [Relationship to upstream](#relationship-to-upstream)
- [License](#license)

---

## What the fork is

A single 27B-class local model on one machine, given a non-trivial specification ("sync payments from a vendor API into a local store and serve them on a web page"), produces an application that runs but gets roughly half of the specified behaviours wrong. That was the measured starting point of this project: a mean score of 0.733 on the campaign scorer, with Tier B (does the app do what the spec says) at 0.538 — 52% of everything lost.

The intuitive answer — add more machines, run more of the work in parallel — turned out to be the wrong theory of the problem. The campaign's central empirical finding, reached after hundreds of instrumented runs, is that **extra nodes convert into quality only through deterministic verification machinery, not through raw throughput**. A three-node fleet that merely executes a plan faster ships the same defects sooner. What moved the score was the set of mechanisms that use spare capacity to *check* work — pre-review of in-flight tasks, deterministic probes against the running application, a repair scheduler that turns findings into dispatched fix tasks, and gates that refuse to call a run green while any finding stands. A day-long audit of four fresh runs and the completion gate (finding F826 in the ledger) put it plainly: across 44 measured findings, only deterministic mechanisms ever acted; advisory ones never did.

goose Local Edition is the sum of those mechanisms, built into goose's agent framework, plus the instrument that forced them to be honest: a benchmark that boots the built application, drives it over HTTP and in a real browser, and grades what a user would actually get.

## Architecture

The swarm engine lives in two places: `crates/goose-swarm` (the scheduler, DAG, judge, pre-reviewer, replanner, event model, and the deterministic coherence primitives) and `crates/goose-cli/src/commands/swarm.rs` (the orchestration loop, the pool configuration, the phase pipeline, the completion gate, and the repair machinery). The desktop app drives the same engine through `ui/desktop/src/components/swarm` and exposes the benchmark through `ui/desktop/src/components/benchmark`.

A run proceeds through named phases: RESEARCH → PLAN (drafting and agreement) → CONTRACTS → DETAIL → EXECUTE (the dispatch DAG) → COMPLETE (the verify/repair tail). Every phase emits structured events to a JSONL log; every claim in this document about what the engine does is checkable against those events.

### Planning: drafts, agreement, and the ladder

Planning on weak local models is the single most expensive phase to get wrong, and the fork treats it as a first-class engineering problem rather than a single model call.

- **Research scouts.** Before planning, independent scout agents fan out across devices to look things up — vendor documentation, the existing tree in amend mode, tool availability. Grounding is measured: a run records whether scouts actually performed lookups (`grounded > 0`) rather than planning from the model's frozen weights. Scouts serialize on one node by construction, so this phase is one of the few that parallelizes cleanly (measured 2.65× across the fleet).
- **Parallel draft plans.** The architect's plan skeleton is drafted best-of-N across devices, and the drafts are pushed toward one canonical decomposition by *convergence molding* — the proven agreement-raiser on this fleet, on by default and A/B-testable off.
- **The agreement ladder.** When structural agreement between drafts stays below a threshold, the engine re-drafts. The ladder was measured as the largest planning tax in the engine (roughly 25 minutes per round when it fires), and one fully traced firing spent 756 seconds re-buying a plan structurally identical to the one it already held. Two shipped fixes govern it now: the redraft rung only fires when a draft that does not yet exist *can* exist (distinct-model headroom, not counter headroom), and `diverse_plan` enforces the same skip predicate the engine's own shadow counterfactual computes. The shadow — a free in-run measurement of "would enabling this lever have skipped the ladder?" — read true in 7 of 7 laddered runs before the lever was armed.
- **Spec-sized plans.** The number of modules the architect is asked for derives from the specification, not the fleet: the same spec used to yield "6 to 12 modules" on three nodes and "2 to 4" on one, a fleet-scaled ask that only ever bound inflationary. Task existence is now the spec's property (`spec_sized_plan`, on by default).

### Frozen contracts

After the plan is agreed, a CONTRACTS phase freezes a **signature-only interface per module** — function and method signatures with bodies removed, type and constant declarations kept verbatim — *before* any implementation is dispatched. Workers implementing different modules then code against each other's frozen contracts, not each other's files.

Two deterministic primitives in `crates/goose-swarm/src/coherence.rs` make this cheap enough to use everywhere:

- `extract_signatures` strips a built dependency's source to its declaration surface, so a consumer's prompt carries the exact API it must call instead of the dependency's whole body.
- `scope_contract_bundle` scopes the global contract bundle to a worker's DAG neighbourhood (its dependencies, its consumers, itself), so per-worker context is proportional to the node's degree in the DAG, not to the total module count.

Both are pure, model-free, and unit-tested. When the heuristic extractor recognizes nothing (unknown language, unusual style), the caller falls back to the original body — a mis-parse degrades to the old behaviour, never to a crash or an empty API. Contracts also enable a flat-fan backstop that flattens the architect's false-serialization dependency chains: modules that only *consume* a contract need not wait for its implementation.

### The dispatch DAG and the scheduler

The detailed plan becomes a dependency DAG of typed tasks (implementation, test-authoring, assets, docs, verification), executed by a weighted work-queue scheduler (`crates/goose-swarm/src/scheduler.rs`):

- **Routing is by model identifier, weighted by measured host speed.** Each device carries a `speed_weight` (per-host throughput rank); dispatch counts are normalized by it, so a faster host accumulates proportionally more tasks before it is considered "even", and every equal-load tie goes to the fastest host. This is a repaired defect, not a design flourish: before the fix, index ties sent every ordinary task to the *slowest* host (measured task split 73/63/42, the exact inverse of the configured ranking).
- **Fat tasks split.** A task whose brief bundles separable work is split into children that inherit the parent's external dependencies and rely on each other's frozen contracts — not on files. Splitting was measured to buy quality and cost time (split runs 0.7710 vs 0.7355, at 108 vs 93 minutes), so it is a lever, on by default in the product regime.
- **Skeleton-first fill for hard modules.** A hard module can expand into a skeleton task plus parallel slot-fill tasks joined by an AST splice, gated on the stub actually parsing, with a byte-fence that refuses any foreign edit outside a fill's assigned slot. The fence has never let a foreign edit reach the real tree.
- **E2E verification shards by job size, not fleet size.** The verify fan is cut from the command union of the spec, fleet-blind — an earlier fleet-sized cut made the same 4-command spec produce 4 shards on three nodes versus 2 on one, buying ~16 minutes of extra fleet task-time for a *longer* slowest shard.
- **Failsafes are generous and re-route rather than kill.** Worker and planner timeouts default to 900 s and exist only to catch a genuine infinite stall on slow local hardware; a progress-based watchdog distinguishes "slow" from "dead", and a stalled stream exits early instead of burning the budget (the first live stall-exit saved ~25 minutes of a run).

### Supervision: the judge and pre-review

Redundant capacity is spent on judgment. This is the design's founding principle — the idle node is the point — and both supervision mechanisms are measured, not assumed:

- **The judge** observes every phase of the run. When a worker loops, the judge redirects it *in-session* with a directional hint instead of killing it, and its verdicts cite deterministic instrument readings (failed tool calls, import health) rather than impressions. Its cost is controlled: an earlier defect had the judge hot-spinning 36,000 observe/skip cycles on a long single-node task; the fix took the same phase to 55 cycles and the trace from 38 MB to 0.16 MB.
- **Pre-review** is the mechanism that actually scales with the fleet. Measured across the corpus, pre-review runs 2.0× per run at one node and 10.2× at three — a 5.1× scaling carried entirely by spare nodes. (The judge, by contrast, does not scale with node count at all — ~88 verdicts per run at one node against ~78 at three — and is treated as fixed overhead.)
- **The tail is an orchestrator.** When the run narrows to a final long-running sink task, idle nodes are given real work: reviewing the tree, generating tests, and racing repair attempts, all scheduled rather than improvised.
- **A running swarm answers questions.** Dropping a text file into a run's `.swarm/questions/` directory causes an idle node to answer it from the run's own state; measured end-to-end at 57 seconds from question to answer.

### The check → block → repair chain

The engine's completion gate is deterministic and adversarial toward its own run. Nothing advisory survives in it, because nothing advisory was ever observed to act.

1. **Check.** At the completion boundary the gate runs the full battery: build, tests, entry-point probes, spec-contract probes against the running app, the render gate, the coherence scans. Every check produces findings with evidence.
2. **Block.** Any standing finding blocks green. Failed tasks block green. A docs-only module gets no import gate (a README should not be import-checked), but everything executable is verified in its own language — the gates emit honest reds in every supported language, and a piped test invocation that hides its exit code (the `collected 0 items` false-green class) is itself a detected defect.
3. **Repair.** Findings become *scheduled fix tasks* — disjoint per file, raced or fanned across the fleet, each fix verified in a shadow tree and promoted only when strictly better than the baseline. Repair budgets are run-derived, scaling with the size of the tree being repaired rather than hardcoded. Fix rounds are progress-based: a round that converts findings (3 → 2 → 1) continues; a flat round ends the loop honestly.
4. **Ship best verified.** The run ends on the best tree any verify round measured — never on the last edit. This rail exists because a measured run served a working application mid-run and shipped a dead one: late fix rounds regressed the tree and nothing restored the best verified state. With the rail on, that class is closed.
5. **Early close.** A repair wave whose findings reach zero closes early and cancels its sibling attempts; observed live on its first run, two waves early-closed (6 → 0 and 2 → 0) and the wall came in at 95 minutes against a comparable 116.

### The detectors

Each detector below exists because a specific measured failure class shipped past everything that came before it. Each was validated against the scorer before being trusted, and each feeds the repair loop as a blocking finding.

- **Spec-contract probes.** Every endpoint the specification documents is probed on the running application — including error paths. The probe's history is instructive: its first version produced phantom findings by probing the vendor mock, and it now carries a standing rule that its findings are verified against ground truth before being believed.
- **Vendor-truth POST checks.** Advertised mutating endpoints are exercised against the live vendor service, not just read paths. The first live firing of the POST probe caught the top two ranked defect classes of its day.
- **DOM-id contract scan.** The frontend's JavaScript and its HTML are checked against each other: an element id the script queries must exist in the markup. This closes the class where three workers each build a plausible page that cannot possibly work together.
- **CSS-coherence scan.** The class vocabulary used by the markup is checked against the stylesheet actually shipped. The motivating incident: a journey app shipped completely unstyled while every gate stayed green, because three workers used three class vocabularies with zero contract between them.
- **The render gate.** The completion gate opens the built page in a real headless browser (a pluggable probe; `product_probe.mjs` in the harness) and asserts the journey basics: the page renders rows, the sync action works, error and empty states appear, no console errors. A blank frontend is a blocking, repairable finding. Before this gate existed, a run scored 0.514 with a page that rendered nothing — engine-green throughout. The gate was observed firing three times and blocking green three times in its first day. When the probe itself dies environmentally (missing browser on the scoring host), that death names itself instead of scoring the app down — a harness gap must never be punished as app quality.

## How quality is enforced

Three rules, learned the expensive way, govern how the engine converts machinery into score:

**Findings block green and drive fix rounds.** The campaign's oldest law is that only checks wired into the check → block → repair chain ever convert into score. A scorer-side check with no engine-side counterpart measures a gap the swarm cannot close; every scorer family therefore has a completion-gate sibling.

**One ruler.** The engine's internal shadow grade counts every category the scoring round counts. An earlier split — where the in-run grade sampled a subset — made final scores a lottery: a run could optimize what its shadow measured and lose on what it didn't. The one-ruler change (F862) removed that class.

**Honest reds are preserved.** An exit code of 1 at the boundary is an honest red finish, not a crash, and is scored as what it is. Over-broad "void" gates twice threw away legitimate rows — including the best product-regime score ever recorded (0.8645), voided by a stop gate that matched too much — and were narrowed to actual death evidence. Conversely, killed engines briefly minted phantom scored rows until the kill-artifact signature became a gate. Both directions matter: a benchmark that silently drops its worst runs or keeps its dead ones is measuring itself, not the software.

## The benchmark product

The desktop app carries a Benchmark page (`ui/desktop/src/components/benchmark/BenchmarkView.tsx`) that packages the whole measurement loop into a product: **Run → Swarm build → Scoring → Publish.**

- **The task.** Every run builds the same frozen application — *vendorsync*, a service that syncs payments from a vendor API into a local SQLite store and serves them on a web page, backend restricted to the Python 3 standard library. The specification (`evals/swarm-bench/spec-build-v2.md`) demands a *product*, not a demo: human-readable dates in the user's locale, visually distinct statuses, pagination wired to the documented API, responsiveness at 375 px, stated performance budgets, intentional design. Each sentence of the spec exists to be checked.
- **The scorer.** sb-5.2 emits 60 checks across seven tiers: **A** structure, **B** behaviour (HTTP against the running app), **C** vendor contract, **D** finesse, **J** journey (a headless browser drives the full user flow), **V** visual (rendered-page checks, mobile layout), **P** performance (response-time budgets measured on the live app). The overall score weights core A–D at 60% (internal weights A 25 / B 30 / C 25 / D 20), journey 15%, visual 10%, performance 5%, and a set of hard blocks (server binds, data rows render) at 10% so that hard failures always move the overall score. The scorer is frozen: only sb-5.2 rows share the leaderboard, because scorer versions are incomparable by construction.
- **Screenshots during repair.** The render probe captures a PNG per scenario (loaded, synced, error, empty, mobile) on every repair epoch, so the published record shows the page *as the swarm repaired it* — first epoch before, final epoch after.
- **Scoring detail.** The app renders the full check-by-check story of a score: the composition table, per-tier check rows with evidence and the consequence of each loss, the findings that held, and the repair-round progression. The same detail travels with a published run (`checksSummary` in the payload) so a score on the public board is auditable, not just a number.
- **Publishing.** Publishing POSTs a strict-allowlisted payload to `leanzero.net/api/benchmark-runs`. Identity is a generated pseudonym (`<adjective>-<animal>-<4hex>`) plus a never-displayed install id; the model identifier is **prefilled from engine truth** — the run's own `pool_resolved` event — and user-editable; per-node detail (device name and model, verbatim from the same event) rides along. A submission goes live immediately — there is no review queue and no editorial gate — and the board page revalidates on post so the new row appears at once. There is no browser submission form; posting happens only from the desktop app.
- **Baselines in-app.** The page shows the frozen Anthropic cloud baselines alongside your fleet's runs, on the same scorer, so a local result is always read against a meaningful scale.

## The fleet

The reference fleet is three Apple-silicon machines running [LM Studio](https://lmstudio.ai/), all serving the same model (the campaign ran a qwen3.6-27B derivative; the default planner is `qwen/qwen3.6-27b`). The engine's fleet model, configured via `goose swarm pool` and persisted in the goose config:

- **Per-node identity is three distinct identifiers**, all load-bearing: the LM Studio *device id*, the *model identifier* the device serves, and the physical *host*. Routing is by model identifier; display and speed weighting key off device and host. `fleet_reload.sh` in the harness documents the reference fleet's device ids, identifiers, quantizations and context sizes.
- **Speed weights** (`speed_weights`, a host-substring → weight map) rank hosts by measured throughput. The scheduler seeds heavy tasks onto the fastest host and normalizes load by weight, so slower machines do proportionally less work.
- **Concurrency ceiling: two tasks per node.** Each device's `weight` caps concurrent tasks routed to it, and the working ceiling is 2 — an LM Studio model instance serves requests serially, with limited PARALLEL headroom; beyond two concurrent tasks a node's throughput degrades rather than scales. The engine never loads extra model instances on a device unless `instances` is raised explicitly.
- **Imposed sampling.** The engine can impose sampling parameters on the fleet (`GOOSE_SWARM_TEMP`, plus top-p/top-k/min-p/repeat-penalty). The product regime runs single-sample paths (workers, detail, sink, judge) at temperature 0.2 — low temperature is correct for code — while plan drafts keep a higher draft temperature for diversity.

## Current results

The public board at [leanzero.net/agentic-benchmarks](https://leanzero.net/agentic-benchmarks) carries frozen Anthropic cloud baselines and the fleet's published entries, organized by scorer era — the board's selector defaults to the newest scorer and keeps every older era viewable as a frozen historic board.

**sb-5.3** (the current scorer — "rendered means seen": a table row counts only if the browser would actually paint it; the sb-5.2 numbers below were measured before that correction and are kept as their own era):

| entrant | score | tiers (A/B/C/D) | wall |
|---|---|---|---|
| Claude Opus 5 (baseline) | 0.9142 | 1.00 / 1.00 / 0.90 / 0.88 | 20 min |
| **3-node local fleet** | **0.93** | 1.00 / 1.00 / 1.00 / 0.94 | 190 min |
| Claude Sonnet 5 (baseline) | 0.4971 | 1.00 / 0.44 / 0.30 / 0.64 | 7 min |
| Claude Haiku 4.5 (baseline) | 0.4615 | 0.83 / 0.37 / 0.10 / 0.63 | 5 min |

Read plainly: on the honest ruler the 3-node local fleet lands level with Opus — 0.93 against a baseline of 0.9142, inside Opus's own repeat spread (0.889–0.960 across reps), so the honest claim is parity, not victory — and roughly doubles Sonnet — the frontier gap on this task is, measured, four thousandths of a point, bought with a ~10× wall clock. The Sonnet and Haiku collapses are not typos: their builds ship frontends whose data the old probe credited while hidden behind error states, and the corrected probe prices that truthfully — the same correction that exposed (and then repaired) the fleet's own frontend defects.

**sb-6.1** (the hard tier — "VendorSync Pro": raw-WebGL 3D visualization graded by analytic pixel recomputation, HMAC webhooks, optimistic concurrency, DST-crossing money math; the hand-written golden reference passes the freeze gate at 100%). All entrants are scored serially and hermetically at the vendor port their spec advertised — the sb-6.0 provisional numbers were re-measured after three probe defects and a scoring-contention artifact were found and fixed the same night:

| entrant | score | run |
|---|---|---|
| GPT-5.6 Sol (baseline) | 0.8690 | clean |
| Claude Opus 5 (baseline) | 0.8281 | clean |
| GPT-5.6 Luna (baseline) | 0.7887 | floor† |
| Claude Sonnet 5 (baseline) | 0.7666 | clean |
| GPT-5.6 Terra (baseline) | 0.7257 | floor† |
| Claude Haiku 4.5 (baseline) | 0.4597 | clean |
| **3-node local fleet** | **0.4527** | clean |

† Luna's and Terra's sessions were cut early by a goose engine defect (compaction re-roled the summarizer's reasoning content into a user message, which Bedrock rejects — every GPT session died at its first compaction; fixed in `19b4ed6ef`). Their scores are floors, not ceilings, and are disclosed as such on the board.

The hard tier does what it was built to do: it separates the field the soft tier compressed. The fleet's build ships a working backend (full 1553-payment sync, sub-2ms API latencies, rows rendering at 21 ms) but scores 0.05 on the 3D tier — the WebGL panel is where the 27B class currently breaks, and that is now measured instead of hidden.

**sb-5.2** (historic era, frozen): Opus 0.9755 · Sonnet 0.9692 · Haiku 0.7861 · fleet (mighty-crane-54f2) 0.6618. The era's compression at the top was measured to be an instrument artifact — 42.8 of 100 points passed for ≥90% of serious builds — which is what drove both the sb-5.3 correction and the sb-6 hard tier above.

Two findings frame these numbers honestly:

- **The swarm's advantage is its machinery, not its parallelism.** Measured mechanisms that make three nodes faster exist (parallel research, parallel planning, execute-phase parallelism); measured mechanisms that make three nodes *better* are exactly the verification set — pre-review scaling 5.1× with spare nodes, the repair scheduler converting findings, the gates blocking green. When those mechanisms are off or broken, the node-count advantage disappears into a sync-bug lottery.
- **The gap to the cloud baselines is real and stated.** Nothing on the board suggests a local fleet currently matches a frontier model on this benchmark. What the board shows is the measured distance, on a frozen ruler, with the machinery that closes it from below shipping in this repository.

## Install and usage

### Build from source

```bash
git clone https://github.com/leanzero-srl/goose-local-edition.git
cd goose-local-edition
source bin/activate-hermit    # pins the toolchain
cargo build --release         # engine + CLI
just release-binary           # release binary
```

### Desktop app

```bash
just run-ui                   # development
# or package:
cd ui/desktop && pnpm install && pnpm run package
```

The packaged app lands in `ui/desktop/out/`. The Benchmark page is in the app's navigation; the swarm run panel shows planning, fleet, work and event-log zones live during a run.

### Configure the fleet

```bash
goose swarm pool              # interactive: devices, weights, enable/disable
```

Each device needs LM Studio reachable at its endpoint with the model loaded. Pool state persists under the `swarm` key in `~/.config/goose/config.yaml`.

### Run the swarm from the CLI

```bash
goose swarm run "build a CLI tool that ..."
```

### Run a benchmark

Open the Benchmark page in the desktop app, choose the node count, and press Run. The app boots the vendor fixture, runs the swarm build against the frozen spec, scores the result with sb-5.2, and offers Publish with the identity, model identifier and screenshots described above. Publishing is opt-in per run.

### Environment levers

The engine's behaviour is governed by `GOOSE_SWARM_*` environment variables (environment beats saved config — a stale `config.yaml` shadowing a baked default is a measured failure class). The levers that matter, as pinned by the campaign's product regime:

| lever | effect |
|---|---|
| `GOOSE_SWARM_MAX_NODES` | cap the number of fleet nodes used |
| `GOOSE_SWARM_FIX_SCHED=1` | findings become scheduled, raced fix tasks (the repair consumer) |
| `GOOSE_SWARM_PROBE_ADVERTISED_POST=1` | vendor-truth probing of advertised mutating endpoints |
| `GOOSE_SWARM_RENDER_PROBE=<probe.mjs>`, `GOOSE_SWARM_RENDER_NODE=<node>` | the render gate's headless-browser probe |
| `GOOSE_SWARM_SHIP_BEST=1` | end the run on the best verified tree, never the last edit |
| `GOOSE_SWARM_DIVERSE_PLAN=1` | skip redraft ladders the engine's own shadow marks unnecessary |
| `GOOSE_SWARM_SPLIT_FAT=1` | split fat tasks into contract-coupled children |
| `GOOSE_SWARM_TESTGEN=1` | idle-node test generation during tails |
| `GOOSE_SWARM_TEMP=0.2` | impose low temperature on single-sample paths |

Most mechanisms above are on by default in the shipped configuration; the levers exist so any of them can be A/B-tested off. The full set (several dozen) is documented at each `default_*` function in `crates/goose-cli/src/commands/swarm.rs`.

## Measurement discipline

The campaign that produced this fork is recorded in `evals/swarm-bench/nodeloop/FINDINGS.md` — a numbered ledger running F1 (2026-08-01) through F873 (2026-08-17). The ledger is the project's method as much as its record:

- **Every claim gets a number and a falsifier.** Findings register predictions before the run that tests them, and the outcome is written next to the prediction — including `FALSIFIED`.
- **Adversarial review is scheduled, not incidental.** Red-team waves re-derive the day's biggest claims from raw logs; the current ledger file carries 74 explicit refutation marks (`RETRACTED`, `OVERTURNED`, `REFUTED`, `FALSIFIED`). Overturned headlines stay in the file, struck and explained, because a corrected record is the only kind that can be trusted. Notable self-corrections include: a "the campaign produced zero data" claim overturned by finding 47% of real runs missing from a survivorship-biased corpus; a task-split harm claim reversed by a corrected join; and a planning-collapse headline reduced from 3.20× to 1.63× once a confounding redraft round was accounted for.
- **The grader is trusted only under controls.** Run-twice determinism, defect-isolation checks (inject an unstyled page, prove only the visual checks drop), and positive controls on every sweep. Two grader flaws were caught by controls before anything was believed (F4).
- **Ground truth is external where possible.** Node counts are oracled against LM Studio's own activity records, independent of the engine's self-report. Phantom rows (killed engines, corpses of watchdog restarts) are detected by artifact signature and refused.
- **A negative that authorises action must be proven on the same object.** Empty results, zero counts and dead probes are treated as claims about the instrument until shown otherwise — several of the campaign's worst errors were gates believing their own blindness.

The harness enforcing this lives in `evals/swarm-bench/`: the sb-5.2 scorer and its controls (`bench/`), the run supervisor and rolling sweep (`nodeloop/`), the product contract shared with the website (`PRODUCT-CONTRACT.md`), and the product-tier design record (`nodeloop/PRODUCT-TIER.md`).

## Relationship to upstream

The fork point is `a0aed81f3` (upstream main, 2026-07-03). Upstream goose has continued to move; this fork tracks it deliberately rather than continuously — upstream changes are ingested at chosen boundaries so that the benchmark binary under measurement stays frozen. Upstream-facing surfaces (providers, ACP, MCP, the desktop chat experience) are intentionally left close to upstream to keep that ingestion tractable.

Nothing in the fork phones home. Benchmark publishing is explicit, opt-in, per run, and pseudonymous.

## License

Apache 2.0, unchanged from upstream. See `LICENSE`.

Upstream goose's own README is preserved verbatim at [`README-upstream.md`](README-upstream.md).
