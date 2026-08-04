#!/usr/bin/env python3
"""Re-derive the audit for stored result rows whose `audit_version` is stale, IN PLACE.

WHY THIS EXISTS. `sweep.complete()` treats a row whose `audit_version` differs from the current one
as INCOMPLETE, so bumping the audit re-runs the unit — roughly two hours of fleet time to recompute a
number that is a pure function of a log already on disk. Worse, doing that mid-curve would re-run
cells the node curve has already collected.

The evidence outlives the verdict: `run.jsonl` is kept, so the audit can always be recomputed. This
script does that and rewrites the row, which is why an instrument fix can now ship at any moment in a
measurement campaign instead of waiting for a boundary.

It NEVER touches score, wall_secs, engine_build or any run-derived field — only the audit blob and
its version stamp.
"""
import json
import sys
from pathlib import Path

import dispatch_audit
import sweep


def main() -> int:
    rows = sorted(sweep.OUT.glob("*/nodeloop-result.json"))
    stale = 0
    for f in rows:
        try:
            r = json.loads(f.read_text())
        except Exception as exc:
            print(f"  UNREADABLE {f.parent.name}: {exc}")
            continue
        if r.get("audit_version") == dispatch_audit.AUDIT_VERSION:
            continue
        was = r.get("audit_version")
        try:
            a = dispatch_audit.audit(f.parent)
        except Exception as exc:
            print(f"  FAILED     {f.parent.name}: {exc}")
            continue
        if not a:
            # A void refusal never produced a log to audit. Stamp it so it stops being re-run, and
            # say so — silently leaving it stale would re-run a 60-second refusal forever.
            r["audit_version"] = dispatch_audit.AUDIT_VERSION
            f.write_text(json.dumps(r, indent=2))
            print(f"  no log     {f.parent.name}: {was} -> {dispatch_audit.AUDIT_VERSION} (void row)")
            stale += 1
            continue
        r["audit"] = a
        r["audit_version"] = a.get("audit_version") or dispatch_audit.AUDIT_VERSION
        f.write_text(json.dumps(r, indent=2))
        print(f"  reaudited  {f.parent.name}: {was} -> {r['audit_version']}  "
              f"kind_mismatch_pct={a.get('kind_mismatch_pct')}  ({a.get('kind_mismatch_basis','')[:40]})")
        stale += 1
    print(f"{stale} of {len(rows)} rows rewritten; current audit is {dispatch_audit.AUDIT_VERSION}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
