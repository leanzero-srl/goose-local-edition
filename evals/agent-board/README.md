# goose Agent Board

A portfolio of named capability benchmarks for agents, run on your own machine, with error bars and
hard-coded cloud baselines to compare against. Not one score — a card per capability.

Nothing here is shared with `evals/swarm-gym`. New fixtures, new probes, new scoring.

## The three rules

1. **The probe never enters the workspace.** The agent gets a prompt and a seed tree. Graders live
   outside and run against a post-run snapshot in a sandbox.
2. **Tamper detection is a published number.** Seeded test files are hashed and RESTORED from
   pristine before grading, so rewriting your own grader gains nothing. A run that edited one is
   flagged `tampered`, scores 0, and feeds the tamper rate.
3. **Ticks, not hours.** A tick is one graded episode — (fixture, entrant, rep). You pick the tick
   budget; that is the time dial. Ticks are emitted in rounds, one per (fixture × entrant) per
   round, shuffled from a fixed seed. Abort after round *k* and you have exactly *k* reps of
   everything — a half-finished board is still a balanced board.

## Running it

```bash
cd evals/agent-board

# what would run, without running it
python3 runner/board.py --dry-run

# the time dial: stop after N ticks
python3 runner/board.py --ticks 12

# one entrant, one vertical
python3 runner/board.py --entrant haiku-4.5 --vertical repair

# a single episode
python3 runner/episode.py --fixture verticals/repair/fixtures/ledgerfold-hard \
  --target single --label opus-5 --provider aws_bedrock \
  --model us.anthropic.claude-opus-5 --env-file ~/.config/agent-board/bedrock.env

# read the results
python3 board/card.py          # the card
python3 board/drift.py         # replicate spread + minimum detectable effect

# trust nothing until the controls pass
python3 probes/repair.py --fixture verticals/repair/fixtures/<name> --self-test
python3 -m pytest probes/test_repair_probe.py board/test_stats.py -q
```

Credentials live **outside the repo** (`~/.config/agent-board/bedrock.env`, mode 600) and are passed
to the child process only. They never enter an episode record. The file is parsed, not sourced —
shell syntax in a value is a hard error, because `${AWS_REGION:-us-east-1}` reaching Bedrock as
literal text once produced a score of 0 that read exactly like a model failing the task.

## Adding a fixture

```
verticals/<vertical>/fixtures/<name>/
  meta.yaml     target_test, protected files, expected control scores, and WHY the defect is fair
  prompt.md     what the agent is told
  seed/         copied into the workspace — this is all the agent sees
  controls/
    reference/        the correct fix          — must score 1.0
    broken_<what>/    a plausible wrong fix    — must score 0.0
```

A fixture is not usable until `--self-test` passes: the reference scores 1.0, every broken control
scores 0.0, and exactly one test fails at seed. The broken controls are the important half — one
that fixes the target while breaking something else is what proves the regression check works.

## What is measured, and what is refused

Every episode records the probe verdict, wall time, exit code, crash/timeout, tamper, and the run's
own **claim** about finishing. Crashes and timeouts score 0 and stay in the denominator.

- **Correctness** comes only from executing the suite. `complete_result.passed`, exit codes and the
  model's closing prose are never evidence — they are only the *claim* side of the honesty card.
- **Honesty** is `NOT COMPUTABLE` for single-agent entrants and says so. Only the swarm emits a
  structured claim; inferring success from prose is the self-report this board refuses.
- **Cost** is absent and labelled absent — goose does not surface token counts to the harness.
- **Ranking** stops where the evidence stops. Entrants whose intervals overlap print TIED and are
  never ordered. Wilson intervals, because at n=5 the normal approximation escapes [0,1].
- **A saturated fixture is named as such.** If every entrant passes, the card says the fixture needs
  a harder rung rather than printing a rep count that implies more runs would help.

## Status

Two verticals, five fixtures, every probe trusted against its controls.

**REPAIR** (binary) — `slugkit-easy`, `shiftlog-medium` (the obvious one-character fix breaks a
passing test), `ledgerfold-hard` (the intuitive fix leaves Decimal and destroys precision past 15
significant digits).

**TEST-WRITING** (continuous, mutation score) — `slugkit-mutants` (6 mutants),
`ledgerfold-mutants` (10 subtle ones across two modules; a happy-path suite kills 3).

### ENGINE vs ENGINE — the comparison that matters

Same engine, same fixture (`slugkit-easy`), same three-worker shape; only the model backend differs.

| | tasks | dispatched | completed | retries | run_finished | wall | score |
|---|---|---|---|---|---|---|---|
| local swarm (qwen 27b x3) | 7 | 11 | 6 | **4** | no | **3600s timeout** | **0.0** |
| cloud swarm (Haiku 4.5 x3) | 7 | 7 | **7** | **0** | yes | **295s** | **1.0** |

**The swarm architecture is not the problem.** Given a fast, reliable model the engine dispatched
exactly its plan, completed every task with zero retries, ran the gates, emitted `complete_result`
and `run_finished`, and delivered a correct fix in under five minutes.

With local models the same engine needed 11 dispatches for 7 tasks, retried 4 times — every one a
`stream decode error (mid-stream body drop)` from the LAN — finished 6 of 7, and never emitted
`run_finished` at all.

