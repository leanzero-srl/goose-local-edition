"""The ONE home for cross-module bench constants (BENCH2 rank 1).

Before this file, probes/vendor_trace.py carried EXPECTED_TOTAL = 47 (a stale example figure
from an early vendor_docs draft) while score_build.py carried the real 247 — so the two dormant
trace checks that read it (all_payments_returned, total_count) would have scored a CORRECT
client 0% the day they were wired. A constant that two modules each define is a constant that
will eventually disagree; every consumer imports from here.
"""

EXPECTED_TOTAL = 247
EXPECTED_SUM = sum(1000 + i * 137 for i in range(EXPECTED_TOTAL))
RETRY_AFTER_SECS = 2
PHASE_MARKER = "__phase__"
