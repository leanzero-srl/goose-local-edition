QUEUED ENGINE INSTRUMENT — specified, deliberately NOT coded while the binary is frozen.

WHAT IS MISSING. `scheduler.rs` emits `Replanned` when the dynamic replanner fires and emits NOTHING
when it does not. So "why did replan fire only once?" cannot be answered from any log.

THE TRIGGER IS EIGHT CONJUNCTIVE TERMS (scheduler.rs:2524-2542):

    !dispatched_now
    && s.total_in_flight() > 0
    && s.ready.is_empty()
    && s.idle_capacity() >= 2
    && s.replans_done < self.max_replans
    && !s.sink_in_flight()
    && replan_has_enough_dag_left(s.mandatory_incomplete(), s.mandatory_total())
    && s.replan_declined_at_incomplete.is_none_or(|prev| s.incomplete_count() < prev)

MEASURED: all four archived 3-node cells fired exactly one round (`round: 0`) while `max_replans`
defaults to 2. So term five — the cap — is NOT the binder. Which of the other seven is, nobody knows,
because none of them is recorded.

`replan_declined_at_incomplete` does NOT fill this gap: it is set only when the replanner itself
answers with an empty spec list, which is a different event from the trigger never being reached,
and it is internal state rather than an emitted field.

THE CHANGE. Evaluate the terms separately and emit the first failing one when idle capacity exists:

    "event": "replan_declined",
    "round": s.replans_done,
    "blocked_by": "sink_in_flight" | "dag_nearly_done" | "ready_not_empty" | "no_idle_capacity"
                  | "cap_reached" | "declined_at_same_size" | "nothing_in_flight",
    "idle_capacity": s.idle_capacity(),
    "mandatory_incomplete": s.mandatory_incomplete(),
    "mandatory_total": s.mandatory_total()

Gate the emit on `idle_capacity() >= 2` so it fires only when the fleet actually had slots to fill —
otherwise it would print on every scheduler pass and become the 666-row failure of F590.

⚠️ NOT WRITTEN AS CODE, ON PURPOSE — the same call as F578. This restructures an eight-term
conjunction inside a lock scope in `scheduler.rs`, and the binary is frozen so it cannot be compiled.
F560 and F567 were queued as COMMITS only because additive JSON fields cannot break a build. This
one can. Committing code I cannot compile into a queue that must build cleanly is how the queue gets
poisoned.

WHAT IT SETTLES. F584 measured ~1.6 of 6 slots idle by construction at three nodes, and
`replan_has_enough_dag_left` switches the idle-filler OFF once 75% of mandatory tasks are done —
which is roughly when the idling starts. Its in-source reason is sound on its face ("near the end, an
injected task has nothing to overlap with and simply becomes the tail"), so this is a question, not
an accusation: does an injected per-module test at 80% completion overlap with the remaining 20%, or
merely extend the tail? One `replan_declined` field answers it from a single run instead of an
argument.
