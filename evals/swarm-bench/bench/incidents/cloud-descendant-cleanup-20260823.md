# Cloud build descendant cleanup incident — 2026-08-23

Gemini 3.7 Flash, Gemini 3.1 Pro Preview, and DeepSeek V4 Flash completed their
full provider episodes normally with 42/42, 73/73, and 97/97
admitted/provider-terminal receipts respectively. Their model-authored integration
tests had launched local service children. Those children were still alive when
each Goose parent exited, so the coordinator terminated them before returning.

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

The detached recovery controller treats its explicitly named entrants as the
minimum incident anchors, then discovers every additional terminal `INCOMPLETE`
entrant carrying this exact sealed failure before supersession. It waits while the
campaign or manager is active, carries siblings only when the coordinator can
prove a completed build outcome, and refuses an active or failed-build sibling
rather than silently rerunning it. A carried `PUBLISH_FAILED` score is retried
from its sealed verdict and screenshots; its provider episode is never rerun.

The first terminal campaign also exposed a separate publication-identity defect.
The frozen campaign manifest records the calibration class as `uncalibrated`,
while the hermetic scorer verdict records the complete disclosure string. The
publisher incorrectly required those two strings to be byte-identical after the
score had already passed every hermetic seal. Publication now requires the two
values to resolve to the same calibration class and still seals the verdict's
complete raw string in the publication receipt. An RC scorer paired with a
`frozen` campaign, or a stable scorer paired with provisional calibration, is
still refused. The SB7 scorer, verdict, public `sb-7.0` mapping, and existing
website documents are unchanged.
