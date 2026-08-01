#!/usr/bin/env python3
"""Run the produced app against the spec's load-bearing requirements. Exit 0 all-pass, 1 otherwise.

WHY THIS EXISTS SEPARATELY FROM score_build.py. The scorer gave a finished unit 50.0% with tier A at
100%, and the app it graded returned 0 of a required 247 payments and died on an uncaught 404. The
scorer was not wrong — tier A asks whether the thing imports, serves a page and has the right health
shape, and it did. But a four-tier weighted mean is easy to read as "half working", and it is not:
the vendor integration did nothing at all.

So this is deliberately NOT a score. It is a short list of facts the spec REQUIRES, each printed with
the value actually observed, so a claim cannot hide inside an average. It is also independent of the
scorer: if the two ever disagree, that disagreement is the finding.

Every check names the spec requirement it enforces. Nothing here is a proxy — each one runs the
produced code against the real vendor mock.

Usage:
    python3 crunch.py <unit-dir>            # e.g. ../runs/nodeloop/baseline-n3-r0
    python3 crunch.py --json <unit-dir>
"""
from __future__ import annotations

import json
import pathlib
import sys

import time

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
CRUNCH_VERSION = "cr-1"

REQUIRED_PAYMENTS = 247      # spec-build.md: the collection the vendor serves
VENDOR_PORT = 8994


def _result(name, requirement, ok, observed, expected):
    return {"check": name, "requirement": requirement, "ok": bool(ok),
            "observed": observed, "expected": expected}


