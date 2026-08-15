"""Prove the grader works in BOTH directions before any score is believed.

Three properties, and only the third is usually tested:

  HIGH   a known-good build scores high
  LOW    a deliberately broken build scores low
  ISOLATION  each injected defect fails ONLY its own check

Isolation is the one that matters most and is almost never checked. A grader with a shared
precondition collapses a mostly-correct build to zero — measured twice in this project, once when a
missing subcommand took a 43/45 build to 0/44, and once when a stale trace field scored a correct
paginator 0%. Both looked like devastating model failures and were neither.

Defects are injected into a COPY of a real known-good tree, so the control tests the grader against
the same shape of artifact it will grade in production, not a hand-written strawman.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path
from typing import Callable, Dict, List

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import score_build  # noqa: E402
import vendor_service  # noqa: E402

ROOT = HERE.parent


def _sub(path: Path, pattern: str, repl: str) -> bool:
    if not path.is_file():
        return False
    text = path.read_text(errors="replace")
    new = re.sub(pattern, repl, text, count=1)
    if new == text:
        return False
    path.write_text(new)
    return True


# Each defect: (name, mutate, checks it SHOULD break). Anything else breaking is a cascade.
DEFECTS: List[Dict] = [
    {
        "name": "no_frontend",
        "expect": {"modules_present", "serves_page", "ui_states", "ui_currency",
                   "ui_offline", "ui_polish", "ui_error_actionable"},
        "apply": lambda pkg: shutil.rmtree(pkg / "web", ignore_errors=True) or True,
    },
    {
        # A method the app never calls at runtime: the AST check must notice, and nothing else may.
        # The first version of this defect renamed `last_sync`, which IS called on the health path —
        # the server then crashed and 24 checks cascaded. That was a badly designed control, not a
        # grader fault: a defect that breaks the app at runtime SHOULD break everything downstream.
        "name": "declared_iface_incomplete",
        "expect": {"interfaces_declared", "client_total_count"},
        "apply": lambda pkg: _sub(pkg / "meridian.py", r"def total_count\(",
                                  "def _total_count_renamed("),
    },
    {
        "name": "default_limit_wrong",
        "expect": {"local_pagination"},
        "apply": lambda pkg: _sub(pkg / "api.py", r"\b25\b", "7"),
    },
    {
        # Keep the write CORRECT but make it non-atomic in shape, so only the finesse check moves.
        # Deleting `ON CONFLICT` outright broke the SQL and nothing persisted at all — again a bad
        # control rather than a bad grader.
        "name": "upsert_not_atomic",
        "expect": {"store_atomic_upsert"},
        # Same table, same columns, still writes — only the ATOMIC form is removed, replaced by
        # INSERT OR REPLACE. Renaming the table instead broke persistence entirely and cascaded,
        # which tested nothing about the atomicity check.
        "apply": lambda pkg: (
            _sub(pkg / "store.py", r"INSERT INTO payments", "INSERT OR REPLACE INTO payments")
            and _sub(pkg / "store.py", r"ON CONFLICT\(id\)[^\"]*?(?=\"\"\")", "")),
    },
    {
        # BENCH2 rank 10: the client stops recognizing 409-as-success — ONLY the replay check
        # may move (create_first still succeeds; the replay raises inside the probe driver).
        "name": "replay_as_error",
        "expect": {"client_create_replay"},
        "apply": lambda pkg: _sub(pkg / "meridian.py", "== 409", "== 499"),
    },
    {
        # BENCH2 rank 10: memory dressed as a database — persistence dies at the SIGKILL, and
        # ONLY restart_persistence may notice (within one process lifetime :memory: is correct).
        "name": "store_forgets_on_restart",
        "expect": {"restart_persistence"},
        "apply": lambda pkg: (
            _sub(pkg / "store.py", "import sqlite3", "import os, sqlite3")
            and _sub(pkg / "store.py", r"sqlite3\.connect\(self\.path\)",
                     "sqlite3.connect(str(self.path) + str(os.getpid()))")),
    },
    {
        # BENCH2 rank 10: the upsert's STATUS assignment becomes a no-op (old value kept):
        # every other field updates, no duplicates, counts right - ONLY update_propagation
        # may notice. (A DO NOTHING rewrite was rejected: the multi-line SET block would
        # break the SQL outright and cascade - a bad control, not a grader test.)
        "name": "update_ignored",
        "expect": {"update_propagation"},
        "apply": lambda pkg: _sub(pkg / "store.py", r"status=excluded\.status",
                                  "status=payments.status"),
    },
]

# ── sb-5 PRODUCT defects (run only under BENCH_PRODUCT; patterns target the v2 reference) ─────
if score_build.PRODUCT:
    DEFECTS += [
        {
            # whenText's formatter path returns the raw ISO string — the exact sin spec v2
            # forbids. Only the rendered-date check may notice.
            "name": "iso_dates",
            "expect": {"v_dates_readable"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r"if \(fmt\) \{ return fmt\.format\(d\); \}",
                                      "if (fmt) { return iso; }"),
        },
        {
            # Badges lose their class: statuses render as plain text. v_styling is adjacent by
            # construction (badge backgrounds count toward the design-effort signal).
            "name": "plain_status",
            "expect": {"v_status_distinct", "v_styling"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r'span\.className = "badge " \+ \(KNOWN\[key\] \|\| "b-other"\);',
                                      'span.className = "";'),
        },
        {
            # The page asks the API for everything at once — the unpaginated dump. The summary
            # bar still claims 247, so j_loads_data must NOT drop (reconciliation holds).
            "name": "drop_pagination",
            "expect": {"v_pagination"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r"var LIMIT = 25;", "var LIMIT = 1000;"),
        },
        {
            # The status filter group never renders. Only v_filter may notice. Inline style,
            # not the `hidden` attribute: the reference's own `.filter-group { display:
            # inline-flex }` author rule overrides the UA's [hidden] display:none (author
            # origin wins at equal specificity) — the first version of this defect stayed
            # visible and the control correctly reported it undetected.
            "name": "drop_filter",
            "expect": {"v_filter"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r'<div class="filter-group"',
                                      '<div style="display:none" class="filter-group"'),
        },
        {
            # The Sync click handler returns immediately: button found, nothing happens. The
            # source still carries the disabled/refresh code, so the ui_polish regexes hold.
            "name": "break_sync_button",
            "expect": {"j_sync_journey"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r'el\("sync"\)\.addEventListener\("click", function \(\) \{',
                                      'el("sync").addEventListener("click", function () { return;'),
        },
        {
            # A deferred throw on load: the console is no longer clean, everything else works.
            "name": "console_error",
            "expect": {"j_console_clean"},
            "apply": lambda pkg: _sub(pkg / "web/index.html",
                                      r"</body>",
                                      '<script>setTimeout(function(){ throw new Error('
                                      '"control-defect: injected"); }, 50);</script></body>'),
        },
    ]
    # The frontend-deleting defect legitimately collapses every browser check with it.
    for _d in DEFECTS:
        if _d["name"] == "no_frontend":
            _d["expect"] |= {
                "j_loads_data", "j_console_clean", "j_sync_journey", "j_error_state",
                "j_empty_state", "p_page_interactive", "v_dates_readable",
                "v_status_distinct", "v_pagination", "v_filter", "v_responsive_375",
                "v_styling"}


def run_control(name: str, workdir: Path, port: int, out: Path) -> Dict:
    trace = out / f"trace-control-{name}.jsonl"
    server = vendor_service.serve(port, trace)
    try:
        ctx = score_build.gather(workdir, port, workdir / "control.db", trace,
                                 mark_phase=vendor_service.mark_phase)
    finally:
        server.shutdown()
    return score_build.evaluate(ctx)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--good", type=Path, default=ROOT / "runs/build/opus-5-r0")
    ap.add_argument("--out", type=Path, default=ROOT / "runs/controls")
    ap.add_argument("--port", type=int, default=8990)
    # BENCH2 rank 10: the HIGH threshold is MEASURED, not guessed — 0.85 was sb-3's number;
    # the first sb-4 controls run publishes the known-good vector and this default follows it.
    ap.add_argument("--high", type=float, default=0.85)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    if not (args.good / "vendorsync").is_dir():
        raise SystemExit(f"no known-good tree at {args.good}")

    print("CONTROL 1/2 — the known-good build must score HIGH\n")
    good_dir = args.out / "good"
    shutil.rmtree(good_dir, ignore_errors=True)
    shutil.copytree(args.good, good_dir, ignore=shutil.ignore_patterns(
        "*.db", "verdict.json", "process.json", "__pycache__", ".swarm"))
    good = run_control("good", good_dir, args.port, args.out)
    good_fail = {c["check"] for c in good["checks"] if c["score"] < 1.0}
    print(f"  known-good scored {100 * good['score']:.1f}%   "
          f"({len(good_fail)} check(s) below full: {sorted(good_fail) or 'none'})")
    ok_high = good["score"] >= args.high
    print(f"  {'PASS' if ok_high else 'FAIL'} — needs >= {100 * args.high:.0f}%\n")

    # BENCH2 rank 10: RUN-TWICE DETERMINISM. The same tree scored twice must produce an
    # IDENTICAL per-check vector — any drift means a check depends on state the harness does
    # not control, and the repair loop that consumes findings cannot tolerate that.
    print("CONTROL determinism — the same tree scored twice must match check-for-check\n")
    # db PLUS -wal/-shm: after the gather's SIGKILL the WAL survives, and a re-score on a
    # resurrected db reads fresh=False at boot — measured as the health_semantics 1.0-vs-0.75
    # determinism drift. The sidecars ARE the state; unlinking only the db is not a reset.
    for p in good_dir.glob("control.db*"):
        p.unlink(missing_ok=True)
    good2 = run_control("good-again", good_dir, args.port + 50, args.out)
    # Drift diagnosis needs the DETAIL strings, not just the score vectors — persist both.
    (args.out / "good-verdict.json").write_text(json.dumps(good, indent=2, default=str))
    (args.out / "good-again-verdict.json").write_text(json.dumps(good2, indent=2, default=str))
    v1 = {c["check"]: c["score"] for c in good["checks"]}
    v2 = {c["check"]: c["score"] for c in good2["checks"]}
    drift = {k: (v1.get(k), v2.get(k)) for k in set(v1) | set(v2) if v1.get(k) != v2.get(k)}
    ok_det = not drift
    if drift:
        for k, (a, b) in sorted(drift.items()):
            print(f"  DRIFT {k}: {a} vs {b}")
    print(f"  {'PASS' if ok_det else 'FAIL'} — {len(drift)} drifting check(s)\n")

    print("CONTROL 2/2 — each injected defect must fail ONLY its own checks\n")
    failures = 0
    port = args.port + 1
    for defect in DEFECTS:
        wd = args.out / defect["name"]
        shutil.rmtree(wd, ignore_errors=True)
        shutil.copytree(args.good, wd, ignore=shutil.ignore_patterns(
            "*.db", "verdict.json", "process.json", "__pycache__", ".swarm"))
        pkg = wd / "vendorsync"
        if not defect["apply"](pkg):
            print(f"  {defect['name']:<22} SKIPPED — the mutation did not apply to this tree")
            continue

        got = run_control(defect["name"], wd, port, args.out)
        port += 1
        # BENCH2 rank 10 fix (from the suite's own first red run): DELTA-based, not
        # membership-based. A GRADED check that WORSENS under a defect was invisible when the
        # known-good already scored it below 1.0 — update_ignored went "undetected" exactly
        # that way. Newly-broken = the score DROPPED against the known-good's own vector.
        gv = {c["check"]: c["score"] for c in good["checks"]}
        newly = {c["check"] for c in got["checks"]
                 if c["score"] < gv.get(c["check"], 1.0) - 1e-9}
        expected, cascade = defect["expect"], newly - defect["expect"]
        hit = newly & expected

        status = "PASS" if hit and not cascade else "FAIL"
        failures += status == "FAIL"
        print(f"  {defect['name']:<22} {100 * got['score']:>5.1f}%  {status}")
        print(f"      broke as intended : {sorted(hit) or 'NOTHING — the defect went undetected'}")
        if cascade:
            print(f"      CASCADE           : {sorted(cascade)}")
        if not hit:
            print(f"      expected to break : {sorted(expected)}")

    trusted = ok_high and ok_det and not failures
    verdict = "GRADER TRUSTED" if trusted else \
              f"GRADER NOT TRUSTED — {failures + (0 if ok_high else 1) + (0 if ok_det else 1)} control failure(s)"
    print(f"\n{verdict}")
    (args.out / "controls.json").write_text(json.dumps(
        {"known_good": good["score"], "known_good_vector": v1,
         "determinism_drift": drift, "failures": failures, "trusted": trusted},
        indent=2, default=str))
    return 0 if trusted else 1


if __name__ == "__main__":
    raise SystemExit(main())
