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

## TICK HYGIENE — do not let the tick lie to you

- **`pgrep -f run_build.py` MATCHES THE SHELL RUNNING THE TICK.** The tick command contains that string,
  so pgrep finds its own zsh and reports `run_build RUNNING` when the harness is dead. Observed 12:44Z
  with a 3/3 IDLE fleet and the run dir already archived. Use `pgrep -f 'Python.*run_build\.py'` — anchor
  on the INTERPRETER, so only a real harness process matches.
- Same shape as every other defect this week: a check that answers a neighbouring question. Anchor every
  process check on something the observer itself cannot contain.

## Kill checkpoints — DELIBERATELY NARROW. Slowness is NOT a kill.

Rewritten 2026-08-28 after counting the kills. Nine runs died; where they died:

    open x2 · research x2 · synthesis x1 · review x4 · integrate x1 · rate x1

Two of those got a long way — `undercovered-0738` reached RATE, meaning it went through BUILD, INTEGRATE
and TEST into REPAIR and produced a scored app (0.0023). `nudge-loop-2257` reached INTEGRATE. So the
pipeline DOES run end to end. It was killed for PLAN QUALITY, not for hanging — and plan quality is the
thing the coverage rewrite has now fixed.

One kill was simply WRONG: `review-3h-silent-1036` was healthy, and I killed it on a local-time-minus-UTC
subtraction that invented a three-hour gap.

So the posture changes. A kill costs hours and cannot be undone; being slow costs only time and often ends
in a finished run. KILL ONLY ON A PROVEN WEDGE OR A PROVEN DEFECT:

| kill when | proof required — all of it, not one snapshot |
|---|---|
| the engine is wedged | NO new event in `run.jsonl` AND no `.swarm/activity/*.json` mtime movement, sampled >=3 times over >=90s, AND `lms ps` shows the fleet idle |
| a lane died after a re-stream | its digest has `thinking_chars: null` or frozen for >5 min AND its last judge verdict was drifting/looping AND no other lane is still feeding the fan |
| a correction is a full re-emission | `plan_patched` absent where the plan changed |
| the join is misnamed or owns files | `plan_loaded.tasks[]` |
| a task dispatches with a one-line description | `plan_loaded.tasks[].description` length |
| anything is stopped by a clock | any `agent stalled` text |
| a judge look genuinely hung | `judge_look_dispatched` with NEITHER `judge_look` NOR `judge_look_abandoned`, paired PER TASK_ID |

**NOT kill conditions, and each of these has caused a wrong kill:** a phase taking a long time; nodes idle
while a fanned straggler finishes (that is what a fan looks like); a judge probe outstanding (it is raced
against the stream now, and abandoned looks are normal); a plan that looks under-covered mid-run — coverage
runs several rounds and the gaps arrive in the later ones.

**Before any kill, state the UTC clock explicitly.** `date -u`, and compare UTC to UTC.

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
- [x] **C. DONE — all three sub-items were ALREADY BUILT; I verified rather than assumed, and nearly
      shipped a duplicate.** (a) known-active-bugs panel: exists at SwarmRunPanel.tsx:2668, labelled
      "the run passed — these are what it passed WITH". I added a SECOND one in error red; the existing
      smoke test caught the duplicate ("Found multiple elements with the text: Known active bugs") and I
      reverted it (d264c278c reverted). (b) phase chips read the engine `phase` event via `foldRunPhase`,
      with a test named "the ribbon reads the engine, never a label" — no regex-on-label path remains.
      (c) LeanZero palette: ZONE_HUES and FORMATION_RAMP already carry #1d4ed8 / #0891b2 / #7c3aed /
      #d97706 / #dc2626 / #db2777. Was: — LeanZero palette pass on the swarm panel; known-active-bugs panel; phase
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
- [x] **I. DONE.** The rater can answer `DUPLICATE <n>`; the engine drops those from the wave. Model decides — no similarity score, no threshold. Backward-only, and `forced` engine checks still win. Was: (`merge_duplicate_findings`) — designed, never shipped.
- [x] **J. DONE.** `DeviceCfg.is_cloud` (54a06a882) then `live_fleet_slots` — fans drop devices LM Studio no longer serves, and never residency-check a cloud device. Falls back to the boot snapshot on any doubt. KNOWN LIMIT: drops the departed, does not re-add a node absent at boot (needs cfg, not the pool). Was: Confirmed real: `worker_models`
      is `fleet_slot_models(devices)` computed ONCE at swarm.rs:26951 (planning) and again at :38053
      (repair), both from the boot pool. Only BUILD sees a rejoin, via the scheduler's `DeviceAdmission`.
      So a machine that returns mid-run sits idle through RESEARCH/REVIEW/TEST/FIX however long they take.
      THE OBVIOUS FIX IS WRONG: filtering the slot list by LM Studio residency drops every CLOUD device,
      because a cloud model never appears in `lms ps` — and cloud nodes are now a supported fleet.
      `DeviceCfg` has no provider field to tell them apart (SwarmDevice does; it is not threaded through).
      PREREQUISITE: put the provider on DeviceCfg, then residency-filter LOCAL devices only. Confidence in
      the diagnosis HIGH, in a safe fix without that prerequisite LOW. Was: RESEARCH/TEST/FIX hold pre-BUILD fleet snapshots, so a
      node that comes back mid-run (gabee) sits idle for those phases.
