"""Grade a TEST-WRITING episode by mutation score: would these tests have caught the bug?

Repair is binary and binary saturates — every cloud baseline cleared every repair rung, so the card
could not rank them. This vertical is CONTINUOUS. The suite is run against K frozen mutants of the
source, and the score is how many it killed.

Two gates, both required, in this order:

  1. The suite must PASS on pristine code. A suite that fails on correct code is worthless however
     many mutants it happens to kill, so this scores 0 outright.
  2. Each mutant is applied to a clean copy and the suite re-run. A mutant is KILLED when the suite
     fails on it. score = killed / K.

`assert True` passes gate 1 and kills nothing, which is exactly the 0.0 it deserves. There is no way
to farm this number by writing more tests — only by writing tests that check behaviour.

The SOURCE is the protected set here, not the tests: the tests are the deliverable, and a run that
edits the code to suit them is measuring nothing.
"""

from __future__ import annotations

import argparse
import json
import tempfile
from pathlib import Path
from typing import Dict, List, Optional

from common import load_meta, normalise, resolve_root, restore, run_suite, sha256, snapshot, stage

NO_TESTS = ("::collection-error", "::timeout")


def _mutant_dirs(fixture: Path, meta: Dict) -> List[Path]:
    return [fixture / "mutants" / name for name in meta["mutants"]]


def grade(fixture: Path, workspace: Path) -> Dict:
    meta = load_meta(fixture)
    seed, protected = fixture / "seed", meta["protected"]
    mutants = _mutant_dirs(fixture, meta)

    result: Dict = {
        "fixture": fixture.name, "score": 0.0, "tampered": False, "tampered_files": [],
        "tests_written": 0, "passes_on_pristine": False,
        "killed": 0, "mutants": len(mutants), "survivors": [], "reason": None,
    }

    root = resolve_root(workspace, seed)
    if root is None:
        result["reason"] = "layout-broken: nothing from the seed tree is present"
        return result

    for rel in protected:
        produced = root / rel
        if not produced.exists() or sha256(produced) != sha256(seed / rel):
            result["tampered"] = True
            result["tampered_files"].append(rel)
    if result["tampered"]:
        result["reason"] = "tampered: the run edited the source it was supposed to test"
        return result

    with tempfile.TemporaryDirectory() as tmp:
        clean = snapshot(root, Path(tmp) / "clean")
        restore(seed, clean, protected)
        pristine = run_suite(clean)

        real = {t: o for t, o in pristine.items() if t not in NO_TESTS}
        result["tests_written"] = len(real)
        if any(t in pristine for t in NO_TESTS):
            result["reason"] = "the suite could not even be collected"
            return result
        if not real:
            result["reason"] = "no tests were written"
            return result
        if any(o != "passed" for o in real.values()):
            failed = sorted(t for t, o in real.items() if o != "passed")
            result["reason"] = f"the suite fails on CORRECT code ({len(failed)} test(s))"
            result["failing_on_pristine"] = failed
            return result
        result["passes_on_pristine"] = True

        for mutant in mutants:
            infected = snapshot(clean, Path(tmp) / f"mut_{mutant.name}")
            stage(mutant, None, infected)
            outcomes = run_suite(infected)
            if any(o != "passed" for o in outcomes.values()) or not outcomes:
                result["killed"] += 1
            else:
                result["survivors"].append(mutant.name)

    result["score"] = result["killed"] / len(mutants)
    result["reason"] = (f"killed {result['killed']}/{len(mutants)} mutants"
                        + (f"; survived: {', '.join(result['survivors'])}"
                           if result["survivors"] else ""))
    return result


def validate_mutants(fixture: Path) -> List[str]:
    """A mutant the GOLD suite cannot kill is equivalent — it caps the achievable score invisibly
    and must be discarded, not shipped."""
    meta = load_meta(fixture)
    survivors = []
    for mutant in _mutant_dirs(fixture, meta):
        with tempfile.TemporaryDirectory() as tmp:
            root = stage(fixture / "seed", fixture / "controls/reference", Path(tmp) / "w")
            stage(mutant, None, root)
            outcomes = run_suite(root)
        if outcomes and all(o == "passed" for o in outcomes.values()):
            survivors.append(mutant.name)
    return survivors


def self_test(fixture: Path) -> int:
    meta = load_meta(fixture)
    failures = 0

    equivalent = validate_mutants(fixture)
    print(f"mutants: {len(meta['mutants'])}, killable by the gold suite: "
          f"{len(meta['mutants']) - len(equivalent)}")
    if equivalent:
        print(f"  FAIL: equivalent mutants (no suite can kill these) — discard them: {equivalent}")
        failures += 1

    for name, expected in sorted(meta["controls"].items()):
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage(fixture / "seed", fixture / "controls" / name, Path(tmp) / "ws")
            got = grade(fixture, ws)
        ok = abs(got["score"] - expected) < 1e-9
        failures += 0 if ok else 1
        print(f"  control {name:18s} expected {expected:.2f} got {got['score']:.2f} "
              f"{'OK' if ok else 'FAIL'}  ({got['reason']})")
    print("PROBE TRUSTED" if not failures else f"PROBE NOT TRUSTED — {failures} failure(s)")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixture", required=True, type=Path)
    ap.add_argument("--workspace", type=Path)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test(args.fixture)
    if not args.workspace:
        ap.error("--workspace is required unless --self-test")
    result = grade(args.fixture, args.workspace)
    text = json.dumps(result, indent=2)
    if args.out:
        args.out.write_text(text)
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
