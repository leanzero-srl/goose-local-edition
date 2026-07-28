"""Grade a BUILD-FROM-SPEC episode against a hidden contract suite.

The agent gets a written spec and an empty workspace. It never sees the tests: they are dropped into
a snapshot AFTER the run, so they cannot be read, edited, or satisfied by coincidence.

Only the hidden suite scores. The agent's own tests are left on disk as evidence and deliberately
not executed — a suite the candidate wrote and the candidate passes measures nothing, and counting
it would pay for writing `assert True` in exactly the way the test-writing vertical exists to catch.

The score is continuous (contract tests passed / total), which matters: the binary repair vertical
saturated against every frontier model, and a metric with no resolution cannot rank anyone.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, List

from common import load_meta, run_suite, snapshot

CONTRACT_DIR = "probe_tests"


def contract_files(fixture: Path) -> List[str]:
    """The contract test FILENAMES. They are copied flat into the graded tree, so pytest is
    pointed at these names — pointing it at the directory collects nothing, and pointing it at
    the tree collects the agent's own tests."""
    return sorted(p.name for p in (fixture / CONTRACT_DIR).glob("test_*.py"))


def _contract_ids(fixture: Path) -> List[str]:
    """Run the contract suite against the reference to learn what a perfect score looks like."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "ref"
        root.mkdir()
        _copy_into(fixture / "controls/reference", root)
        _copy_into(fixture / CONTRACT_DIR, root)
        return sorted(run_suite(root, only=contract_files(fixture)))


def _copy_into(src: Path, dest: Path) -> None:
    import shutil
    shutil.copytree(src, dest, dirs_exist_ok=True)


def grade(fixture: Path, workspace: Path) -> Dict:
    meta = load_meta(fixture)
    package = meta.get("package", "")
    result: Dict = {
        "fixture": fixture.name, "score": 0.0, "tampered": False, "tampered_files": [],
        "passed": 0, "total": 0, "failed": [], "reason": None,
    }

    with tempfile.TemporaryDirectory() as tmp:
        graded = snapshot(workspace, Path(tmp) / "graded")
        # The agent must not have shipped anything named like the hidden suite; if it did, it is
        # about to be overwritten, and that is worth recording rather than silently resolving.
        for name in contract_files(fixture):
            if (graded / name).exists():
                result["tampered"] = True
                result["tampered_files"].append(name)
        _copy_into(fixture / CONTRACT_DIR, graded)

        if package and not (graded / package).is_dir():
            nested = [p for p in graded.iterdir() if p.is_dir() and (p / package).is_dir()]
            if nested:
                graded = nested[0]
                _copy_into(fixture / CONTRACT_DIR, graded)
            else:
                result["reason"] = f"no {package}/ package was produced"
                return result

        # ONLY the hidden contract. Collecting the whole tree ran the agent's own tests too
        # and scored one episode 21/21 against a 10-test contract.
        outcomes = run_suite(graded, only=contract_files(fixture))

    if "::collection-error" in outcomes:
        result["reason"] = "the contract suite could not be collected against this tree"
        return result

    result["total"] = len(outcomes)
    result["passed"] = sum(1 for o in outcomes.values() if o == "passed")
    result["failed"] = sorted(t for t, o in outcomes.items() if o != "passed")
    if not result["total"]:
        result["reason"] = "the contract suite produced no results"
        return result

    result["score"] = result["passed"] / result["total"]
    result["reason"] = (f"{result['passed']}/{result['total']} contract tests passed"
                        + (f"; failed: {', '.join(t.split('::')[-1] for t in result['failed'][:4])}"
                           if result["failed"] else ""))
    return result


def self_test(fixture: Path) -> int:
    meta = load_meta(fixture)
    ids = _contract_ids(fixture)
    print(f"contract suite: {len(ids)} tests")
    failures = 0
    if not ids:
        print("  FAIL: the contract suite collected nothing")
        failures += 1

    import shutil
    for name, expected in sorted(meta["controls"].items()):
        with tempfile.TemporaryDirectory() as tmp:
            ws = Path(tmp) / "ws"
            shutil.copytree(fixture / "controls" / name, ws)
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