- [x] **K. DONE.** Judge's `eta_mins` now surfaces per running lane (latest wins). The run-level band stays arithmetic and is honest about being an extrapolation. Was: — the judge estimates remaining time; surface it in the panel.
- [x] **N. DONE.** `cut_request_into_portions` cuts by character count; deliberately NOT named 'weight' (four other meanings exist). Was: Measured: part 1 got 72
      components, part 3 got 9; one lane ran 25+ min while two sat idle. Cut on section headings, or
      rebalance by character weight — the same "no slice more than ~2x another" rule OPEN applies to slices.
- [x] **L. DONE (as far as is honest).** Rows now say "built — the app has not been run yet; verified end-to-end after Repair". NO earlier trigger exists: the `smoke` gate is superseded by GOOSE_SWARM_COMPLETE and has never fired on any run; the panel also drops its findings list so it could not tell pass from fail. Promotion stays complete_result.passed && verified — engine truth, not a model claim. The board has never flipped green because no run has finished, not because the rule is wrong. Was: There must be a real transition to verified when the engine
      has evidence; today no event ever promotes it.

## Evidence worth acting on

- **THE CLARIFY-PROXY RACE FIX IS CONFIRMED IN PRODUCTION (13:23:40-13:25:20Z).**
  ask 13:23:40 -> proxy armed + question asked 13:23:40 -> answered 13:25:19 -> low_confidence_answered
  13:25:20 -> research 13:25:20. The run WAITED 99 SECONDS for the proxy and NO
  `low_confidence_ask_timeout` was emitted, so the answers reached the plan.
  Compare the defect: reader gave up at 5s, proxy answered 42s LATER, plan built from guesses while the
  log said a node had answered. The exit condition is the proxy TASK now, not a clock, and it holds.

- **BEST OPEN YET (13:23:40Z, 1982s).** 139 components enumerated across parts of 25/57/57 with **0
  unowned in every part**, and 12 slices that are the request's own components: ledgerd-core, vendor-sync,
  event-ledger-outbox, webhook-handling, approval-workflow, notifierd-service, ledgerd-api,
  frontend-structure-style, frontend-behavior, decisions-readme, frontend-3d-rendering,
  frontend-3d-interaction. No coverage_gap was needed — OPEN got it right first time.
  - The BALANCED SPLITTER is confirmed: 25/57/57 against the previous run's 72/9.
  - Judge health: 2 nudges, 3 abandoned probes for the whole phase, against 213 abandoned on ONE lane
    two runs ago.
  - Cost: OPEN 33 min against 20 min, for 139 enumerated components against 49. Worth it.

- **LENS 3, measured this run — one fix works, one did not.** Abandoned judge probes: 213 on one lane
  before, 1 worst-lane / 2 total now — the storm fix WORKS. But coverage lanes were still judged `looping`
  at produced_since_last_look 4000 and 4001, because the recurrence sentence asserted "a healthy advancing
  call measures under 5%" — a prose threshold applied to a TABLE, which measures high by construction. A
  model choosing between a deterministic detector with a stated healthy range and a paragraph of
  reassurance believes the detector. Fixed by removing the verdict from the measurement and pointing the
  judge at whether the repeats ADVANCE.

- **RUN KILLED 12:40Z — `-KILLED-review-cannot-converge-1240` — and the reason is a fixed defect not yet
  in the binary.** REVIEW rounds 1 and 2 each reported the SAME defects in different words and both logged
  `repeated: 0`, because the cross-round de-dupe is a 120-char lowercase PREFIX comparison. The stop rule
  is "a round with no NEW finding", so a reviewer that rephrases can never satisfy it. Round 3 ran 19
  minutes. Verified with a positive control that none of the four completion-critical fixes were in the
  running binary: ALREADY REPORTED, DUPLICATE <n>, the fix worker's real done-condition, and the
  advertised surface. Letting it run would have re-proved the old REPAIR does not finish — which nine
  runs already established.
- **WHAT THAT RUN PROVED BEFORE IT DIED, and it is a lot:** 13 correct slices naming the request's own
  components; coverage 49 components / 0 unowned; all 13 briefs substantial (3,530-23,059 chars);
  SYNTHESIS produced a DAG; the fanned REVIEW ran on 3 nodes; judge probe accounting fully balanced. Every
  phase before REPAIR now works. The remaining unknown is BUILD onward.

