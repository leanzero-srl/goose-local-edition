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
| a judge look hung | `judge_look_dispatched` with no matching `judge_look` for that task_id |

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
- [~] **B. Virtual nodes — PARTLY DONE.** Engine: SwarmDevice gains speed_weight+supervision, threaded into DeviceCfg and carried across pool rebuild (d4fdbeeb2). UI: per-node Smartest/supervisor control shipped. Cloud-node-per-provider already existed (CloudPane). REMAINING: unified Node A/B/C list with `+`, provider picker per node in one place. Original ask: — a Node is a slot that picks a PROVIDER + MODEL.
      Settings > Swarm > Nodes. Node A picks a provider; LM Studio populates from loaded models; cloud
      providers must be configured first to appear. `+` adds Node B, C… Each node independently chooses.
      Per-node role hints: which is FASTEST, which is SMARTEST. Engine must consume the role hints.
- [ ] **C. UI improvements** — LeanZero palette pass on the swarm panel; known-active-bugs panel; phase
      chips read the engine `phase` event.
- [ ] **D. Dead-code sweep** — 79 dead-code warnings exist at HEAD in `swarm.rs` (old planning path:
      SCOUT_LENSES, plan_agreement, consensus_backbone, fan_verify_split, …). The clippy gate has been
      passing on STALE per-crate cache. Delete bottom-up: methods/fns first to fixpoint, then structs.
      A broken compile must never read as "no warnings".
- [x] **E. Coverage — DONE (4411fff13).** Enumerate-then-prove: the component->owner table IS the output, an owner must quote the slice's own words, `coverage_enumerated` logs the table. Old text: Fanned coverage got 10 -> 13 slices but named components stayed
      2/11. Next fix: each shard must ENUMERATE its portion's components first, then match against the
      slice list — two steps in one call. Generic slice names (`api-backend`) absorb everything.
- [ ] **F. Rebuild + relaunch the local run**, then tick every 10 min.
- [ ] **G. Fan REVIEW across the fleet.** It reads the 54KB spec + the whole plan in ONE call — the same
      volume ceiling that made single-call coverage useless. Same fix shape as `cover_slices_fanned`.
- [x] **H. DONE (612cbd4cb).** Seen twice: the nudge was literally "Check the
      slice list against the request section by section" — the job restated, costing a restream. The judge
      must add information or return OK.
- [ ] **I. Semantic de-duplication of findings** (`merge_duplicate_findings`) — designed, never shipped.
- [ ] **J. Node re-admission covers BUILD only.** RESEARCH/TEST/FIX hold pre-BUILD fleet snapshots, so a
      node that comes back mid-run (gabee) sits idle for those phases.
- [ ] **K. Judge-supplied ETA** — the judge estimates remaining time; surface it in the panel.
- [ ] **L. Everything renders "unverified".** There must be a real transition to verified when the engine
      has evidence; today no event ever promotes it.

## Evidence worth acting on

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
