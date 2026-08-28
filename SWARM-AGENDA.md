# SWARM AGENDA — the live source of truth

Read this at EVERY tick. Do not re-derive it from context; context compacts and the agenda gets lost.
That already happened once and cost a whole night: the run was killed and nothing continued.

## The goal

Make the swarm BUILD BETTER SOFTWARE on local models, then beat the published `brun-fleet-qwen38-brainwaves-sb70`
(0.0273) on leanzero.net. Numbers follow from the product, not the other way round.

## Standing constraints — absolute, never negotiate them

- **NO deterministic caps, timers, thresholds or gates.** A clock may SUMMON the judge or SUGGEST to a
  worker; it may NEVER CUT one. No new literal seconds constant as a mechanism, ever.
- **Never reconfigure the fleet** on my own initiative. `lms ps` + `pgrep` before ANY run.
- **Desktop app only** for local runs. No headless.
- **Kill on divergence** — never let a diverged run finish. Then make a DIFFERENT fix, never the same one.
- **Tick every 10 minutes, without exception**, for as long as anything is running.
- Temperature: leave it unset so the user's 0.7 applies. Never pin 0.2.
- UI: no left accent rail, no faded/washed-out colours, no native `<select>`/`alert`/`confirm`/`prompt`.
  LeanZero palette. Solid saturated accents.
- Commit EVERY change as it lands.

## Kill checkpoints — stop the local run the moment one trips

| checkpoint | read from |
|---|---|
| no engine event for > 20 min while a call is live | newest `run.jsonl` mtime vs `.swarm/activity/*.json` |
| REVIEW past round 3 still surfacing new findings | `review_findings.new > 0` at `round >= 3` |
| a phase runs > 45 min with 2+ nodes IDLE | `phase` event age + `lms ps` |
| a correction is a full re-emission, not a patch | `plan_patched` absent where a plan changed |
| the join task is not named `integrate-verify`, or owns files | `plan_loaded.tasks[]` |
| any task dispatches with a one-line description | `plan_loaded.tasks[].description` length |
| anything at all is stopped by a clock | any `agent stalled` text |
| a judge look hung | `judge_look_dispatched` with NEITHER a `judge_look` NOR a `judge_look_abandoned` for that task_id. Pair PER TASK_ID — global-order pairing falsely flags three tasks at once. An abandoned look is the design working (the call finished first), not a hang. |

## DONE (committed on `local-edition`)

- [x] `010f43fe1` clarify proxy race — answers were computed, logged as success, thrown away
- [x] `845b35713` deleted the three 420s stopwatch verdicts + tail-review cap (were parked behind `&& false`)
- [x] `ad0b1d0ea` Synthesize row stops reading as live when Review opens (was a permanent display lie)
- [x] coverage fans across the fleet (`4e5cec44d`)
- [x] GLM-5.3-Flash cloud campaign launched and publishing (see below)

## TODO — in order. Check items off IN THIS FILE as they land.

- [x] **A. REVIEW wedge — FIXED.** Root cause: the judge's own model call was `.await`ed serially inside
      the stream loop it supervises, and `judge_look` is emitted AFTER it, so a hung supervisor froze the
      worker and logged nothing. The probe now races the stream; deferred events drain in order; a call
      that finishes while the judge is out abandons the look, not the result. Detector shipped:
      `judge_look_dispatched` before the call — a dispatched with no matching `judge_look` = hung judge.
- [x] **B. Virtual nodes — DONE.** Node-first unified list (Node A/B/C, provider chip per node, union of local+cloud — 02c79ac77), per-node Smartest/supervisor (67b431faf), '+ Add node' picker, engine fields carried across pool rebuild (d4fdbeeb2). Earlier note: Engine: SwarmDevice gains speed_weight+supervision, threaded into DeviceCfg and carried across pool rebuild (d4fdbeeb2). UI: per-node Smartest/supervisor control shipped. Cloud-node-per-provider already existed (CloudPane). REMAINING: unified Node A/B/C list with `+`, provider picker per node in one place. Original ask: — a Node is a slot that picks a PROVIDER + MODEL.
      Settings > Swarm > Nodes. Node A picks a provider; LM Studio populates from loaded models; cloud
      providers must be configured first to appear. `+` adds Node B, C… Each node independently chooses.
      Per-node role hints: which is FASTEST, which is SMARTEST. Engine must consume the role hints.
- [ ] **C. UI improvements** — LeanZero palette pass on the swarm panel; known-active-bugs panel; phase
      chips read the engine `phase` event.
- [ ] **D. Dead-code sweep — DEFERRED UNTIL NO LOCAL RUN IS LIVE.** `cargo clippy` starves the fleet:
      with the sweep running, judge probe latency went 16s -> 18s -> 27s -> **117s**, clippy-driver at
      55%+30% CPU. I was degrading my own run to tidy code. Ordering also matters and cost one aborted
      pass: delete FUNCTIONS to fixpoint, THEN structs/enums, THEN consts last — a const deleted while a
      dead function still references it breaks the build. Script: scratchpad/sweep3.sh (builds after each
      pass, `git checkout` on failure, so a broken compile can never read as "no warnings left").
      Original: — 79 dead-code warnings exist at HEAD in `swarm.rs` (old planning path:
      SCOUT_LENSES, plan_agreement, consensus_backbone, fan_verify_split, …). The clippy gate has been
      passing on STALE per-crate cache. Delete bottom-up: methods/fns first to fixpoint, then structs.
      A broken compile must never read as "no warnings".
