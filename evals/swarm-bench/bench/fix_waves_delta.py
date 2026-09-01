#!/usr/bin/env python3
"""FIX-WAVE VALUE: the pre-fix tree scored beside the final tree -- same seed, same port, same scorer.

WHY (value audit 2026-09-01, VA-019/VA-020): r5's fix waves cost 171 node-minutes and r6c's 458, for one
promotion each and zero criticals moved -- and whether they moved the SCORE was unmeasurable by
construction, because only the final tree is ever scored. Gate 9 says fix waves earn or die on r6e, so
the instrument must exist before r6e finishes. This module is the pure half of it; the hermetic scoring
itself stays in `~/goose-builds/loop-state/score_run.sh --prefix`, which owns the five wrong-number gates.

Three responsibilities, each a function over the run's OWN artifacts, never a guess:

  provenance(run)     WHICH directory is the pre-fix tree, proven from run.jsonl. The engine's
                      `.swarm/best-tree` is rsync --delete'd IN PLACE on every strictly-better verify
                      (swarm.rs `best_tree_snapshot`), so it is the pre-fix tree only while its LAST
                      successful `best_tree_snapshot` is round 0 -- that event fires after the round-0
                      verify and before `phase fix round 0`, when no wave has touched the tree yet.
                      Measured on r6c: round 1 (8 findings, 23:42:21Z) overwrote round 0 (9 findings,
                      22:15:31Z); the pre-fix tree is GONE and the survivor is byte-identical to the
                      final tree. A write-once `.swarm/prefix-tree/` (engine VA, batch 2a) takes
                      precedence the moment the engine writes it -- PROVIDED its `prefix_tree_snapshot`
                      write event precedes the first `complete_fix_dispatched` (VA-043: a resume into
                      REPAIR of a pre-prefix-tree run writes the dir from a post-fix tree). No source
                      proven == REFUSE with the reason; the harness never invents a pre-fix tree.

  identical(a, b)     whether two trees carry the same APP bytes (the engine's F886 exclusions plus
                      scorer/harness debris ignored), so an identical pair is never scored twice and the
                      zero delta called a wave effect.

  line(prefix, final) the ONE comparison line a RUN-LEDGER row carries:
                      fix_waves_delta: prefix <inner>/<crit_mult>/<score> -> final <inner>/<crit_mult>/<score>;
                      criticals moved: <list>
                      Refuses when the two verdicts were scored on different seeds (a cross-seed delta is
                      wrong-number mechanism #4 wearing a comparison's clothes).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Tuple

# The engine's own snapshot exclusions (swarm.rs F886) -- what is NOT the app tree -- plus what the
# scorer and harness drop into a tree after the fact. Everything else is compared byte-for-byte.
ENGINE_EXCLUDED_NAMES = {".swarm", "run.jsonl", "bench-shots", "heartbeat", "graded.db"}
DEBRIS_DIRS = {"__pycache__", ".pytest_cache", "node_modules"}
# Directory names the scorer/harness create INSIDE a tree (score_sb7.py's hermetic wipes list, and
# snapshot_run.py's _scoring_artifact): matched by prefix on any path part.
DEBRIS_PART_PREFIXES = ("graded", "sb7-shots", "sb7-empty-db", "sb7-combined-db", "_sb4trees")
DEBRIS_SUFFIXES = (".log", ".pyc")
DEBRIS_PREFIXES = ("verdict", "harness-grade", "sb7-expect", "sb7-tokens", "graded", "score-",
                   "fix-waves-delta")
DEBRIS_FILES = {"trace.jsonl", "vendor-trace-sb7.jsonl", ".DS_Store"}

PREFIX_TREE = Path(".swarm/prefix-tree")
BEST_TREE = Path(".swarm/best-tree")
LINE_KEY = "fix_waves_delta"
LINE_FILE = "fix-waves-delta.txt"


def _events(run: Path) -> List[dict]:
    log = run / "run.jsonl"
    if not log.is_file():
        return []
    out = []
    for raw in log.open(errors="replace"):
        raw = raw.strip()
        if not raw.startswith("{"):
            continue
        try:
            out.append(json.loads(raw))
        except json.JSONDecodeError:
            continue
    return out


def _is_debris(rel: Path) -> bool:
    if any(part in ENGINE_EXCLUDED_NAMES or part in DEBRIS_DIRS or part.startswith(DEBRIS_PART_PREFIXES)
           for part in rel.parts):
        return True
    name = rel.name
    if name in DEBRIS_FILES or name.endswith(DEBRIS_SUFFIXES):
        return True
    return name.startswith(DEBRIS_PREFIXES)


def _digest(root: Path) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for p in sorted(root.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(root)
        if _is_debris(rel):
            continue
        out[rel.as_posix()] = hashlib.sha256(p.read_bytes()).hexdigest()
    return out


def identical(a: Path, b: Path) -> Tuple[bool, List[str]]:
    """Same app bytes? Returns (verdict, the relative paths that differ or exist on one side only)."""
    da, db = _digest(a), _digest(b)
    diff = sorted(k for k in set(da) | set(db) if da.get(k) != db.get(k))
    return (not diff, diff)


def _parse_ts(raw) -> Optional[datetime]:
    try:
        return datetime.fromisoformat(str(raw).replace("Z", "+00:00"))
    except (TypeError, ValueError):
        return None


def provenance(run: Path) -> dict:
    """Which on-disk tree is the pre-fix tree, and the evidence. `source` is None when none is proven."""
    ev = _events(run)
    snaps = [e for e in ev if e.get("event") == "best_tree_snapshot"]
    ok_snaps = [e for e in snaps if e.get("ok")]
    restored = next((e for e in ev if e.get("event") == "best_tree_restored"), None)
    repair_ts = next((e.get("ts") for e in ev
                      if e.get("event") == "phase" and e.get("phase") == "repair"), None)
    info = {
        "run": str(run),
        "snapshots": [{"round": e.get("round"), "findings": e.get("findings"),
                       "established": e.get("established"), "ok": e.get("ok"), "ts": e.get("ts")}
                      for e in snaps],
        "restored": bool(restored),
        "repair_phase_ts": repair_ts,
        "source": None,
        "label": None,
        "reason": "",
        "best_tree_identical_to_final": None,
    }
    prefix_dir = run / PREFIX_TREE
    best_dir = run / BEST_TREE
    if prefix_dir.is_dir() and any(prefix_dir.iterdir()):
        # VA-043: the dir alone says nothing about WHEN it was written. run.jsonl is opened append-only, so a
        # RESUME into REPAIR of a run that predates the write-once tree writes `.swarm/prefix-tree` from the
        # CURRENT tree -- after that run's fix waves already landed -- and the same log holds both the wave
        # dispatches and the late write. The write event (ok, not `skipped`) must precede the first
        # complete_fix_dispatched, else the dir is a post-fix tree wearing the pre-fix name: REFUSE.
        writes = [e for e in ev if e.get("event") == "prefix_tree_snapshot" and e.get("ok") and not e.get("skipped")]
        first_fix = next((e for e in ev if e.get("event") == "complete_fix_dispatched"), None)
        info["prefix_tree_written_ts"] = writes[0].get("ts") if writes else None
        info["first_fix_dispatched_ts"] = first_fix.get("ts") if first_fix else None
        if not writes:
            info["reason"] = (".swarm/prefix-tree is present but run.jsonl carries no prefix_tree_snapshot WRITE "
                              "event (ok, not skipped) -- nothing in the run's history says when it was taken")
            return info
        w_ts = _parse_ts(writes[0].get("ts"))
        f_ts = _parse_ts(first_fix.get("ts")) if first_fix else None
        if w_ts is None or (first_fix is not None and f_ts is None):
            info["reason"] = (f"prefix_tree_snapshot ts {writes[0].get('ts')!r} / first complete_fix_dispatched ts "
                              f"{first_fix.get('ts') if first_fix else None!r} not parseable -- timing unprovable")
            return info
        if f_ts is not None and not w_ts < f_ts:
            info["reason"] = (f"NOT the pre-fix tree: .swarm/prefix-tree was written at {writes[0].get('ts')}, AFTER "
                              f"the first complete_fix_dispatched at {first_fix.get('ts')} (round {first_fix.get('round')}) "
                              "-- a resume into REPAIR snapshotted a tree the fix waves had already touched")
            return info
        info["source"] = str(prefix_dir)
        info["label"] = "prefix-tree"
        info["reason"] = (f"engine wrote the write-once pre-fix tree at {writes[0].get('ts')}"
                          + (f", before the first complete_fix_dispatched at {first_fix.get('ts')}" if first_fix
                             else "; run.jsonl carries no complete_fix_dispatched (no wave ever ran)")
                          + " (.swarm/prefix-tree)")
        return info
    if best_dir.is_dir():
        same, diff = identical(best_dir, run)
        info["best_tree_identical_to_final"] = same
        info["best_tree_vs_final_diff"] = diff[:20]
    if not ok_snaps:
        info["reason"] = (
            "no successful best_tree_snapshot event in run.jsonl"
            + (" (and no .swarm/best-tree dir)" if not best_dir.is_dir() else
               " -- .swarm/best-tree exists but nothing in the run's history says which tree it is")
        )
        return info
    last = ok_snaps[-1]
    if not best_dir.is_dir():
        info["reason"] = (f"run.jsonl records best_tree_snapshot round {last.get('round')} but "
                          f".swarm/best-tree is not in the archive")
        return info
    if last.get("round") == 0:
        info["source"] = str(best_dir)
        info["label"] = "best-tree@r0"
        info["reason"] = (f"the last successful best_tree_snapshot is round 0 ({last.get('findings')} "
                          f"findings at {last.get('ts')}): taken after the round-0 verify and before "
                          f"'phase fix round 0', so no wave had touched the tree")
        return info
    first = ok_snaps[0]
    info["reason"] = (
        f"NOT the pre-fix tree: .swarm/best-tree holds the round {last.get('round')} snapshot "
        f"({last.get('findings')} findings at {last.get('ts')}) -- the engine rsyncs --delete into ONE "
        f"dir, so round {first.get('round')}'s pre-fix snapshot ({first.get('findings')} findings at "
        f"{first.get('ts')}) was overwritten"
        + ("; the survivor is byte-identical to the final tree (nothing left to measure)"
           if info["best_tree_identical_to_final"] else
           f"; it differs from the final tree in {len(info.get('best_tree_vs_final_diff') or [])} path(s)")
        + ". The engine must write a write-once .swarm/prefix-tree at the INTEGRATE->REPAIR handover "
          "(VA batch 2a); this harness does not invent one"
    )
    return info


def criticals(verdict: dict) -> List[str]:
    rows = (verdict.get("critical") or {}).get("rows") or []
    return [str(r.get("check")) for r in rows if r.get("check")]


def _fmt(verdict: dict) -> str:
    inner = verdict.get("inner")
    mult = (verdict.get("critical") or {}).get("multiplier")
    score = verdict.get("score")
    f = lambda v: "?" if v is None else f"{float(v):.4f}"
    m = "?" if mult is None else f"{float(mult):.3f}"
    return f"{f(inner)}/{m}/{f(score)}"


def moved(prefix_v: dict, final_v: dict) -> str:
    before, after = criticals(prefix_v), criticals(final_v)
    closed = [c for c in before if c not in after]
    opened = [c for c in after if c not in before]
    if not closed and not opened:
        return "none" + (f" ({len(after)} unsuppressed: {', '.join(after)})" if after else "")
    parts = []
    if closed:
        parts.append("closed [" + ", ".join(closed) + "]")
    if opened:
        parts.append("opened [" + ", ".join(opened) + "]")
    return " ".join(parts)


def line(prefix_v: dict, final_v: dict, prefix_label: str = "prefix") -> str:
    sp, sf = prefix_v.get("fixture_seed"), final_v.get("fixture_seed")
    if not sp or not sf or sp.lower() != sf.lower():
        raise SystemExit(f"REFUSED: the two verdicts were scored on different seeds "
                         f"(prefix {sp!r}, final {sf!r}) -- a cross-seed delta compares nothing")
    return (f"{LINE_KEY}: {prefix_label} {_fmt(prefix_v)} → final {_fmt(final_v)}; "
            f"criticals moved: {moved(prefix_v, final_v)}")


def find_verdict(run: Path, seed: Optional[str], kind: str) -> Optional[Path]:
    """The newest hermetic verdict of `kind` ('final' or 'prefix') beside the run, seed-matched.

    The final verdict follows the hand convention that already exists in the archives
    (verdict-hermetic-seed<8>-port<port>-<score>.json); the prefix one is
    verdict-hermetic-prefix-<seed>-<score>.json. Both are read for their fixture_seed, never trusted
    by name alone."""
    cands = []
    for p in run.glob("verdict-hermetic-*.json"):
        is_prefix = p.name.startswith("verdict-hermetic-prefix-")
        is_snapshot = p.name.startswith("verdict-hermetic-snapshot-")
        if kind == "final" and (is_prefix or is_snapshot):
            continue
        if kind == "prefix" and not is_prefix:
            continue
        try:
            v = json.loads(p.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if seed and str(v.get("fixture_seed", "")).lower() != seed.lower():
            continue
        cands.append(p)
    return max(cands, key=lambda p: p.stat().st_mtime) if cands else None


def _run_seed(run: Path) -> Optional[str]:
    tr = run / "trace.jsonl"
    if not tr.is_file():
        return None
    for raw in tr.open(errors="replace"):
        if raw.strip().startswith("{"):
            try:
                return json.loads(raw).get("fixture_seed")
            except json.JSONDecodeError:
                return None
    return None


def main(argv: Optional[Iterable[str]] = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p1 = sub.add_parser("provenance", help="which dir is the pre-fix tree; exit 2 when none is proven")
    p1.add_argument("run", type=Path)
    p1.add_argument("--json", action="store_true")
    p2 = sub.add_parser("identical", help="do two trees carry the same app bytes")
    p2.add_argument("a", type=Path)
    p2.add_argument("b", type=Path)
    p3 = sub.add_parser("line", help="print the fix_waves_delta line for a run; --write stores it beside the verdicts")
    p3.add_argument("run", type=Path)
    p3.add_argument("--prefix-verdict", type=Path)
    p3.add_argument("--final-verdict", type=Path)
    p3.add_argument("--write", action="store_true")
    a = ap.parse_args(list(argv) if argv is not None else None)

    if a.cmd == "provenance":
        info = provenance(a.run.resolve())
        if a.json:
            print(json.dumps(info, indent=1))
        else:
            for s in info["snapshots"]:
                print(f"best_tree_snapshot round={s['round']} findings={s['findings']} ok={s['ok']} ts={s['ts']}")
            if info["restored"]:
                print("best_tree_restored: yes (final tree == best tree by construction)")
            print(f"source={info['source'] or 'NONE'}")
            print(f"label={info['label'] or '-'}")
            print(f"reason: {info['reason']}")
        return 0 if info["source"] else 2

    if a.cmd == "identical":
        same, diff = identical(a.a.resolve(), a.b.resolve())
        print("IDENTICAL" if same else "DIFFER: " + ", ".join(diff[:30]))
        return 0 if same else 1

    run = a.run.resolve()
    seed = _run_seed(run)
    fp = a.prefix_verdict or find_verdict(run, seed, "prefix")
    ff = a.final_verdict or find_verdict(run, seed, "final")
    if fp is None or ff is None:
        print(f"REFUSED: need both verdicts beside {run.name} (prefix={fp}, final={ff}); "
              f"score_run.sh writes the final one, score_run.sh --prefix the other", file=sys.stderr)
        return 2
    out = line(json.loads(fp.read_text()), json.loads(ff.read_text()))
    print(out)
    print(f"  prefix: {fp.name}\n  final:  {ff.name}", file=sys.stderr)
    if a.write:
        (run / LINE_FILE).write_text(out + "\n")
        print(f"  wrote {run / LINE_FILE}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
