# Sink idle-fill — adversarially vetted design (workflow wbbbt8mq2, 2026-07-04)
Both skeptics: positive_or_neutral=TRUE, write_safe=TRUE, real_idle_fill=TRUE, recommendation=IMPLEMENT-WITH-CHANGE.

## The opportunity (verified)
The integrate-verify SINK runs SOLO at the end (owns no files but WRITES/integrates for a long stretch, judge.rs:323 ~420s+). By then pre_review is exhausted (pick_prereview_request marks each task once, scheduler.rs:851 -> returns None) so BOTH other nodes go fully idle for all of T_sink. That is the biggest single-node window (the user's 'one node crunching'). Today the read-only REVIEW-FANOUT (review_dimension + verify_finding, both &[]-extensions = physically cannot write) runs SERIALLY AFTER the sink. Overlapping it with the sink cuts the tail from T_sink + T_review to ~max(T_sink, T_review).

## Design (idle-fill = read-only review overlapped with the sink, over a FROZEN snapshot)
1. Snapshot the tree (cp -r) at the TRUE sink dispatch, run review_dimension on the SNAPSHOT (not current_dir) on the 2 idle nodes while the sink rewrites the live tree -> read-only + snapshot => NO write-race, NO torn read.
2. New idle-job class in the scheduler idle loop (scheduler.rs:1790-1828), fired when sink_in_flight() (301-306) AND pick_prereview_request exhausted; reuse IdleSlotGuard + per-device in_flight (1810-1826) so it never oversubscribes / never steals the sink's node.
3. Consume the pre-warmed findings in the post-sink REVIEW-FANOUT block (swarm.rs:7827-7917); RE-VERIFY each via verify_finding against the FINAL post-sink tree, fail-closed (6474-6475) -> a finding the sink obsoleted is refuted+dropped. So delivered output is UNCHANGED (advisory) or improved-and-reverified.

## REQUIRED CHANGES (skeptic-mandated) before ship
1. SNAPSHOT TIMING: do NOT snapshot in the idle loop (it wakes on a 30s tick AFTER sink_in_flight() is already true -> the sink may have begun rewriting -> the cp -r is itself torn). Take the freeze at the TRUE sink dispatch call-site (where integrate-verify's DispatchRequest is issued) — a more invasive but correct hook.
2. VERIFY TIMING: re-verify the pre-warmed findings right after the SINK (against the post-sink tree), not only in the post-COMPLETE block, so the re-gate is timely.

## Invariant (why quality is unchanged)
Read-only (&[]) or shadow-isolated => no write-race; frozen snapshot => no torn read; every surviving finding re-verified against the FINAL tree fail-closed => no stale finding drives a change. Env-gated default-OFF.

## Verdict
Genuine master-goal win (fills the biggest single-node window) + vetted safe. Moderate scheduler change (snapshot-at-dispatch + idle-job class + trait method + post-sink consume). Implement CAREFULLY with the 2 required changes + PRESENT the scheduler diff (lower-confidence scheduler edit). Modest wall-clock saving (~the review time overlapped) but the right direction for the serial tail.
