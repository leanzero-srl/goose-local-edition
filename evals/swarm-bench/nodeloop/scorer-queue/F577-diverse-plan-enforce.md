QUEUED ENGINE CHANGE — DO NOT FLIP WHILE THE BINARY IS FROZEN.

`diverse_plan` (swarm.rs:493, baked default `false` at :1235) turns the structural-convergence
measurement from a shadow into an enforcement: when parallel drafts strongly converge, agreement
confidence is lifted past the ask floor and the redraft ladder is skipped.

WHY IT IS QUEUED RATHER THAN FLIPPED. The binary is frozen at 717ff4a6e and `baseline-n3-r1` is cell
2 of the 4-cell F533 spread measurement. Enabling it now puts the treatment inside the measurement.

THE EVIDENCE, from `plan_convergence` in the archived frozen-era cells:

  cell             agreement_conf  best2  pool_penalty  struct_conv  enforced  would_skip_ladder  ladder
  baseline-n3-r0               69     88            19          100     false               TRUE   2099s
  baseline-n3-r2               88    100            12          100     false               true      0s
  baseline-n3-r3              100    100             0          100     false              false      0s

`baseline-n3-r0` is the whole case in one row. 88 minus a POOL PENALTY OF 19 is 69, which is under
the 85 floor, which bought three rounds of draft-then-detail of which two were discarded entirely —
2099 seconds, 35 minutes, 26% of that cell's 134.6-minute wall. Its own shadow says the ladder would
have been skipped. The penalty is levied for having three nodes rather than one.

The engine root-caused this itself at swarm.rs:14246: `plan_agreement` is max-min spread plus mean
pairwise Jaccard, `best_subset_agreement`'s doc says both "only worsen (or hold) as the pool grows",
and so a bigger fleet drafts more, is scored lower for it, and pays a ladder the smaller fleet is
never eligible for. That is the mechanism of "more nodes make it worse" on the speed pillar.

⚠️⚠️ THE EVIDENCE AGAINST THIS CHANGE IS STRONGER THAN THE CAVEAT I FIRST WROTE HERE (F579).

The engine had already measured the proposal and recorded the result, in its own words at :14277:
"Emitted rather than acted on: the cells that laddered scored 0.9343/0.7147/0.8157 against
0.6030/0.6695 for the two that did not, so the ladder may be buying the quality and a silent flip
here could spend that. Measure first."

That is +0.185 IN FAVOUR OF LADDERING. My original caveat here rested only on the frozen-era spread
— 0.6457 laddered against 0.4624 and 0.7703 not, a mere +0.03. The two eras AGREE IN DIRECTION and
disagree in magnitude; pooling all eight cells gives laddered 0.7776 against non-laddered 0.6263,
+0.151, though pooling across binaries is itself questionable.

It is observational, not randomised, and that cuts BOTH ways: a cell ladders BECAUSE its drafts
disagreed, and disagreement may itself mark a harder planning situation. Nobody has run the arm.

⇒ SO THE HONEST POSTURE IS THE ENGINE'S OWN: MEASURE FIRST. This change buys back 35 minutes of the
speed pillar by potentially spending the quality pillar, and the main goal requires BOTH — a
speed-only win here would not be a win at all.

RELATED, AND THE ENGINE SAYS THE FIX MAY BE IN THE WRONG PLACE ENTIRELY: `best_subset_agreement`
exists so a growing pool can only RAISE the metric, and is wired in as `consensus_k` — but applied
"retarget only", i.e. only AFTER the ladder has triggered, never at the round-1 decision point where
the pool penalty is what triggers it. The engine's verdict on that ordering is "That is backwards."
`consensus_k` is NOT a config field (3 references in the whole file, no `pub`, no SwarmConfig entry),
so it cannot be switched on without editing the call site. k=2 is principled rather than arbitrary:
it is what a 1-node fleet drafts, so every fleet is reported on the same footing, and at 2 drafts it
falls through to the full-set measure so the field is a no-op on 1 node (unit-tested at :9437).

⚠️ AND THE ERA GAP IS UNEXPLAINED. The in-source comment records 3-node agreement at 50/52/54 with
ladders of 786/821/1657s on EVERY 3-node cell; the frozen era shows 69/88/100 with one of three
paying. Something improved in between and I have not identified what. Do not attribute it.

WHAT THE ARM MUST MEASURE: ladder incidence (confidence_retarget count), prefix seconds, wall, and
score — against the 3-node baseline on the same frozen binary, n>=4 per side, because the replicate
spread is the thing every claim here is measured against.
