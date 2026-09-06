---
title: Agent Work
sidebar_position: 40
description: Run a desk agent on your swarm — tick on a cadence, poll, investigate across nodes, keep a ledger, draft, review, post through one gated script.
---

# Agent Work

A **project** run builds software. An **agent** run keeps a desk: on a cadence, inside a working
window, it polls what needs attention, fans one lane per item across your nodes, has every draft
attacked by reviewers, stages what survives, and posts it through the desk's one write command on a
later tick — asking you when only you can decide. The desk's memory lives in files next to it: a
ledger of facts, a scratchpad, a daily log, a pending file.

## The agent directory

An agent is a directory with an `agent.yaml`. Create one from the desktop (**Agent Work → New
agent**) or on the command line:

```bash
goose swarm agent init ~/desks/axpo --name axpo
goose swarm agent check ~/desks/axpo     # validates the manifest, prints what would run
goose swarm agent tick ~/desks/axpo      # one tick now
goose swarm agent run ~/desks/axpo       # tick on the cadence inside the window until stopped
goose swarm agent status ~/desks/axpo
```

```yaml
name: axpo
title: Axpo Atlassian desk
charter: CHARTER.md            # the desk's rules, read whole into the orchestrator's prompt
timezone: Europe/Zurich
window: { days: [mon, tue, wed, thu, fri], from: "09:00", to: "18:00", always: false }
cadence: 30m                   # between tick STARTS; a tick is never cut
env_file: references/credentials.env
guard:                         # run first; exit 3 = hold this tick (say why on stdout)
  - "test ! -f state/DISABLED || { echo 'DISABLED'; exit 3; }"
poll:                          # read-only; stdout is the tick's inbox
  - "python3 scripts/aj.py checkin"
surgeons:
  - name: access
    brief: "permissions, seats, groups — do the user→groups→permission homework, prove every negative on the same object"
    match: [access, permission, group]
    read_only: true
review:
  lenses: [factual, duplication, voice]
post:                          # the ONE write path; {id} = draft id, {file} = its body
  command: "python3 scripts/post_comment.py \"$AGENT_DRAFT_TARGET\" \"$AGENT_DRAFT_FILE\""
  approval: human              # human | none
ledger: DAILY-LOG.md
pending: PENDING.md
scratchpad: SCRATCHPAD.md
commit: true
```

A full example for one of the operator's desks is in `evals/agent-work/examples/axpo.agent.yaml`.

## One tick

| phase | what happens |
|---|---|
| **guard** | the human's decisions are folded in (approve / decline / answer), the `paused` flag and the guard scripts are checked; a hold ends the tick with its reason |
| **poll** | the poll scripts run in the agent directory with the env file sourced; their stdout is the inbox |
| **orient** | the orchestrator (the planner / supervision node) reads the charter, the scratchpad, the **ledger snowball**, your notes and the inbox, and decides the lanes, the asks and the drops |
| **lanes** | one surgeon call per item, fanned across the fleet's slots (a device serves `weight` lanes at once; the rest queue). Read-only surgeons have no file-writing tools. Each returns a handoff: homework, finding, an optional draft, an ask, a route, a 0–3 confidence, evidence, the next step |
| **review** | every draft is attacked by every lens in parallel (`factual` re-derives the claims from the object, `duplication` reads the live thread and the ledger, `voice` checks it reads as the person); each says PASS or REFUTED with quotes |
| **synthesis** | the orchestrator closes the tick: what to stage, what to ask, what facts the ledger keeps, the daily-log line, the scratchpad rewritten whole, the handoff to the next tick |
| **post** | drafts staged on an **earlier** tick and approved (or, with `approval: none`, any staged draft) go out through the post command, one by one, inside the window only |
| **close** | close scripts, the daily-log line, a commit of the agent directory |

Nothing in a tick is bounded by a clock: the cadence says when the next tick *starts*, the window
says when the desk is open, and a lane that needs an hour gets an hour.

## What the desk keeps

Under `<agent dir>/.swarm/agent/`: `state.json` (the phase clock and the next tick), `run.jsonl`
(every event), `ledger/*.json` minis rebuilt whole into `ledger.json`, `prepared.json` (every draft
and its fate), `asks.json`, `decisions.jsonl` (what you decided), `ticks/<n>.json` (the full record
of each tick), `drafts/<id>.md`. The lanes' words are in `.swarm/activity/` — the same digest and
durable `.log` / `.think.log` files a project run writes, so the desktop reads them live.

In the agent directory itself: the daily log, the pending file (asks only you can clear) and the
scratchpad the orchestrator rewrites every tick.

## The desktop view

**Agent Work** lists your desks and, for the selected one: **when the next tick is** (a countdown,
the wall-clock time in the desk's zone, and why), the phase ribbon with the elapsed time on the live
phase, every lane of the current tick with the node it runs on and its live words (click one for the
whole reasoning and answer channels and its tool calls), the node occupancy and queue, **Needs you**
(answer an ask, approve / decline a draft), a note box the orchestrator reads next tick, and the
ledger: every tick with its cost in lane-minutes beside what it delivered, the facts, every draft's
fate, the scratchpad, the pending file and the daily log.
