#!/usr/bin/env python3
"""Boot the sb-7 vendor service and write the SUBSTITUTED prompt, for a DESKTOP-driven run.

WHY THIS EXISTS. `run_build.py --sb7` does three things before an entrant ever sees the spec: it serves
`vendor_service_v3` on a port, it builds the 12,288-payment fixture set behind it, and it substitutes
`{BASE_URL}` / `{DOCS_URL}` / `{API_KEY}` into the spec text. `launch.sh` does none of that — it types the
raw spec file into the desktop chat over CDP.

MEASURED, and this is the whole reason for the file: every local run on 2026-08-28 dispatched a prompt
still containing the literal strings `{API_KEY}`, `{BASE_URL}` and `{DOCS_URL}`, with no vendor listening
anywhere. The swarm was asked to build a payments console that syncs from a placeholder. It could not
have worked, and no score taken from those trees meant anything.

run_build.py cannot simply be used instead: it invokes the entrant itself, headless. Runs happen in the
desktop app. So this splits the harness — it owns the vendor and the prompt, the desktop owns the build —
and it stays alive for the whole run because the app syncs from the vendor throughout, not just at start.

Usage:
    python3 sb7_local_vendor.py --port 8850 --out /tmp/sb7-prompt.md --trace /tmp/sb7-trace.jsonl
Prints one JSON line (port, seed, prompt path) and then serves until killed. The SEED matters: scoring
must re-serve the vendor with the same seed or the expectation pack does not match the tree.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

import score_sb7  # noqa: E402
import vendor_service_v3 as vendor  # noqa: E402


def build_prompt(port: int) -> str:
    """The spec with its three placeholders resolved — byte-identical to run_build.build_prompt."""
    spec = (ROOT / "spec-build-sb7.md").read_text()
    docs_path = getattr(vendor, "DOCS_PATH", "/v3/docs")
    return (
        spec.replace("{DOCS_URL}", f"http://127.0.0.1:{port}{docs_path}")
        .replace("{BASE_URL}", f"http://127.0.0.1:{port}")
        .replace("{API_KEY}", vendor.API_KEY)
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8850)
    ap.add_argument("--out", type=Path, required=True, help="where to write the substituted prompt")
    ap.add_argument("--trace", type=Path, required=True, help="vendor trace jsonl")
    ap.add_argument("--seed", default=None, help="reuse a seed (scoring must match the run)")
    args = ap.parse_args()

    seed = args.seed or score_sb7._draw_seed()  # noqa: SLF001 — the scorer owns seed policy
    prompt = build_prompt(args.port)

    # REFUSE to write a prompt that still carries a placeholder. This is the exact defect the file exists
    # to prevent, so it must fail loudly here rather than surface as an app that cannot reach its vendor.
    import re

    left = sorted(set(re.findall(r"\{[A-Z_]+\}", prompt)))
    if left:
        print(f"REFUSING: placeholders unresolved after substitution: {left}", file=sys.stderr)
        return 2

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(prompt)

    server = vendor.serve(args.port, args.trace, seed=seed)

    # PROVE IT SERVES before claiming it does — a port that accepts but 404s the docs is the same failure
    # as no vendor at all, one layer down.
    import urllib.request

    docs = f"http://127.0.0.1:{args.port}{getattr(vendor, 'DOCS_PATH', '/v3/docs')}"
    try:
        with urllib.request.urlopen(docs, timeout=10) as r:  # noqa: S310 — loopback, our own server
            body_len = len(r.read())
            code = r.status
    except Exception as exc:  # noqa: BLE001
        server.shutdown()
        print(f"REFUSING: vendor did not serve {docs}: {exc}", file=sys.stderr)
        return 3

    print(
        json.dumps(
            {
                "port": args.port,
                "seed": seed,
                "prompt": str(args.out),
                "prompt_chars": len(prompt),
                "docs": docs,
                "docs_status": code,
                "docs_bytes": body_len,
                "trace": str(args.trace),
                "api_key": vendor.API_KEY,
            }
        ),
        flush=True,
    )
    threading.Event().wait()  # serve until killed; the app syncs from it for the whole run
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
