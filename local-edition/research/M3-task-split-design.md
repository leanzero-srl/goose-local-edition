# M3 — Judge Task-Splitting (design)

## Goal
When a subtask runs too long, the judge splits it into smaller sub-subtasks (parallelizable) instead of
just re-dispatching or terminal-failing. Pairs with M2 (small files): M2 *prevents* monoliths up front;
M3 is the *runtime* recovery when a task is still too big/slow.

## Split vs re-dispatch vs terminal-fail (the key distinction)
- **re-dispatch** (exists): worker is MISBEHAVING (over-reading, looping, no files written) → restart w/ hint.
- **terminal-fail** (exists): worker stuck at the intervention cap → kill + fail descendants.
- **SPLIT (new)**: the TASK is genuinely TOO BIG — worker is *producing* (writing files, progressing) but
  the scope is too large for one worker in reasonable time. Detection must distinguish "big" from "stuck".

## Detection (in the judge)
Split a task when ALL hold:
1. elapsed >= SPLIT_THRESHOLD — hard cap (e.g. 900s) OR ~2x the running median completed-task time.
2. the worker is PRODUCING, not stuck: tool_calls includes writes / files exist for it, AND it is NOT
   tripping the over-read rule (so it's not the re-dispatch case).
3. the task owns >= 2 files (splittable) — a 1-file task can't be partitioned; leave it alone.
4. split_count[task] == 0 (cap: split each task at most ONCE — prevents infinite/runaway splitting).
If elapsed>threshold but worker is over-reading/looping → existing re-dispatch/terminal path, NOT split.

## What the split produces (judge LLM proposal)
The judge asks the LLM to PARTITION the long task's remaining work into 2–4 child subtasks, each owning a
DISJOINT SUBSET of the ORIGINAL task's owned files (no new files, no overlap, union == original files).
Children are independent where possible (parallel) or a shallow chain if one file needs another.
Output (parsed): list of {id, files[], depends_on[] (subset of new child ids)}.

## Scheduler injection — the risky part (DAG mutation mid-run)
On an accepted, VALIDATED split proposal:
1. Abort the original worker (abort handle), release its device + file locks.
2. Validate proposal: every child file ∈ original.files; children files pairwise-disjoint; union covers the
   files still needed; child depends_on ⊆ child ids (no dangling); no cycle. REJECT (fall back to
   re-dispatch) if invalid — never inject a malformed DAG.
3. Insert child tasks into the DAG: each child.depends_on(real) = original.deps (already satisfied) +
   any intra-child deps. State = Ready.
4. RE-POINT dependents: every task that depended on `original` now depends on ALL children (so
   integrate-verify waits for the whole split). This is the easiest place to introduce a bug — miss one
   and integrate-verify runs early.
5. Mark `original` as Split (a terminal, non-failed state) — its dependents were moved to the children.
6. Dispatch ready children.

## Risks + mitigations
- Orphaned deps / early integrate-verify → step 4 must re-point EVERY dependent; add a unit test.
- File ownership drift → strict validation (subset+disjoint+cover); reject+fallback if violated.
- Partial writes from the aborted worker → children just overwrite their files fresh; accept minor re-work.
- Runaway splitting → split_count cap = 1 per task.
- Cycle introduction → children only depend on original.deps (earlier) + sibling children; validate acyclic.

## Implementation steps (incremental, with confidence)
1. judge.rs: add `Verdict::Split` + `JudgeOutcome.proposed_split: Option<Vec<ChildSpec>>` + JudgeConfig
   `split_threshold_secs`. Deterministic detection helper. **Confidence: HIGH** (pure types + thresholds).
2. scheduler.rs `apply_judge_outcome`: handle Split — abort, validate, inject children, re-point dependents,
   mark Split, dispatch. **Confidence: MEDIUM-LOW** — this is the DAG mutation; write it defensively +
   heavily unit-tested (a mock DAG with deps, assert dependents re-pointed + acyclic + files partitioned).
3. swarm.rs judge impl: when deterministic detection fires, prompt the LLM to propose the partition; parse;
   pass as proposed_split. **Confidence: MEDIUM** (prompt + parse, qwen-robust).
4. tests: scheduler_mock — a long "big" task splits, children dispatch, integrate-verify waits for all,
   run completes. **Confidence: HIGH** once 2 is right.
5. Gate behind config (split_enabled, default on) so it's revertible. Commit per step.

## Note
Step 2 (DAG mutation) is the crux and where I'll be most careful + most honest about confidence. If it
proves too risky to do safely mid-run, the fallback is a *softer* split: re-dispatch the original with a
STRONG "your task is too big — implement ONLY <subset> now, the rest is a separate task" hint + enqueue the
remainder as a new sibling task (less elegant, but no live re-pointing of existing dependents).
