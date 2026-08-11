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
