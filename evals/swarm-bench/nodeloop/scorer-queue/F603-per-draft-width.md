QUEUED ENGINE INSTRUMENT — specified, deliberately NOT coded while the binary is frozen.

WHAT IS MISSING. The engine logs which skeleton it PICKED and never what it picked FROM. Measured by
dumping every key across the three candidate events in the archive:

    skeleton_drafts      chars, dead, requested, returned, secs, straggler_aborted, worker_count
    plan_convergence     agreement_conf, agreement_best2, pool_penalty, struct_conv, struct_stop,
                         enforced, would_skip_ladder, drafts, detail        (ALL AGGREGATE)
    retarget_discarded   tasks                                (ONE plan — the discarded one)

`chars` is the only per-candidate field anywhere, and a character count says nothing about DAG shape.

WHY IT MATTERS NOW. F600 established the one finding that survived every test today: at one node the
FLEET binds and at three nodes the PLAN binds (7/7 vs 8/9, five builds). F601 then measured the
delivered plan barely widening with the fleet — 1.11x of plan for 3.00x of hardware, with roots
5.1 -> 5.6 and depth 4.1 -> 4.0. F602 found `score_skeleton` is ALREADY fleet-aware and already
rewards width (`independent.min(wc) * 10`), so the selector is not the problem — a selector can only
pick the widest candidate it is GIVEN.

That leaves exactly one question: DO THE COMPETING DRAFTS DIFFER IN WIDTH AT ALL? If they do and the
selector still ships a narrow one, the scorer's other terms are outvoting width. If they do not, the
lever is generation and the fix is to make the parallel drafts target DIFFERENT widths. Those are
opposite fixes and nothing on disk can distinguish them.

THE CHANGE. At the `plan_convergence` emit, `valid1` — the parsed valid drafts — is already in scope
(it is passed to `best_subset_agreement(&valid1, converge, 2)` two lines above). Add:

    "draft_shapes": valid1.iter().map(|d| {
        let ids: std::collections::HashSet<&str> = d.iter().map(|s| s.id.as_str()).collect();
        let roots = d.iter()
            .filter(|s| s.deps.iter().all(|x| !ids.contains(x.as_str())) && s.id != "integrate-verify")
            .count();
        serde_json::json!({"tasks": d.len(), "roots": roots})
    }).collect::<Vec<_>>(),

⚠️ NOT WRITTEN AS CODE, ON PURPOSE — same call as F578 and F591. This is more than a field: it needs
a HashSet and a closure, it touches a lock-scoped emit, and THE BINARY IS FROZEN SO IT CANNOT BE
COMPILED. F560 and F567 were queued as COMMITS only because they are single additive JSON values that
cannot break a build. Committing code I cannot compile into a queue that must build cleanly is how
the queue gets poisoned.

WHAT IT SETTLES IN ONE READ. Whether the three drafts arrive at 5/5/5 roots or at 4/6/8. The first
means the architect emits one width regardless of fleet and no amount of selection can help; the
second means the range exists and the scorer is discarding it. One field, one run, no new cells — and
the campaign cannot afford new cells (F594: 102 per arm for quality, 776 for speed).

⚠️ AND THE PREMISE IS NOT YET PROVEN EITHER WAY. F601 measured the SHIPPED plan, not the drafts. I am
not asserting the drafts are uniform; I am asserting nobody can currently tell.
