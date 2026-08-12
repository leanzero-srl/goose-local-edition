"""BENCH2 rank 4: the direct-invocation driver that wakes the dormant vendor_trace checks.

create_payment is REQ-1's dead surface — never called by any endpoint, never evaluated in
campaign history. This probe imports the BUILT MeridianClient (the spec pins the exact API:
__init__(base_url, api_key), fetch_all_payments(), total_count(), create_payment(amount_minor,
currency, idempotency_key)) and exercises it against a FRESH vendor instance on its own
port/trace with fresh one-shot traps. Each step is guarded independently — one failing method
loses its own checks and nothing else (the 0/44-collapse lesson).

The driver runs as a SUBPROCESS from the tree root (180s hard cap, one JSON object on stdout);
a deviation from the spec's pinned API is an honest per-step error, which IS the finding.
"""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, Optional

DRIVER = r'''
import json, os, sys
out = {}
def step(name, fn):
    try:
        out[name] = fn()
    except Exception as e:
        out[name] = None
        out.setdefault("_errors", {})[name] = f"{type(e).__name__}: {e}"[:200]
try:
    from vendorsync.meridian import MeridianClient
except Exception as e:
    print(json.dumps({"_errors": {"import": f"{type(e).__name__}: {e}"[:200]}}))
    sys.exit(0)
def make():
    return MeridianClient(os.environ["MERIDIAN_BASE_URL"], os.environ["MERIDIAN_API_KEY"])
step("_constructed", lambda: bool(make()))
c = None
try:
    c = make()
except Exception:
    pass
if c is not None:
    step("fetch_all_payments", lambda: c.fetch_all_payments())
    step("total_count", lambda: c.total_count())
    step("create_first", lambda: c.create_payment(4242, "EUR", "probe-key-1"))
    step("create_replay", lambda: c.create_payment(4242, "EUR", "probe-key-1"))
print(json.dumps(out, default=str))
'''


def run_client_probe(root: Path, vendor_service, timeout: int = 180) -> Dict:
    """Returns {'results': dict for the results-consuming checks, 'trace': list of vendor
    entries from THIS probe's own vendor instance}. Never raises; failures live in
    results['_errors']."""
    import socket

    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()

    results: Dict = {}
    trace_entries = []
    with tempfile.TemporaryDirectory() as td:
        trace = Path(td) / "client-probe-trace.jsonl"
        driver = Path(td) / "driver.py"
        driver.write_text(DRIVER)
        server = vendor_service.serve(port, trace)
        try:
            vendor_service.mark_phase("client_probe")
            proc = subprocess.run(
                [sys.executable, str(driver)],
                cwd=root, capture_output=True, text=True, timeout=timeout,
                env={"PYTHONPATH": str(root), "PYTHONDONTWRITEBYTECODE": "1",
                     "PATH": "/usr/bin:/bin",
                     "MERIDIAN_BASE_URL": f"http://127.0.0.1:{port}",
                     "MERIDIAN_API_KEY": "sk_test_meridian"},
                start_new_session=True)
            last = (proc.stdout or "").strip().splitlines()
            if last:
                try:
                    results = json.loads(last[-1])
                except json.JSONDecodeError:
                    results = {"_errors": {"stdout": last[-1][:200]}}
            else:
                results = {"_errors": {"driver": (proc.stderr or "no output")[-200:]}}
        except subprocess.TimeoutExpired:
            results = {"_errors": {"driver": f"timed out at {timeout}s"}}
        finally:
            server.shutdown()
        if trace.is_file():
            for line in trace.read_text(errors="replace").splitlines():
                try:
                    trace_entries.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    # The TRUE chronological order comes from the vendor's own ground truth, never the client.
    results["_true_order"] = [
        p["id"] for p in sorted(vendor_service.PAYMENTS, key=lambda p: p["_instant"])
    ]
    return {"results": results, "trace": trace_entries}
