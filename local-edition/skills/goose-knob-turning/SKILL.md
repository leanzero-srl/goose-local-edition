---
name: goose-knob-turning
description: SUPERSEDED IN PLACE — the maintained goose-knob-turning skill lives at ~/.claude/skills/goose-knob-turning. This in-repo copy is a stub kept only so a session that finds this path is told where the real one is, instead of reading a description of an engine that no longer exists.
---

# This copy is a stub. Read `~/.claude/skills/goose-knob-turning/SKILL.md` instead.

**Do not tune the swarm from this file.** Until 2026-08-27 it held a full copy of the knob-turning skill, last
touched 2026-07-04, describing `research_scouts`, `parallel_planning`, `max_research_questions`,
`dynamic_replan`/`max_replans`, `worker_timeout_secs` as a per-task wall-clock cap, and a SCOUT → PLAN →
EXECUTE phase banner. **Every one of those is gone from the engine**, and a duplicate skill does not fail
loudly — it shadows, and it reads authoritative. That is exactly how a session gets sent down a road that was
closed.

The engine as it exists is:

```
FLEET → OPEN → ASK → RESEARCH → SYNTHESIS → REVIEW → CONTRACTS → PILLARS → BUILD → INTEGRATE
      → REPAIR [ TEST → RATE → FIX → VERDICT ]
```

`run_linear_plan` in `crates/goose-cli/src/commands/swarm.rs` is the only planning path. `run_scouts`,
`parallel_plan` and `detail_plan` are still compiled and never called.

Two skills own this work, both under `~/.claude/skills/`:

- **`goose-knob-turning`** — edits `swarm.rs` and `crates/goose-swarm`, the phase prompts, the deterministic
  correctors, the builds and the releases.
- **`goose-swarm-campaign`** — launches a run, holds the 5-minute vigil, kills it on a named-field checkpoint,
  scores and verdicts it.

If you want this checkout to carry a swarm skill of its own, sync it FROM `~/.claude/skills/` in the same
commit as the engine change it describes. A copy that drifts is worse than no copy.
