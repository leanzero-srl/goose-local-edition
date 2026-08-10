#!/usr/bin/env python3
"""Does the release binary actually CONTAIN the engine changes I think I shipped? (BATCH.md §4 step 4)

WHY A STRING PROBE AND NOT `cargo build` SUCCEEDING. `cargo test` and `cargo clippy` do NOT rebuild
`target/release/goose`, and an empirical behaviour check against a stale binary silently exercises
the OLD code. A build that prints "Finished" is not proof the running binary carries the edit — the
sweep may have been holding an older file, the build may have gone to a different profile, or the
edit may sit behind a feature gate.

WHY LITERALS AND NOT FUNCTION NAMES. `strings` cannot see private Rust fn names — three known-present
private fns probe as ZERO — so "fn is_test_path not found" would be a FALSE alarm. Emitted string
literals (event names, field keys) DO survive into the binary. That distinction is what makes a zero
here a PROVEN negative rather than an observed one.

THE CONTROL THAT MAKES THIS WORTH RUNNING. A probe that only ever runs against the NEW binary cannot
tell "the literal is present" from "my grep matches anything". So this is designed to run TWICE:

    BEFORE the rebuild  -> NEW literals must be ABSENT, old literals PRESENT   (negative arm)
    AFTER  the rebuild  -> NEW literals must be PRESENT, old literals PRESENT  (positive arm)

Running only the second half is the same half-a-control mistake that has cost this campaign six
findings. The before-arm is UNRECOVERABLE once the rebuild lands, so it is taken first.

`--baseline` records the pre-rebuild reading to probe-baseline.json; `--verify` reads it back and
requires each NEW literal to have FLIPPED absent -> present. A literal that was already present
before the rebuild proves nothing about the rebuild and is reported as such.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOOSE = Path("/Users/mihaiperdum/Projects/goose/target/release/goose")
BASELINE = HERE / "probe-baseline.json"

# Literals introduced by THIS batch. Each must flip absent -> present across the rebuild.
# PENDING literals — must flip absent -> present across the NEXT rebuild.
#
# ⚠️ A LITERAL GRADUATES OUT OF THIS SET THE MOMENT IT LANDS. Leaving a landed literal here makes
# every subsequent run report it "present before AND after — vacuous", which is true, useless, and
# trains the reader to skim past the one line that might say STILL ABSENT. Move it to
# POSITIVE_CONTROLS instead, where being present is exactly the job.
NEW = {
    "GOOSE_SWARM_PROBE_ADVERTISED_POST": "F738 — the gate that probes advertised POST endpoints",
    "is not CHEAP on a repeat run": "F740 — the NotCheap finding (the arm that caught my own false pass)",
    "it is the previous page's tag": "F740 — the corrected ETag remediation, keyed per request",
}
# Deliberately NOT probed: `clamped`. It is a real new field, but the bare word is common enough in
# a 236 MB binary that a match would not attribute to this edit — and a probe that cannot attribute
# is a probe that manufactures confidence. The two C5(A) fields above are distinctive and cover it.
#
# Deliberately NOT probed: `inconclusive_reasons` (C6 step 1). The baseline run found it ALREADY
# PRESENT — it has shipped on the `spec_contract` event since before this batch (swarm.rs:26503), and
# C6 deliberately REUSES the name on `complete_verify` (:26570) so both events read the same shape.
# Two different events, no duplicate key — checked, because a duplicate key inside one json! macro
# silently keeps only the last value. But a literal that was present before the rebuild can never
# evidence the rebuild, so probing it would be a vacuous pass dressed as a verification.
# C6 IS VERIFIED A DIFFERENT WAY: the first post-rebuild run's log must show `inconclusive_reasons`
# on a `complete_verify` event, not merely somewhere in the binary. Recorded here so the gap is
# deliberate and visible rather than silently dropped.

# Literals that exist in BOTH binaries. If one of these reads absent, the probe itself is broken
# (wrong path, wrong tool, stripped binary) and NO conclusion may be drawn from the NEW readings.
POSITIVE_CONTROLS = {
    "would_skip_ladder": "the pre-existing shadow diagnostic",
    "plan_convergence": "emitted every planning phase",
    "task_dispatched": "the most common event in any run log",
    # GRADUATED from NEW after the 2026-08-09 15:15 rebuild, where all six were watched flipping
    # absent -> present. They now earn their keep as controls: if any of them ever reads ABSENT the
    # build is not the one I think it is, and nothing below may be interpreted.
    "skipped_tests": "F711/C1 — landed 2026-08-09",
    "would_skip_ladder_prelift": "F707 — landed 2026-08-09",
    "description_chars": "C12(b) — landed 2026-08-09",
    "requested_best_of_n": "C5(A) — landed 2026-08-09",
    "distinct_draft_models": "C5(A) — landed 2026-08-09",
    "The spec names these documents": "C7 — landed 2026-08-09",
}

# Must never appear. Catches a grep that matches everything, or a probe run against the wrong file.
NEGATIVE_CONTROL = "goose_swarm_bogus_marker_that_must_not_exist"


def literals(path: Path) -> set[str]:
    out = subprocess.run(["strings", str(path)], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"strings failed on {path}: {out.stderr.strip()}")
    return set(out.stdout.splitlines())


def present(hay: set[str], needle: str) -> bool:
    """Substring match — a literal is usually embedded in a larger constant, not on its own line."""
    return any(needle in line for line in hay)


def read(path: Path) -> dict:
    if not path.exists():
        sys.exit(f"binary missing: {path}")
    hay = literals(path)
    return {
        "mtime": os.path.getmtime(path),
        "size": os.path.getsize(path),
        "new": {k: present(hay, k) for k in NEW},
        "controls": {k: present(hay, k) for k in POSITIVE_CONTROLS},
        "negative": present(hay, NEGATIVE_CONTROL),
    }


def check_controls(r: dict) -> None:
    bad = [k for k, v in r["controls"].items() if not v]
    if bad:
        sys.exit(f"🔴 POSITIVE CONTROL FAILED: {bad} absent from the binary. The probe is BLIND — "
                 "no conclusion about the new literals is valid. Check the path/profile first.")
    if r["negative"]:
        sys.exit("🔴 NEGATIVE CONTROL FAILED: the bogus marker matched. The probe matches anything.")
    print(f"  controls: {len(r['controls'])} known-present literals found, bogus marker absent ✅")


def main() -> int:
    import time
    mode = "--verify" if "--verify" in sys.argv else "--baseline"
    r = read(GOOSE)
    stamp = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(r["mtime"]))
    print(f"binary {GOOSE}\n  built {stamp}  size {r['size']:,}")
    check_controls(r)

    if mode == "--baseline":
        already = [k for k, v in r["new"].items() if v]
        for k, v in r["new"].items():
            print(f"  {'PRESENT' if v else 'absent ':8s} {k:32s} {NEW[k]}")
        BASELINE.write_text(json.dumps(r, indent=2))
        if already:
            print(f"\n⚠️  {already} ALREADY PRESENT before the rebuild. These cannot serve as evidence "
                  "that the rebuild landed — the flip test is vacuous for them.")
        else:
            print(f"\n✅ BASELINE RECORDED: all {len(NEW)} new literals ABSENT pre-rebuild, so a "
                  "PRESENT reading after the rebuild is a real flip and not a pre-existing string.")
        return 0

    if not BASELINE.exists():
        sys.exit("🔴 no probe-baseline.json — the pre-rebuild arm was never taken. A PRESENT reading "
                 "now cannot be distinguished from a literal that was always there. Do NOT proceed.")
    before = json.loads(BASELINE.read_text())
    if before["mtime"] == r["mtime"]:
        sys.exit(f"🔴 BINARY NOT REBUILT — mtime unchanged ({stamp}). cargo said Finished but this "
                 "file is the same one the baseline read. Nothing was verified.")
    ok = True
    for k in NEW:
        was, now = before["new"][k], r["new"][k]
        if now and not was:
            print(f"  ✅ FLIPPED absent -> present   {k:32s} {NEW[k]}")
        elif now and was:
            print(f"  ⚠️  present before AND after   {k:32s} — vacuous, proves nothing")
        else:
            print(f"  🔴 STILL ABSENT               {k:32s} {NEW[k]}")
            ok = False
    print(f"\nbinary mtime moved {stamp} (was "
          f"{time.strftime('%H:%M:%S', time.localtime(before['mtime']))})")
    if not ok:
        print("🔴 THE REBUILD DID NOT CARRY THE EDIT. Do not start the sweep on this binary.")
        return 1
    print("✅ VERIFIED: every new literal flipped absent -> present, controls held.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
