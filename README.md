# Goose Swarm

**Download the macOS app:** [Goose-2.0.3.dmg](https://github.com/leanzero-srl/goose-local-edition/releases/tag/v2.0.3) — Developer-ID signed and notarized by Apple (Gatekeeper: "accepted, Notarized Developer ID"), Apple silicon. Drag to Applications; no right-click-to-open dance.


Goose Swarm (the repository is still named goose-local-edition) is a fork of [goose](https://github.com/aaif-goose/goose) (originally `block/goose`, now part of the Agentic AI Foundation at the Linux Foundation) adapted to answer one question: can several machines running the same local model, orchestrated correctly, build working software of measurably higher quality than one machine running that model alone?

The fork adds these things to upstream goose:

1. **A swarm engine** (`crates/goose-swarm` and the `goose swarm` CLI command) that plans, dispatches, supervises, verifies and repairs a software build across a pool of LM Studio devices — one model, several machines.
2. **A benchmark product** — a Benchmark page in the desktop app that runs the swarm against a frozen specification, scores the resulting application with an execution-based scorer (sb-5.3, 60 checks across seven tiers; older eras stay published under the board's scorer selector), and publishes the result to the public board at [leanzero.net/agentic-benchmarks](https://leanzero.net/agentic-benchmarks).
3. **A measurement harness** (`evals/swarm-bench/`) — the scorer, its determinism and isolation controls, the run supervisor, and the findings ledger that recorded the entire campaign (873 numbered findings over roughly two and a half weeks).
4. **An in-house MLX inference engine** (`crates/goose-sidecar`, a supervised [Rapid-MLX](https://github.com/leanzero-srl/Rapid-MLX) sidecar) that mounts a local model on Apple silicon next to LM Studio — the app's Goose Swarm hub mounts it, and the swarm treats it as one more node.
5. **The Swarm provider** (`crates/goose/src/providers/swarm.rs`): choosing *Goose Swarm* in a chat routes each turn to an idle node of your pool through a process-wide idle guard (`swarm_router.rs`: sticky per conversation, then most free slots, then a real queue with no timeout); *Goose Swarm · Build* turns a message into a multi-agent build brief. The local edition offers exactly these providers plus the swarm's cloud families — nothing else.
6. **LeanZero Link** (`crates/leanzero-link`): a self-hosted mesh (Headscale) so several Macs form one swarm, each account isolated.

Everything upstream goose does — the desktop app, the CLI, providers, MCP extensions — continues to work. The fork point is upstream commit `a0aed81f3` (2026-07-03); the `local-edition` branch carries approximately 2,257 commits on top of it.

The register of this document is deliberate: it describes what was built, what was measured, and what the numbers are — including the ones that are not flattering. Scores below are stated against the published cloud baselines, not against 1.0, because the scorer is built so that 1.0 stays out of reach.

---

## Contents

- [What the fork is](#what-the-fork-is)
- [Architecture](#architecture)
  - [The multi-engine layer](#the-multi-engine-layer)
- [How quality is enforced](#how-quality-is-enforced)
- [The benchmark product](#the-benchmark-product)
- [The fleet](#the-fleet)
- [Current results](#current-results)
- [Install and usage](#install-and-usage)
- [Releases](#releases)
- [Measurement discipline](#measurement-discipline)
- [Relationship to upstream](#relationship-to-upstream)
- [License](#license)

---

## What the fork is

A single 27B-class local model on one machine, given a non-trivial specification ("sync payments from a vendor API into a local store and serve them on a web page"), produces an application that runs but gets roughly half of the specified behaviours wrong. That was the measured starting point of this project: a mean score of 0.733 on the campaign scorer, with Tier B (does the app do what the spec says) at 0.538 — 52% of everything lost.

The intuitive answer — add more machines, run more of the work in parallel — turned out to be the wrong theory of the problem. The campaign's central empirical finding, reached after hundreds of instrumented runs, is that **extra nodes convert into quality only through deterministic verification machinery, not through raw throughput**. A three-node fleet that merely executes a plan faster ships the same defects sooner. What moved the score was the set of mechanisms that use spare capacity to *check* work — a research fan that answers each slice's questions before anything is planned, shards verified against their declared interface before they merge, deterministic probes against the running application, a repair tail that reproduces a finding before it promotes a fix, and gates that refuse to call a run green while any render-class finding stands. A day-long audit of four fresh runs and the completion gate (finding F826 in the ledger) put it plainly: across 44 measured findings, only deterministic mechanisms ever acted; advisory ones never did.

goose Local Edition is the sum of those mechanisms, built into goose's agent framework, plus the instrument that forced them to be honest: a benchmark that boots the built application, drives it over HTTP and in a real browser, and grades what a user would actually get.

## Architecture

The engine that ships is the one that was measured: commit `393a99351` (tag `r6h-golden-0.4616`), the run that scored 0.4616 on sb-7 with three local nodes. It lives in `crates/goose-cli/src/commands/swarm.rs` with its modules under `commands/swarm/` (the orchestration loop, the pool, the phase pipeline, the completion gate, the repair machinery) and `crates/goose-swarm` (the scheduler, DAG, judge, event model). The desktop drives the same engine through `ui/desktop/src/components/swarm` and exposes the benchmark through `ui/desktop/src/components/benchmark`. Any change to these paths lands only with a new measured run that scores at least 0.4616.

A run proceeds through these phases, each emitting structured events to `run.jsonl` that the run panel renders live:

1. **OPEN** — one planner call slices the specification into balanced semantic slices, each owning its spec sections.
2. **ASK** — only when the opener leaves open decisions: the user is asked once, up front; unanswered decisions ride the research fan and settle at plan time.
3. **RESEARCH FAN** — one read-only lane per slice, in parallel across the fleet, answering the slice's design and external questions from the spec and the vendor's live API; answers snowball into a ledger and into every brief. A miss is a loud `research_unanswered`, never a block.
4. **SYNTHESIS** — planning over answered material: tasks, owned files, dependencies, the integrate-verify join that owns no files.
5. **Deterministic plan repairs** (`finalize_plan_before_dag`) — code, not a model, repairs the plan's structural defects: a task owning nothing, shared files, module/package shadows, the join's files, unowned entry points. A correction is a patch, never a re-emission.
6. **THE SPLIT** — a task measured fat (above mean+σ of the plan and at least twice the median) is split by code into shards sized to the free hosts, with a merger task and a declared interface; every shard is verified at completion.
7. **BUILD** — the dispatch DAG runs one call per fleet slot; every brief opens with one named write and carries the real dependency sources and the ledger, not stubs. No wall clock, turn ceiling or retry count bounds model work; terminators are progress-based.
8. **INTEGRATE → REPAIR** — the join boots the application, probes it, and the repair tail reproduces each finding first and promotes a fix only on the finding's own flip. `passed` requires zero render-class known bugs.

Two supervision principles run through all of it. **The judge nudges, it does not kill**: a lane is looked at on evidence only (a repeat, a degenerate answer, a forming-channel stall) and steered with the words it produced, never on a cadence. **A missing input never silently substitutes content**: every absence is a named event the operator can read, never a template or a quiet default.

What was deleted, and why, is as much the design as what stayed. The LLM review round, the dynamic replanner, frozen contracts, the coverage pass and the research resplit were each measured across runs at hours of node time for output nothing downstream consumed, and were removed rather than capped. `EXPERIMENTS-LEDGER.md` and `REFUSED.md` hold the receipts so they are not tried twice.

### The multi-engine layer

`commands/swarm_engine.rs` lets the pool mix engines: LM Studio nodes reached through one LM Link endpoint, an in-house MLX sidecar (`crates/goose-sidecar`, a supervised Rapid-MLX process the app mounts from its Goose Swarm hub), and cloud nodes. Each device is judged by its own engine's probe; a sidecar that cannot answer or cannot mount is named in `run.jsonl` and leaves the pool by name. On an all-LM-Studio pool this layer is inert, which is how the golden run's behaviour is preserved.

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

**sb-7** (the current specification: a payments sync from a vendor API into a local store, served on a web page, scored by execution across seven tiers). The golden run, engine `393a99351`, three local nodes:

| entrant | score | wall | tasks |
|---|---|---|---|
| **3-node local fleet (r6h, the engine that ships)** | **0.4616** | 507 min | 10/10 done, 0 failed, 0 retries |

Two later engine iterations (r6i, r6j) regressed to 0.1112 on the same specification and were removed from `main`; their commits live only under `archive/*` tags. sb-7 is a harder tier than the sb-5.x results below, so the numbers are not comparable across eras.

The public board at [leanzero.net/agentic-benchmarks](https://leanzero.net/agentic-benchmarks) carries frozen Anthropic cloud baselines and the fleet's published entries, organized by scorer era — the board's selector defaults to the newest scorer and keeps every older era viewable as a frozen historic board.

**sb-5.3** (the current scorer — "rendered means seen": a table row counts only if the browser would actually paint it; the sb-5.2 numbers below were measured before that correction and are kept as their own era):

| entrant | score | tiers (A/B/C/D) | wall |
|---|---|---|---|
| Claude Opus 5 (baseline) | 0.9142 | 1.00 / 1.00 / 0.90 / 0.88 | 20 min |
| **3-node local fleet** | **0.93** | 1.00 / 1.00 / 1.00 / 0.94 | 190 min |
| Claude Sonnet 5 (baseline) | 0.4971 | 1.00 / 0.44 / 0.30 / 0.64 | 7 min |
| Claude Haiku 4.5 (baseline) | 0.4615 | 0.83 / 0.37 / 0.10 / 0.63 | 5 min |

Read plainly: on the honest ruler the 3-node local fleet lands level with Opus — 0.93 against a baseline of 0.9142, inside Opus's own repeat spread (0.889–0.960 across reps), so the honest claim is parity, not victory — and roughly doubles Sonnet — the frontier gap on this task is, measured, four thousandths of a point, bought with a ~10× wall clock. The Sonnet and Haiku collapses are not typos: their builds ship frontends whose data the old probe credited while hidden behind error states, and the corrected probe prices that truthfully — the same correction that exposed (and then repaired) the fleet's own frontend defects.

**sb-6** (the hard tier — "VendorSync Pro": raw-WebGL 3D visualization graded by analytic pixel recomputation, HMAC webhooks, optimistic concurrency, DST-crossing money math; the hand-written golden reference passes the freeze gate at 100%; the scorer stays versioned sb-6.0 until declared stable). All entrants are scored serially and hermetically at the vendor port their spec advertised — the first-night provisional numbers were re-measured after three probe defects and a scoring-contention artifact were found and fixed the same night:

| entrant | score | run |
|---|---|---|
| GPT-5.6 Sol (baseline) | 0.9956 | clean |
| Claude Opus 5 (baseline) | 0.9307 | clean |
| GPT-5.6 Luna (baseline) | 0.8671 | floor† |
| Claude Sonnet 5 (baseline) | 0.8635 | clean |
| GPT-5.6 Terra (baseline) | 0.3252 | floor† |
| Claude Haiku 4.5 (baseline) | 0.1887 | clean |
| **3-node local fleet** | **0.1837** | clean |

† Luna's and Terra's sessions were cut early by a goose engine defect (compaction re-roled the summarizer's reasoning content into a user message, which Bedrock rejects — every GPT session died at its first compaction; fixed in `19b4ed6ef`). Their scores are floors, not ceilings, and are disclosed as such on the board.

The hard tier does what it was built to do: it separates the field the soft tier compressed — the spread runs 0.003 to 0.996. Scores are composed as `(0.88 × core + 0.12 × excellence) × critical`, where the excellence slice unlocks per named perfection condition and the critical multiplier compounds for every measured crash, wrong-money, data-loss, or dead-primary-flow defect (a monotonicity selftest in the freeze gate proves severity ordering can never invert). Terra's 0.33 is the model in action: a flawless backend whose frontend shows users zero rows is priced as the dead product a user would experience, not the good API a curl would see. The fleet's build ships a working backend (full 1553-payment sync, sub-2ms API latencies, rows rendering at 21 ms) but scores 0.05 on the 3D tier — the WebGL panel is where the 27B class currently breaks, and that is now measured instead of hidden.

**sb-5.2** (historic era, frozen): Opus 0.9755 · Sonnet 0.9692 · Haiku 0.7861 · fleet (mighty-crane-54f2) 0.6618. The era's compression at the top was measured to be an instrument artifact — 42.8 of 100 points passed for ≥90% of serious builds — which is what drove both the sb-5.3 correction and the sb-6 hard tier above.

Two findings frame these numbers honestly:

- **The swarm's advantage is its machinery, not its parallelism.** Measured mechanisms that make three nodes faster exist (parallel research, parallel planning, execute-phase parallelism); measured mechanisms that make three nodes *better* are exactly the verification set — the research fan and shard verification spending spare nodes on checking, the repair tail converting reproduced findings, the gates blocking green. When those mechanisms are off or broken, the node-count advantage disappears into a sync-bug lottery.
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

Most people want the notarized build from the [Releases](https://github.com/leanzero-srl/goose-local-edition/releases) page. To build it yourself:

```bash
cd ui/desktop && pnpm install && pnpm run package     # unsigned local package in ui/desktop/out/
just release-notarized 2.0.4                          # Developer-ID signed + Apple-notarized DMG (see ui/desktop/NOTARIZATION.md)
```

`just run-ui` (the Vite dev server) is broken in this fork; package and drive the app over CDP (`ui/desktop/scripts/cdp-probe.mjs`) instead.

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
| `GOOSE_SWARM_PROBE_ADVERTISED_POST=1` | vendor-truth probing of advertised mutating endpoints |
| `GOOSE_SWARM_RENDER_PROBE=<probe.mjs>`, `GOOSE_SWARM_RENDER_NODE=<node>` | the render gate's headless-browser probe |
| `GOOSE_SWARM_SHIP_BEST=1` | end the run on the best verified tree, never the last edit |
| `GOOSE_SWARM_DIVERSE_PLAN=1` | skip redraft ladders the engine's own shadow marks unnecessary |
| `GOOSE_SWARM_TESTGEN=1` | idle-node test generation during tails |
| `GOOSE_SWARM_TEMP=0.2` | impose low temperature on single-sample paths |

Most mechanisms above are on by default in the shipped configuration; the levers exist so any of them can be A/B-tested off. The full set (several dozen) is documented at each `default_*` function in `crates/goose-cli/src/commands/swarm.rs`. Retired and read by the engine nowhere — listed under `retired_levers` in the run's `levers_resolved` event, each row `{reason, configured}` (the reason says the mechanism is dead; `configured` is the value a stale pin in config.yaml or the env still names, `null` when nothing names it), so a stale pin is visible rather than certified or silently ignored: `GOOSE_SWARM_FIX_SCHED` (the fix scheduler died in P1-9) and `GOOSE_SWARM_SPLIT_FAT` (`split_fat_modules` is test-only since b0dd68eac; the config default is false since r6e).

## Measurement discipline

The campaign that produced this fork is recorded in `evals/swarm-bench/nodeloop/FINDINGS.md` — a numbered ledger running F1 (2026-08-01) through F873 (2026-08-17). The ledger is the project's method as much as its record:

- **Every claim gets a number and a falsifier.** Findings register predictions before the run that tests them, and the outcome is written next to the prediction — including `FALSIFIED`.
- **Adversarial review is scheduled, not incidental.** Red-team waves re-derive the day's biggest claims from raw logs; the current ledger file carries 74 explicit refutation marks (`RETRACTED`, `OVERTURNED`, `REFUTED`, `FALSIFIED`). Overturned headlines stay in the file, struck and explained, because a corrected record is the only kind that can be trusted. Notable self-corrections include: a "the campaign produced zero data" claim overturned by finding 47% of real runs missing from a survivorship-biased corpus; a task-split harm claim reversed by a corrected join; and a planning-collapse headline reduced from 3.20× to 1.63× once a confounding redraft round was accounted for.
- **The grader is trusted only under controls.** Run-twice determinism, defect-isolation checks (inject an unstyled page, prove only the visual checks drop), and positive controls on every sweep. Two grader flaws were caught by controls before anything was believed (F4).
- **Ground truth is external where possible.** Node counts are oracled against LM Studio's own activity records, independent of the engine's self-report. Phantom rows (killed engines, corpses of watchdog restarts) are detected by artifact signature and refused.
- **A negative that authorises action must be proven on the same object.** Empty results, zero counts and dead probes are treated as claims about the instrument until shown otherwise — several of the campaign's worst errors were gates believing their own blindness.

The harness enforcing this lives in `evals/swarm-bench/`: the sb-5.2 scorer and its controls (`bench/`), the run supervisor and rolling sweep (`nodeloop/`), the product contract shared with the website (`PRODUCT-CONTRACT.md`), and the product-tier design record (`nodeloop/PRODUCT-TIER.md`).

## Releases

Every macOS release is signed with the LeanZero Developer ID and notarized by Apple in one command (`just release-notarized <version>`); the procedure and the proof it prints live in `ui/desktop/NOTARIZATION.md`.

| version | date | what |
|---|---|---|
| [v2.0.3](https://github.com/leanzero-srl/goose-local-edition/releases/tag/v2.0.3) | 2026-09-05 | First notarized build. The swarm engine is the r6h golden (commit 393a99351, sb-7 0.4616) with the multi-engine layer; Goose Swarm naming; MLX sidecar on Rapid-MLX v0.13.4-lz.1 with the launcher migration; the Swarm provider's idle-node router and the provider allow-list; LeanZero Link mesh. |

The app updates itself from this repository's releases (`latest-mac.yml` + `Goose.zip` ride each release).

## Relationship to upstream

The fork point is `a0aed81f3` (upstream main, 2026-07-03). Upstream goose has continued to move; this fork tracks it deliberately rather than continuously — upstream changes are ingested at chosen boundaries so that the benchmark binary under measurement stays frozen. Upstream-facing surfaces (providers, ACP, MCP, the desktop chat experience) are intentionally left close to upstream to keep that ingestion tractable.

Nothing in the fork phones home. Benchmark publishing is explicit, opt-in, per run, and pseudonymous.

## License

Apache 2.0, unchanged from upstream. See `LICENSE`.

Upstream goose's own README is preserved verbatim at [`README-upstream.md`](README-upstream.md).
