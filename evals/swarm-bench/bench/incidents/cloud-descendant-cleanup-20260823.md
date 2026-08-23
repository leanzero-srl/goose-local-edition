# Cloud build descendant cleanup incident — 2026-08-23

Gemini 3.7 Flash and Gemini 3.1 Pro Preview completed their full provider episodes
normally with 42/42 and 73/73 admitted/provider-terminal receipts respectively.
Their model-authored integration tests had launched local service children. Those
children were still alive when each Goose parent exited, so the coordinator
terminated them before returning.

`redacted_copy_until_process_exit` nevertheless returned an unclean result when
it had merely *observed* a live descendant at parent exit. This conflated two
different states: a descendant that needed coordinator cleanup and a descendant
that survived coordinator cleanup. The campaign therefore rejected a complete,
fully accounted build even though all recorded process identities were dead and
the owned process group was empty.

The coordinator now accepts a build only when descendant cleanup is proven. It
still returns failure if group cleanup fails, an identity-bound descendant
survives SIGTERM/SIGKILL, or the inherited output pipe cannot be drained within
the bounded cleanup window. The regression exercises both directions: successful
cleanup is accepted and deliberately unproven cleanup is rejected.

Same-binary supersession now permits this coordinator-only infrastructure repair
after a failed entrant has fully terminal, unambiguous provider lifecycle and no
score. Successful entrants remain immutable and are carried forward; unstarted
entrants remain unstarted. An ambiguous admission or an outcome-bearing entrant
is still ineligible for retry.