- [x] **E. Coverage — DONE (4411fff13).** Enumerate-then-prove: the component->owner table IS the output, an owner must quote the slice's own words, `coverage_enumerated` logs the table. Old text: Fanned coverage got 10 -> 13 slices but named components stayed
      2/11. Next fix: each shard must ENUMERATE its portion's components first, then match against the
      slice list — two steps in one call. Generic slice names (`api-backend`) absorb everything.
- [x] **F. DONE 12:11.** Run `swarm-3node-r0` live on the FRESH engine (build_sha 79e0f6d41).
      TRAP HIT AND CLEARED: the first launch ran `/Applications/Goose.app` from **Jul 28** — a month-old
      engine, proven by three missing strings with a positive control. Always verify the INSTALLED binary,
      not the built one. `just make-ui` writes to ui/desktop/out; /Applications must be replaced separately.
- [x] **M. DONE.** `benchmark` now emitted in `levers_resolved`. Was: It is read from config.yaml (SwarmConfig.benchmark) but is
      absent from the `levers_resolved` event, so the log cannot show whether a run was unattended. Add it.
      Functional proof meanwhile: `clarify_proxy_armed {mode:"immediate", wait_secs:0}` at ASK.
- [x] **G. DONE.** `review_plan_fanned` — one portion per lane against the whole plan, patches union with per-id dedupe, lanes<=1 falls through to the old path. Needs the next build to take effect. Was: It reads the 54KB spec + the whole plan in ONE call — the same
      volume ceiling that made single-call coverage useless. Same fix shape as `cover_slices_fanned`.
- [x] **H. DONE (612cbd4cb).** Seen twice: the nudge was literally "Check the
      slice list against the request section by section" — the job restated, costing a restream. The judge
      must add information or return OK.
- [ ] **I. Semantic de-duplication of findings** (`merge_duplicate_findings`) — designed, never shipped.
- [ ] **J. Node re-admission covers BUILD only.** RESEARCH/TEST/FIX hold pre-BUILD fleet snapshots, so a
      node that comes back mid-run (gabee) sits idle for those phases.
- [x] **K. DONE.** Judge's `eta_mins` now surfaces per running lane (latest wins). The run-level band stays arithmetic and is honest about being an extrapolation. Was: — the judge estimates remaining time; surface it in the panel.
- [x] **N. DONE.** `cut_request_into_portions` cuts by character count; deliberately NOT named 'weight' (four other meanings exist). Was: Measured: part 1 got 72
      components, part 3 got 9; one lane ran 25+ min while two sat idle. Cut on section headings, or
      rebalance by character weight — the same "no slice more than ~2x another" rule OPEN applies to slices.
- [x] **L. DONE (as far as is honest).** Rows now say "built — the app has not been run yet; verified end-to-end after Repair". NO earlier trigger exists: the `smoke` gate is superseded by GOOSE_SWARM_COMPLETE and has never fired on any run; the panel also drops its findings list so it could not tell pass from fail. Promotion stays complete_result.passed && verified — engine truth, not a model claim. The board has never flipped green because no run has finished, not because the rule is wrong. Was: There must be a real transition to verified when the engine
      has evidence; today no event ever promotes it.

## Evidence worth acting on

- **COVERAGE IS ENUMERATING PROPERLY NOW (2026-08-28 run).** part 1/3 -> **72 components, 1 unowned**;
  part 3/3 -> 9 components, 0 unowned. Previous runs found 1-5 gaps IN TOTAL. The table is in the request's
  own vocabulary (`vs7dbg.pickPixel`, `#viz-labels`, `GET /api/stream`, `DECISIONS.md`) and owners are
  specific slices (`viz-interaction`, `viz-rendering`, `decisions-doc`) rather than layers.
- **BUT THE SPLIT IS BADLY UNBALANCED:** 72 components in part 1 against 9 in part 3, because the fan cuts
  by PARAGRAPH COUNT, not by content weight. One lane grinds while two idle. Same defect shape as the
  slice-balance rule OPEN already applies to itself. -> new item N.

- **THE WEDGE FIX IS CONFIRMED LIVE (2026-08-28 09:16-09:18Z).** `open` judge look 4 was dispatched at
  09:16:30 and never returned; the phase advanced to `open-resplit` at 09:18:01 regardless. The OPEN call
  finished while its probe was still out and the "abandon the look, not the result" path fired. Under the
  old serial `.await` that probe would have blocked the loop and OPEN could not have completed. Measured
  probe latencies this run: 16s, 18s, 27s, and one >117s — every one of those was previously time the
  supervised worker spent frozen.
- **The re-stream baseline reset is also confirmed:** `open-resplit` look 1 reported
  `produced_since_last_look: 2002` against `thinking_chars: 2002`. Before the fix this reported 0, which
  the rate block hands to the judge as "a DEAD STREAM ... Say LOOPING" — the symptom the engine invented.

- **GLM-5.3-flash (cloud) is producing what the local swarm never has**: `DECISIONS.md`, `notifierd.log`,
  `ledgerd.pid`, `notifier.db`, 95 files. The local coverage failure (named components stuck at 2/11) is
  therefore a DECOMPOSITION defect, not a model-capability ceiling. Compare its plan against ours.

## Commands

```bash
# build + install the desktop app (NEVER run headless)
source bin/activate-hermit && cargo fmt && cargo build --release && just make-ui

# launch a local run
~/goose-builds/loop-state/launch.sh <name> 9897 <spec> benchmark=true

# GLM-5.3-flash cloud run
cd /private/tmp/goose-cloud-launch-glm53flash
python3 evals/swarm-bench/bench/cloud_sb7.py status --root ~/goose-builds/cloud-sb7-glm53flash-20260828-r0
# publishes to brun-baseline-glm-5-3-flash-sb70 ; key ZHIPU_API_KEY in
# ~/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env
```
