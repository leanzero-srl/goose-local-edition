QUEUED ENGINE INSTRUMENT — specified, deliberately NOT coded while the binary is frozen.

WHAT WAS MEASURED (F618). Replan-injected tasks account for **42.2% of all solo node-time in
three-node runs** — 2704s / 45 min across 11 runs, ~4.1 min per run — where "solo" means EXACTLY ONE
TASK IN FLIGHT on a six-slot fleet. That is more than the integrate-verify sink (39.5%); the original
plan's own tasks are 16.5%.

The control is a natural experiment, not one I arranged: F607 proved dynamic replan is
ARITHMETICALLY IMPOSSIBLE at one node (`total_in_flight() > 0` and `idle_capacity() >= 2` are
mutually exclusive when capacity is 2). So the 1-node arm MUST show zero injected solo time.
**It shows exactly 0.0s.** Membership was verified per-run against each log's own `replanned.added`,
not inferred from task names.

## The guard already exists, was reasoned carefully, and I am not claiming otherwise

`scheduler.rs:2530` gates the trigger on `replan_has_enough_dag_left`, with this comment:

> Near the end, an injected task has nothing to overlap with and simply becomes the tail

and the function's own doc says, in full awareness of the failure mode:

> THE BAR IS A FRACTION, NOT A COUNT, and the first version of this got that wrong. … The harm is
> "nothing left to overlap with", which is inherently relative to the plan's size.
> A quarter of the plan still outstanding clears **both measured harms (14% and 11%)** while leaving
> mid-run injection untouched. Deliberately conservative: it can only ever refuse a replan, so it
> cannot make a run longer.

**The author diagnosed exactly the harm I measured.** The bar was calibrated against two observed
cases at 14% and 11% remaining, and set at 25% to clear them.

**What is new is that the harm recurs ABOVE that bar.** `mandatory_incomplete * 4 >= mandatory_total`
is a PROXY — fraction of mandatory tasks outstanding — for the thing that actually matters, which is
whether the injected work will run CONCURRENTLY with anything. Ten replanning runs say the proxy and
the target have come apart: the gate passes, and the injected task still runs alone.

## The change — an EVENT, not a behaviour change

At the replan accept site, emit what the scheduler already knows at that moment:

    "event": "replan_accepted",
    "round": s.replans_done,
    "idle_capacity": s.idle_capacity(),
    "in_flight": s.total_in_flight(),
    "mandatory_incomplete": s.mandatory_incomplete(),
    "mandatory_total": s.mandatory_total(),
    "ready_after_splice": <count of tasks runnable once the injected specs are spliced>,
    "added": [...]

Then `replan_accepted.in_flight` can be joined against each injected task's own dispatch/completion
span to answer, per injection rather than per corpus, whether it overlapped.

⚠️ NOT WRITTEN AS CODE, ON PURPOSE — same call as F591/F578/F603. The binary is frozen and this
touches a lock-scoped decision site. F560/F567 were queued as commits only because they are single
additive JSON values.

## The evidence AGAINST acting on this, which is stronger than it first looks

- **The injected work is TESTS.** Every injected id in the corpus is a `test-*` or `harden-*` task.
  F616 measured three-node verification finding **2.82x** the problems per call as one node, and
  detecting `spec_drift` **22 times against zero** in 680 one-node verdicts. Suppressing injections
  suppresses that.
- **F611 priced the quality side as unmeasurable** — 183 cells per bucket for 80% power. So there is
  no way, on this corpus, to show that refusing an injection is safe for the build.
- **The fork's own doctrine argues the other way**: split-time saved is meant to be REINVESTED into
  quality, and "when a knob trades wall-clock for more working coverage, that's usually the right
  trade."
- **F618 explicitly did NOT prove the run is longer.** The counterfactual — what the fleet would have
  done in those 4.1 minutes — is unobservable, and 4.1 min/run sits against a 6.6 min arm difference
  that is itself only 0.58 SE.
- **The existing guard's safety property is worth preserving**: it "can only ever refuse a replan, so
  it cannot make a run longer." Any replacement must inherit that, or it is a worse trade than the
  thing it replaces.

**Therefore: ship the event, not the conclusion.** The correct next step is a field that makes the
overlap question answerable per-injection, not a stricter gate justified by a corpus-level share.

---

## AMENDED (F622) — this entry was HALF the story. There is a SIZING problem too.

Everything above assumes the injections are normally-sized work that merely lands badly, i.e. a
**timing** defect. Measured, that assumption is wrong:

| group (sink excluded) | n | mean | median |
|---|---|---|---|
| **replan-injected** | 30 | **825.6s** | **675.7s** |
| original plan tasks | 238 | 451.1s | 390.9s |

**1.83x on the mean, 1.73x on the median, +374.5s at 4.47 SE.** The median tracking the mean kills the
confound that a few un-aborted survivors carry the result — the whole distribution is shifted. The
one-node injected group is EMPTY (n=0), which is the control passing, since F607 proved replan cannot
fire there.

**The replanner emits work roughly twice the size of the planner's own tasks.** That is a GENERATION
defect sitting upstream of the timing one, and a concurrency gate alone would leave it untouched.

It also re-explains F621 without invoking timing at all: an injected task is the biggest single
consumer of node-busy in 6 of 11 three-node runs, and at 1.83x the size of a plan task it would tend
to be the biggest task *wherever* it ran. Two findings that looked like one effect are two.

**So the queue entry now has two halves, and the second one is cheaper to act on:**

1. the `replan_accepted` event above (answers *when* it runs), and
2. a **size constraint on what the replanner may ask for** — the architect prompt is already
   fleet-relative and explicit about task granularity; the replan prompt is where to look first,
   because 13.8 minutes per injected task on a 27B local worker is also more exposure to the
   turn-cap, straggler and spiral guards than a 7.5-minute plan task carries.

⚠️ **The counter-evidence above applies unchanged, and one wrinkle is added:** spans are ATTEMPTS, so
a retried task contributes more than one span, and a longer task is likelier to be retried. That
could inflate the injected span COUNT without inflating the per-span mean — which is exactly why the
per-span mean and median are the statistics quoted, and why the 30 is not itself evidence of
anything.
