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
