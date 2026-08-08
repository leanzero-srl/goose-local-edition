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

⚠️ THIS IS A TRADE AND MUST BE JUDGED AS ONE. Skipping the ladder saves the 35 minutes but also
skips a re-plan the engine judged necessary. The one cell that paid it scored 0.6457 — MID-RANGE,
between 0.4624 and 0.7703. Whether skipping would have scored better or worse is UNMEASURED. So
this gets its own arm once the freeze lifts, judged on BOTH pillars; a speed-only verdict would be
exactly the half-reading F539 warns against.

⚠️ AND THE ERA GAP IS UNEXPLAINED. The in-source comment records 3-node agreement at 50/52/54 with
ladders of 786/821/1657s on EVERY 3-node cell; the frozen era shows 69/88/100 with one of three
paying. Something improved in between and I have not identified what. Do not attribute it.

WHAT THE ARM MUST MEASURE: ladder incidence (confidence_retarget count), prefix seconds, wall, and
score — against the 3-node baseline on the same frozen binary, n>=4 per side, because the replicate
spread is the thing every claim here is measured against.
