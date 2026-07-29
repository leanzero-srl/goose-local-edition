"""Exercise a candidate MeridianClient and capture what it returned.

Runs in a subprocess so a client that hangs, crashes or calls sys.exit cannot take the harness with
it. Every step is independently guarded: a failure in create_payment must not erase the evidence
already gathered about pagination.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import traceback
from pathlib import Path


def load_client(path: Path):
    spec = importlib.util.spec_from_file_location("candidate_client", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["candidate_client"] = module
    spec.loader.exec_module(module)
    return getattr(module, "MeridianClient")


def main() -> int:
    client_path, base_url, api_key, out_path = (Path(sys.argv[1]), sys.argv[2], sys.argv[3],
                                                Path(sys.argv[4]))
    results = {"errors": {}}

    try:
        MeridianClient = load_client(client_path)
        client = MeridianClient(base_url, api_key)
    except Exception:
        results["errors"]["construct"] = traceback.format_exc()[-1500:]
        out_path.write_text(json.dumps(results))
        return 1

    def step(name, fn):
        try:
            results[name] = fn()
        except Exception:
            results["errors"][name] = traceback.format_exc()[-1500:]

    # First sync — pagination, the throttle, and the short page.
    step("fetch_all_payments", lambda: client.fetch_all_payments())
    # Second identical sync — this is where a conditional request should appear.
    step("fetch_all_payments_again", lambda: client.fetch_all_payments())
    step("total_count", lambda: client.total_count())
    # Create, then replay the SAME idempotency key: the documented 409-is-success path.
    step("create_first", lambda: client.create_payment(4500, "EUR", "bench-key-1"))
    step("create_replay", lambda: client.create_payment(4500, "EUR", "bench-key-1"))

    out_path.write_text(json.dumps(results, default=str))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
