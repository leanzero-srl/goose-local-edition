---
paths:
  - "crates/goose-swarm/**/*.rs"
---

# goose-swarm — scheduler, judge, patch, events

## `prior_hints` holds ONE String per task, removed at the next dispatch

A bare `insert` silently discards whatever another path had already written for that same dispatch. Use
`add_prior_hint` unless you mean to replace — the guided retry and the judge restart deliberately do,
because their note is the freshest statement of why the last attempt died.

**This bit the tree warden.** Its findings were written with `add_prior_hint`, clobbered by a retry's
bare insert, and then never re-stated because the dedup key remembered them forever. Absence of a hint is
ambiguous: DISPATCH removed it because it delivered it (never repeat), a bare insert removed it because
it clobbered it (must repeat). `warden_should_state` separates the two, and `warden_pending` is cleared
at dispatch and only at dispatch.

## The warden is READ-ONLY

`sweep_tree_defects` inspects what a completed dependency actually left on disk and appends a hint. It
must never change a task's state or outcome. `scheduler_mock.rs:2109` asserts both tasks still finish.

## Comments here have been wrong in ways that cost real time

Three in one review: `DeliveredFile::present` claimed it shared `owned_file_written`'s predicate while
being a second hand-rolled copy of the same expression; `build_in_flight`'s doc named a caller that had
been deleted; the `TreeDefect` doc claimed a finding always reaches the next dispatch. If you assert a
shared rule in a comment, make it actually shared — `file_written` exists for this now.

## The census must distinguish a FAILED task

`tree_file_status` keyed on done-vs-not-done, so a dead task's files read "in progress" and the
supervisor waited for something that was never coming. Failed reads `ABANDONED, …`.

Gate: `cargo test -p goose-swarm` — SUM the `test result:` lines; `tail -3` shows one binary only.
