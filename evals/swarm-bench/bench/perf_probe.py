"""Performance probes for the vendorsync scorer — RELATIVE budgets, never wall-clock constants.

The one design rule here: **budgets derive from a known-good reference measured on the SAME
machine, never from hardcoded wall-clock constants.** A "sync must finish in 30s" literal is
meaningless across the fleet — the same build syncs in 4s on the Studio and 19s on a loaded
MacBook, and a constant tuned for one silently fails or free-passes the other. Hardcoded timing
constants are exactly what the campaign's G-batch exists to remove. So this module only ever
MEASURES (measure_endpoint / measure_sync / measure_page, all deterministic given the server's
behaviour) and scores strictly AGAINST A REFERENCE (score_against_reference): the reference
metrics are captured once from the known-good build via save_reference, and a candidate earns
1.0 at or under reference*slack, falling off linearly to 0.0 at 4x that budget. No number in
this file is a claim about how fast anything "should" be in absolute terms.
"""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Dict, List, Optional


def _timed(req, timeout: float):
    """One request, wall-timed. Returns (ok, elapsed_ms, raw_body, error_str)."""
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            ms = (time.perf_counter() - t0) * 1000.0
            if r.status != 200:
                return False, ms, raw, f"status {r.status}"
            return True, ms, raw, ""
    except urllib.error.HTTPError as e:
        ms = (time.perf_counter() - t0) * 1000.0
        return False, ms, e.read(), f"status {e.code}"
    except Exception as e:
        ms = (time.perf_counter() - t0) * 1000.0
        return False, ms, b"", f"{type(e).__name__}: {e}"


def _percentile(sorted_ms: List[float], q: float) -> float:
    """Sorted-list index method: value at index int(q * (len-1))."""
    return sorted_ms[int(q * (len(sorted_ms) - 1))]


def measure_endpoint(base_url: str, path: str, n: int = 20, timeout: float = 10.0) -> Dict:
    """n sequential GETs against base_url+path; latency distribution of the successes.

    Errors are recorded and EXCLUDED from the percentiles — a distribution over failures is
    not a latency. If fewer than n//2 requests succeed the percentiles are meaningless and
    reported as None; the caller scores that via score_against_reference (None -> 0.0).
    """
    url = base_url.rstrip("/") + path
    ok_ms: List[float] = []
    errors: List[str] = []
    for _ in range(n):
        ok, ms, _raw, err = _timed(urllib.request.Request(url), timeout)
        if ok:
            ok_ms.append(ms)
        else:
            errors.append(err)
    if not ok_ms or len(ok_ms) < n // 2:
        return {"n_ok": len(ok_ms), "p50_ms": None, "p95_ms": None,
                "max_ms": None, "errors": errors}
    ok_ms.sort()
    return {
        "n_ok": len(ok_ms),
        "p50_ms": _percentile(ok_ms, 0.50),
        "p95_ms": _percentile(ok_ms, 0.95),
        "max_ms": ok_ms[-1],
        "errors": errors,
    }


def measure_sync(base_url: str, timeout: float = 240.0) -> Dict:
    """One POST /api/sync, wall-timed. The heavyweight operation of the app."""
    url = base_url.rstrip("/") + "/api/sync"
    ok, ms, raw, err = _timed(urllib.request.Request(url, data=b"", method="POST"), timeout)
    try:
        response = json.loads(raw)
    except Exception:
        response = None
    return {"ok": ok, "wall_ms": ms, "response": response if ok else (response or err)}


def measure_page(base_url: str, timeout: float = 10.0) -> Dict:
    """GET /, wall-timed, with body size — the page must both arrive fast and actually exist."""
    ok, ms, raw, _err = _timed(urllib.request.Request(base_url.rstrip("/") + "/"), timeout)
    return {"ok": ok, "wall_ms": ms, "bytes": len(raw)}


# ── reference metrics ─────────────────────────────────────────────────────────────────────────

def load_reference(path) -> Optional[Dict]:
    p = Path(path)
    if not p.is_file():
        return None
    try:
        return json.loads(p.read_text())
    except Exception:
        return None


def save_reference(path, metrics: Dict) -> None:
    Path(path).write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n")


def score_against_reference(measured, reference, slack: float) -> float:
    """Score a measured timing against the reference budget. Returns a float in [0,1].

    budget = reference * slack. At or under budget -> 1.0; linear falloff to 0.0 at 4x the
    budget; a None measurement (endpoint never answered enough to time) -> 0.0.
    """
    if measured is None:
        return 0.0
    budget = reference * slack
    if budget <= 0:
        return 1.0 if measured <= 0 else 0.0
    if measured <= budget:
        return 1.0
    if measured >= 4 * budget:
        return 0.0
    return 1.0 - (measured - budget) / (3 * budget)


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        print("usage: perf_probe.py <base_url>", file=sys.stderr)
        sys.exit(2)
    base = sys.argv[1]
    out = {
        "sync": measure_sync(base),
        "page": measure_page(base),
        "health": measure_endpoint(base, "/api/health"),
        "payments": measure_endpoint(base, "/api/payments"),
        "summary": measure_endpoint(base, "/api/summary"),
    }
    print(json.dumps(out, indent=2))
