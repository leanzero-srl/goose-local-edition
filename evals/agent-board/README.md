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

**Repair** is the only vertical built. Three rungs — `slugkit-easy`, `shiftlog-medium` (the obvious
one-character fix breaks a passing test), `ledgerfold-hard` (the intuitive fix leaves Decimal and
destroys precision past 15 significant digits). All three probes trusted against their controls.

Measured so far: every cloud baseline clears every rung, and the local 27b single-agent clears
`slugkit-easy` too — in 876.8s against haiku-4.5's 31.3s. **Correctness has saturated; time is
currently the discriminator**, so the card ranks it as a column. The two are never merged.

Not yet built: the other three code verticals (repair is the sharpest, so it went first), the agent
cards (tool use, clarification, fleet scaling), the scoring model — which stays undesigned until
drift calibration says how many reps a vertical needs — and the website export.
