@AGENTS.md

## Durable context in this repo — read the one that matches what you are doing

Compaction destroys conversation knowledge, so it lives in files instead.

**`.claude/rules/*.md` load only when you open a matching file with the `Read` tool.** `cat`, `sed`,
`grep`, Grep and Glob do NOT arm them — measured on 2.1.247. That matters here because the files are
large enough that `grep -n` + `sed -n 'A,Bp'` is the correct way to open them, so **you can work all
session and never see the rule for the file you are editing.** Two consequences: the invariants that
must never be broken are repeated in `AGENTS.md`, which loads unconditionally; and if you are about to
change something load-bearing, open `.claude/rules/` for that area DELIBERATELY rather than assuming it
arrived.

These you should open yourself:

- **`NOW.md`** — the current thread. Read this BEFORE `SWARM-AGENDA.md`, which is 2,400 lines of history.
- **`EXPERIMENTS-LEDGER.md`** — what was tried, what it measured, why it is not coming back. **Read before
  proposing an engine change**: several ideas here have been tried twice because the first failure lived
  only in a compacted conversation.
- **`RUN-LEDGER.md`** — one row per run, in comparable numbers, so runs are judged by measurement rather
  than recollection.
- **`TICK-NOTES.md`** — every finding, newest last.
- **`DESIGN-STABILITY-FIRST.md`** — the next evolution (BP-1, STABILITY > SPEED > QUALITY): what is
  deleted, what is kept, the 14 ranked steps and the all-vs-single control. **Read before implementing
  anything in the planner or REPAIR tail**; it supersedes the plan-mode file for those areas.

## Working in this repo without drowning in it

The files here are large enough that naive reading is the main cause of lost work. Measured:

| object | size | never |
|---|---|---|
| `crates/goose-cli/src/commands/swarm.rs` | **42,165 lines** | read whole; `grep -n` then `sed -n 'A,Bp'` |
| `crates/goose-swarm/src/scheduler.rs` | 4,616 lines | — |
| `ui/desktop/src/components/swarm/SwarmRunPanel.tsx` | 3,971 lines | — |
| `ui/desktop/src/components/swarm/useSwarmRun.ts` | 3,858 lines | — |
| `SWARM-AGENDA.md` | 2,780 lines | read whole; `grep -n '^- \[ \]'` for open items |
| the session transcript | tens of MB | read at all — delegate it |

**Delegate the reading, keep the conclusion.** A subagent burns its own context, not yours, and returns
a finding instead of a file dump. Any question answered by sweeping many files — "where else does this
rule live", "which of these 40 findings are still real", "review this 15k-line diff" — belongs in a
subagent or a `Workflow`, not in your own window. This is not a style preference: a 70-agent recovery
sweep over this repo returned 218 findings for the cost of one summary, and reading the same material
directly would have consumed the window before the first fix.

**Fan out, then synthesise.** For work with independent parts, run them concurrently and combine the
results yourself:
- **Batch independent tool calls into ONE message.** Two greps that do not depend on each other are one
  turn, not two.
- **`Workflow` for anything with a shape**: fan a dimension per agent, then verify each finding with a
  *separate* adversarial agent whose job is to REFUTE it. On this repo that pass refuted 13 of 34
  findings — a third of them were plausible and wrong, and would have become plausible and wrong commits.
- **Batch by FILE, never by theme**, when agents will edit. Two agents in one file collide; one agent per
  file does not, and it is faster than serial.
- **Give parallel reviewers different lenses** (correctness, security, does-it-reproduce) rather than the
  same prompt N times. Redundancy finds the same thing N times; diversity finds N things.

**Treat context as a budget with a floor.** When it gets tight, the failure mode is silent: work in
flight is summarised, and the summary keeps the SHAPE of the task while losing the THREAD — which fix
was mid-flight, which of the user's exact words are load-bearing. Before you are close to that:

1. **Write the finding down, do not carry it.** `note.sh <kind> "<finding>"` costs nothing and survives;
   holding it in context costs on every subsequent turn and survives nothing.
2. **Commit.** A committed step is recoverable; an uncommitted one is not. A brace-matching script here
   once deleted 34,827 lines of `swarm.rs` and `git checkout` undid it in one command *because the
   previous step was committed*.
3. **Update `NOW.md` in the same commit as the work**, so the thread itself is durable rather than only
   its output.

**After a compaction, re-mine on a budget (~10k tokens), do not resume from the summary.** Read `NOW.md`,
then `grep -n '^- \[ \]' SWARM-AGENDA.md`, then `git log --oneline -12`. Only if the thread is still
unclear, spend ONE Explore subagent on the raw transcript and ask for under 400 words. State the current
thread in one line before continuing; if it contradicts the summary, the sources win.