def crunch(unit: pathlib.Path) -> dict:
    sys.path.insert(0, str(BENCH))
    import vendor_service  # noqa: E402

    checks: list[dict] = []
    trace = pathlib.Path("/tmp") / f"crunch-{unit.name}.jsonl"
    srv = vendor_service.serve(VENDOR_PORT, trace)
    sys.path.insert(0, str(unit))
    try:
        # 1. It must import at all. A tree that does not import cannot be "partly working".
        try:
            from vendorsync.meridian import MeridianClient  # noqa: E402
            from vendorsync.store import Store  # noqa: E402
            checks.append(_result("imports", "the package exists and imports", True, "ok", "ok"))
        except Exception as exc:  # noqa: BLE001 - an import failure IS the result
            checks.append(_result("imports", "the package exists and imports",
                                  False, f"{type(exc).__name__}: {exc}", "ok"))
            return _finish(unit, checks)

        c = MeridianClient(f"http://127.0.0.1:{VENDOR_PORT}", vendor_service.API_KEY)

        # 2. THE check the last unit failed while scoring 50%. fetch_all_payments must return the
        #    whole collection — this is the one number that says the vendor integration works.
        try:
            t0 = time.time()
            pays = c.fetch_all_payments()
            checks.append(_result("fetch_all_payments", f"returns all {REQUIRED_PAYMENTS} payments",
                                  len(pays) == REQUIRED_PAYMENTS,
                                  f"{len(pays)} in {time.time() - t0:.1f}s", REQUIRED_PAYMENTS))
        except Exception as exc:  # noqa: BLE001
            pays = []
            checks.append(_result("fetch_all_payments", f"returns all {REQUIRED_PAYMENTS} payments",
                                  False, f"{type(exc).__name__}: {exc}", REQUIRED_PAYMENTS))

        # 3. Oldest first by when the payment occurred, across MIXED UTC offsets. Sorting the raw
        #    strings passes on a single offset and fails here, which is the point of the trap.
        if pays:
            # Compare INSTANTS, not strings. The vendor serves mixed UTC offsets on purpose, so a
            # CORRECTLY ordered collection is NOT lexicographically sorted — my first version compared
            # the raw strings and failed the known-good 98% tree, inventing a defect in an app that was
            # right. That is the exact failure this project has a law about: a grader's bugs invent
            # defects rather than excuse them, because a broken comparison returns falsy.
            from datetime import datetime

            def instant(v):
                try:
                    return datetime.fromisoformat(str(v).replace("Z", "+00:00")).timestamp()
                except ValueError:
                    return None

            xs = [instant(p.get("created_at")) for p in pays]
            xs = [x for x in xs if x is not None]
            ok = len(xs) == len(pays) and all(a <= b for a, b in zip(xs, xs[1:]))
            checks.append(_result(
                "chronological", "oldest first by INSTANT, mixed offsets normalised", ok,
                f"{pays[0].get('created_at')} .. {pays[-1].get('created_at')}"
                + ("" if len(xs) == len(pays) else f"  ({len(pays) - len(xs)} unparseable)"),
                "non-decreasing instants"))

        # 4. total_count must agree with what was actually fetched. Disagreement means one of the two
        #    endpoints is wrong, and either way the app cannot be trusted about its own data.
        try:
            n = c.total_count()
            checks.append(_result("total_count", "agrees with the fetched collection",
                                  n == REQUIRED_PAYMENTS, n, REQUIRED_PAYMENTS))
        except Exception as exc:  # noqa: BLE001
            checks.append(_result("total_count", "agrees with the fetched collection",
                                  False, f"{type(exc).__name__}: {exc}", REQUIRED_PAYMENTS))

        # 5. create_payment is documented safe to call twice with one key. A 409 on the replay is
        #    SUCCESS, so an uncaught HTTPError here is the app failing its own documented contract.
        try:
            k = "crunch-idempotency-key"
            a = c.create_payment(1234, "EUR", k)
            b = c.create_payment(1234, "EUR", k)
            checks.append(_result("idempotent_create", "same key twice yields the same payment id",
                                  a == b and a, f"{a!r} then {b!r}", "identical ids"))
        except Exception as exc:  # noqa: BLE001
            checks.append(_result("idempotent_create", "same key twice yields the same payment id",
                                  False, f"{type(exc).__name__}: {exc}", "identical ids"))

        # 6. A re-sync must UPDATE rather than duplicate. select-then-insert passes a first sync and
        #    fails this, which is exactly the defect the spec's idempotency requirement targets.
        if pays:
            try:
                db = unit / "crunch-store.db"
                if db.exists():
                    db.unlink()
                st = Store(str(db))
                first = st.upsert_many(pays)
                second = st.upsert_many(pays)
                checks.append(_result("resync_idempotent", "a second sync inserts 0 new rows",
                                      second == 0 and st.count() == len(pays),
                                      f"first={first} second={second} count={st.count()}",
                                      f"first={len(pays)} second=0 count={len(pays)}"))
            except Exception as exc:  # noqa: BLE001
                checks.append(_result("resync_idempotent", "a second sync inserts 0 new rows",
                                      False, f"{type(exc).__name__}: {exc}", "second=0"))
    finally:
        srv.shutdown()

    return _finish(unit, checks)


def _finish(unit: pathlib.Path, checks: list[dict]) -> dict:
    n_ok = sum(1 for c in checks if c["ok"])
    return {"crunch_version": CRUNCH_VERSION, "unit": unit.name,
            "passed": n_ok, "total": len(checks),
            "all_pass": n_ok == len(checks) and bool(checks), "checks": checks}


def render(r: dict) -> str:
    out = [f"=== CRUNCH {r['unit']}  ({r['crunch_version']})  {r['passed']}/{r['total']}"]
    for c in r["checks"]:
        mark = "PASS" if c["ok"] else "FAIL"
        out.append(f"  [{mark}] {c['check']:<20} {c['requirement']}")
        out.append(f"         observed: {c['observed']}   expected: {c['expected']}")
    if not r["all_pass"]:
        out.append("  -> the app does NOT meet the spec. A score is not a substitute for this.")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        return 2
    unit = pathlib.Path(args[0]).resolve()
    if not unit.is_dir():
        print(f"no such unit dir: {unit}")
        return 2
    r = crunch(unit)
    print(json.dumps(r, indent=1) if "--json" in argv else render(r))
    return 0 if r["all_pass"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