- **RESEARCH BRIEFS ARE SUBSTANTIAL (11:58:09Z, 1564s).** brief_chars = [5073, 5808, 6995, 5971, 15292,
  8110, 5838, 7123, 5512, 11256, 3530, 23059, 7442] — all 13, min 3,530. The historical failure was a
  122-char brief scoring 42.7% where a 1,497-char spec scored 88.7%. The DETAIL-fan deletion is vindicated.
- **REPAIR'S FOUR DEFECTS, all pre-existing, all fixed in source (0a5d4e8c1 + the spec fix):** the fix
  worker's done-condition was already true before it started (verify_recipe cannot see TEST-fan, DOM, CSS,
  HTTP or spec-contract findings); criticals were computed then went OUT OF SCOPE before the wave; the
  benchmark proxy bought rounds forever; and the fix worker never saw the request. None needed a cap.

- **THE DECOMPOSITION PROBLEM IS SOLVED (11:29:59Z, first verified sb-7 run).** 13 slices in 1181s:
  boot-contract, vendor-sync, **event-ledger-outbox**, **webhook-handling**, **approval-workflow**,
  ledgerd-api, **notifierd-service**, frontend-html, frontend-css, frontend-app-js, **decisions-docs**,
  **viz-rendering-engine**, **viz-interaction**. Those are the REQUEST'S components, not program layers.
  Every previous run scored 2 of 11 named components; this is 9+. Coverage enumerated 49 across three
  parts with 0 unowned. This is what the enumerate-and-prove rewrite was for.
- **MY OWN DEFECT, found the same tick: 213 abandoned judge looks on one lane.** open-coverage-2
  dispatched 218 and abandoned 213, looks 2-214 over 838s. Once a stream ends mid-probe the loop drains
  its deferred events, and the judge trigger — which sits at the TOP of that loop — fired again on each
  one, dispatching a fresh probe against an ended stream. Correctness was fine (abandoning is the right
  behaviour) but it is 213 started-and-dropped model calls, scaling with the deferred backlog rather than
  with anything real. Fixed by teaching the trigger that an ended call is not worth looking at.

- **THE COVERAGE-OBJECTIVE FIX WORKS (measured 11:19-11:24Z).** BEFORE it, `looping` fired on coverage
  lanes at `produced_since_last_look=4003` — calls producing healthily, judged as looping because the
  table's rows repeat. AFTER it, looping fires only at `produced=1,2,3` — a genuine stall — and the
  re-stream RECOVERS the lane (coverage-1: think 2002 stalled -> restream -> 4002 produced -> 8005). The
  judge also carries `established` forward, so the replacement stream resumes rather than restarting
  blind. The judge is now right when it acts, which is the whole point.
- **OPEN LANES STALL NEAR ROUND NUMBERS — worth watching, not yet explained.** Stalls observed at
  think=2001, 2002, 4400, 4403; recoveries then run to exactly 4002/4003/8005. Repeated stalls landing on
  ~2k/~4k boundaries look like a provider-side buffer or chunk edge rather than model behaviour. If this
  recurs, splice the stream and compare against LM Studio's own chunking before blaming the engine.

- **VERIFIED sb-7 RUN LIVE from 11:10Z** — the first correct local run of the day. prompt 53,634 chars,
  "# Build `app` — Meridian Payments Console", all of ledgerd/notifierd/viz.js/DECISIONS.md/outbox/12,288
  present, BENCH_SPEC=spec-build-sb7.md, vendor 200, secrets store=file, benchmark=true.
  `bench_dispatch.mjs` now asserts these markers itself and exits 6 rather than reporting success.

- **THE BENCHMARK VIEW COULD NEVER RUN sb-7 (fixed 80ccfe548).** `benchmark-run` set `BENCH_SPEC` with no
  sb-7 branch, and `build_prompt` reads BENCH_SPEC BEFORE the regime — so selecting sb-7 ran the sb-5
  spec. The 10:44Z run received 6,278 chars of "# Build `vendorsync`" instead of 54,146 chars of Meridian,
  and looked healthy: 9 balanced slices, coverage 0 unowned, clean verdicts. ALWAYS verify the tier from
  `run_started.prompt` (54,146 chars, contains ledgerd/notifierd/viz.js/outbox/12,288), never from the flag.
  `GOOSE_SWARM_RENDER_PROBE` had the identical gap and would have graded Meridian by VendorSync's rules.

