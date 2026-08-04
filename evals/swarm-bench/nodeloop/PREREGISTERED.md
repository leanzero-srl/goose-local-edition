# PRE-REGISTERED — the node curve

Written **2026-08-04 22:35 local**, while `baseline-n3-r0` was still in EXECUTE and **no matched pair
existed**. Nothing below may be edited after a pair lands; a change goes in as a dated amendment with
its reason.

## The claim under test

**A 3-node run beats a 1-node run on BOTH wall-clock AND shipped quality, by a margin that clears the
measured replicate spread.**

## Design

Same spec, same frozen binary (`complete()` keys on `engine_build`), alternating
`baseline-n3-rK` / `baseline-n1-rK`, **K = 0..4**. Matched pairs by construction: pair K is the two
runs sharing a replicate index.

## The test, chosen before the data

**One-sided sign test over the 5 matched pairs**, separately for wall-clock and for build score.
`p = 0.5**5 = 0.031` if all five pairs favour 3 nodes. **A single pair going the other way gives
p = 0.187 and the claim FAILS.** No post-hoc switch to a different statistic, and no dropping a pair.

## Why n=5 and not the n=3 this campaign has used all along

Computed from the design, not from any result:

    sign test, one-sided, smallest attainable p
      n=3  ->  0.125    CANNOT reach 0.05 even on perfect separation
      n=4  ->  0.0625   still misses
      n=5  ->  0.031    clears

Read unpaired (exact permutation) instead and n=3 gives exactly 0.05 on perfect separation and
**0.20 the moment one replicate crosses**. Identical-config replicates on this fleet have scored
**44.2 / 86.7 / 90.0**, and real unit walls run **6376-8729 s**. One crossing is the expected case.
**n=3 spends ~12 h of fleet time on a number that could never clear the bar; n=5 costs ~20 h and can.**

## Registered falsifiers

1. **Any VOID cell voids its pair.** A unit whose engine-resolved pool is smaller than the cell asked
   for is a different experiment wearing the right name (F227). Its partner is dropped with it.
2. **A boundary crossed mid-curve voids every cell collected so far** (F253) — the arms would no
   longer share a binary.
3. **If wall-clock favours 3 nodes but score does not, the claim FAILS.** Both, or neither. Speed
   bought by shipping less is not what was claimed.
4. **If the result is significant only after removing a pair, it is not significant.**

## What is NOT being claimed

Nothing about other specs — this is ONE spec, and F125/F126 stand: a pattern in one population is a
hypothesis until a second sees it. Nothing about node counts other than 3 vs 1.
