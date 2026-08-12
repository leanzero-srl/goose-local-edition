#!/usr/bin/env python3
"""The F768 bridge duty: at a unit's end, score its FRESH tree under BOTH scorer legs.

Archived trees cannot be re-scored (F768: re-exercised apps never reach a fresh vendor), so
the sb-3 <-> sb-4 mapping is built from LIVE trees, at unit end, before the sweep's dir reuse
wipes them. Usage: python3 dualscore.py <unit-dir>. Appends one JSON line per invocation to
nodeloop/bridge-ledger.jsonl; the mapping is the ledger's pairs.
"""
import importlib, json, socket, sys, tempfile, time
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent / "bench"
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(BENCH / "legacy"))
import vendor_service  # noqa: E402


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def leg(mod_name: str, root: Path) -> dict:
    mod = importlib.import_module(mod_name)
    port = free_port()
    with tempfile.TemporaryDirectory() as td:
        trace, db = Path(td) / "t.jsonl", Path(td) / "b.db"
        server = vendor_service.serve(port, trace)
        try:
            ctx = mod.gather(root, port, db, trace, mark_phase=vendor_service.mark_phase)
        finally:
            server.shutdown()
        v = mod.evaluate(ctx)
    return {"score": v["score"], "version": v["scorer_version"],
            "tiers": {k: t["mean"] for k, t in v["tiers"].items()},
            "vendor_reqs_seen": bool(ctx.sync1)}


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    if not (root / "vendorsync").is_dir() and not any(
        (p / "vendorsync").is_dir() for p in root.iterdir() if p.is_dir()
    ):
        print(f"no app tree under {root}", file=sys.stderr)
        return 1
    out = {"unit": root.name, "t": round(time.time(), 1)}
    for name in ("score_build_sb3", "score_build"):
        out[name] = leg(name, root)
        # F768's tell: a leg whose app never reached the vendor is a VOID reading, not a score.
        if not out[name]["vendor_reqs_seen"]:
            out[name]["void"] = "app made no vendor requests — F768 class, do not use"
    ledger = Path(__file__).resolve().parent / "bridge-ledger.jsonl"
    with ledger.open("a") as fh:
        fh.write(json.dumps(out) + "\n")
    print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