That is a different diagnosis from the one the local run alone supported ("the swarm is bad at small
tasks"). It is model throughput and LAN stream stability, not the architecture. Only the
engine-vs-engine run could separate them.

Caveats held: n=1 per side, and the cloud run includes ~40s hand-answering a low-confidence clarify
gate that Claude raised and the local run never did.

### CORRECTNESS SATURATES FOR THE LOCAL FLEET TOO

The local 27b, single-agent, on the hardest fixtures authored:

| fixture | vertical | result | wall |
|---|---|---|---|
| `slugkit-easy` | repair | pass | 877s |
| `ledgerfold-hard` | repair | pass — took the Decimal route, **not** the float trap | 589s |
| `ledgerfold-mutants` | testwrite | **10/10 mutants killed** | 1163s |

It writes the same correct code as Opus 5 and kills every mutant a frontier model kills. The gap is
**time (18-27x), not capability** — on tasks of this size.

### The swarm result

3 nodes, `slugkit-easy`, **timed out at exactly the 3600s cap**: 11 dispatches, 4 retries, one
replan — for a ONE-LINE fix. The artifact was correct (it found `result.rstrip("-")` and never
touched the protected test file); it simply could not stop. haiku-4.5 did the same repair in 31s.

Recorded as `score 0.0`, `artifact_score 1.0`, `scored_zero_for: timeout`, so "got it right, could
not finish" stays legible rather than being flattened into an ordinary failure.

**This reading was wrong and the engine-vs-engine run above corrected it.** From the local run alone
the obvious conclusion was "repair is the wrong shape for a swarm — nothing to parallelise, so
overhead is the whole cost". Then the same engine on Haiku completed the same fixture in 295s with
zero retries. The overhead is not what killed it; local throughput and LAN stream stability are.

Kept here deliberately, because the sequence is the point: a single-arm result supported a confident,
plausible, wrong diagnosis, and only the controlled comparison overturned it.

### What 45 repair episodes actually showed

- **Correctness drift is zero.** `11111` for every entrant on every rung. No flakes, no crashes, no
  tampering. The instrument is perfectly stable — and saturated, so it cannot rank.
- **Time is the only axis with variance**, and it has a lot: replicate CV 23–29%, spread to 105%.
  So a time gap under ~58% is jitter. haiku 32.8s, sonnet 40.2s, opus 51.7s all sit inside it —
  they are the SAME SPEED here, and the card refuses to order them.
- **The one real separation** is local-single at 876.8s, 27× haiku's median. Far outside the floor.

The lesson the board is built around: at the sample sizes anyone will sit through, most differences
are not differences. Saying so is the whole point.

### Running Claude through the SWARM ENGINE (the fair comparison)

Single-agent cloud vs local swarm compares harnesses, not models. To get engine-vs-engine, an
OpenAI-compatible gateway (LiteLLM) fronts Bedrock and the swarm points at it — `swarm.rs:19175`
sets `LMSTUDIO_HOST` from `swarm.endpoint`, and `lmstudio` is an OpenAI-format client, so no engine
change is needed.

```bash
litellm --config ~/.config/agent-board/litellm-bedrock.yaml --port 4000
python3 runner/proxy_fidelity.py            # MUST print GATEWAY TRUSTED first
python3 runner/swarm_profile.py --apply cloud-haiku --nodes 3
GOOSE_SWARM_REQUIRE_SERVABLE=0 python3 runner/episode.py --target swarm --label cloud-swarm-haiku-3node ...
python3 runner/swarm_profile.py --restore   # always
```

Three traps, each of which silently produced a wrong answer before being found:

- **Never round-trip `config.yaml` through pyyaml.** goose reads it with serde_yaml (YAML 1.2) where
  `research_planning: on` is the STRING `"on"`. pyyaml is YAML 1.1, where `on` is a BOOLEAN, so a
  load/dump cycle writes `true`, the whole swarm block fails to deserialise, and the engine falls
  back to BAKED DEFAULTS with no error printed. `swarm_profile.py` edits text surgically instead.
- **A run builds its pool from `lms ps`, not from `config.devices`.** `pool show` reads the config,
  but the run asks the endpoint for the LIVE model ids — so the gateway must answer to those exact
  names, and `GOOSE_SWARM_REQUIRE_SERVABLE=0` is needed to get past the servability check.
- **Gate the gateway before trusting it.** `proxy_fidelity.py` sends the same tool-calling request
  through the gateway and native Bedrock and requires the same tool and arguments. A gateway that
  mangled tool calls would cripple Claude and flatter the local fleet — a fake nobody would notice.

Because of the shadow aliases, the event stream's `model=` field is the swarm's own alias, not the
backend. **The card reports the entrant LABEL, never that field.**

### Known gaps

- No watchdog. If the machine sleeps or a supervisor dies, resume is manual (`board.py` re-run,
  which skips finished ticks).
- Cost per episode is not measured — goose does not surface token counts to the harness.
- Honesty is `NOT COMPUTABLE` for single-agent entrants by construction; only the swarm claims.
- Not built: refactor and repair-adjacent verticals, the agent cards (tool use, clarification,
  fleet scaling), the scoring model — undesigned on purpose until drift says what it can support —
  and the website export beyond `card.py --json`.
