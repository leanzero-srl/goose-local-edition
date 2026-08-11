# S3 — skeleton-then-parallel-fill: the splice helper's contract (design, pre-implementation)

## The pure helper

    fn splice_functions(
        skeleton_src: &str,              // the contract-derived module skeleton on the real tree
        shadow_src: &str,                // one filler's completed shadow copy of the same module
        slots: &[String],                // the function names THIS filler owns
    ) -> Result<String, SpliceRefusal>   // the skeleton with ONLY those bodies replaced

Refusal is a first-class outcome, never a panic: `SlotMissingInShadow`, `SlotMissingInSkeleton`,
`ShadowTouchedForeignSlot` (byte-fence: any diff outside the owned slots' spans refuses the whole
splice — CooperBench's law clause 2 enforced mechanically), `Unparseable(side)`.

## Mechanics (Python first; TargetLang gates)

ast module spans via the existing python3 -c oneliner pattern (the drift checks already shell out
to ast): per top-level def/class, byte spans in BOTH sources; replacement in DESCENDING offset
order (the per-finding shard design's rule); import lines: the shadow may ADD imports — merged at
the top, deduped, refused on conflict (same name, different target).

## Test plan (both directions, before any wiring)

1. Two fillers, disjoint slots → both splice; result parses; foreign bodies byte-identical.
2. A filler that edited a sibling's function → ShadowTouchedForeignSlot refusal, tree untouched.
3. A filler that deleted its own slot → SlotMissingInShadow.
4. Import add (clean) merges; import conflict refuses.
5. The composed module passes the same contract validation the skeleton passed.

## Wiring order (later increments)

i2: the detailer emits `subsplit` function lists for hard modules (plan-time, latent).
i3: dispatcher fans fillers into shadows; splice on completion; per-slot verify (generated tests
from S7) BEFORE splice — the full Parsel shape with the engine's own machinery.

## Increment 2 — the detailer emits latent subsplits (designed 2026-08-12, parser landed first)

Scope: HARD tasks owning exactly one .py file. The detail prompt (swarm.rs ~:16065) gains one
optional trailing line: `SUBSPLIT: name1, name2, name3` — 2-4 top-level function/class names,
each independently implementable against the module's contract; omitted when the module does
not naturally decompose. Plan-time and LATENT: nothing dispatches differently until S4's
expand_subsplits (or S3 i3's fill fan) consumes the list when the fleet idles.

Pieces:
1. `extract_subsplit(spec_text) -> Vec<String>` — pure: the last `SUBSPLIT:` line, comma-split,
   each a valid Python identifier, 2-4 names else EMPTY (1 name = no split; 5+ = the model is
   listing, not decomposing). The line stays in the spec (harmless worker context).
2. CONTRACT-ANCHORED guard at consumption time (not parse time — the contract lands after
   detail): names not present as top-level defs in the module's frozen stub are dropped; a list
   that loses names below 2 collapses to empty. The prompt is a hope; the contract is the truth.
3. `TaskSpec.subsplit: Vec<String>` — compiler-enforced plumb through every construction site
   (default empty), stamped from the detail result. detail_completed event gains
   `subsplit: <n>` so the mechanism is observable the run it ships.

Registered checks: MECHANISM n=1 — a hard single-file task's detail_completed carries
subsplit>=2. INERTNESS — with no consumer wired, run behaviour is byte-identical (the readout
is the event field only); any wall/score movement on the landing boundary indicts the prompt
change, not the latent list, and reverts it.

## Increment 3 — the fill fan (designed 2026-08-12; implementation next)

The dispatcher-internal shape — the SCHEDULER SEES ONE TASK, unchanged. When the worker
dispatch site receives a Ready task that is Hard, owns exactly one .py file, and carries
subsplit>=2 (after contract anchoring), the DISPATCHER fans instead of running one agent:

0. GATE: GOOSE_SWARM_FILL_FAN, default OFF, an arm. Any precondition failing → the existing
   serial path byte-identically.
1. ANCHOR: parse the module's `### module: <id>` stub section (the same text every worker
   prompt gets); subsplit names not present as top-level defs in the stub are dropped; <2
   survivors → serial path. The stub also becomes the SKELETON: it is contract text that
   already had to parse (drop_unparseable_stubs guarantees the section survived validation),
   so writing it to the owned file with `raise NotImplementedError` bodies is DETERMINISTIC —
   zero model calls, and the skeleton is BY CONSTRUCTION the contract the siblings import.
2. FAN: one filler per surviving slot name via fanout_over_fleet (its queue already respects
   the 2/node ceiling), each speculative:true rooted at a shadow of the skeleton tree, each
   prompt: "implement ONLY <slot>; every other def stays exactly as given" — the fence makes
   that instruction ENFORCED, not hoped.
3. SPLICE on each completion: splice_functions(current, root=skeleton, shadow, [slot]).
   Refusal → discard that shadow, log it; the slot keeps its NotImplementedError body and the
   run's own verify/fix chain owns it from there (monotone: landed fills never revert).
4. COMPLETE: task Done when every filler resolved. All-refused/all-failed → ONE serial rescue
   attempt on the skeleton (the current path, warm), so the fan can never do worse than a
   delayed serial run.

Events: `fill_fan{task_id, slots, spliced, refused, secs}` per round + per-fill
`fill_completed{slot, spliced, refusal}`.

Registered checks (before the arm runs):
- MECHANISM n=1: fill_fan{slots>=2} with spliced>=1 on a hard module; the module file parses
  after every splice (py_module_spans is called inside splice_functions — refusal is the check).
- SAFETY: zero ShadowTouchedForeignSlot promotes (the fence holds); a slot refused never
  leaves partial foreign bytes on the real tree.
- WALL: p90 hard-module task time vs the corpus p90 (1,522s); the design's claim is ~655s
  three-way — anything above the serial median kills the arm on speed alone.
- QUALITY GATE: stable-24 not below the pre-arm mean − spread.

Confidence note (flagged per the standing rule): the skeleton-write step and the fence are
HIGH confidence (deterministic, tested); the fan's completion semantics inside the dispatcher
run path is MEDIUM — the dispatcher's run() contract (one agent, one outcome) gets an internal
fan and the timeout/retry interaction there is where a subtle bug could hide. The
implementation tick starts by reading run()'s retry/timeout callers before touching anything.
