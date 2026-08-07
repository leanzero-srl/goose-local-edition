#!/usr/bin/env python3
"""Manual, dry-run-by-default entry point to the sweep's OWN orphan reaper.

THIS FILE DELIBERATELY IMPLEMENTS NOTHING. Its first draft was a second, independent reaper with a
different discriminator (command name vs working directory) and a different age floor (10 min vs 3 h)
— two versions of one rule, which is the failure mode this campaign keeps rediscovering. `sweep.py`
already owned the rule, with two guards mine did not have: it matches on the process's CWD being
inside `runs/` (so nothing of Mihai's can ever match) and it walks ppid to the sweep's root (so the
live engine, its shells and their children are protected by construction, however they are grouped).

What the first draft DID surface, and what went back into the real reaper rather than staying here,
is that its three-hour age floor was too slow for the commonest leak: the two pytest orphans burning
50 CPU-minutes during the cell they were corrupting were 55 and 48 minutes old. `orphan_age_secs`
now gives ppid-1 processes a ten-minute floor, because a reparented process has no waiter by
definition rather than by inference from age.

⚠️ A RUNNING SWEEP DOES NOT SEE THIS EDIT. The supervisor that has been up for a day is executing the
old function from memory, so between now and its next restart this script is the only path to the new
floor — which is exactly why a manual entry point is worth having at all.

    python3 reap.py            # report only
    python3 reap.py --kill     # actually signal
"""
import argparse
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)


def describe(pids: list[int]) -> None:
    if not pids:
        print("  (none)")
        return
    out = subprocess.run(
        ["ps", "-o", "pid,ppid,pgid,etime,time,command", *[str(p) for p in pids]],
        capture_output=True, text=True).stdout
    for line in out.splitlines():
        print("  " + line[:150])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--kill", action="store_true", help="actually signal; default is report only")
    ap.add_argument("--orphan-age-secs", type=int, default=600)
    args = ap.parse_args()

    import sweep  # noqa: E402 — path set above

    # Reported BEFORE signalling, so what was killed is auditable rather than asserted. The dry pass
    # also protects the operator from a change in the rule they did not expect: if this ever lists
    # something surprising, they see it before anything dies.
    doomed = sweep.reap_run_orphans(orphan_age_secs=args.orphan_age_secs, dry_run=True)
    print(f"=== ORPHANS UNDER {os.path.join(os.path.dirname(HERE), 'runs')} ({len(doomed)}) ===")
    describe(doomed)

    if not args.kill:
        print("\nDRY RUN — nothing signalled. Re-run with --kill.")
        return 0
    killed = sweep.reap_run_orphans(orphan_age_secs=args.orphan_age_secs)
    print(f"\nkilled {len(killed)} process(es): {sorted(killed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