- **THE FIRST VALID LOCAL RUN of 2026-08-28 started 10:44Z** through the Benchmark view. All three
  conditions hold and were checked from the run's own events, not the UI: `run_build.py --sb7` running,
  its own vendor answering /v3/docs 200, `secrets_source {store:"file"}`, `levers_resolved
  {benchmark:true}`. Everything before this was a chat with a placeholder spec and no vendor.

- **THE CHAIN THAT KILLED RUN swarm-3node-r0 (archived -KILLED-restream-killed-coverage-lanes-1008).**
  Four links, and each is worth keeping straight:
  1. The new coverage deliverable IS a table — dozens of near-identical rows. The recurrence detector
     reads structural repetition, so it trips on exactly the output the task was redesigned to produce.
  2. The judge returns `drifting` (or `looping`). DRIFTING acts on the FIRST look with no corroboration —
     deliberately, because tail recurrence cannot corroborate "working on the wrong thing".
  3. Delivery is a re-stream, and it could not have been anything else: a coverage lane makes ZERO tool
     calls, so `actions_since_last_look` is 0, so `can_steer` is false. THIS IS A CONSEQUENCE OF MY OWN
     can_steer FIX and it is still the right rule — a pure-reasoning call has no turn boundary for a steer
     to land on until it finishes. For such calls, re-stream is the ONLY delivery.
  4. The re-stream dropped the socket and the new stream produced NOTHING. `open-coverage-1` sat with
     `thinking_chars: null` and a frozen digest for 9.6 minutes; two of three lanes idle, OPEN at 56 min.
  Fix landed for link 1 (fcef6947f: the judge is told repetition is the deliverable here). Links 2-4 are
  untouched and are the next thing to watch: if a healthy coverage lane still gets nudged, the objective
  fix was not enough.
- [x] **O. DONE (4e70ad192).** The judge's readiness floor now reads `thinking_total` (survives the re-stream reset) instead of `thinking_chars`, so a re-streamed lane that produces nothing can still be looked at. Detector `post_restream_silence` ships with it. Was: The lane dies silently — no event, no
      retry, and the fan waits for it forever. Either detect a re-stream that yields nothing and fall
      back to the pre-re-stream partial, or do not re-stream a call that is producing steadily.

- **THE JUDGE RE-STREAMS COVERAGE SHARDS ON A FALSE LOOP READ (fixed in source, needs next build).** The
  coverage table is repetitive by construction, the recurrence detector trips on it, and the re-stream
  delivery discards the partial table and restarts from the top. Three times in one run = 30-minute
  coverage rounds. Fixed by telling the judge what repetition means for that call.
- **Dispatch/return imbalance to re-check next run:** 2,140 `judge_look_dispatched` vs 63 `judge_look` on
  the 0828 run. Partly abandonment (call finished first) but the running binary predates
  `judge_look_abandoned`, so it cannot be attributed yet. CHECK THIS on the next run — if the gap is not
  fully explained by abandonments, there is a second defect.

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

## HOW TO LAUNCH — THROUGH THE BENCHMARK VIEW, NOT A CHAT

    pkill -9 -f 'Goose.app/Contents/MacOS/Goose'
    open -n /Applications/Goose.app --args --remote-debugging-port=9897
    node ~/goose-builds/loop-state/bench_dispatch.mjs 9897 sb-7 3

`benchmark-run` spawns run_build.py --sb7 --timeout 0 with GOOSE_SWARM_BENCHMARK=1; run_build serves the
vendor, builds the fixtures, substitutes the spec placeholders and SCORES at the end. Verify with
`pgrep -fl run_build.py` (must carry --sb7) and `curl 127.0.0.1:8850/v3/docs` (200). Do NOT start a vendor
yourself — run_build owns the port. A benchmark run must NOT appear as a "# Build `app`" chat in the
sidebar; if it does, it is a chat, not a benchmark.

## SUPERSEDED — the chat path, kept only as a warning

`launch.sh` alone is WRONG and produced a full day of void runs: it types the raw spec into the desktop,
so the prompt keeps its literal {BASE_URL} / {DOCS_URL} / {API_KEY} and there is no vendor to sync from.

    cd evals/swarm-bench/bench
    nohup python3 sb7_local_vendor.py --port 8850 \
        --out /tmp/sb7-prompt.md --trace /tmp/sb7-trace.jsonl > /tmp/sb7-vendor.log 2>&1 &
    # it prints {port, seed, prompt, docs_status} — KEEP THE SEED, scoring needs the same one
    ~/goose-builds/loop-state/launch.sh swarm-3node-r0 9897 /tmp/sb7-prompt.md benchmark=true

CHECK BEFORE TRUSTING ANY RUN: the dispatched prompt must contain ZERO `{[A-Z_]+}` placeholders and the
vendor must answer /v3/docs with 200. Both are asserted by the script, and both must be re-checked from
`run_started.prompt` in run.jsonl. The vendor must stay up for the WHOLE run.

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
